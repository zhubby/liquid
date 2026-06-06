use liquid_agent::{LlmWorkbenchAgent, LlmWorkbenchContext, WorkbenchResponse};
use liquid_core::{
    AgentAction, AgentActionKind, AgentEventRecord, AgentEventType, AgentMessageRole,
    AgentResourceKind, AgentTurn, AgentTurnStatus, ApproveSqlAuditRequest, BiCardKind,
    BiCardLayout, BiChartConfig, BiPanel, BiPanelCard, BiQueryResult, CreateAgentActionRequest,
    CreateBiPanelCardRequest, CreateSqlAuditRequest, PublicUser, RejectSqlAuditRequest,
    SqlAuditRecord,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    bi_panels::materialize_bi_query,
    error::ApiError,
    llm_provider::user_llm_provider_for_user,
    sql_audits::{create_sql_audit_for_user, execute_sql_audit_for_user},
    state::ApiState,
};

const MISSING_LLM_PROVIDER_MESSAGE: &str = "LLM provider is not configured. Configure a provider and API key before using AI workbench chat.";

#[derive(Debug, Deserialize)]
struct CreateSqlAuditActionPayload {
    managed_database_id: String,
    request: CreateSqlAuditRequest,
}

#[derive(Debug, Deserialize)]
struct SqlAuditActionPayload {
    sql_audit_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct CreateBiCardActionPayload {
    pub(crate) managed_database_id: String,
    pub(crate) title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
    pub(crate) kind: BiCardKind,
    pub(crate) sql: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) chart: Option<BiChartConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) layout: Option<BiCardLayout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) result: Option<BiQueryResult>,
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
        json!({ "name": "load_workbench_context" }),
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
        .list_sql_audits(&user.id, turn.managed_database_id.as_deref(), None, 20)
        .await?;
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
    let agent = LlmWorkbenchAgent::new(provider.client, provider.model, provider.protocol);
    let response = agent
        .respond(LlmWorkbenchContext {
            messages,
            managed_database,
            selected_sql_audit_id,
            audit_summary: summary,
            recent_sql_audits,
            recent_actions,
        })
        .await?;

    let latest_turn = state.store.get_agent_turn(&user.id, &turn_id).await?;
    if latest_turn.status.is_terminal() {
        return Ok(());
    }

    persist_workbench_response(&state, &user.id, &turn, response).await?;

    Ok(())
}

async fn persist_workbench_response(
    state: &ApiState,
    owner_user_id: &str,
    turn: &AgentTurn,
    response: WorkbenchResponse,
) -> anyhow::Result<()> {
    for suggestion in response.actions {
        let suggestion = prepare_workbench_action(state, owner_user_id, suggestion).await?;
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

    let assistant_message = state
        .store
        .append_agent_message(
            owner_user_id,
            &turn.conversation_id,
            Some(&turn.id),
            AgentMessageRole::Assistant,
            &response.content,
            None,
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
        json!({ "content": response.content }),
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

    Ok(())
}

async fn prepare_workbench_action(
    state: &ApiState,
    owner_user_id: &str,
    mut suggestion: liquid_agent::WorkbenchActionSuggestion,
) -> anyhow::Result<liquid_agent::WorkbenchActionSuggestion> {
    if suggestion.kind != AgentActionKind::CreateBiCard {
        return Ok(suggestion);
    }

    let mut payload: CreateBiCardActionPayload = serde_json::from_value(suggestion.payload.clone())
        .map_err(|error| anyhow::anyhow!("invalid BI card action payload: {error}"))?;
    let result = materialize_bi_query(
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
        .map_err(|error| anyhow::anyhow!("failed to serialize BI card payload: {error}"))?;

    Ok(suggestion)
}

pub(crate) async fn apply_agent_action(
    state: &ApiState,
    owner_user_id: &str,
    action: &AgentAction,
) -> Result<(AgentResourceKind, String, AgentEventType, Value), ApiError> {
    match action.kind {
        AgentActionKind::CreateSqlAudit => {
            let payload: CreateSqlAuditActionPayload =
                serde_json::from_value(action.payload.clone())
                    .map_err(|error| ApiError::bad_request(error.to_string()))?;
            let record = create_sql_audit_for_user(
                state,
                owner_user_id,
                &payload.managed_database_id,
                payload.request,
            )
            .await?;

            Ok(sql_audit_result(record, AgentEventType::ResourceCreated))
        }
        AgentActionKind::CreateBiCard => {
            let payload: CreateBiCardActionPayload = serde_json::from_value(action.payload.clone())
                .map_err(|error| ApiError::bad_request(error.to_string()))?;
            let result = payload.result.ok_or_else(|| {
                ApiError::bad_request("BI card action does not include a materialized result")
            })?;
            let panel = state
                .store
                .get_or_create_bi_panel(owner_user_id, &action.conversation_id)
                .await?;
            let card = state
                .store
                .create_bi_panel_card(
                    owner_user_id,
                    &panel.id,
                    CreateBiPanelCardRequest {
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

            Ok(bi_card_result(card, AgentEventType::ResourceCreated))
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
            let record =
                execute_sql_audit_for_user(state, owner_user_id, &payload.sql_audit_id).await?;

            Ok(sql_audit_result(record, AgentEventType::ResourceUpdated))
        }
        AgentActionKind::CreateManagedDatabase
        | AgentActionKind::UpdateManagedDatabase
        | AgentActionKind::DeleteManagedDatabase
        | AgentActionKind::StartDatabaseBackup
        | AgentActionKind::StartDatabaseRestore => Err(ApiError::conflict(
            "this agent action kind is not supported by the workbench API yet",
        )),
    }
}

fn ensure_chart_keys_available(
    chart: Option<&BiChartConfig>,
    result: &BiQueryResult,
) -> anyhow::Result<()> {
    let Some(chart) = chart else {
        return Ok(());
    };

    if !result.columns.iter().any(|column| column == &chart.x_key) {
        anyhow::bail!("BI chart x_key is not present in query results");
    }

    for key in &chart.y_keys {
        if !result.columns.iter().any(|column| column == key) {
            anyhow::bail!("BI chart y_key is not present in query results: {key}");
        }
    }

    Ok(())
}

fn default_card_layout(kind: BiCardKind) -> BiCardLayout {
    match kind {
        BiCardKind::Table => BiCardLayout {
            x: 0,
            y: 0,
            w: 12,
            h: 5,
        },
        BiCardKind::Chart => BiCardLayout {
            x: 0,
            y: 0,
            w: 6,
            h: 5,
        },
    }
}

fn next_card_layout(panel: &BiPanel, kind: BiCardKind) -> BiCardLayout {
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

fn bi_card_result(
    card: BiPanelCard,
    event_type: AgentEventType,
) -> (AgentResourceKind, String, AgentEventType, Value) {
    (
        AgentResourceKind::BiPanelCard,
        card.id.clone(),
        event_type,
        json!({
            "resource_kind": "bi_panel_card",
            "resource_id": card.id,
            "record": card,
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
