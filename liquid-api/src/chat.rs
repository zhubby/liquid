use std::{convert::Infallible, time::Duration};

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
    ChatStreamStage, ChatTurn, ChatTurnDashboardContext, CreateAgentConversationRequest,
    CreateAgentTurnRequest, CreateChatConversationRequest, CreateChatTurnRequest, ManagedDatabase,
    UpdateAgentConversationRequest, UpdateChatConversationRequest,
};
use serde::Deserialize;
use serde_json::Value;
use tokio::time::sleep;

use crate::{
    agent_workbench::{append_event, apply_agent_action, run_agent_turn},
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
    let current_database = selected_chat_database(&state, &user.id).await?;
    let conversations = state
        .store
        .list_agent_conversations(&user.id, query.limit.unwrap_or(50))
        .await?
        .into_iter()
        .map(|conversation| chat_conversation(conversation, current_database.as_ref()))
        .collect();

    Ok(Json(conversations))
}

async fn create_conversation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateChatConversationRequest>,
) -> Result<Json<ChatConversation>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let conversation = state
        .store
        .create_agent_conversation(&user.id, create_agent_conversation_request(request))
        .await?;
    let current_database = selected_chat_database(&state, &user.id).await?;

    Ok(Json(chat_conversation(
        conversation,
        current_database.as_ref(),
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
    let current_database = selected_chat_database(&state, &user.id).await?;

    Ok(Json(chat_conversation(
        conversation,
        current_database.as_ref(),
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
    let current_database = selected_chat_database(&state, &user.id).await?;

    Ok(Json(chat_conversation(
        conversation,
        current_database.as_ref(),
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
                        if let Some(event) = chat_stream_event(
                            &store,
                            &owner_user_id,
                            &turn_id,
                            event_record,
                            &mut assistant_content,
                            &mut assistant_message_id,
                        )
                        .await {
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
    let updated = match apply_agent_action(&state, &user.id, &action).await {
        Ok((resource_kind, resource_id, event_type, payload)) => {
            let updated = state
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
            append_event(
                &state,
                &user.id,
                &action.turn_id,
                AgentEventType::ResourceUpdated,
                serde_json::json!({ "action": updated }),
            )
            .await?;
            updated
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

    Ok(Json(chat_action(updated)))
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

    Ok(Json(chat_action(action)))
}

async fn chat_stream_event(
    store: &std::sync::Arc<dyn liquid_storage::LiquidStore>,
    owner_user_id: &str,
    turn_id: &str,
    event: AgentEventRecord,
    assistant_content: &mut String,
    assistant_message_id: &mut Option<String>,
) -> Option<ChatStreamEvent> {
    match event.event_type {
        AgentEventType::TurnStarted => Some(ChatStreamEvent::TurnStarted {
            turn_id: event.turn_id,
        }),
        AgentEventType::MessageCreated => {
            let message_id = event.payload.get("message_id")?.as_str()?;
            let turn = store.get_agent_turn(owner_user_id, turn_id).await.ok()?;
            let message = store
                .list_agent_messages(owner_user_id, &turn.conversation_id, 200, None)
                .await
                .ok()?
                .into_iter()
                .find(|message| message.id == message_id)?;
            let is_assistant = message.role == AgentMessageRole::Assistant;

            if is_assistant {
                *assistant_message_id = Some(message.id.clone());
                Some(ChatStreamEvent::AssistantDone {
                    message: chat_message(message),
                })
            } else {
                Some(ChatStreamEvent::MessageCreated {
                    message: chat_message(message),
                })
            }
        }
        AgentEventType::AssistantDelta => {
            let delta = payload_string(&event.payload, "content")?;
            assistant_content.clear();
            assistant_content.push_str(&delta);
            let message_id = assistant_message_id
                .clone()
                .unwrap_or_else(|| format!("stream-{turn_id}"));

            Some(ChatStreamEvent::AssistantDelta {
                message_id,
                delta,
                accumulated: Some(assistant_content.clone()),
            })
        }
        AgentEventType::ToolCallStarted => {
            let stage = match payload_string(&event.payload, "name").as_deref() {
                Some("load_workbench_context") => ChatStreamStage::LoadingContext,
                _ => ChatStreamStage::Thinking,
            };

            Some(ChatStreamEvent::StatusChanged { stage })
        }
        AgentEventType::ToolCallFinished => Some(ChatStreamEvent::StatusChanged {
            stage: ChatStreamStage::Thinking,
        }),
        AgentEventType::ActionProposed => {
            let action = event
                .payload
                .get("action")
                .cloned()
                .and_then(|value| serde_json::from_value::<AgentAction>(value).ok())?;

            Some(ChatStreamEvent::ActionProposed {
                action: chat_action(action),
            })
        }
        AgentEventType::ResourceUpdated | AgentEventType::ResourceCreated => {
            let action = event
                .payload
                .get("action")
                .cloned()
                .and_then(|value| serde_json::from_value::<AgentAction>(value).ok())?;

            Some(ChatStreamEvent::ActionUpdated {
                action: chat_action(action),
            })
        }
        AgentEventType::TurnCompleted => {
            let turn = store.get_agent_turn(owner_user_id, turn_id).await.ok()?;
            if let Some(message_id) = turn.assistant_message_id.clone() {
                *assistant_message_id = Some(message_id);
            }

            Some(ChatStreamEvent::TurnCompleted {
                turn: chat_turn(turn),
            })
        }
        AgentEventType::TurnFailed => {
            let message = payload_string(&event.payload, "error")
                .or_else(|| payload_string(&event.payload, "status"))
                .unwrap_or_else(|| "turn failed".to_owned());
            let error_code = chat_error_code(&message);

            Some(ChatStreamEvent::TurnFailed {
                turn_id: event.turn_id,
                error_code,
                message_key: error_code.message_key().to_owned(),
                message,
            })
        }
    }
}

fn chat_conversation(
    conversation: AgentConversation,
    selected_database: Option<&ManagedDatabase>,
) -> ChatConversation {
    ChatConversation {
        id: conversation.id,
        title: conversation.title,
        selected_database: selected_database.map(chat_database_summary),
        created_at: conversation.created_at,
        updated_at: conversation.updated_at,
    }
}

async fn selected_chat_database(
    state: &ApiState,
    owner_user_id: &str,
) -> Result<Option<ManagedDatabase>, ApiError> {
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
    let parts = if message.role == AgentMessageRole::Assistant {
        vec![ChatMessagePart::Markdown {
            markdown: message.content.clone(),
        }]
    } else {
        vec![ChatMessagePart::Text {
            text: message.content.clone(),
        }]
    };

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
    }
}

fn chat_action_preview(action: &AgentAction) -> Option<ChatActionPreview> {
    if action.kind != liquid_core::AgentActionKind::CreateSqlAudit {
        return None;
    }

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

fn chat_error_code(message: &str) -> ChatErrorCode {
    if message.contains(MISSING_LLM_PROVIDER_ERROR) {
        ChatErrorCode::ProviderNotConfigured
    } else if message.contains("not valid JSON") || message.contains("response was empty") {
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
) -> CreateAgentConversationRequest {
    CreateAgentConversationRequest {
        title: request.title,
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
