use std::{convert::Infallible, time::Duration};

use async_stream::stream;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
};
use futures_util::Stream;
use liquid_agent::{RuleBasedWorkbenchAgent, WorkbenchContext};
use liquid_core::{
    AgentAction, AgentActionDecisionRequest, AgentActionKind, AgentActionStatus,
    AgentCapabilitiesResponse, AgentCapability, AgentConversation, AgentEventRecord,
    AgentEventType, AgentMessage, AgentMessageRole, AgentResourceKind, AgentTurn, AgentTurnStatus,
    ApproveSqlAuditRequest, CreateAgentActionRequest, CreateAgentConversationRequest,
    CreateAgentTurnRequest, CreateSqlAuditRequest, PublicUser, RejectSqlAuditRequest,
    SqlAuditRecord,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::sleep;

use crate::{
    auth::authenticated_user,
    error::ApiError,
    sql_audits::{create_sql_audit_for_user, execute_sql_audit_for_user},
    state::ApiState,
};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/api/v1/agent/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route(
            "/api/v1/agent/conversations/{conversation_id}",
            get(get_conversation).patch(update_conversation),
        )
        .route(
            "/api/v1/agent/conversations/{conversation_id}/messages",
            get(list_messages),
        )
        .route(
            "/api/v1/agent/conversations/{conversation_id}/turns",
            post(create_turn),
        )
        .route(
            "/api/v1/agent/turns/{turn_id}/events",
            get(stream_turn_events),
        )
        .route("/api/v1/agent/turns/{turn_id}/cancel", post(cancel_turn))
        .route("/api/v1/agent/actions", get(list_actions))
        .route(
            "/api/v1/agent/actions/{action_id}/apply",
            post(apply_action),
        )
        .route(
            "/api/v1/agent/actions/{action_id}/reject",
            post(reject_action),
        )
        .route("/api/v1/agent/capabilities", get(capabilities))
}

#[derive(Debug, Deserialize)]
struct ListConversationsQuery {
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ListMessagesQuery {
    limit: Option<i64>,
    before: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListActionsQuery {
    conversation_id: Option<String>,
    status: Option<AgentActionStatus>,
}

#[derive(Debug, Deserialize)]
struct EventsQuery {
    after_seq: Option<i32>,
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

async fn list_conversations(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<ListConversationsQuery>,
) -> Result<Json<Vec<AgentConversation>>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let conversations = state
        .store
        .list_agent_conversations(&user.id, query.limit.unwrap_or(50))
        .await?;

    Ok(Json(conversations))
}

async fn create_conversation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateAgentConversationRequest>,
) -> Result<Json<AgentConversation>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let conversation = state
        .store
        .create_agent_conversation(&user.id, request)
        .await?;

    Ok(Json(conversation))
}

async fn get_conversation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
) -> Result<Json<AgentConversation>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let conversation = state
        .store
        .get_agent_conversation(&user.id, &conversation_id)
        .await?;

    Ok(Json(conversation))
}

async fn update_conversation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Json(request): Json<liquid_core::UpdateAgentConversationRequest>,
) -> Result<Json<AgentConversation>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let conversation = state
        .store
        .update_agent_conversation(&user.id, &conversation_id, request)
        .await?;

    Ok(Json(conversation))
}

async fn list_messages(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<Vec<AgentMessage>>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let messages = state
        .store
        .list_agent_messages(
            &user.id,
            &conversation_id,
            query.limit.unwrap_or(100),
            query.before.as_deref(),
        )
        .await?;

    Ok(Json(messages))
}

async fn create_turn(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Json(request): Json<CreateAgentTurnRequest>,
) -> Result<Json<AgentTurn>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let turn = state
        .store
        .create_agent_turn(&user.id, &conversation_id, request)
        .await?;
    let runner_state = state.clone();
    let runner_user = user.clone();
    let runner_turn_id = turn.id.clone();

    tokio::spawn(async move {
        if let Err(error) = run_agent_turn(runner_state, runner_user, runner_turn_id).await {
            tracing::warn!(error = %error, "agent turn runner failed");
        }
    });

    Ok(Json(turn))
}

async fn stream_turn_events(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(turn_id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    state.store.get_agent_turn(&user.id, &turn_id).await?;
    let store = state.store.clone();
    let owner_user_id = user.id;
    let mut after_seq = query.after_seq.unwrap_or(0).max(0);

    let events = stream! {
        loop {
            let next_events = store
                .list_agent_turn_events(&owner_user_id, &turn_id, after_seq)
                .await;

            match next_events {
                Ok(next_events) => {
                    for event_record in next_events {
                        after_seq = event_record.seq;
                        let data = serde_json::to_string(&event_record)
                            .unwrap_or_else(|_| "{}".to_owned());
                        yield Ok(Event::default().event("agent.event").data(data));
                    }
                }
                Err(error) => {
                    let data = json!({
                        "seq": after_seq + 1,
                        "turn_id": turn_id,
                        "type": "turn_failed",
                        "payload": { "error": error.to_string() },
                    })
                    .to_string();
                    yield Ok(Event::default().event("agent.event").data(data));
                    break;
                }
            }

            let turn = store.get_agent_turn(&owner_user_id, &turn_id).await;
            if turn
                .map(|turn| turn.status.is_terminal())
                .unwrap_or(true)
            {
                break;
            }

            yield Ok(Event::default().event("ping").data("{}"));
            sleep(Duration::from_millis(750)).await;
        }
    };

    Ok(Sse::new(events).keep_alive(KeepAlive::default()))
}

async fn cancel_turn(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(turn_id): Path<String>,
) -> Result<Json<AgentTurn>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let turn = state
        .store
        .update_agent_turn_status(&user.id, &turn_id, AgentTurnStatus::Cancelled, None)
        .await?;
    append_event(
        &state,
        &user.id,
        &turn_id,
        AgentEventType::TurnFailed,
        json!({ "status": "cancelled" }),
    )
    .await?;

    Ok(Json(turn))
}

async fn list_actions(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<ListActionsQuery>,
) -> Result<Json<Vec<AgentAction>>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let actions = state
        .store
        .list_agent_actions(&user.id, query.conversation_id.as_deref(), query.status)
        .await?;

    Ok(Json(actions))
}

async fn apply_action(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(action_id): Path<String>,
    _request: Option<Json<AgentActionDecisionRequest>>,
) -> Result<Json<AgentAction>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let action = state.store.get_agent_action(&user.id, &action_id).await?;
    let applied = match apply_agent_action(&state, &user.id, &action).await {
        Ok((resource_kind, resource_id, event_type, payload)) => {
            let applied = state
                .store
                .update_agent_action_status(
                    &user.id,
                    &action.id,
                    AgentActionStatus::Applied,
                    Some(resource_kind),
                    Some(resource_id),
                )
                .await?;
            append_event(&state, &user.id, &action.turn_id, event_type, payload).await?;
            applied
        }
        Err(error) => {
            let _ = state
                .store
                .update_agent_action_status(
                    &user.id,
                    &action.id,
                    AgentActionStatus::Failed,
                    None,
                    None,
                )
                .await;
            return Err(error);
        }
    };

    Ok(Json(applied))
}

async fn reject_action(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(action_id): Path<String>,
    _request: Option<Json<AgentActionDecisionRequest>>,
) -> Result<Json<AgentAction>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let action = state
        .store
        .update_agent_action_status(
            &user.id,
            &action_id,
            AgentActionStatus::Rejected,
            None,
            None,
        )
        .await?;
    append_event(
        &state,
        &user.id,
        &action.turn_id,
        AgentEventType::ResourceUpdated,
        json!({
            "action_id": action.id,
            "status": "rejected",
        }),
    )
    .await?;

    Ok(Json(action))
}

async fn capabilities(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<AgentCapabilitiesResponse>, ApiError> {
    let _user = authenticated_user(&state, &headers).await?;
    let mode = if state.approved_write_execution_enabled {
        "write_gated"
    } else {
        "readonly"
    };

    Ok(Json(AgentCapabilitiesResponse {
        mode: mode.to_owned(),
        capabilities: vec![
            AgentCapability {
                name: "read_dashboard".to_owned(),
                description: "Read audit summary and managed database context.".to_owned(),
                requires_confirmation: false,
            },
            AgentCapability {
                name: "create_sql_audit".to_owned(),
                description: "Prepare a persisted SQL audit for a selected managed database."
                    .to_owned(),
                requires_confirmation: true,
            },
            AgentCapability {
                name: "approve_or_reject_sql_audit".to_owned(),
                description: "Prepare approval or rejection actions for selected SQL audits."
                    .to_owned(),
                requires_confirmation: true,
            },
            AgentCapability {
                name: "execute_sql_audit".to_owned(),
                description: "Execute already-approved SQL audits only through write-gated checks."
                    .to_owned(),
                requires_confirmation: true,
            },
        ],
    }))
}

async fn run_agent_turn(state: ApiState, user: PublicUser, turn_id: String) -> anyhow::Result<()> {
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

    let user_message = state
        .store
        .list_agent_messages(&user.id, &turn.conversation_id, 1, None)
        .await?
        .into_iter()
        .find(|message| message.id == turn.user_message_id)
        .ok_or_else(|| anyhow::anyhow!("agent user message not found"))?;

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
            }
        }),
    )
    .await?;

    let response = RuleBasedWorkbenchAgent.respond(WorkbenchContext {
        message: user_message.content,
        managed_database_id: turn.managed_database_id.clone(),
        selected_sql_audit_id: turn
            .dashboard_context
            .as_ref()
            .and_then(|context| context.selected_sql_audit_id.clone()),
        managed_database_count: databases.len(),
        audit_score: summary.map(|summary| summary.audit_score),
    });

    for suggestion in response.actions {
        let action = state
            .store
            .create_agent_action(
                &user.id,
                &turn_id,
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
            &state,
            &user.id,
            &turn_id,
            AgentEventType::ActionProposed,
            json!({ "action": action }),
        )
        .await?;
    }

    let assistant_message = state
        .store
        .append_agent_message(
            &user.id,
            &turn.conversation_id,
            Some(&turn_id),
            AgentMessageRole::Assistant,
            &response.content,
            None,
        )
        .await?;
    state
        .store
        .set_agent_turn_assistant_message(&user.id, &turn_id, &assistant_message.id)
        .await?;
    append_event(
        &state,
        &user.id,
        &turn_id,
        AgentEventType::AssistantDelta,
        json!({ "content": response.content }),
    )
    .await?;
    append_event(
        &state,
        &user.id,
        &turn_id,
        AgentEventType::MessageCreated,
        json!({
            "message_id": assistant_message.id,
            "role": "assistant",
        }),
    )
    .await?;
    state
        .store
        .update_agent_turn_status(&user.id, &turn_id, AgentTurnStatus::Completed, None)
        .await?;
    append_event(
        &state,
        &user.id,
        &turn_id,
        AgentEventType::TurnCompleted,
        json!({ "status": "completed" }),
    )
    .await?;

    Ok(())
}

async fn apply_agent_action(
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

async fn append_event(
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
