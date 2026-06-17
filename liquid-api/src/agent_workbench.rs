use std::time::Instant;

use anyhow::Context;
use liquid_agent::{
    LlmWorkbenchAgent, LlmWorkbenchContext, PostgresToolConfig, PostgresToolExecutionMode,
    ToolRegistry, WorkbenchResponse, WorkbenchToolStep,
    tools::{
        DatabaseOperationToolContext,
        sets::{workbench_database_backup_tools, workbench_readonly_postgres_tools},
    },
};
use liquid_core::{
    AgentAction, AgentActionKind, AgentEventRecord, AgentEventType, AgentMessageRole,
    AgentResourceKind, AgentTurn, AgentTurnStatus, ApproveSqlAuditRequest,
    CreateAgentActionRequest, CreateDatabaseDiagramRequest, CreateDatapanelCardRequest,
    CreateSqlAuditRequest, DatabaseDiagram, Datapanel, DatapanelCard, DatapanelCardKind,
    DatapanelCardLayout, DatapanelChartConfig, DatapanelChartType, DatapanelQueryResult,
    EnqueueDatabaseRestore, ManagedDatabasePoolKey, PublicUser, RejectSqlAuditRequest,
    SqlAuditRecord, SqlAuditStatus,
};
use liquid_storage::SqlAuditListFilters;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;

use crate::{
    datapanels::materialize_datapanel_query,
    error::ApiError,
    llm_provider::user_llm_provider_for_user,
    sql_audits::{SqlAuditExecutionOutcome, create_sql_audit_for_user, execute_sql_audit_for_user},
    state::ApiState,
};

const MISSING_LLM_PROVIDER_MESSAGE: &str = "LLM provider is not configured. Configure a provider and API key before using AI workbench chat.";
const MAX_ASSISTANT_QUERY_RESULT_TABLES: usize = 3;

fn recent_sql_audits_filter(managed_database_id: Option<&str>) -> SqlAuditListFilters<'_> {
    SqlAuditListFilters {
        managed_database_id,
        status: None,
        audit_status: None,
        execution_status: None,
        created_from: None,
        created_to: None,
        page: 1,
        page_size: 20,
    }
}

#[derive(Debug, Deserialize)]
struct CreateSqlAuditActionPayload {
    managed_database_id: String,
    request: CreateSqlAuditRequest,
}

#[derive(Debug, Deserialize)]
struct SqlAuditActionPayload {
    sql_audit_id: String,
}

#[derive(Debug, Deserialize)]
struct StartDatabaseRestoreActionPayload {
    backup_id: String,
    target_managed_database_id: String,
    purpose: String,
    confirm_destructive_restore: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct CreateDatapanelCardActionPayload {
    pub(crate) managed_database_id: String,
    pub(crate) title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
    pub(crate) kind: DatapanelCardKind,
    pub(crate) sql: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) chart: Option<DatapanelChartConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) layout: Option<DatapanelCardLayout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) result: Option<DatapanelQueryResult>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct CreateDatabaseDiagramActionPayload {
    pub(crate) managed_database_id: String,
    pub(crate) managed_database_name: String,
    pub(crate) title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AssistantQueryResultTable {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    managed_database_id: String,
    sql: String,
    result: DatapanelQueryResult,
}

#[derive(Debug, Deserialize)]
struct ReadonlyToolQueryResult {
    columns: Vec<String>,
    rows: Vec<Value>,
    row_count: i32,
    truncated: bool,
    elapsed_ms: i64,
}

pub(crate) async fn run_agent_turn(
    state: ApiState,
    user: PublicUser,
    turn_id: String,
) -> anyhow::Result<()> {
    match run_agent_turn_inner(state.clone(), user.clone(), turn_id.clone()).await {
        Ok(()) => Ok(()),
        Err(error) => {
            let error_message = error.to_string();
            let status = if error_message == MISSING_LLM_PROVIDER_MESSAGE {
                AgentTurnStatus::Blocked
            } else {
                AgentTurnStatus::Failed
            };
            let _ = state
                .store
                .update_agent_turn_status(&user.id, &turn_id, status, Some(error_message.clone()))
                .await;
            let _ = append_event(
                &state,
                &user.id,
                &turn_id,
                AgentEventType::TurnFailed,
                json!({ "error": error_message }),
            )
            .await;

            Err(error)
        }
    }
}

async fn run_agent_turn_inner(
    state: ApiState,
    user: PublicUser,
    turn_id: String,
) -> anyhow::Result<()> {
    let turn = state.store.get_agent_turn(&user.id, &turn_id).await?;

    if turn.status.is_terminal() {
        return Ok(());
    }

    if turn.status == AgentTurnStatus::WaitingForUser {
        return Ok(());
    }

    state
        .store
        .update_agent_turn_status(&user.id, &turn_id, AgentTurnStatus::Running, None)
        .await?;
    append_event(
        &state,
        &user.id,
        &turn_id,
        AgentEventType::TurnStarted,
        json!({ "turn_id": turn_id }),
    )
    .await?;

    append_event(
        &state,
        &user.id,
        &turn_id,
        AgentEventType::MessageCreated,
        json!({
            "message_id": turn.user_message_id,
            "role": "user",
        }),
    )
    .await?;

    append_event(
        &state,
        &user.id,
        &turn_id,
        AgentEventType::ToolCallStarted,
        json!({
            "name": "load_workbench_context",
            "stage": "loading_context",
            "summary": "Loading database and audit context"
        }),
    )
    .await?;
    let databases = state.store.list_managed_databases(&user.id).await?;
    let summary = state.agent.audit_summary().await.ok();
    let selected_sql_audit_id = turn
        .dashboard_context
        .as_ref()
        .and_then(|context| context.selected_sql_audit_id.clone());
    let recent_sql_audits = state
        .store
        .list_sql_audits(
            &user.id,
            recent_sql_audits_filter(turn.managed_database_id.as_deref()),
        )
        .await?
        .records;
    let recent_actions = state
        .store
        .list_agent_actions(&user.id, Some(&turn.conversation_id), None)
        .await?;
    let messages = state
        .store
        .list_agent_messages(&user.id, &turn.conversation_id, 40, None)
        .await?;
    messages
        .iter()
        .find(|message| message.id == turn.user_message_id)
        .ok_or_else(|| anyhow::anyhow!("agent user message not found"))?;
    let managed_database = turn
        .managed_database_id
        .as_deref()
        .and_then(|managed_database_id| {
            databases
                .iter()
                .find(|database| database.id == managed_database_id)
                .cloned()
        });
    append_event(
        &state,
        &user.id,
        &turn_id,
        AgentEventType::ToolCallFinished,
        json!({
            "name": "load_workbench_context",
            "next_summary": "Planning the next step",
            "output": {
                "managed_database_count": databases.len(),
                "audit_score": summary.as_ref().map(|summary| summary.audit_score),
                "recent_sql_audit_count": recent_sql_audits.len(),
                "recent_action_count": recent_actions.len(),
            }
        }),
    )
    .await?;

    let Some(provider) = user_llm_provider_for_user(&state, &user.id).await? else {
        anyhow::bail!(MISSING_LLM_PROVIDER_MESSAGE);
    };
    let tools = workbench_tool_registry(&state, &user.id, &turn).await?;
    let streaming_message_id = format!("stream-{turn_id}");
    let agent = LlmWorkbenchAgent::new(provider.client, provider.model, provider.protocol)
        .with_max_tool_rounds(state.workbench.max_tool_rounds)
        .with_max_output_tokens(state.workbench.max_output_tokens)
        .with_streaming_enabled(provider.streaming_enabled);
    let response = agent
        .respond_with_tools_and_text_delta(
            LlmWorkbenchContext {
                messages,
                managed_database,
                write_sql_execution_enabled: state.approved_write_execution_enabled,
                selected_sql_audit_id,
                audit_summary: summary,
                recent_sql_audits,
                recent_actions,
            },
            tools,
            {
                let state = state.clone();
                let owner_user_id = user.id.clone();
                let turn_id = turn_id.clone();
                move |delta| {
                    let state = state.clone();
                    let owner_user_id = owner_user_id.clone();
                    let turn_id = turn_id.clone();
                    let streaming_message_id = streaming_message_id.clone();
                    async move {
                        if let Err(error) = append_event(
                            &state,
                            &owner_user_id,
                            &turn_id,
                            AgentEventType::AssistantDelta,
                            json!({
                                "message_id": streaming_message_id,
                                "content": delta,
                                "append": true,
                            }),
                        )
                        .await
                        {
                            tracing::warn!(
                                turn_id = %turn_id,
                                error = %error,
                                "failed to append streamed assistant delta"
                            );
                        }
                    }
                }
            },
        )
        .await?;

    let latest_turn = state.store.get_agent_turn(&user.id, &turn_id).await?;
    if latest_turn.status.is_terminal() {
        return Ok(());
    }

    persist_workbench_response(&state, &user.id, &turn, response).await?;

    Ok(())
}

async fn workbench_tool_registry(
    state: &ApiState,
    owner_user_id: &str,
    turn: &AgentTurn,
) -> anyhow::Result<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    if let Some(managed_database_id) = turn.managed_database_id.as_deref() {
        let pool = state
            .managed_database_pools
            .get_pool(ManagedDatabasePoolKey::new(
                owner_user_id.to_owned(),
                managed_database_id.to_owned(),
            ))
            .await?;
        registry.extend(workbench_readonly_postgres_tools(PostgresToolConfig::new(
            Some(pool),
            state.sql_metadata_required,
            PostgresToolExecutionMode::Readonly,
        )));
    }
    registry.extend(workbench_database_backup_tools(
        DatabaseOperationToolContext::new(owner_user_id, state.database_backups.clone())
            .with_chat_context(Some(turn.conversation_id.clone()), Some(turn.id.clone())),
    ));

    Ok(registry)
}

async fn persist_workbench_response(
    state: &ApiState,
    owner_user_id: &str,
    turn: &AgentTurn,
    response: WorkbenchResponse,
) -> anyhow::Result<()> {
    for step in &response.tool_steps {
        append_workbench_tool_step(state, owner_user_id, turn, step).await?;
    }

    let query_result_tables = assistant_query_result_tables(turn, &response.tool_steps);
    let assistant_metadata = if query_result_tables.is_empty() {
        None
    } else {
        Some(json!({
            "kind": "assistant_response",
            "query_result_tables": query_result_tables,
        }))
    };

    let mut prepared_actions = Vec::new();
    for suggestion in response.actions {
        prepared_actions.push(prepare_workbench_action(state, owner_user_id, suggestion).await?);
    }

    let assistant_message = state
        .store
        .append_agent_message(
            owner_user_id,
            &turn.conversation_id,
            Some(&turn.id),
            AgentMessageRole::Assistant,
            &response.content,
            assistant_metadata,
        )
        .await?;
    state
        .store
        .set_agent_turn_assistant_message(owner_user_id, &turn.id, &assistant_message.id)
        .await?;
    append_event(
        state,
        owner_user_id,
        &turn.id,
        AgentEventType::AssistantDelta,
        json!({
            "message_id": assistant_message.id,
            "content": response.content
        }),
    )
    .await?;
    append_event(
        state,
        owner_user_id,
        &turn.id,
        AgentEventType::MessageCreated,
        json!({
            "message_id": assistant_message.id,
            "role": "assistant",
        }),
    )
    .await?;

    let waiting_for_user = !prepared_actions.is_empty();
    for suggestion in prepared_actions {
        let action = state
            .store
            .create_agent_action(
                owner_user_id,
                &turn.id,
                CreateAgentActionRequest {
                    kind: suggestion.kind,
                    title: suggestion.title,
                    description: suggestion.description,
                    payload: suggestion.payload,
                    resource_kind: suggestion.resource_kind,
                    resource_id: suggestion.resource_id,
                    requires_confirmation: suggestion.requires_confirmation,
                },
            )
            .await?;
        append_event(
            state,
            owner_user_id,
            &turn.id,
            AgentEventType::ActionProposed,
            json!({ "action": action }),
        )
        .await?;
    }

    if waiting_for_user {
        let waiting_turn = state
            .store
            .update_agent_turn_status(
                owner_user_id,
                &turn.id,
                AgentTurnStatus::WaitingForUser,
                None,
            )
            .await?;
        append_event(
            state,
            owner_user_id,
            &turn.id,
            AgentEventType::TurnWaitingForUser,
            json!({ "turn": waiting_turn }),
        )
        .await?;
    } else {
        state
            .store
            .update_agent_turn_status(owner_user_id, &turn.id, AgentTurnStatus::Completed, None)
            .await?;
        append_event(
            state,
            owner_user_id,
            &turn.id,
            AgentEventType::TurnCompleted,
            json!({ "status": "completed" }),
        )
        .await?;
    }

    Ok(())
}

fn assistant_query_result_tables(
    turn: &AgentTurn,
    steps: &[WorkbenchToolStep],
) -> Vec<AssistantQueryResultTable> {
    let Some(managed_database_id) = turn.managed_database_id.as_deref() else {
        return Vec::new();
    };

    steps
        .iter()
        .rev()
        .filter(|step| step.name == "pg_execute_readonly_sql" && step.succeeded)
        .filter_map(|step| assistant_query_result_table(managed_database_id, step))
        .take(MAX_ASSISTANT_QUERY_RESULT_TABLES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn assistant_query_result_table(
    managed_database_id: &str,
    step: &WorkbenchToolStep,
) -> Option<AssistantQueryResultTable> {
    let sql = step
        .arguments
        .get("sql")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|sql| !sql.is_empty())?
        .to_owned();
    let query_result =
        serde_json::from_str::<ReadonlyToolQueryResult>(&step.output.content).ok()?;

    Some(AssistantQueryResultTable {
        title: None,
        description: None,
        managed_database_id: managed_database_id.to_owned(),
        sql,
        result: DatapanelQueryResult {
            columns: query_result.columns,
            rows: query_result.rows,
            row_count: query_result.row_count,
            truncated: query_result.truncated,
            elapsed_ms: query_result.elapsed_ms,
            refreshed_at: OffsetDateTime::now_utc(),
        },
    })
}

async fn prepare_workbench_action(
    state: &ApiState,
    owner_user_id: &str,
    mut suggestion: liquid_agent::WorkbenchActionSuggestion,
) -> anyhow::Result<liquid_agent::WorkbenchActionSuggestion> {
    if suggestion.kind != AgentActionKind::CreateDatapanelCard {
        return Ok(suggestion);
    }

    let mut payload: CreateDatapanelCardActionPayload =
        serde_json::from_value(suggestion.payload.clone())
            .map_err(|error| anyhow::anyhow!("invalid Datapanel card action payload: {error}"))?;
    let result = materialize_datapanel_query(
        state,
        owner_user_id,
        &payload.managed_database_id,
        &payload.sql,
        payload.limit.unwrap_or(100),
    )
    .await
    .map_err(|error| anyhow::anyhow!("{error}"))?;

    ensure_chart_keys_available(payload.chart.as_ref(), &result)?;
    payload.layout = Some(default_card_layout(payload.kind));
    payload.result = Some(result);
    suggestion.payload = serde_json::to_value(payload)
        .map_err(|error| anyhow::anyhow!("failed to serialize Datapanel card payload: {error}"))?;

    Ok(suggestion)
}

async fn append_workbench_tool_step(
    state: &ApiState,
    owner_user_id: &str,
    turn: &AgentTurn,
    step: &WorkbenchToolStep,
) -> anyhow::Result<()> {
    let display = workbench_tool_display(&step.name, step.succeeded);
    let assistant_content = serde_json::to_string(&json!({
        "type": "assistant_tool_call",
        "tool_call": {
            "id": step.id,
            "name": step.name,
            "arguments": step.arguments,
        }
    }))
    .unwrap_or_else(|_| "assistant tool call".to_owned());
    state
        .store
        .append_agent_message(
            owner_user_id,
            &turn.conversation_id,
            Some(&turn.id),
            AgentMessageRole::Assistant,
            &assistant_content,
            Some(json!({
                "kind": "assistant_tool_call",
                "visibility": "timeline",
                "tool_call_id": step.id,
                "tool_name": step.name,
                "arguments": step.arguments,
            })),
        )
        .await?;
    append_event(
        state,
        owner_user_id,
        &turn.id,
        AgentEventType::ToolCallStarted,
        json!({
            "id": step.id,
            "name": step.name,
            "title": display.title,
            "summary": display.started_summary,
            "stage": display.stage,
        }),
    )
    .await?;
    append_event(
        state,
        owner_user_id,
        &turn.id,
        AgentEventType::ToolCallFinished,
        json!({
            "id": step.id,
            "name": step.name,
            "status": if step.succeeded { "succeeded" } else { "failed" },
            "summary": display.finished_summary,
            "elapsed_ms": step.elapsed_ms,
            "output_preview": tool_output_preview(&step.output.content),
            "next_summary": display.next_summary,
        }),
    )
    .await?;

    let tool_content = step.output.content.trim();
    state
        .store
        .append_agent_message(
            owner_user_id,
            &turn.conversation_id,
            Some(&turn.id),
            AgentMessageRole::Tool,
            if tool_content.is_empty() {
                "{}"
            } else {
                tool_content
            },
            Some(json!({
                "kind": "tool_result",
                "visibility": "timeline",
                "tool_call_id": step.id,
                "tool_name": step.name,
                "succeeded": step.succeeded,
                "elapsed_ms": step.elapsed_ms,
            })),
        )
        .await?;

    Ok(())
}

struct WorkbenchToolDisplay {
    stage: &'static str,
    title: &'static str,
    started_summary: &'static str,
    finished_summary: &'static str,
    next_summary: &'static str,
}

fn workbench_tool_display(name: &str, succeeded: bool) -> WorkbenchToolDisplay {
    let finished_summary = if succeeded {
        "Tool completed"
    } else {
        "Tool failed"
    };

    match name {
        "pg_execute_readonly_sql" => WorkbenchToolDisplay {
            stage: "executing_sql",
            title: "Run read-only SQL",
            started_summary: "Executing a read-only SQL query",
            finished_summary: if succeeded {
                "Read-only query completed"
            } else {
                finished_summary
            },
            next_summary: "Reading the query result",
        },
        "pg_explain_sql" => WorkbenchToolDisplay {
            stage: "loading_context",
            title: "Explain SQL",
            started_summary: "Inspecting the query plan",
            finished_summary: if succeeded {
                "Query plan ready"
            } else {
                finished_summary
            },
            next_summary: "Using the plan result",
        },
        "pg_list_schemas" => WorkbenchToolDisplay {
            stage: "loading_context",
            title: "List schemas",
            started_summary: "Loading database schemas",
            finished_summary: if succeeded {
                "Schemas loaded"
            } else {
                finished_summary
            },
            next_summary: "Checking what to inspect next",
        },
        "pg_list_relations" => WorkbenchToolDisplay {
            stage: "loading_context",
            title: "List relations",
            started_summary: "Loading tables and views",
            finished_summary: if succeeded {
                "Relations loaded"
            } else {
                finished_summary
            },
            next_summary: "Checking the returned relations",
        },
        "pg_describe_relation" => WorkbenchToolDisplay {
            stage: "loading_context",
            title: "Describe relation",
            started_summary: "Reading table structure",
            finished_summary: if succeeded {
                "Table structure loaded"
            } else {
                finished_summary
            },
            next_summary: "Using the schema details",
        },
        "propose_sql_operation" => WorkbenchToolDisplay {
            stage: "proposing_action",
            title: "Prepare SQL operation",
            started_summary: "Preparing a confirmed SQL operation",
            finished_summary: if succeeded {
                "SQL operation is ready for confirmation"
            } else {
                finished_summary
            },
            next_summary: "Waiting for confirmation",
        },
        "propose_datapanel_card_action" => WorkbenchToolDisplay {
            stage: "proposing_action",
            title: "Prepare Datapanel card",
            started_summary: "Preparing a Datapanel card confirmation",
            finished_summary: if succeeded {
                "Datapanel card is ready for confirmation"
            } else {
                finished_summary
            },
            next_summary: "Waiting for confirmation",
        },
        "propose_database_diagram_action" => WorkbenchToolDisplay {
            stage: "proposing_action",
            title: "Prepare database design",
            started_summary: "Preparing a database design confirmation",
            finished_summary: if succeeded {
                "Database design is ready for confirmation"
            } else {
                finished_summary
            },
            next_summary: "Waiting for confirmation",
        },
        "propose_sql_audit_decision" => WorkbenchToolDisplay {
            stage: "proposing_action",
            title: "Prepare audit decision",
            started_summary: "Preparing a confirmed audit decision",
            finished_summary: if succeeded {
                "Audit decision is ready for confirmation"
            } else {
                finished_summary
            },
            next_summary: "Waiting for confirmation",
        },
        _ => WorkbenchToolDisplay {
            stage: "thinking",
            title: "Run tool",
            started_summary: "Running a tool",
            finished_summary,
            next_summary: "Thinking through the result",
        },
    }
}

fn tool_output_preview(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    let preview = if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        summarize_tool_output_value(&value).unwrap_or_else(|| trimmed.to_owned())
    } else {
        trimmed.to_owned()
    };

    Some(truncate_preview(&preview, 220))
}

fn summarize_tool_output_value(value: &Value) -> Option<String> {
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Some(error.to_owned());
    }

    if let Some(row_count) = value.get("row_count").and_then(Value::as_u64) {
        let truncated = value
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        return Some(format!(
            "{row_count} row{} returned{}",
            if row_count == 1 { "" } else { "s" },
            if truncated { " (truncated)" } else { "" }
        ));
    }

    if let Some(relations) = value.get("relations").and_then(Value::as_array) {
        return Some(format!(
            "{} relation{} found",
            relations.len(),
            if relations.len() == 1 { "" } else { "s" }
        ));
    }

    if let Some(schemas) = value.get("schemas").and_then(Value::as_array) {
        return Some(format!(
            "{} schema{} found",
            schemas.len(),
            if schemas.len() == 1 { "" } else { "s" }
        ));
    }

    if value.get("columns").is_some() {
        return Some("Relation structure loaded".to_owned());
    }

    None
}

fn truncate_preview(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();

    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

pub(crate) async fn apply_agent_action(
    state: &ApiState,
    owner_user_id: &str,
    action: &AgentAction,
) -> Result<(AgentResourceKind, String, AgentEventType, Value), ApiError> {
    let started_at = Instant::now();
    let result = apply_agent_action_inner(state, owner_user_id, action).await;
    match &result {
        Ok((resource_kind, resource_id, _, _)) => {
            tracing::info!(
                action_id = %action.id,
                action_kind = %action.kind.as_str(),
                turn_id = %action.turn_id,
                conversation_id = %action.conversation_id,
                resource_kind = %resource_kind.as_str(),
                resource_id = %resource_id,
                elapsed_ms = started_at.elapsed().as_millis(),
                "agent action core execution completed"
            );
        }
        Err(error) => {
            tracing::error!(
                action_id = %action.id,
                action_kind = %action.kind.as_str(),
                turn_id = %action.turn_id,
                conversation_id = %action.conversation_id,
                error = %error,
                elapsed_ms = started_at.elapsed().as_millis(),
                "agent action core execution failed"
            );
        }
    }

    result
}

async fn apply_agent_action_inner(
    state: &ApiState,
    owner_user_id: &str,
    action: &AgentAction,
) -> Result<(AgentResourceKind, String, AgentEventType, Value), ApiError> {
    match action.kind {
        AgentActionKind::CreateSqlAudit => {
            apply_create_sql_audit_action(state, owner_user_id, action).await
        }
        AgentActionKind::CreateDatapanelCard => {
            let payload: CreateDatapanelCardActionPayload =
                serde_json::from_value(action.payload.clone())
                    .map_err(|error| ApiError::bad_request(error.to_string()))?;
            let result = payload.result.ok_or_else(|| {
                ApiError::bad_request(
                    "Datapanel card action does not include a materialized result",
                )
            })?;
            let panel = state
                .store
                .get_or_create_datapanel(owner_user_id, &action.conversation_id)
                .await?;
            let card = state
                .store
                .create_datapanel_card(
                    owner_user_id,
                    &panel.id,
                    CreateDatapanelCardRequest {
                        managed_database_id: payload.managed_database_id,
                        source_action_id: Some(action.id.clone()),
                        title: payload.title,
                        description: payload.description,
                        kind: payload.kind,
                        sql: payload.sql,
                        chart: payload.chart,
                        layout: next_card_layout(&panel, payload.kind),
                        result,
                    },
                )
                .await?;

            Ok(datapanel_card_result(card, AgentEventType::ResourceCreated))
        }
        AgentActionKind::CreateDatabaseDiagram => {
            apply_create_database_diagram_action(state, owner_user_id, action).await
        }
        AgentActionKind::ApproveSqlAudit => {
            let payload = sql_audit_payload(action)?;
            let record = state
                .store
                .approve_sql_audit(
                    owner_user_id,
                    &payload.sql_audit_id,
                    ApproveSqlAuditRequest {
                        comment: Some("Approved from agent workbench.".to_owned()),
                    },
                )
                .await?;

            Ok(sql_audit_result(record, AgentEventType::ResourceUpdated))
        }
        AgentActionKind::RejectSqlAudit => {
            let payload = sql_audit_payload(action)?;
            let record = state
                .store
                .reject_sql_audit(
                    owner_user_id,
                    &payload.sql_audit_id,
                    RejectSqlAuditRequest {
                        comment: Some("Rejected from agent workbench.".to_owned()),
                    },
                )
                .await?;

            Ok(sql_audit_result(record, AgentEventType::ResourceUpdated))
        }
        AgentActionKind::ExecuteSqlAudit => {
            let payload = sql_audit_payload(action)?;
            let outcome =
                execute_sql_audit_for_user(state, owner_user_id, &payload.sql_audit_id).await?;

            Ok(sql_audit_execution_result(
                outcome,
                AgentEventType::ResourceUpdated,
            ))
        }
        AgentActionKind::StartDatabaseRestore => {
            let payload: StartDatabaseRestoreActionPayload =
                serde_json::from_value(action.payload.clone())
                    .map_err(|error| ApiError::bad_request(error.to_string()))?;
            if !payload.confirm_destructive_restore {
                return Err(ApiError::bad_request(
                    "confirm_destructive_restore must be true",
                ));
            }
            let restore = state
                .database_backups
                .enqueue_database_restore(
                    owner_user_id,
                    EnqueueDatabaseRestore {
                        backup_id: payload.backup_id,
                        target_managed_database_id: payload.target_managed_database_id,
                        purpose: payload.purpose,
                        conversation_id: Some(action.conversation_id.clone()),
                        created_from_turn_id: Some(action.turn_id.clone()),
                    },
                )
                .await
                .map_err(|error| ApiError::internal(anyhow::anyhow!(error.to_string())))?;

            Ok((
                AgentResourceKind::DatabaseRestore,
                restore.id.clone(),
                AgentEventType::ResourceCreated,
                serde_json::json!({ "restore": restore }),
            ))
        }
        AgentActionKind::CreateManagedDatabase
        | AgentActionKind::UpdateManagedDatabase
        | AgentActionKind::DeleteManagedDatabase
        | AgentActionKind::StartDatabaseBackup => Err(ApiError::conflict(
            "this agent action kind is not supported by the workbench API yet",
        )),
    }
}

async fn apply_create_database_diagram_action(
    state: &ApiState,
    owner_user_id: &str,
    action: &AgentAction,
) -> Result<(AgentResourceKind, String, AgentEventType, Value), ApiError> {
    let payload: CreateDatabaseDiagramActionPayload =
        serde_json::from_value(action.payload.clone())
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let pool = state
        .managed_database_pools
        .get_pool(ManagedDatabasePoolKey::new(
            owner_user_id.to_owned(),
            payload.managed_database_id.clone(),
        ))
        .await?;
    let tool_id = format!("{}:database_diagram", action.id);
    let started_at = Instant::now();

    append_tool_started(
        state,
        owner_user_id,
        &action.turn_id,
        &tool_id,
        "database_diagram_generation",
        "Generate database design",
        "Reading PostgreSQL catalog metadata",
        "loading_context",
    )
    .await?;
    let document = match state.database_diagram_generator.generate(pool).await {
        Ok(document) => {
            append_tool_finished(
                state,
                owner_user_id,
                &action.turn_id,
                ToolFinishedPayload {
                    id: &tool_id,
                    name: "database_diagram_generation",
                    status: "succeeded",
                    summary: "Database design document generated",
                    elapsed_ms: started_at.elapsed().as_millis() as u64,
                    output_preview: Some(format!(
                        "{} table{} and {} relationship{}",
                        document.tables.len(),
                        if document.tables.len() == 1 { "" } else { "s" },
                        document.relationships.len(),
                        if document.relationships.len() == 1 {
                            ""
                        } else {
                            "s"
                        }
                    )),
                    next_summary: Some("Creating the database design record"),
                },
            )
            .await?;
            document
        }
        Err(error) => {
            let message = error.to_string();
            append_tool_finished(
                state,
                owner_user_id,
                &action.turn_id,
                ToolFinishedPayload {
                    id: &tool_id,
                    name: "database_diagram_generation",
                    status: "failed",
                    summary: "Database design generation failed",
                    elapsed_ms: started_at.elapsed().as_millis() as u64,
                    output_preview: Some(message.clone()),
                    next_summary: Some("Preparing the failure response"),
                },
            )
            .await?;
            return Err(ApiError::internal(anyhow::anyhow!(message)));
        }
    };
    let diagram = state
        .store
        .create_database_diagram(
            owner_user_id,
            CreateDatabaseDiagramRequest {
                title: payload.title,
                description: payload.description,
                document: Some(document),
            },
        )
        .await?;

    Ok(database_diagram_result(
        diagram,
        AgentEventType::ResourceCreated,
    ))
}

async fn apply_create_sql_audit_action(
    state: &ApiState,
    owner_user_id: &str,
    action: &AgentAction,
) -> Result<(AgentResourceKind, String, AgentEventType, Value), ApiError> {
    let payload: CreateSqlAuditActionPayload = serde_json::from_value(action.payload.clone())
        .map_err(|error| ApiError::bad_request(error.to_string()))?;

    let audit_tool_id = format!("{}:sql_audit", action.id);
    let audit_started_at = Instant::now();
    append_tool_started(
        state,
        owner_user_id,
        &action.turn_id,
        &audit_tool_id,
        "sql_audit",
        "Audit SQL",
        "Checking SQL safety and policy",
        "auditing_sql",
    )
    .await?;
    let record = match create_sql_audit_for_user(
        state,
        owner_user_id,
        &payload.managed_database_id,
        payload.request,
    )
    .await
    {
        Ok(record) => {
            append_tool_finished(
                state,
                owner_user_id,
                &action.turn_id,
                ToolFinishedPayload {
                    id: &audit_tool_id,
                    name: "sql_audit",
                    status: "succeeded",
                    summary: "SQL audit completed",
                    elapsed_ms: audit_started_at.elapsed().as_millis() as u64,
                    output_preview: Some(format!(
                        "Audit {} is {} with risk {}/100",
                        record.id,
                        record.status.as_str(),
                        record.risk_score
                    )),
                    next_summary: Some("Continuing with the confirmed action"),
                },
            )
            .await?;
            record
        }
        Err(error) => {
            append_tool_finished(
                state,
                owner_user_id,
                &action.turn_id,
                ToolFinishedPayload {
                    id: &audit_tool_id,
                    name: "sql_audit",
                    status: "failed",
                    summary: "SQL audit failed",
                    elapsed_ms: audit_started_at.elapsed().as_millis() as u64,
                    output_preview: Some(error.to_string()),
                    next_summary: Some("Preparing the failure response"),
                },
            )
            .await?;
            return Err(error);
        }
    };

    if !matches!(record.status, SqlAuditStatus::PendingApproval) {
        return Ok(sql_audit_result(record, AgentEventType::ResourceCreated));
    }

    let execute_tool_id = format!("{}:sql_execute", action.id);
    let execute_started_at = Instant::now();
    append_tool_started(
        state,
        owner_user_id,
        &action.turn_id,
        &execute_tool_id,
        "sql_execute",
        "Execute SQL",
        "Executing the approved SQL operation",
        "executing_sql",
    )
    .await?;
    let approved = state
        .store
        .approve_sql_audit(
            owner_user_id,
            &record.id,
            ApproveSqlAuditRequest {
                comment: Some("Approved from confirmed chat action.".to_owned()),
            },
        )
        .await?;
    let outcome = match execute_sql_audit_for_user(state, owner_user_id, &approved.id).await {
        Ok(outcome) => {
            append_tool_finished(
                state,
                owner_user_id,
                &action.turn_id,
                ToolFinishedPayload {
                    id: &execute_tool_id,
                    name: "sql_execute",
                    status: "succeeded",
                    summary: "SQL execution completed",
                    elapsed_ms: execute_started_at.elapsed().as_millis() as u64,
                    output_preview: Some(sql_execution_output_preview(&outcome)),
                    next_summary: Some("Preparing the final response"),
                },
            )
            .await?;
            outcome
        }
        Err(error) => {
            append_tool_finished(
                state,
                owner_user_id,
                &action.turn_id,
                ToolFinishedPayload {
                    id: &execute_tool_id,
                    name: "sql_execute",
                    status: "failed",
                    summary: "SQL execution failed",
                    elapsed_ms: execute_started_at.elapsed().as_millis() as u64,
                    output_preview: Some(error.to_string()),
                    next_summary: Some("Preparing the failure response"),
                },
            )
            .await?;
            return Err(error);
        }
    };

    Ok(sql_audit_execution_result(
        outcome,
        AgentEventType::ResourceUpdated,
    ))
}

pub(crate) async fn synthesize_action_observation(
    state: &ApiState,
    owner_user_id: &str,
    action: &AgentAction,
    observation: Value,
) -> anyhow::Result<()> {
    append_event(
        state,
        owner_user_id,
        &action.turn_id,
        AgentEventType::ToolCallStarted,
        json!({
            "name": "synthesize_observation",
            "stage": "synthesizing",
            "summary": "Preparing the final response"
        }),
    )
    .await?;

    let Some(provider) = user_llm_provider_for_user(state, owner_user_id).await? else {
        anyhow::bail!(MISSING_LLM_PROVIDER_MESSAGE);
    };
    let turn = state
        .store
        .get_agent_turn(owner_user_id, &action.turn_id)
        .await?;
    let context = load_llm_workbench_context(state, owner_user_id, &turn).await?;
    let streaming_message_id = format!("stream-{}", action.turn_id);
    let agent = LlmWorkbenchAgent::new(provider.client, provider.model, provider.protocol)
        .with_max_tool_rounds(state.workbench.max_tool_rounds)
        .with_max_output_tokens(state.workbench.max_output_tokens)
        .with_streaming_enabled(provider.streaming_enabled);
    let response = agent
        .synthesize_observation_with_text_delta(context, observation, {
            let state = state.clone();
            let owner_user_id = owner_user_id.to_owned();
            let turn_id = action.turn_id.clone();
            move |delta| {
                let state = state.clone();
                let owner_user_id = owner_user_id.clone();
                let turn_id = turn_id.clone();
                let streaming_message_id = streaming_message_id.clone();
                async move {
                    if let Err(error) = append_event(
                        &state,
                        &owner_user_id,
                        &turn_id,
                        AgentEventType::AssistantDelta,
                        json!({
                            "message_id": streaming_message_id,
                            "content": delta,
                            "append": true,
                        }),
                    )
                    .await
                    {
                        tracing::warn!(
                            turn_id = %turn_id,
                            error = %error,
                            "failed to append streamed assistant delta"
                        );
                    }
                }
            }
        })
        .await
        .context("LLM observation synthesis failed")?;

    persist_workbench_response(state, owner_user_id, &turn, response)
        .await
        .context("failed to persist synthesized workbench response")?;

    Ok(())
}

async fn load_llm_workbench_context(
    state: &ApiState,
    owner_user_id: &str,
    turn: &AgentTurn,
) -> anyhow::Result<LlmWorkbenchContext> {
    let databases = state.store.list_managed_databases(owner_user_id).await?;
    let summary = state.agent.audit_summary().await.ok();
    let selected_sql_audit_id = turn
        .dashboard_context
        .as_ref()
        .and_then(|context| context.selected_sql_audit_id.clone());
    let recent_sql_audits = state
        .store
        .list_sql_audits(
            owner_user_id,
            recent_sql_audits_filter(turn.managed_database_id.as_deref()),
        )
        .await?
        .records;
    let recent_actions = state
        .store
        .list_agent_actions(owner_user_id, Some(&turn.conversation_id), None)
        .await?;
    let messages = state
        .store
        .list_agent_messages(owner_user_id, &turn.conversation_id, 40, None)
        .await?;
    let managed_database = turn
        .managed_database_id
        .as_deref()
        .and_then(|managed_database_id| {
            databases
                .iter()
                .find(|database| database.id == managed_database_id)
                .cloned()
        });

    Ok(LlmWorkbenchContext {
        messages,
        managed_database,
        write_sql_execution_enabled: state.approved_write_execution_enabled,
        selected_sql_audit_id,
        audit_summary: summary,
        recent_sql_audits,
        recent_actions,
    })
}

struct ToolFinishedPayload<'a> {
    id: &'a str,
    name: &'a str,
    status: &'a str,
    summary: &'a str,
    elapsed_ms: u64,
    output_preview: Option<String>,
    next_summary: Option<&'a str>,
}

async fn append_tool_started(
    state: &ApiState,
    owner_user_id: &str,
    turn_id: &str,
    id: &str,
    name: &str,
    title: &str,
    summary: &str,
    stage: &str,
) -> Result<(), ApiError> {
    append_event(
        state,
        owner_user_id,
        turn_id,
        AgentEventType::ToolCallStarted,
        json!({
            "id": id,
            "name": name,
            "title": title,
            "summary": summary,
            "stage": stage,
        }),
    )
    .await?;

    Ok(())
}

async fn append_tool_finished(
    state: &ApiState,
    owner_user_id: &str,
    turn_id: &str,
    payload: ToolFinishedPayload<'_>,
) -> Result<(), ApiError> {
    append_event(
        state,
        owner_user_id,
        turn_id,
        AgentEventType::ToolCallFinished,
        json!({
            "id": payload.id,
            "name": payload.name,
            "status": payload.status,
            "summary": payload.summary,
            "elapsed_ms": payload.elapsed_ms,
            "output_preview": payload.output_preview,
            "next_summary": payload.next_summary,
        }),
    )
    .await?;

    Ok(())
}

fn sql_execution_output_preview(outcome: &SqlAuditExecutionOutcome) -> String {
    if let Some(result) = &outcome.record.execution_result {
        return format!(
            "{} completed; affected rows: {}",
            result.statement_kind.as_str(),
            result.affected_rows
        );
    }

    if let Some(query_result) = &outcome.query_result {
        return format!("{} rows returned", query_result.row_count);
    }

    format!(
        "Audit {} is {}",
        outcome.record.id,
        outcome.record.status.as_str()
    )
}

fn ensure_chart_keys_available(
    chart: Option<&DatapanelChartConfig>,
    result: &DatapanelQueryResult,
) -> anyhow::Result<()> {
    let Some(chart) = chart else {
        return Ok(());
    };
    let y_keys = chart.y_keys.as_deref().unwrap_or(&[]);
    let series = chart.series.as_deref().unwrap_or(&[]);
    let group_keys = chart.group_keys.as_deref().unwrap_or(&[]);

    match chart.chart_type {
        DatapanelChartType::Line
        | DatapanelChartType::Bar
        | DatapanelChartType::Area
        | DatapanelChartType::Pie
        | DatapanelChartType::Scatter
        | DatapanelChartType::Radar
        | DatapanelChartType::RadialBar
        | DatapanelChartType::Funnel => {
            ensure_required_chart_column("x_key", chart.x_key.as_deref(), result)?;
            ensure_chart_columns("y_key", y_keys, result)?;
        }
        DatapanelChartType::Composed => {
            ensure_required_chart_column("x_key", chart.x_key.as_deref(), result)?;

            if series.is_empty() {
                anyhow::bail!("datapanel chart series is required");
            }

            for item in series {
                ensure_chart_column("series.key", &item.key, result)?;
            }
        }
        DatapanelChartType::Treemap | DatapanelChartType::Sunburst => {
            ensure_chart_columns("group_key", group_keys, result)?;
            ensure_required_chart_column("value_key", chart.value_key.as_deref(), result)?;
        }
    }

    if let Some(z_key) = &chart.z_key {
        ensure_chart_column("z_key", z_key, result)?;
    }

    for key in y_keys {
        ensure_chart_column("y_key", key, result)?;
    }

    Ok(())
}

fn ensure_required_chart_column(
    field: &str,
    key: Option<&str>,
    result: &DatapanelQueryResult,
) -> anyhow::Result<()> {
    let Some(key) = key else {
        anyhow::bail!("datapanel chart {field} is required");
    };

    ensure_chart_column(field, key, result)
}

fn ensure_chart_columns(
    field: &str,
    keys: &[String],
    result: &DatapanelQueryResult,
) -> anyhow::Result<()> {
    if keys.is_empty() {
        anyhow::bail!("datapanel chart {field} is required");
    }

    for key in keys {
        ensure_chart_column(field, key, result)?;
    }

    Ok(())
}

fn ensure_chart_column(
    field: &str,
    key: &str,
    result: &DatapanelQueryResult,
) -> anyhow::Result<()> {
    if !result.columns.iter().any(|column| column == key) {
        anyhow::bail!("datapanel chart {field} is not present in query results: {key}");
    }

    Ok(())
}

fn default_card_layout(kind: DatapanelCardKind) -> DatapanelCardLayout {
    match kind {
        DatapanelCardKind::Table => DatapanelCardLayout {
            x: 0,
            y: 0,
            w: 12,
            h: 5,
        },
        DatapanelCardKind::Chart => DatapanelCardLayout {
            x: 0,
            y: 0,
            w: 6,
            h: 5,
        },
    }
}

fn next_card_layout(panel: &Datapanel, kind: DatapanelCardKind) -> DatapanelCardLayout {
    let mut layout = default_card_layout(kind);
    layout.y = panel
        .cards
        .iter()
        .map(|card| card.layout.y + card.layout.h)
        .max()
        .unwrap_or(0);
    layout
}

fn sql_audit_payload(action: &AgentAction) -> Result<SqlAuditActionPayload, ApiError> {
    serde_json::from_value(action.payload.clone())
        .map_err(|error| ApiError::bad_request(error.to_string()))
}

fn sql_audit_result(
    record: SqlAuditRecord,
    event_type: AgentEventType,
) -> (AgentResourceKind, String, AgentEventType, Value) {
    (
        AgentResourceKind::SqlAudit,
        record.id.clone(),
        event_type,
        json!({
            "resource_kind": "sql_audit",
            "resource_id": record.id,
            "record": record,
        }),
    )
}

fn sql_audit_execution_result(
    outcome: SqlAuditExecutionOutcome,
    event_type: AgentEventType,
) -> (AgentResourceKind, String, AgentEventType, Value) {
    let SqlAuditExecutionOutcome {
        record,
        query_result,
    } = outcome;
    (
        AgentResourceKind::SqlAudit,
        record.id.clone(),
        event_type,
        json!({
            "resource_kind": "sql_audit",
            "resource_id": record.id,
            "record": record,
            "query_result": query_result,
        }),
    )
}

fn datapanel_card_result(
    card: DatapanelCard,
    event_type: AgentEventType,
) -> (AgentResourceKind, String, AgentEventType, Value) {
    (
        AgentResourceKind::DatapanelCard,
        card.id.clone(),
        event_type,
        json!({
            "resource_kind": "datapanel_card",
            "resource_id": card.id,
            "record": card,
        }),
    )
}

fn database_diagram_result(
    diagram: DatabaseDiagram,
    event_type: AgentEventType,
) -> (AgentResourceKind, String, AgentEventType, Value) {
    (
        AgentResourceKind::DatabaseDiagram,
        diagram.id.clone(),
        event_type,
        json!({
            "resource_kind": "database_diagram",
            "resource_id": diagram.id,
            "record": diagram,
        }),
    )
}

pub(crate) async fn append_event(
    state: &ApiState,
    owner_user_id: &str,
    turn_id: &str,
    event_type: AgentEventType,
    payload: Value,
) -> Result<AgentEventRecord, ApiError> {
    state
        .store
        .append_agent_turn_event(owner_user_id, turn_id, event_type, payload)
        .await
        .map_err(ApiError::from)
}

#[cfg(test)]
mod tests {
    use liquid_agent::ToolOutput;
    use liquid_core::{DatapanelChartSeries, DatapanelChartSeriesKind};
    use time::OffsetDateTime;

    use super::*;

    #[test]
    fn assistant_query_result_tables_extract_successful_readonly_tool_results() {
        let turn = AgentTurn {
            id: "turn-1".to_owned(),
            conversation_id: "conversation-1".to_owned(),
            status: AgentTurnStatus::Running,
            user_message_id: "message-1".to_owned(),
            assistant_message_id: None,
            error: None,
            client_request_id: None,
            managed_database_id: Some("db-1".to_owned()),
            dashboard_context: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            completed_at: None,
        };
        let step = WorkbenchToolStep {
            id: "call-1".to_owned(),
            name: "pg_execute_readonly_sql".to_owned(),
            arguments: json!({
                "sql": "select id, event_type from agent_events order by id limit 2",
                "limit": 100
            }),
            output: ToolOutput::json(json!({
                "columns": ["id", "event_type"],
                "rows": [
                    { "id": 1, "event_type": "turn_started" },
                    { "id": 2, "event_type": "message_created" }
                ],
                "row_count": 2,
                "truncated": false,
                "elapsed_ms": 4
            })),
            succeeded: true,
            elapsed_ms: 4,
            proposal: None,
        };

        let tables = assistant_query_result_tables(&turn, &[step]);

        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].managed_database_id, "db-1");
        assert_eq!(
            tables[0].sql,
            "select id, event_type from agent_events order by id limit 2"
        );
        assert_eq!(tables[0].result.row_count, 2);
        assert_eq!(tables[0].result.columns, vec!["id", "event_type"]);
    }

    #[test]
    fn chart_key_validation_accepts_scatter_z_key() {
        let result = chart_result(vec!["day", "risk_count", "risk_weight"]);
        let chart = DatapanelChartConfig {
            chart_type: DatapanelChartType::Scatter,
            x_key: Some("day".to_owned()),
            y_keys: Some(vec!["risk_count".to_owned()]),
            z_key: Some("risk_weight".to_owned()),
            series: None,
            group_keys: None,
            value_key: None,
        };

        ensure_chart_keys_available(Some(&chart), &result).unwrap();
    }

    #[test]
    fn chart_key_validation_rejects_missing_composed_series_key() {
        let result = chart_result(vec!["day", "revenue"]);
        let chart = DatapanelChartConfig {
            chart_type: DatapanelChartType::Composed,
            x_key: Some("day".to_owned()),
            y_keys: Some(vec!["revenue".to_owned(), "cost".to_owned()]),
            z_key: None,
            series: Some(vec![
                DatapanelChartSeries {
                    key: "revenue".to_owned(),
                    kind: DatapanelChartSeriesKind::Bar,
                },
                DatapanelChartSeries {
                    key: "cost".to_owned(),
                    kind: DatapanelChartSeriesKind::Line,
                },
            ]),
            group_keys: None,
            value_key: None,
        };

        let error = ensure_chart_keys_available(Some(&chart), &result).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("series.key is not present in query results: cost")
        );
    }

    #[test]
    fn chart_key_validation_accepts_hierarchy_keys() {
        let result = chart_result(vec!["region", "product", "revenue"]);
        let chart = DatapanelChartConfig {
            chart_type: DatapanelChartType::Sunburst,
            x_key: None,
            y_keys: None,
            z_key: None,
            series: None,
            group_keys: Some(vec!["region".to_owned(), "product".to_owned()]),
            value_key: Some("revenue".to_owned()),
        };

        ensure_chart_keys_available(Some(&chart), &result).unwrap();
    }

    #[test]
    fn chart_key_validation_rejects_missing_hierarchy_value_key() {
        let result = chart_result(vec!["region", "product"]);
        let chart = DatapanelChartConfig {
            chart_type: DatapanelChartType::Treemap,
            x_key: None,
            y_keys: None,
            z_key: None,
            series: None,
            group_keys: Some(vec!["region".to_owned(), "product".to_owned()]),
            value_key: Some("revenue".to_owned()),
        };

        let error = ensure_chart_keys_available(Some(&chart), &result).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("value_key is not present in query results: revenue")
        );
    }

    fn chart_result(columns: Vec<&str>) -> DatapanelQueryResult {
        DatapanelQueryResult {
            columns: columns.into_iter().map(str::to_owned).collect(),
            rows: vec![],
            row_count: 0,
            truncated: false,
            elapsed_ms: 1,
            refreshed_at: OffsetDateTime::UNIX_EPOCH,
        }
    }
}
