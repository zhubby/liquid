use liquid_agent::{LlmWorkbenchAgent, LlmWorkbenchContext, WorkbenchResponse};
use liquid_core::{
    AgentAction, AgentActionKind, AgentEventRecord, AgentEventType, AgentMessageRole,
    AgentResourceKind, AgentTurn, AgentTurnStatus, ApproveSqlAuditRequest,
    CreateAgentActionRequest, CreateSqlAuditRequest, PublicUser, RejectSqlAuditRequest,
    SqlAuditRecord,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
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
