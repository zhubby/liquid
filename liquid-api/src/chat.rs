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
    AgentMessage, AgentMessageRole, AgentTurn, AgentTurnStatus, BiQueryResult, ChatAction,
    ChatActionDecisionRequest, ChatActionPreview, ChatConversation, ChatErrorCode,
    ChatManagedDatabaseSummary, ChatMessage, ChatMessagePart, ChatMessageStatus, ChatStreamEvent,
    ChatStreamStage, ChatTurn, ChatTurnDashboardContext, CreateAgentConversationRequest,
    CreateAgentTurnRequest, CreateChatConversationRequest, CreateChatTurnRequest, ManagedDatabase,
    SqlAuditRecord, UpdateAgentConversationRequest, UpdateChatConversationRequest,
};
use serde::Deserialize;
use serde_json::Value;
use tokio::time::sleep;

use crate::{
    agent_workbench::CreateBiCardActionPayload,
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
            let result_payload = payload.clone();
            let updated = state
                .store
                .update_agent_action_status(
                    &user.id,
                    &action.id,
                    AgentActionStatus::Applied,
                    Some(resource_kind),
                    Some(resource_id.clone()),
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
            append_action_result_message(
                &state,
                &user.id,
                &updated,
                Some(resource_kind),
                Some(resource_id),
                Some(&result_payload),
            )
            .await;
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
            Some(liquid_core::AgentResourceKind::BiPanelCard) => format!(
                "BI panel card created. Card ID: {}.",
                resource_id.unwrap_or("unknown")
            ),
            Some(kind) => format!(
                "Action applied. Resource {kind:?}: {}.",
                resource_id.unwrap_or("unknown")
            ),
            None => "Action applied.".to_owned(),
        },
        AgentActionStatus::Rejected => "Action rejected.".to_owned(),
        AgentActionStatus::Failed => "Action failed.".to_owned(),
        AgentActionStatus::Proposed | AgentActionStatus::Superseded => {
            "Action status updated.".to_owned()
        }
    }
}

fn sql_audit_record_from_result_payload(payload: &Value) -> Option<SqlAuditRecord> {
    payload
        .get("record")
        .cloned()
        .and_then(|record| serde_json::from_value::<SqlAuditRecord>(record).ok())
}

fn sql_audit_query_result_from_payload(payload: &Value) -> Option<BiQueryResult> {
    payload
        .get("query_result")
        .cloned()
        .and_then(|query_result| serde_json::from_value::<Option<BiQueryResult>>(query_result).ok())
        .flatten()
}

fn sql_audit_action_result_message(
    record: &SqlAuditRecord,
    query_result: Option<&BiQueryResult>,
) -> String {
    let title = if query_result.is_some() {
        "SQL query result"
    } else {
        "SQL audit completed"
    };
    let mut message = format!(
        "### {title}\n\n- Audit ID: `{}`\n- Database: `{}`\n- Audit status: `{}`\n- Risk score: `{}/100`",
        record.id,
        record.managed_database_name,
        record.status.as_str(),
        record.risk_score,
    );

    if let Some(statement_kind) = record.statement_kind {
        message.push_str(&format!("\n- Statement: `{}`", statement_kind.as_str()));
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

    if let Some(query_result) = query_result {
        message.push_str("\n\n**Query result**\n\n");
        message.push_str(&query_result_markdown(query_result));
    } else if let Some(execution_result) = &record.execution_result {
        message.push_str(&format!(
            "\n\n**Execution result**\n\n- Statement: `{}`\n- Affected rows: `{}`\n- Elapsed: `{}ms`\n- Risk floor: `{}/100`",
            execution_result.statement_kind.as_str(),
            execution_result.affected_rows,
            execution_result.elapsed_ms,
            execution_result.risk_floor,
        ));
    } else if let Some(execution_error) = record.execution_error.as_deref() {
        message.push_str("\n\n**Execution error**\n\n");
        message.push_str(execution_error.trim());
    } else if record
        .statement_kind
        .is_some_and(|kind| kind == liquid_core::SqlStatementKind::Select)
    {
        message.push_str(
            "\n\n_This action created an audit report; it does not return SELECT rows. Use a BI card action when you want query result rows in the workspace._",
        );
    }

    message
}

fn query_result_markdown(result: &BiQueryResult) -> String {
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

#[cfg(test)]
mod tests {
    use liquid_core::BiQueryResult;
    use serde_json::json;
    use time::OffsetDateTime;

    use super::query_result_markdown;

    #[test]
    fn query_result_markdown_renders_result_table() {
        let result = BiQueryResult {
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
    let parts = if message.role == AgentMessageRole::Assistant || is_action_result_message(&message)
    {
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

fn is_action_result_message(message: &AgentMessage) -> bool {
    message
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("kind"))
        .and_then(Value::as_str)
        == Some("action_result")
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
    match action.kind {
        liquid_core::AgentActionKind::CreateSqlAudit => chat_sql_audit_preview(action),
        liquid_core::AgentActionKind::CreateBiCard => chat_bi_card_preview(action),
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

fn chat_bi_card_preview(action: &AgentAction) -> Option<ChatActionPreview> {
    let payload =
        serde_json::from_value::<CreateBiCardActionPayload>(action.payload.clone()).ok()?;
    let result = payload.result?;

    Some(ChatActionPreview::BiCard {
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
