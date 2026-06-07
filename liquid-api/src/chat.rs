use std::{
    convert::Infallible,
    time::{Duration, Instant},
};

use async_stream::stream;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
};
use futures_util::Stream;
use liquid_core::{
    AgentAction, AgentActionStatus, AgentConversation, AgentEventRecord, AgentEventType,
    AgentMessage, AgentMessageRole, AgentTurn, AgentTurnStatus, ChatAction,
    ChatActionDecisionRequest, ChatActionPreview, ChatConversation, ChatErrorCode,
    ChatManagedDatabaseSummary, ChatMessage, ChatMessagePart, ChatMessageStatus, ChatStreamEvent,
    ChatStreamStage, ChatToolStatus, ChatTurn, ChatTurnDashboardContext,
    CreateAgentConversationRequest, CreateAgentTurnRequest, CreateChatConversationRequest,
    CreateChatTurnRequest, DatapanelQueryResult, ManagedDatabase, SqlAuditRecord,
    UpdateAgentConversationRequest, UpdateChatConversationRequest,
};
use liquid_storage::StorageError;
use serde::Deserialize;
use serde_json::Value;
use tokio::{spawn, time::sleep};

use crate::{
    agent_workbench::CreateDatapanelCardActionPayload,
    agent_workbench::{
        append_event, apply_agent_action, run_agent_turn, synthesize_action_observation,
    },
    auth::authenticated_user,
    error::ApiError,
    state::ApiState,
};

const MISSING_LLM_PROVIDER_ERROR: &str = "LLM provider is not configured";

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/api/v1/chat/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route(
            "/api/v1/chat/conversations/{conversation_id}",
            get(get_conversation)
                .patch(update_conversation)
                .delete(delete_conversation),
        )
        .route(
            "/api/v1/chat/conversations/{conversation_id}/messages",
            get(list_messages),
        )
        .route(
            "/api/v1/chat/conversations/{conversation_id}/actions",
            get(list_actions),
        )
        .route(
            "/api/v1/chat/conversations/{conversation_id}/turns",
            post(create_turn),
        )
        .route("/api/v1/chat/turns/{turn_id}/stream", get(stream_turn))
        .route("/api/v1/chat/turns/{turn_id}/cancel", post(cancel_turn))
        .route("/api/v1/chat/actions/{action_id}/apply", post(apply_action))
        .route(
            "/api/v1/chat/actions/{action_id}/reject",
            post(reject_action),
        )
}

#[derive(Debug, Deserialize)]
struct ListConversationsQuery {
    managed_database_id: Option<String>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ListMessagesQuery {
    limit: Option<i64>,
    before: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListActionsQuery {
    status: Option<AgentActionStatus>,
}

#[derive(Debug, Deserialize)]
struct StreamQuery {
    after_seq: Option<i32>,
}

async fn list_conversations(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<ListConversationsQuery>,
) -> Result<Json<Vec<ChatConversation>>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let selected_database =
        selected_chat_database(&state, &user.id, query.managed_database_id.as_deref()).await?;
    let conversations = state
        .store
        .list_agent_conversations(
            &user.id,
            selected_database
                .as_ref()
                .map(|database| database.id.as_str()),
            query.limit.unwrap_or(50),
        )
        .await?
        .into_iter()
        .map(|conversation| chat_conversation(conversation, selected_database.as_ref()))
        .collect();

    Ok(Json(conversations))
}

async fn create_conversation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateChatConversationRequest>,
) -> Result<Json<ChatConversation>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let selected_database =
        selected_chat_database(&state, &user.id, request.managed_database_id.as_deref()).await?;
    let conversation = state
        .store
        .create_agent_conversation(
            &user.id,
            create_agent_conversation_request(
                request,
                selected_database
                    .as_ref()
                    .map(|database| database.id.clone()),
            ),
        )
        .await?;

    Ok(Json(chat_conversation(
        conversation,
        selected_database.as_ref(),
    )))
}

async fn get_conversation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
) -> Result<Json<ChatConversation>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let conversation = state
        .store
        .get_agent_conversation(&user.id, &conversation_id)
        .await?;
    let selected_database = conversation_database(&state, &user.id, &conversation).await?;

    Ok(Json(chat_conversation(
        conversation,
        selected_database.as_ref(),
    )))
}

async fn update_conversation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Json(request): Json<UpdateChatConversationRequest>,
) -> Result<Json<ChatConversation>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let conversation = state
        .store
        .update_agent_conversation(
            &user.id,
            &conversation_id,
            update_agent_conversation_request(request),
        )
        .await?;
    let selected_database = conversation_database(&state, &user.id, &conversation).await?;

    Ok(Json(chat_conversation(
        conversation,
        selected_database.as_ref(),
    )))
}

async fn delete_conversation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    state
        .store
        .delete_agent_conversation(&user.id, &conversation_id)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn list_messages(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<Vec<ChatMessage>>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let messages = state
        .store
        .list_agent_messages(
            &user.id,
            &conversation_id,
            query.limit.unwrap_or(100),
            query.before.as_deref(),
        )
        .await?
        .into_iter()
        .filter(|message| !is_timeline_only_message(message))
        .map(chat_message)
        .collect();

    Ok(Json(messages))
}

async fn list_actions(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Query(query): Query<ListActionsQuery>,
) -> Result<Json<Vec<ChatAction>>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    state
        .store
        .get_agent_conversation(&user.id, &conversation_id)
        .await?;
    let actions = state
        .store
        .list_agent_actions(&user.id, Some(&conversation_id), query.status)
        .await?
        .into_iter()
        .map(chat_action)
        .collect();

    Ok(Json(actions))
}

async fn create_turn(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Json(request): Json<CreateChatTurnRequest>,
) -> Result<Json<ChatTurn>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let turn = state
        .store
        .create_agent_turn(
            &user.id,
            &conversation_id,
            create_agent_turn_request(request),
        )
        .await?;
    let runner_state = state.clone();
    let runner_user = user.clone();
    let runner_turn_id = turn.id.clone();

    tokio::spawn(async move {
        if let Err(error) = run_agent_turn(runner_state, runner_user, runner_turn_id).await {
            tracing::warn!(error = %error, "chat turn runner failed");
        }
    });

    Ok(Json(chat_turn(turn)))
}

async fn stream_turn(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(turn_id): Path<String>,
    Query(query): Query<StreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    state.store.get_agent_turn(&user.id, &turn_id).await?;
    let store = state.store.clone();
    let owner_user_id = user.id;
    let mut after_seq = query.after_seq.unwrap_or(0).max(0);
    let mut assistant_content = String::new();
    let mut assistant_message_id: Option<String> = None;

    let events = stream! {
        loop {
            let next_events = store
                .list_agent_turn_events(&owner_user_id, &turn_id, after_seq)
                .await;

            match next_events {
                Ok(next_events) => {
                    for event_record in next_events {
                        after_seq = event_record.seq;
                        let events = chat_stream_events(
                            &store,
                            &owner_user_id,
                            &turn_id,
                            event_record,
                            &mut assistant_content,
                            &mut assistant_message_id,
                        )
                        .await;
                        for event in events {
                            yield Ok(sse_chat_event(event));
                        }
                    }
                }
                Err(error) => {
                    yield Ok(sse_chat_event(ChatStreamEvent::TurnFailed {
                        turn_id: turn_id.clone(),
                        error_code: ChatErrorCode::StorageError,
                        message_key: ChatErrorCode::StorageError.message_key().to_owned(),
                        message: error.to_string(),
                    }));
                    break;
                }
            }

            let turn = store.get_agent_turn(&owner_user_id, &turn_id).await;
            let should_stop = match turn {
                Ok(turn) if turn.status == AgentTurnStatus::WaitingForUser => true,
                Ok(turn) if turn.status.is_terminal() => {
                    !turn_has_applying_actions(&store, &owner_user_id, &turn).await
                }
                Ok(_) => false,
                Err(_) => true,
            };

            if should_stop {
                break;
            }

            yield Ok(Event::default().event("ping").data("{}"));
            sleep(Duration::from_millis(750)).await;
        }
    };

    Ok(Sse::new(events).keep_alive(KeepAlive::default()))
}

async fn turn_has_applying_actions(
    store: &std::sync::Arc<dyn liquid_storage::LiquidStore>,
    owner_user_id: &str,
    turn: &AgentTurn,
) -> bool {
    match store
        .list_agent_actions(
            owner_user_id,
            Some(&turn.conversation_id),
            Some(AgentActionStatus::Applying),
        )
        .await
    {
        Ok(actions) => actions.iter().any(|action| action.turn_id == turn.id),
        Err(error) => {
            tracing::warn!(
                turn_id = %turn.id,
                conversation_id = %turn.conversation_id,
                error = %error,
                "failed to check applying chat actions while streaming turn"
            );
            false
        }
    }
}

async fn cancel_turn(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(turn_id): Path<String>,
) -> Result<Json<ChatTurn>, ApiError> {
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
        serde_json::json!({ "status": "cancelled" }),
    )
    .await?;

    Ok(Json(chat_turn(turn)))
}

async fn apply_action(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(action_id): Path<String>,
    _request: Option<Json<ChatActionDecisionRequest>>,
) -> Result<Json<ChatAction>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let action = state.store.get_agent_action(&user.id, &action_id).await?;
    if !matches!(
        action.status,
        AgentActionStatus::Proposed | AgentActionStatus::Failed
    ) {
        let details = action_error_details(&action);
        tracing::warn!(
            action_id = %action.id,
            action_kind = %action.kind.as_str(),
            action_status = %action.status.as_str(),
            turn_id = %action.turn_id,
            conversation_id = %action.conversation_id,
            "chat action apply rejected because the action is no longer actionable"
        );
        return Err(ApiError::conflict_with_details(
            format!(
                "agent action cannot be applied from {} status",
                action.status.as_str()
            ),
            details,
        ));
    }

    let apply_started_at = Instant::now();
    tracing::info!(
        action_id = %action.id,
        action_kind = %action.kind.as_str(),
        action_status = %action.status.as_str(),
        turn_id = %action.turn_id,
        conversation_id = %action.conversation_id,
        "chat action apply started"
    );

    let applying = state
        .store
        .update_agent_action_status(
            &user.id,
            &action.id,
            AgentActionStatus::Applying,
            None,
            None,
        )
        .await?;
    state
        .store
        .update_agent_turn_status(&user.id, &action.turn_id, AgentTurnStatus::Running, None)
        .await?;
    let apply_event = append_action_update_event(&state, &user.id, &applying).await;
    let apply_status_event = append_action_apply_status_event(&state, &user.id, &applying).await;
    let stream_after_seq = apply_event
        .as_ref()
        .or(apply_status_event.as_ref())
        .map(|event| event.seq);

    let background_state = state.clone();
    let background_user_id = user.id.clone();
    let background_action = applying.clone();
    spawn(async move {
        finish_action_apply(background_state, background_user_id, background_action).await;
    });

    tracing::info!(
        action_id = %action.id,
        action_kind = %action.kind.as_str(),
        turn_id = %action.turn_id,
        conversation_id = %action.conversation_id,
        elapsed_ms = apply_started_at.elapsed().as_millis(),
        "chat action apply accepted for background execution"
    );

    Ok(Json(chat_action_with_stream_after_seq(
        applying,
        stream_after_seq,
    )))
}

fn action_error_details(action: &AgentAction) -> Value {
    serde_json::json!({
        "action_id": action.id,
        "action_kind": action.kind.as_str(),
        "action_status": action.status.as_str(),
        "turn_id": action.turn_id,
        "conversation_id": action.conversation_id,
    })
}

async fn finish_action_apply(state: ApiState, owner_user_id: String, action: AgentAction) {
    let apply_started_at = Instant::now();

    match apply_agent_action(&state, &owner_user_id, &action).await {
        Ok((resource_kind, resource_id, event_type, payload)) => {
            let core_elapsed_ms = apply_started_at.elapsed().as_millis();
            let result_payload = payload.clone();
            let resource_id_for_log = resource_id.clone();
            let updated = match state
                .store
                .update_agent_action_status(
                    &owner_user_id,
                    &action.id,
                    AgentActionStatus::Applied,
                    Some(resource_kind),
                    Some(resource_id.clone()),
                )
                .await
            {
                Ok(updated) => updated,
                Err(error) => {
                    tracing::error!(
                        action_id = %action.id,
                        action_kind = %action.kind.as_str(),
                        turn_id = %action.turn_id,
                        conversation_id = %action.conversation_id,
                        error = %error,
                        "failed to mark background chat action as applied"
                    );
                    return;
                }
            };

            append_action_resource_event(&state, &owner_user_id, &action, event_type, payload)
                .await;
            let _ = append_action_update_event(&state, &owner_user_id, &updated).await;
            let observation = action_observation_payload(
                &updated,
                true,
                Some(resource_kind),
                Some(resource_id.clone()),
                Some(&result_payload),
                None,
            );
            append_tool_observation_message(&state, &owner_user_id, &updated, &observation).await;
            if let Err(error) =
                synthesize_action_observation(&state, &owner_user_id, &updated, observation).await
            {
                fail_action_turn(&state, &owner_user_id, &updated.turn_id, error.to_string()).await;
            }
            tracing::info!(
                action_id = %action.id,
                action_kind = %action.kind.as_str(),
                turn_id = %action.turn_id,
                conversation_id = %action.conversation_id,
                resource_kind = %resource_kind.as_str(),
                resource_id = %resource_id_for_log,
                core_elapsed_ms,
                total_elapsed_ms = apply_started_at.elapsed().as_millis(),
                "background chat action apply completed"
            );
        }
        Err(error) => {
            let error_message = error.to_string();
            tracing::error!(
                action_id = %action.id,
                action_kind = %action.kind.as_str(),
                action_status = %action.status.as_str(),
                turn_id = %action.turn_id,
                conversation_id = %action.conversation_id,
                error = %error_message,
                elapsed_ms = apply_started_at.elapsed().as_millis(),
                "background chat action apply failed"
            );
            match state
                .store
                .update_agent_action_status(
                    &owner_user_id,
                    &action.id,
                    AgentActionStatus::Failed,
                    None,
                    None,
                )
                .await
            {
                Ok(updated) => {
                    let _ = append_action_update_event(&state, &owner_user_id, &updated).await;
                    let observation = action_observation_payload(
                        &updated,
                        false,
                        None,
                        None,
                        None,
                        Some(error_message),
                    );
                    append_tool_observation_message(&state, &owner_user_id, &updated, &observation)
                        .await;
                    if let Err(error) =
                        synthesize_action_observation(&state, &owner_user_id, &updated, observation)
                            .await
                    {
                        fail_action_turn(
                            &state,
                            &owner_user_id,
                            &updated.turn_id,
                            error.to_string(),
                        )
                        .await;
                    }
                }
                Err(status_error) => {
                    tracing::error!(
                        action_id = %action.id,
                        action_kind = %action.kind.as_str(),
                        action_status = %action.status.as_str(),
                        turn_id = %action.turn_id,
                        conversation_id = %action.conversation_id,
                        error = %status_error,
                        "failed to mark background chat action as failed"
                    );
                }
            }
        }
    }
}

async fn append_action_update_event(
    state: &ApiState,
    owner_user_id: &str,
    action: &AgentAction,
) -> Option<AgentEventRecord> {
    match append_event(
        state,
        owner_user_id,
        &action.turn_id,
        AgentEventType::ResourceUpdated,
        serde_json::json!({ "action": action }),
    )
    .await
    {
        Ok(event) => Some(event),
        Err(error) => {
            tracing::warn!(
                action_id = %action.id,
                action_kind = %action.kind.as_str(),
                action_status = %action.status.as_str(),
                error = %error,
                "failed to append chat action update event"
            );
            None
        }
    }
}

async fn append_action_apply_status_event(
    state: &ApiState,
    owner_user_id: &str,
    action: &AgentAction,
) -> Option<AgentEventRecord> {
    match append_event(
        state,
        owner_user_id,
        &action.turn_id,
        AgentEventType::ToolCallStarted,
        serde_json::json!({
            "name": "apply_agent_action",
            "stage": "planning",
            "summary": "Applying the confirmed action",
        }),
    )
    .await
    {
        Ok(event) => Some(event),
        Err(error) => {
            tracing::warn!(
                action_id = %action.id,
                action_kind = %action.kind.as_str(),
                error = %error,
                "failed to append chat action apply status event"
            );
            None
        }
    }
}

fn action_observation_payload(
    action: &AgentAction,
    success: bool,
    resource_kind: Option<liquid_core::AgentResourceKind>,
    resource_id: Option<String>,
    result_payload: Option<&Value>,
    error: Option<String>,
) -> Value {
    serde_json::json!({
        "type": "tool_observation",
        "success": success,
        "action": {
            "id": action.id,
            "kind": action.kind,
            "status": action.status,
            "title": action.title,
            "description": action.description,
        },
        "resource": {
            "kind": resource_kind,
            "id": resource_id,
        },
        "result": result_payload,
        "error": error,
    })
}

async fn append_tool_observation_message(
    state: &ApiState,
    owner_user_id: &str,
    action: &AgentAction,
    observation: &Value,
) {
    let content = serde_json::to_string_pretty(observation)
        .unwrap_or_else(|_| "{\"type\":\"tool_observation\"}".to_owned());
    let metadata = serde_json::json!({
        "kind": "tool_observation",
        "visibility": "timeline",
        "action_id": action.id,
        "action_kind": action.kind,
        "action_status": action.status,
        "observation": observation,
    });

    match state
        .store
        .append_agent_message(
            owner_user_id,
            &action.conversation_id,
            Some(&action.turn_id),
            AgentMessageRole::Tool,
            &content,
            Some(metadata),
        )
        .await
    {
        Ok(message) => {
            if let Err(error) = append_event(
                state,
                owner_user_id,
                &action.turn_id,
                AgentEventType::MessageCreated,
                serde_json::json!({
                    "message_id": message.id,
                    "role": "tool",
                }),
            )
            .await
            {
                tracing::warn!(
                    action_id = %action.id,
                    action_kind = %action.kind.as_str(),
                    error = %error,
                    "failed to append tool observation message event"
                );
            }
        }
        Err(error) => {
            tracing::warn!(
                action_id = %action.id,
                action_kind = %action.kind.as_str(),
                error = %error,
                "failed to append tool observation message"
            );
        }
    }
}

async fn fail_action_turn(state: &ApiState, owner_user_id: &str, turn_id: &str, message: String) {
    if let Err(error) = state
        .store
        .update_agent_turn_status(
            owner_user_id,
            turn_id,
            AgentTurnStatus::Failed,
            Some(message.clone()),
        )
        .await
    {
        tracing::error!(
            turn_id,
            error = %error,
            "failed to mark chat action turn as failed"
        );
    }

    if let Err(error) = append_event(
        state,
        owner_user_id,
        turn_id,
        AgentEventType::TurnFailed,
        serde_json::json!({ "error": message }),
    )
    .await
    {
        tracing::warn!(
            turn_id,
            error = %error,
            "failed to append chat action turn failure event"
        );
    }
}

async fn append_action_resource_event(
    state: &ApiState,
    owner_user_id: &str,
    action: &AgentAction,
    event_type: AgentEventType,
    payload: Value,
) {
    if let Err(error) =
        append_event(state, owner_user_id, &action.turn_id, event_type, payload).await
    {
        tracing::warn!(
            action_id = %action.id,
            action_kind = %action.kind.as_str(),
            error = %error,
            "failed to append chat action resource event"
        );
    }
}

async fn reject_action(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(action_id): Path<String>,
    _request: Option<Json<ChatActionDecisionRequest>>,
) -> Result<Json<ChatAction>, ApiError> {
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
        serde_json::json!({ "action": action }),
    )
    .await?;
    append_action_result_message(&state, &user.id, &action, None, None, None).await;

    Ok(Json(chat_action(action)))
}

async fn append_action_result_message(
    state: &ApiState,
    owner_user_id: &str,
    action: &AgentAction,
    resource_kind: Option<liquid_core::AgentResourceKind>,
    resource_id: Option<String>,
    result_payload: Option<&Value>,
) {
    let content = action_result_message(
        action,
        resource_kind,
        resource_id.as_deref(),
        result_payload,
    );
    let metadata = serde_json::json!({
        "kind": "action_result",
        "action_id": action.id,
        "action_kind": action.kind,
        "action_status": action.status,
        "resource_kind": resource_kind,
        "resource_id": resource_id,
        "result": result_payload,
    });

    let message = state
        .store
        .append_agent_message(
            owner_user_id,
            &action.conversation_id,
            Some(&action.turn_id),
            AgentMessageRole::Tool,
            &content,
            Some(metadata),
        )
        .await;

    match message {
        Ok(message) => {
            if let Err(error) = append_event(
                state,
                owner_user_id,
                &action.turn_id,
                AgentEventType::MessageCreated,
                serde_json::json!({
                    "message_id": message.id,
                    "role": "tool",
                }),
            )
            .await
            {
                tracing::warn!(
                    action_id = %action.id,
                    error = %error,
                    "failed to append action result message event"
                );
            }
        }
        Err(error) => {
            tracing::warn!(
                action_id = %action.id,
                error = %error,
                "failed to append action result message"
            );
        }
    }
}

fn action_result_message(
    action: &AgentAction,
    resource_kind: Option<liquid_core::AgentResourceKind>,
    resource_id: Option<&str>,
    result_payload: Option<&Value>,
) -> String {
    match action.status {
        AgentActionStatus::Applied => match resource_kind {
            Some(liquid_core::AgentResourceKind::SqlAudit) => result_payload
                .and_then(sql_audit_record_from_result_payload)
                .map(|record| {
                    let query_result = result_payload.and_then(sql_audit_query_result_from_payload);
                    sql_audit_action_result_message(&record, query_result.as_ref())
                })
                .unwrap_or_else(|| {
                    format!(
                        "SQL audit created and reviewed. Audit ID: {}.",
                        resource_id.unwrap_or("unknown")
                    )
                }),
            Some(liquid_core::AgentResourceKind::DatapanelCard) => format!(
                "Datapanel card created. Card ID: {}.",
                resource_id.unwrap_or("unknown")
            ),
            Some(kind) => format!(
                "Action applied. Resource {kind:?}: {}.",
                resource_id.unwrap_or("unknown")
            ),
            None => "Action applied.".to_owned(),
        },
        AgentActionStatus::Rejected => "Action rejected.".to_owned(),
        AgentActionStatus::Failed => result_payload
            .and_then(|payload| payload.get("error"))
            .and_then(Value::as_str)
            .map(|error| format!("Action failed.\n\n{error}"))
            .unwrap_or_else(|| "Action failed.".to_owned()),
        AgentActionStatus::Proposed
        | AgentActionStatus::Applying
        | AgentActionStatus::Superseded => "Action status updated.".to_owned(),
    }
}

fn sql_audit_record_from_result_payload(payload: &Value) -> Option<SqlAuditRecord> {
    payload
        .get("record")
        .cloned()
        .and_then(|record| serde_json::from_value::<SqlAuditRecord>(record).ok())
}

fn sql_audit_query_result_from_payload(payload: &Value) -> Option<DatapanelQueryResult> {
    payload
        .get("query_result")
        .cloned()
        .and_then(|query_result| {
            serde_json::from_value::<Option<DatapanelQueryResult>>(query_result).ok()
        })
        .flatten()
}

fn sql_audit_action_result_message(
    record: &SqlAuditRecord,
    query_result: Option<&DatapanelQueryResult>,
) -> String {
    if let Some(query_result) = query_result {
        let mut message = sql_action_reference_message("SQL query result", record);
        message.push_str("\n\n**Query result**\n\n");
        message.push_str(&query_result_markdown(query_result));
        return message;
    }

    if let Some(execution_result) = &record.execution_result {
        let mut message = sql_action_reference_message("SQL execution completed", record);
        message.push_str(&format!(
            "\n\n**Execution result**\n\n- Statement: `{}`\n- Affected rows: `{}`\n- Elapsed: `{}ms`\n- Risk floor: `{}/100`",
            execution_result.statement_kind.as_str(),
            execution_result.affected_rows,
            execution_result.elapsed_ms,
            execution_result.risk_floor,
        ));
        return message;
    }

    if let Some(execution_error) = record.execution_error.as_deref() {
        let mut message = sql_action_reference_message("SQL execution failed", record);
        message.push_str("\n\n**Execution error**\n\n");
        message.push_str(execution_error.trim());
        return message;
    }

    let mut message = sql_action_reference_message("SQL audit completed", record);

    if record
        .statement_kind
        .is_some_and(|kind| kind == liquid_core::SqlStatementKind::Select)
    {
        message.push_str(
            "\n\n_This action created an audit report; it does not return SELECT rows. Use a Datapanel card action when you want query result rows in the workspace._",
        );
    }

    if let Some(report) = &record.report {
        message.push_str("\n\n**Audit summary**\n\n");
        message.push_str(report.summary.trim());

        message.push_str("\n\n**Findings**\n\n");
        if report.findings.is_empty() {
            message.push_str("No findings.");
        } else {
            for finding in &report.findings {
                message.push_str(&format!(
                    "- **{}** (`{}`): {} Recommendation: {}",
                    finding.title,
                    risk_severity_label(&finding.severity),
                    finding.explanation,
                    finding.recommendation,
                ));
                message.push('\n');
            }
            message = message.trim_end().to_owned();
        }
    }

    message
}

fn sql_action_reference_message(title: &str, record: &SqlAuditRecord) -> String {
    let mut message = format!(
        "### {title}\n\n- Audit reference: `{}`\n- Database: `{}`\n- Status: `{}`\n- Risk score: `{}/100`",
        record.id,
        record.managed_database_name,
        record.status.as_str(),
        record.risk_score,
    );

    if let Some(statement_kind) = record.statement_kind {
        message.push_str(&format!("\n- Statement: `{}`", statement_kind.as_str()));
    }

    message
}

fn query_result_markdown(result: &DatapanelQueryResult) -> String {
    let mut message = format!(
        "{} row{} returned in {}ms{}.",
        result.row_count,
        if result.row_count == 1 { "" } else { "s" },
        result.elapsed_ms,
        if result.truncated { " (truncated)" } else { "" },
    );

    if result.columns.is_empty() {
        return message;
    }

    message.push_str("\n\n");
    message.push('|');
    for column in &result.columns {
        message.push(' ');
        message.push_str(&escape_markdown_table_cell(column));
        message.push_str(" |");
    }
    message.push('\n');
    message.push('|');
    for _ in &result.columns {
        message.push_str(" --- |");
    }

    for row in result.rows.iter().take(20) {
        message.push('\n');
        message.push('|');
        for column in &result.columns {
            let value = row
                .get(column)
                .map(format_query_result_value)
                .unwrap_or_default();
            message.push(' ');
            message.push_str(&escape_markdown_table_cell(&value));
            message.push_str(" |");
        }
    }

    if result.rows.len() > 20 {
        message.push_str("\n\n_Showing first 20 rows._");
    }

    message
}

fn format_query_result_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn escape_markdown_table_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn risk_severity_label(severity: &liquid_core::RiskSeverity) -> &'static str {
    match severity {
        liquid_core::RiskSeverity::Low => "low",
        liquid_core::RiskSeverity::Medium => "medium",
        liquid_core::RiskSeverity::High => "high",
        liquid_core::RiskSeverity::Critical => "critical",
    }
}

async fn chat_stream_events(
    store: &std::sync::Arc<dyn liquid_storage::LiquidStore>,
    owner_user_id: &str,
    turn_id: &str,
    event: AgentEventRecord,
    assistant_content: &mut String,
    assistant_message_id: &mut Option<String>,
) -> Vec<ChatStreamEvent> {
    match event.event_type {
        AgentEventType::TurnStarted => vec![ChatStreamEvent::TurnStarted {
            turn_id: event.turn_id,
        }],
        AgentEventType::MessageCreated => {
            let Some(message_id) = event.payload.get("message_id").and_then(Value::as_str) else {
                return Vec::new();
            };
            let Ok(turn) = store.get_agent_turn(owner_user_id, turn_id).await else {
                return Vec::new();
            };
            let Ok(messages) = store
                .list_agent_messages(owner_user_id, &turn.conversation_id, 200, None)
                .await
            else {
                return Vec::new();
            };
            let Some(message) = messages
                .into_iter()
                .find(|message| message.id == message_id)
            else {
                return Vec::new();
            };

            if is_timeline_only_message(&message) {
                return Vec::new();
            }

            let is_assistant = message.role == AgentMessageRole::Assistant;

            if is_assistant {
                *assistant_message_id = Some(message.id.clone());
                vec![ChatStreamEvent::AssistantDone {
                    message: chat_message(message),
                }]
            } else {
                vec![ChatStreamEvent::MessageCreated {
                    message: chat_message(message),
                }]
            }
        }
        AgentEventType::AssistantDelta => {
            let Some(delta) = payload_string(&event.payload, "content") else {
                return Vec::new();
            };
            assistant_content.clear();
            assistant_content.push_str(&delta);
            let message_id = payload_string(&event.payload, "message_id")
                .or_else(|| assistant_message_id.clone())
                .unwrap_or_else(|| format!("stream-{turn_id}"));

            vec![ChatStreamEvent::AssistantDelta {
                message_id,
                delta,
                accumulated: Some(assistant_content.clone()),
            }]
        }
        AgentEventType::ToolCallStarted => tool_started_events(&event.payload),
        AgentEventType::ToolCallFinished => tool_finished_events(&event.payload),
        AgentEventType::ActionProposed => {
            let Some(action) = event
                .payload
                .get("action")
                .cloned()
                .and_then(|value| serde_json::from_value::<AgentAction>(value).ok())
            else {
                return Vec::new();
            };

            vec![ChatStreamEvent::ActionProposed {
                action: chat_action(action),
            }]
        }
        AgentEventType::TurnWaitingForUser => {
            let turn = event
                .payload
                .get("turn")
                .cloned()
                .and_then(|value| serde_json::from_value::<AgentTurn>(value).ok());
            let turn = match turn {
                Some(turn) => turn,
                None => match store.get_agent_turn(owner_user_id, turn_id).await {
                    Ok(turn) => turn,
                    Err(_) => return Vec::new(),
                },
            };

            vec![ChatStreamEvent::TurnWaitingForUser {
                turn: chat_turn(turn),
            }]
        }
        AgentEventType::ResourceUpdated | AgentEventType::ResourceCreated => {
            let Some(action) = event
                .payload
                .get("action")
                .cloned()
                .and_then(|value| serde_json::from_value::<AgentAction>(value).ok())
            else {
                return Vec::new();
            };

            vec![ChatStreamEvent::ActionUpdated {
                action: chat_action(action),
            }]
        }
        AgentEventType::TurnCompleted => {
            let Ok(turn) = store.get_agent_turn(owner_user_id, turn_id).await else {
                return Vec::new();
            };
            if let Some(message_id) = turn.assistant_message_id.clone() {
                *assistant_message_id = Some(message_id);
            }

            vec![ChatStreamEvent::TurnCompleted {
                turn: chat_turn(turn),
            }]
        }
        AgentEventType::TurnFailed => {
            let message = payload_string(&event.payload, "error")
                .or_else(|| payload_string(&event.payload, "status"))
                .unwrap_or_else(|| "turn failed".to_owned());
            let error_code = chat_error_code(&message);

            vec![ChatStreamEvent::TurnFailed {
                turn_id: event.turn_id,
                error_code,
                message_key: error_code.message_key().to_owned(),
                message,
            }]
        }
    }
}

fn tool_started_events(payload: &Value) -> Vec<ChatStreamEvent> {
    let stage = chat_stream_stage_from_payload(payload);
    let summary = payload_string(payload, "summary").or_else(|| default_stage_summary(stage));

    if let Some(id) = payload_string(payload, "id") {
        let name = payload_string(payload, "name").unwrap_or_else(|| "tool".to_owned());
        let title = payload_string(payload, "title").unwrap_or_else(|| default_tool_title(&name));

        return vec![ChatStreamEvent::ToolStarted {
            id,
            name,
            title,
            summary: summary.unwrap_or_else(|| "Running tool".to_owned()),
        }];
    }

    vec![ChatStreamEvent::StatusChanged { stage, summary }]
}

fn tool_finished_events(payload: &Value) -> Vec<ChatStreamEvent> {
    let mut events = Vec::new();

    if let Some(id) = payload_string(payload, "id") {
        let name = payload_string(payload, "name").unwrap_or_else(|| "tool".to_owned());
        let status = match payload_string(payload, "status").as_deref() {
            Some("failed") | Some("error") => ChatToolStatus::Failed,
            _ => ChatToolStatus::Succeeded,
        };
        let summary = payload_string(payload, "summary").unwrap_or_else(|| match status {
            ChatToolStatus::Succeeded => "Tool completed".to_owned(),
            ChatToolStatus::Failed => "Tool failed".to_owned(),
        });
        let elapsed_ms = payload
            .get("elapsed_ms")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let output_preview = payload_string(payload, "output_preview");

        events.push(ChatStreamEvent::ToolFinished {
            id,
            name,
            status,
            summary,
            elapsed_ms,
            output_preview,
        });
    }

    events.push(ChatStreamEvent::StatusChanged {
        stage: ChatStreamStage::Thinking,
        summary: payload_string(payload, "next_summary"),
    });

    events
}

fn chat_stream_stage_from_payload(payload: &Value) -> ChatStreamStage {
    match payload_string(payload, "stage").as_deref() {
        Some("planning") => ChatStreamStage::Planning,
        Some("loading_context") => ChatStreamStage::LoadingContext,
        Some("proposing_action") => ChatStreamStage::ProposingAction,
        Some("auditing_sql") => ChatStreamStage::AuditingSql,
        Some("executing_sql") => ChatStreamStage::ExecutingSql,
        Some("synthesizing") => ChatStreamStage::Synthesizing,
        Some("thinking") => ChatStreamStage::Thinking,
        _ => match payload_string(payload, "name").as_deref() {
            Some("load_workbench_context") => ChatStreamStage::LoadingContext,
            Some("sql_audit") => ChatStreamStage::AuditingSql,
            Some("sql_execute") => ChatStreamStage::ExecutingSql,
            Some("synthesize_observation") => ChatStreamStage::Synthesizing,
            _ => ChatStreamStage::Thinking,
        },
    }
}

fn default_stage_summary(stage: ChatStreamStage) -> Option<String> {
    let summary = match stage {
        ChatStreamStage::Planning => "Preparing the confirmed action",
        ChatStreamStage::Thinking => "Thinking",
        ChatStreamStage::LoadingContext => "Loading workspace context",
        ChatStreamStage::ProposingAction => "Preparing an action",
        ChatStreamStage::AuditingSql => "Checking SQL safety and policy",
        ChatStreamStage::ExecutingSql => "Executing the approved SQL",
        ChatStreamStage::Synthesizing => "Preparing the final response",
    };

    Some(summary.to_owned())
}

fn default_tool_title(name: &str) -> String {
    match name {
        "sql_audit" => "Audit SQL".to_owned(),
        "sql_execute" => "Execute SQL".to_owned(),
        "apply_agent_action" => "Apply action".to_owned(),
        "create_datapanel_card" => "Create Datapanel card".to_owned(),
        _ => name.replace('_', " "),
    }
}

fn chat_conversation(
    conversation: AgentConversation,
    selected_database: Option<&ManagedDatabase>,
) -> ChatConversation {
    ChatConversation {
        id: conversation.id,
        title: conversation.title,
        managed_database_id: conversation.managed_database_id,
        selected_database: selected_database.map(chat_database_summary),
        created_at: conversation.created_at,
        updated_at: conversation.updated_at,
    }
}

async fn selected_chat_database(
    state: &ApiState,
    owner_user_id: &str,
    requested_database_id: Option<&str>,
) -> Result<Option<ManagedDatabase>, ApiError> {
    if let Some(database_id) = requested_database_id {
        return load_chat_database(state, owner_user_id, database_id)
            .await
            .map(Some);
    }

    let current_database = state
        .store
        .get_current_managed_database(owner_user_id)
        .await?;

    if current_database.is_some() {
        return Ok(current_database);
    }

    Ok(state
        .store
        .list_managed_databases(owner_user_id)
        .await?
        .into_iter()
        .next())
}

async fn conversation_database(
    state: &ApiState,
    owner_user_id: &str,
    conversation: &AgentConversation,
) -> Result<Option<ManagedDatabase>, ApiError> {
    match conversation.managed_database_id.as_deref() {
        Some(database_id) => load_chat_database(state, owner_user_id, database_id)
            .await
            .map(Some),
        None => selected_chat_database(state, owner_user_id, None).await,
    }
}

async fn load_chat_database(
    state: &ApiState,
    owner_user_id: &str,
    database_id: &str,
) -> Result<ManagedDatabase, ApiError> {
    state
        .store
        .list_managed_databases(owner_user_id)
        .await?
        .into_iter()
        .find(|database| database.id == database_id)
        .ok_or_else(|| ApiError::from(StorageError::NotFound))
}

fn chat_database_summary(database: &ManagedDatabase) -> ChatManagedDatabaseSummary {
    ChatManagedDatabaseSummary {
        id: database.id.clone(),
        name: database.name.clone(),
        engine: database.engine,
        host: database.host.clone(),
        port: database.port,
        database: database.database.clone(),
        username: database.username.clone(),
        ssl_mode: database.ssl_mode,
    }
}

fn chat_message(message: AgentMessage) -> ChatMessage {
    let status = ChatMessageStatus::Complete;
    let mut parts =
        if message.role == AgentMessageRole::Assistant || is_action_result_message(&message) {
            vec![ChatMessagePart::Markdown {
                markdown: message.content.clone(),
            }]
        } else {
            vec![ChatMessagePart::Text {
                text: message.content.clone(),
            }]
        };

    if message.role == AgentMessageRole::Assistant {
        parts.extend(query_result_table_parts(message.metadata.as_ref()));
    }

    ChatMessage {
        id: message.id,
        role: message.role,
        status,
        content: message.content,
        parts,
        turn_id: message.turn_id,
        created_at: message.created_at,
    }
}

#[derive(Debug, Deserialize)]
struct QueryResultTablePartMetadata {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    managed_database_id: String,
    sql: String,
    result: DatapanelQueryResult,
}

fn query_result_table_parts(metadata: Option<&Value>) -> Vec<ChatMessagePart> {
    metadata
        .and_then(|metadata| metadata.get("query_result_tables"))
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<QueryResultTablePartMetadata>>(value).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|part| ChatMessagePart::QueryResultTable {
            title: part.title,
            description: part.description,
            managed_database_id: part.managed_database_id,
            sql: part.sql,
            result: part.result,
        })
        .collect()
}

fn is_action_result_message(message: &AgentMessage) -> bool {
    message
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("kind"))
        .and_then(Value::as_str)
        == Some("action_result")
}

fn is_timeline_only_message(message: &AgentMessage) -> bool {
    message
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("visibility"))
        .and_then(Value::as_str)
        == Some("timeline")
}

fn chat_turn(turn: AgentTurn) -> ChatTurn {
    let error_code = turn.error.as_deref().map(chat_error_code);

    ChatTurn {
        id: turn.id,
        conversation_id: turn.conversation_id,
        status: turn.status,
        input_message_id: turn.user_message_id,
        output_message_id: turn.assistant_message_id,
        error_code,
        error_message: turn.error,
    }
}

fn chat_action(action: AgentAction) -> ChatAction {
    chat_action_with_stream_after_seq(action, None)
}

fn chat_action_with_stream_after_seq(
    action: AgentAction,
    stream_after_seq: Option<i32>,
) -> ChatAction {
    let preview = chat_action_preview(&action);

    ChatAction {
        id: action.id,
        turn_id: action.turn_id,
        kind: action.kind,
        status: action.status,
        title: action.title,
        description: action.description,
        resource_kind: action.resource_kind,
        resource_id: action.resource_id,
        requires_confirmation: action.requires_confirmation,
        preview,
        stream_after_seq,
    }
}

fn chat_action_preview(action: &AgentAction) -> Option<ChatActionPreview> {
    match action.kind {
        liquid_core::AgentActionKind::CreateSqlAudit => chat_sql_audit_preview(action),
        liquid_core::AgentActionKind::CreateDatapanelCard => chat_datapanel_card_preview(action),
        _ => None,
    }
}

fn chat_sql_audit_preview(action: &AgentAction) -> Option<ChatActionPreview> {
    let request = action.payload.get("request")?;
    let sql = request.get("sql")?.as_str()?.to_owned();
    let context = request
        .get("context")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let database_name = action
        .payload
        .get("managed_database_name")
        .and_then(Value::as_str)
        .map(str::to_owned);

    Some(ChatActionPreview::SqlAudit {
        sql,
        database_name,
        context,
    })
}

fn chat_datapanel_card_preview(action: &AgentAction) -> Option<ChatActionPreview> {
    let payload =
        serde_json::from_value::<CreateDatapanelCardActionPayload>(action.payload.clone()).ok()?;
    let result = payload.result?;

    Some(ChatActionPreview::DatapanelCard {
        title: payload.title,
        description: payload.description,
        card_kind: payload.kind,
        sql: payload.sql,
        chart: payload.chart,
        result,
    })
}

fn chat_error_code(message: &str) -> ChatErrorCode {
    if message.contains(MISSING_LLM_PROVIDER_ERROR) {
        ChatErrorCode::ProviderNotConfigured
    } else if message.contains("not valid JSON")
        || message.contains("response was empty")
        || message.contains("maximum tool rounds")
        || message.contains("requested tools after creating a confirmation proposal")
    {
        ChatErrorCode::InvalidModelResponse
    } else if message.contains("sql_audit_id is not available")
        || message.contains("unsupported")
        || message.contains("required")
    {
        ChatErrorCode::InvalidActionIntent
    } else if message == "cancelled" {
        ChatErrorCode::TurnCancelled
    } else if message.contains("provider") || message.contains("LLM") {
        ChatErrorCode::ProviderRequestFailed
    } else {
        ChatErrorCode::StorageError
    }
}

fn payload_string(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn sse_chat_event(event: ChatStreamEvent) -> Event {
    let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_owned());
    Event::default().event("chat.event").data(data)
}

fn create_agent_conversation_request(
    request: CreateChatConversationRequest,
    managed_database_id: Option<String>,
) -> CreateAgentConversationRequest {
    CreateAgentConversationRequest {
        title: request.title,
        managed_database_id,
    }
}

fn update_agent_conversation_request(
    request: UpdateChatConversationRequest,
) -> UpdateAgentConversationRequest {
    UpdateAgentConversationRequest {
        title: request.title,
    }
}

fn create_agent_turn_request(request: CreateChatTurnRequest) -> CreateAgentTurnRequest {
    CreateAgentTurnRequest {
        message: request.message,
        managed_database_id: request.managed_database_id,
        dashboard_context: request.dashboard_context.map(agent_dashboard_context),
        client_request_id: request.client_request_id,
    }
}

fn agent_dashboard_context(
    context: ChatTurnDashboardContext,
) -> liquid_core::AgentDashboardContext {
    liquid_core::AgentDashboardContext {
        active_view: context.active_view,
        selected_sql_audit_id: context.selected_sql_audit_id,
        date_range: context.date_range,
    }
}

#[cfg(test)]
mod tests {
    use liquid_core::DatapanelQueryResult;
    use serde_json::json;
    use time::OffsetDateTime;

    use super::query_result_markdown;

    #[test]
    fn query_result_markdown_renders_result_table() {
        let result = DatapanelQueryResult {
            columns: vec!["datname".to_owned(), "size".to_owned()],
            rows: vec![
                json!({ "datname": "postgres", "size": "7 MB" }),
                json!({ "datname": "liquid", "size": "12 MB" }),
            ],
            row_count: 2,
            truncated: false,
            elapsed_ms: 5,
            refreshed_at: OffsetDateTime::UNIX_EPOCH,
        };

        let markdown = query_result_markdown(&result);

        assert!(markdown.contains("2 rows returned in 5ms."));
        assert!(markdown.contains("| datname | size |"));
        assert!(markdown.contains("| postgres | 7 MB |"));
        assert!(markdown.contains("| liquid | 12 MB |"));
    }
}
