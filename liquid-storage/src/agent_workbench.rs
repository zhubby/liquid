use liquid_core::{
    AgentAction, AgentActionKind, AgentActionStatus, AgentConversation, AgentEventRecord,
    AgentEventType, AgentMessage, AgentMessageRole, AgentResourceKind, AgentTurn, AgentTurnStatus,
    CreateAgentActionRequest, CreateAgentConversationRequest, CreateAgentTurnRequest,
    UpdateAgentConversationRequest,
};
use serde_json::Value;
use time::OffsetDateTime;

use crate::{
    error::{StorageError, map_database_error},
    store::Storage,
    validation::{optional_string, required_string},
};

const AGENT_CONVERSATION_COLUMNS: &str = r#"
id::text,
owner_user_id::text,
title,
created_at,
updated_at
"#;

const AGENT_MESSAGE_COLUMNS: &str = r#"
id::text,
conversation_id::text,
turn_id::text,
role,
content,
metadata,
created_at
"#;

const AGENT_TURN_COLUMNS: &str = r#"
id::text,
conversation_id::text,
status,
user_message_id::text,
assistant_message_id::text,
error,
client_request_id,
managed_database_id::text,
dashboard_context,
created_at,
updated_at,
completed_at
"#;

const AGENT_EVENT_COLUMNS: &str = r#"
seq,
turn_id::text,
event_type,
payload,
created_at
"#;

const AGENT_ACTION_COLUMNS: &str = r#"
id::text,
conversation_id::text,
turn_id::text,
kind,
status,
title,
description,
payload,
resource_kind,
resource_id::text,
requires_confirmation,
created_at,
updated_at
"#;

pub(crate) async fn list_agent_conversations(
    storage: &Storage,
    owner_user_id: &str,
    limit: i64,
) -> Result<Vec<AgentConversation>, StorageError> {
    let rows = sqlx::query_as::<_, AgentConversationRow>(&format!(
        r#"
        select {AGENT_CONVERSATION_COLUMNS}
        from agent_conversations
        where owner_user_id = $1::uuid
        order by updated_at desc
        limit $2
        "#
    ))
    .bind(owner_user_id)
    .bind(limit.clamp(1, 100))
    .fetch_all(&storage.pool)
    .await
    .map_err(map_database_error)?;

    rows.into_iter().map(AgentConversation::try_from).collect()
}

pub(crate) async fn create_agent_conversation(
    storage: &Storage,
    owner_user_id: &str,
    request: CreateAgentConversationRequest,
) -> Result<AgentConversation, StorageError> {
    let title =
        optional_string("title", request.title)?.unwrap_or_else(|| "New conversation".into());
    let row = sqlx::query_as::<_, AgentConversationRow>(&format!(
        r#"
        insert into agent_conversations (owner_user_id, title)
        values ($1::uuid, $2)
        returning {AGENT_CONVERSATION_COLUMNS}
        "#
    ))
    .bind(owner_user_id)
    .bind(title)
    .fetch_one(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.try_into()
}

pub(crate) async fn get_agent_conversation(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<AgentConversation, StorageError> {
    let row = sqlx::query_as::<_, AgentConversationRow>(&format!(
        r#"
        select {AGENT_CONVERSATION_COLUMNS}
        from agent_conversations
        where id = $1::uuid
          and owner_user_id = $2::uuid
        "#
    ))
    .bind(id)
    .bind(owner_user_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

pub(crate) async fn update_agent_conversation(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
    request: UpdateAgentConversationRequest,
) -> Result<AgentConversation, StorageError> {
    let title = optional_string("title", request.title)?;
    let row = sqlx::query_as::<_, AgentConversationRow>(&format!(
        r#"
        update agent_conversations
        set title = coalesce($3::text, title),
            updated_at = now()
        where id = $1::uuid
          and owner_user_id = $2::uuid
        returning {AGENT_CONVERSATION_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(owner_user_id)
    .bind(title)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

pub(crate) async fn delete_agent_conversation(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<(), StorageError> {
    let result = sqlx::query(
        r#"
        delete from agent_conversations
        where id = $1::uuid
          and owner_user_id = $2::uuid
        "#,
    )
    .bind(id)
    .bind(owner_user_id)
    .execute(&storage.pool)
    .await
    .map_err(map_database_error)?;

    if result.rows_affected() == 0 {
        return Err(StorageError::NotFound);
    }

    Ok(())
}

pub(crate) async fn list_agent_messages(
    storage: &Storage,
    owner_user_id: &str,
    conversation_id: &str,
    limit: i64,
    before_message_id: Option<&str>,
) -> Result<Vec<AgentMessage>, StorageError> {
    ensure_conversation(storage, owner_user_id, conversation_id).await?;
    let rows = sqlx::query_as::<_, AgentMessageRow>(&format!(
        r#"
        with before_message as (
            select created_at, id
            from agent_messages
            where id = $4::uuid
              and conversation_id = $1::uuid
              and owner_user_id = $2::uuid
        )
        select {AGENT_MESSAGE_COLUMNS}
        from agent_messages
        where conversation_id = $1::uuid
          and owner_user_id = $2::uuid
          and (
            $4::uuid is null
            or (created_at, id) < (select created_at, id from before_message)
          )
        order by created_at desc, id desc
        limit $3
        "#
    ))
    .bind(conversation_id)
    .bind(owner_user_id)
    .bind(limit.clamp(1, 200))
    .bind(before_message_id)
    .fetch_all(&storage.pool)
    .await
    .map_err(map_database_error)?;

    let mut messages = rows
        .into_iter()
        .map(AgentMessage::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    messages.reverse();
    Ok(messages)
}

pub(crate) async fn append_agent_message(
    storage: &Storage,
    owner_user_id: &str,
    conversation_id: &str,
    turn_id: Option<&str>,
    role: AgentMessageRole,
    content: &str,
    metadata: Option<Value>,
) -> Result<AgentMessage, StorageError> {
    let content = required_string("content", content)?;
    ensure_conversation(storage, owner_user_id, conversation_id).await?;
    let row = sqlx::query_as::<_, AgentMessageRow>(&format!(
        r#"
        insert into agent_messages (
            conversation_id,
            owner_user_id,
            turn_id,
            role,
            content,
            metadata
        )
        values ($1::uuid, $2::uuid, $3::uuid, $4, $5, $6)
        returning {AGENT_MESSAGE_COLUMNS}
        "#
    ))
    .bind(conversation_id)
    .bind(owner_user_id)
    .bind(turn_id)
    .bind(role.as_str())
    .bind(content)
    .bind(metadata)
    .fetch_one(&storage.pool)
    .await
    .map_err(map_database_error)?;

    touch_conversation(storage, owner_user_id, conversation_id).await?;
    row.try_into()
}

pub(crate) async fn create_agent_turn(
    storage: &Storage,
    owner_user_id: &str,
    conversation_id: &str,
    request: CreateAgentTurnRequest,
) -> Result<AgentTurn, StorageError> {
    let message = required_string("message", &request.message)?;
    ensure_conversation(storage, owner_user_id, conversation_id).await?;
    let dashboard_context = request
        .dashboard_context
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| StorageError::Validation(error.to_string()))?;

    let mut transaction = storage.pool.begin().await.map_err(map_database_error)?;
    let message_row = sqlx::query_as::<_, AgentMessageRow>(&format!(
        r#"
        insert into agent_messages (conversation_id, owner_user_id, role, content)
        values ($1::uuid, $2::uuid, 'user', $3)
        returning {AGENT_MESSAGE_COLUMNS}
        "#
    ))
    .bind(conversation_id)
    .bind(owner_user_id)
    .bind(message)
    .fetch_one(&mut *transaction)
    .await
    .map_err(map_database_error)?;
    let user_message = AgentMessage::try_from(message_row)?;

    let row = sqlx::query_as::<_, AgentTurnRow>(&format!(
        r#"
        insert into agent_turns (
            conversation_id,
            owner_user_id,
            user_message_id,
            client_request_id,
            managed_database_id,
            dashboard_context
        )
        values ($1::uuid, $2::uuid, $3::uuid, $4, $5::uuid, $6)
        returning {AGENT_TURN_COLUMNS}
        "#
    ))
    .bind(conversation_id)
    .bind(owner_user_id)
    .bind(&user_message.id)
    .bind(optional_string(
        "client_request_id",
        request.client_request_id,
    )?)
    .bind(request.managed_database_id)
    .bind(dashboard_context)
    .fetch_one(&mut *transaction)
    .await
    .map_err(map_database_error)?;

    sqlx::query(
        r#"
        update agent_messages
        set turn_id = $1::uuid
        where id = $2::uuid
          and owner_user_id = $3::uuid
        "#,
    )
    .bind(&row.id)
    .bind(&user_message.id)
    .bind(owner_user_id)
    .execute(&mut *transaction)
    .await
    .map_err(map_database_error)?;

    sqlx::query(
        r#"
        update agent_conversations
        set updated_at = now()
        where id = $1::uuid
          and owner_user_id = $2::uuid
        "#,
    )
    .bind(conversation_id)
    .bind(owner_user_id)
    .execute(&mut *transaction)
    .await
    .map_err(map_database_error)?;

    transaction.commit().await.map_err(map_database_error)?;
    row.try_into()
}

pub(crate) async fn get_agent_turn(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<AgentTurn, StorageError> {
    fetch_agent_turn(storage, owner_user_id, id).await
}

pub(crate) async fn update_agent_turn_status(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
    status: AgentTurnStatus,
    error: Option<String>,
) -> Result<AgentTurn, StorageError> {
    let error = optional_string("error", error)?;
    let row = sqlx::query_as::<_, AgentTurnRow>(&format!(
        r#"
        update agent_turns
        set status = $3,
            error = $4,
            updated_at = now(),
            completed_at = case
                when $5 then coalesce(completed_at, now())
                else completed_at
            end
        where id = $1::uuid
          and owner_user_id = $2::uuid
        returning {AGENT_TURN_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(owner_user_id)
    .bind(status.as_str())
    .bind(error)
    .bind(status.is_terminal())
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

pub(crate) async fn set_agent_turn_assistant_message(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
    assistant_message_id: &str,
) -> Result<AgentTurn, StorageError> {
    let row = sqlx::query_as::<_, AgentTurnRow>(&format!(
        r#"
        update agent_turns
        set assistant_message_id = $3::uuid,
            updated_at = now()
        where id = $1::uuid
          and owner_user_id = $2::uuid
        returning {AGENT_TURN_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(owner_user_id)
    .bind(assistant_message_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

pub(crate) async fn append_agent_turn_event(
    storage: &Storage,
    owner_user_id: &str,
    turn_id: &str,
    event_type: AgentEventType,
    payload: Value,
) -> Result<AgentEventRecord, StorageError> {
    let row = sqlx::query_as::<_, AgentEventRow>(&format!(
        r#"
        with next_seq as (
            select coalesce(max(seq), 0) + 1 as seq
            from agent_turn_events
            where turn_id = $2::uuid
        )
        insert into agent_turn_events (
            turn_id,
            owner_user_id,
            seq,
            event_type,
            payload
        )
        select t.id, t.owner_user_id, next_seq.seq, $3, $4
        from agent_turns t, next_seq
        where t.id = $2::uuid
          and t.owner_user_id = $1::uuid
        returning {AGENT_EVENT_COLUMNS}
        "#
    ))
    .bind(owner_user_id)
    .bind(turn_id)
    .bind(event_type.as_str())
    .bind(payload)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

pub(crate) async fn list_agent_turn_events(
    storage: &Storage,
    owner_user_id: &str,
    turn_id: &str,
    after_seq: i32,
) -> Result<Vec<AgentEventRecord>, StorageError> {
    let rows = sqlx::query_as::<_, AgentEventRow>(&format!(
        r#"
        select {AGENT_EVENT_COLUMNS}
        from agent_turn_events e
        join agent_turns t on t.id = e.turn_id
        where e.turn_id = $1::uuid
          and e.owner_user_id = $2::uuid
          and t.owner_user_id = $2::uuid
          and e.seq > $3
        order by e.seq
        "#
    ))
    .bind(turn_id)
    .bind(owner_user_id)
    .bind(after_seq.max(0))
    .fetch_all(&storage.pool)
    .await
    .map_err(map_database_error)?;

    if rows.is_empty() {
        fetch_agent_turn(storage, owner_user_id, turn_id).await?;
    }

    rows.into_iter().map(AgentEventRecord::try_from).collect()
}

pub(crate) async fn create_agent_action(
    storage: &Storage,
    owner_user_id: &str,
    turn_id: &str,
    request: CreateAgentActionRequest,
) -> Result<AgentAction, StorageError> {
    let title = required_string("title", &request.title)?;
    let description = required_string("description", &request.description)?;
    let row = sqlx::query_as::<_, AgentActionRow>(&format!(
        r#"
        insert into agent_actions (
            conversation_id,
            turn_id,
            owner_user_id,
            kind,
            title,
            description,
            payload,
            resource_kind,
            resource_id,
            requires_confirmation
        )
        select
            t.conversation_id,
            t.id,
            t.owner_user_id,
            $3,
            $4,
            $5,
            $6,
            $7,
            $8::uuid,
            $9
        from agent_turns t
        where t.id = $2::uuid
          and t.owner_user_id = $1::uuid
        returning {AGENT_ACTION_COLUMNS}
        "#
    ))
    .bind(owner_user_id)
    .bind(turn_id)
    .bind(request.kind.as_str())
    .bind(title)
    .bind(description)
    .bind(request.payload)
    .bind(request.resource_kind.map(AgentResourceKind::as_str))
    .bind(request.resource_id)
    .bind(request.requires_confirmation)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

pub(crate) async fn list_agent_actions(
    storage: &Storage,
    owner_user_id: &str,
    conversation_id: Option<&str>,
    status: Option<AgentActionStatus>,
) -> Result<Vec<AgentAction>, StorageError> {
    let rows = sqlx::query_as::<_, AgentActionRow>(&format!(
        r#"
        select {AGENT_ACTION_COLUMNS}
        from agent_actions
        where owner_user_id = $1::uuid
          and ($2::uuid is null or conversation_id = $2::uuid)
          and ($3::text is null or status = $3)
        order by created_at desc
        limit 100
        "#
    ))
    .bind(owner_user_id)
    .bind(conversation_id)
    .bind(status.map(AgentActionStatus::as_str))
    .fetch_all(&storage.pool)
    .await
    .map_err(map_database_error)?;

    rows.into_iter().map(AgentAction::try_from).collect()
}

pub(crate) async fn get_agent_action(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<AgentAction, StorageError> {
    fetch_agent_action(storage, owner_user_id, id).await
}

pub(crate) async fn update_agent_action_status(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
    status: AgentActionStatus,
    resource_kind: Option<AgentResourceKind>,
    resource_id: Option<String>,
) -> Result<AgentAction, StorageError> {
    let row = sqlx::query_as::<_, AgentActionRow>(&format!(
        r#"
        update agent_actions
        set status = $3,
            resource_kind = coalesce($4::text, resource_kind),
            resource_id = coalesce($5::uuid, resource_id),
            updated_at = now()
        where id = $1::uuid
          and owner_user_id = $2::uuid
          and status = 'proposed'
        returning {AGENT_ACTION_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(owner_user_id)
    .bind(status.as_str())
    .bind(resource_kind.map(AgentResourceKind::as_str))
    .bind(resource_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    if let Some(row) = row {
        return row.try_into();
    }

    let existing = fetch_agent_action(storage, owner_user_id, id).await?;
    Err(StorageError::Conflict(format!(
        "agent action is already {}",
        existing.status.as_str()
    )))
}

pub(crate) async fn fail_stale_agent_turns(
    storage: &Storage,
    stale_after_seconds: i64,
) -> Result<u64, StorageError> {
    let result = sqlx::query(
        r#"
        update agent_turns
        set status = 'failed',
            error = 'agent turn did not complete before server restart',
            updated_at = now(),
            completed_at = now()
        where status in ('queued', 'running')
          and updated_at < now() - make_interval(secs => $1)
        "#,
    )
    .bind(stale_after_seconds.max(1))
    .execute(&storage.pool)
    .await
    .map_err(map_database_error)?;

    Ok(result.rows_affected())
}

async fn ensure_conversation(
    storage: &Storage,
    owner_user_id: &str,
    conversation_id: &str,
) -> Result<(), StorageError> {
    get_agent_conversation(storage, owner_user_id, conversation_id)
        .await
        .map(|_| ())
}

async fn touch_conversation(
    storage: &Storage,
    owner_user_id: &str,
    conversation_id: &str,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        update agent_conversations
        set updated_at = now()
        where id = $1::uuid
          and owner_user_id = $2::uuid
        "#,
    )
    .bind(conversation_id)
    .bind(owner_user_id)
    .execute(&storage.pool)
    .await
    .map_err(map_database_error)?;

    Ok(())
}

async fn fetch_agent_turn(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<AgentTurn, StorageError> {
    let row = sqlx::query_as::<_, AgentTurnRow>(&format!(
        r#"
        select {AGENT_TURN_COLUMNS}
        from agent_turns
        where id = $1::uuid
          and owner_user_id = $2::uuid
        "#
    ))
    .bind(id)
    .bind(owner_user_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

async fn fetch_agent_action(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<AgentAction, StorageError> {
    let row = sqlx::query_as::<_, AgentActionRow>(&format!(
        r#"
        select {AGENT_ACTION_COLUMNS}
        from agent_actions
        where id = $1::uuid
          and owner_user_id = $2::uuid
        "#
    ))
    .bind(id)
    .bind(owner_user_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

#[derive(Debug)]
struct AgentConversationRow {
    id: String,
    owner_user_id: String,
    title: String,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

#[derive(Debug)]
struct AgentMessageRow {
    id: String,
    conversation_id: String,
    turn_id: Option<String>,
    role: String,
    content: String,
    metadata: Option<Value>,
    created_at: OffsetDateTime,
}

#[derive(Debug)]
struct AgentTurnRow {
    id: String,
    conversation_id: String,
    status: String,
    user_message_id: String,
    assistant_message_id: Option<String>,
    error: Option<String>,
    client_request_id: Option<String>,
    managed_database_id: Option<String>,
    dashboard_context: Option<Value>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    completed_at: Option<OffsetDateTime>,
}

#[derive(Debug)]
struct AgentEventRow {
    seq: i32,
    turn_id: String,
    event_type: String,
    payload: Value,
    created_at: OffsetDateTime,
}

#[derive(Debug)]
struct AgentActionRow {
    id: String,
    conversation_id: String,
    turn_id: String,
    kind: String,
    status: String,
    title: String,
    description: String,
    payload: Value,
    resource_kind: Option<String>,
    resource_id: Option<String>,
    requires_confirmation: bool,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for AgentConversationRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        Ok(Self {
            id: row.try_get("id")?,
            owner_user_id: row.try_get("owner_user_id")?,
            title: row.try_get("title")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for AgentMessageRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        Ok(Self {
            id: row.try_get("id")?,
            conversation_id: row.try_get("conversation_id")?,
            turn_id: row.try_get("turn_id")?,
            role: row.try_get("role")?,
            content: row.try_get("content")?,
            metadata: row.try_get("metadata")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for AgentTurnRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        Ok(Self {
            id: row.try_get("id")?,
            conversation_id: row.try_get("conversation_id")?,
            status: row.try_get("status")?,
            user_message_id: row.try_get("user_message_id")?,
            assistant_message_id: row.try_get("assistant_message_id")?,
            error: row.try_get("error")?,
            client_request_id: row.try_get("client_request_id")?,
            managed_database_id: row.try_get("managed_database_id")?,
            dashboard_context: row.try_get("dashboard_context")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            completed_at: row.try_get("completed_at")?,
        })
    }
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for AgentEventRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        Ok(Self {
            seq: row.try_get("seq")?,
            turn_id: row.try_get("turn_id")?,
            event_type: row.try_get("event_type")?,
            payload: row.try_get("payload")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for AgentActionRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;

        Ok(Self {
            id: row.try_get("id")?,
            conversation_id: row.try_get("conversation_id")?,
            turn_id: row.try_get("turn_id")?,
            kind: row.try_get("kind")?,
            status: row.try_get("status")?,
            title: row.try_get("title")?,
            description: row.try_get("description")?,
            payload: row.try_get("payload")?,
            resource_kind: row.try_get("resource_kind")?,
            resource_id: row.try_get("resource_id")?,
            requires_confirmation: row.try_get("requires_confirmation")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

impl TryFrom<AgentConversationRow> for AgentConversation {
    type Error = StorageError;

    fn try_from(row: AgentConversationRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            owner_user_id: row.owner_user_id,
            title: row.title,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

impl TryFrom<AgentMessageRow> for AgentMessage {
    type Error = StorageError;

    fn try_from(row: AgentMessageRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            conversation_id: row.conversation_id,
            turn_id: row.turn_id,
            role: parse_message_role(&row.role)?,
            content: row.content,
            metadata: row.metadata,
            created_at: row.created_at,
        })
    }
}

impl TryFrom<AgentTurnRow> for AgentTurn {
    type Error = StorageError;

    fn try_from(row: AgentTurnRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            conversation_id: row.conversation_id,
            status: parse_turn_status(&row.status)?,
            user_message_id: row.user_message_id,
            assistant_message_id: row.assistant_message_id,
            error: row.error,
            client_request_id: row.client_request_id,
            managed_database_id: row.managed_database_id,
            dashboard_context: row
                .dashboard_context
                .filter(|value| !value.is_null())
                .map(serde_json::from_value)
                .transpose()
                .map_err(json_storage_error)?,
            created_at: row.created_at,
            updated_at: row.updated_at,
            completed_at: row.completed_at,
        })
    }
}

impl TryFrom<AgentEventRow> for AgentEventRecord {
    type Error = StorageError;

    fn try_from(row: AgentEventRow) -> Result<Self, Self::Error> {
        Ok(Self {
            seq: row.seq,
            turn_id: row.turn_id,
            event_type: parse_event_type(&row.event_type)?,
            payload: row.payload,
            created_at: row.created_at,
        })
    }
}

impl TryFrom<AgentActionRow> for AgentAction {
    type Error = StorageError;

    fn try_from(row: AgentActionRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            conversation_id: row.conversation_id,
            turn_id: row.turn_id,
            kind: parse_action_kind(&row.kind)?,
            status: parse_action_status(&row.status)?,
            title: row.title,
            description: row.description,
            payload: row.payload,
            resource_kind: row
                .resource_kind
                .as_deref()
                .map(parse_resource_kind)
                .transpose()?,
            resource_id: row.resource_id,
            requires_confirmation: row.requires_confirmation,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

fn parse_message_role(value: &str) -> Result<AgentMessageRole, StorageError> {
    match value {
        "user" => Ok(AgentMessageRole::User),
        "assistant" => Ok(AgentMessageRole::Assistant),
        "tool" => Ok(AgentMessageRole::Tool),
        "system" => Ok(AgentMessageRole::System),
        other => Err(StorageError::Validation(format!(
            "unsupported agent message role: {other}"
        ))),
    }
}

fn parse_turn_status(value: &str) -> Result<AgentTurnStatus, StorageError> {
    match value {
        "queued" => Ok(AgentTurnStatus::Queued),
        "running" => Ok(AgentTurnStatus::Running),
        "completed" => Ok(AgentTurnStatus::Completed),
        "blocked" => Ok(AgentTurnStatus::Blocked),
        "failed" => Ok(AgentTurnStatus::Failed),
        "cancelled" => Ok(AgentTurnStatus::Cancelled),
        other => Err(StorageError::Validation(format!(
            "unsupported agent turn status: {other}"
        ))),
    }
}

fn parse_event_type(value: &str) -> Result<AgentEventType, StorageError> {
    match value {
        "turn_started" => Ok(AgentEventType::TurnStarted),
        "message_created" => Ok(AgentEventType::MessageCreated),
        "assistant_delta" => Ok(AgentEventType::AssistantDelta),
        "tool_call_started" => Ok(AgentEventType::ToolCallStarted),
        "tool_call_finished" => Ok(AgentEventType::ToolCallFinished),
        "resource_created" => Ok(AgentEventType::ResourceCreated),
        "resource_updated" => Ok(AgentEventType::ResourceUpdated),
        "action_proposed" => Ok(AgentEventType::ActionProposed),
        "turn_completed" => Ok(AgentEventType::TurnCompleted),
        "turn_failed" => Ok(AgentEventType::TurnFailed),
        other => Err(StorageError::Validation(format!(
            "unsupported agent event type: {other}"
        ))),
    }
}

fn parse_action_kind(value: &str) -> Result<AgentActionKind, StorageError> {
    match value {
        "create_sql_audit" => Ok(AgentActionKind::CreateSqlAudit),
        "approve_sql_audit" => Ok(AgentActionKind::ApproveSqlAudit),
        "reject_sql_audit" => Ok(AgentActionKind::RejectSqlAudit),
        "execute_sql_audit" => Ok(AgentActionKind::ExecuteSqlAudit),
        "create_managed_database" => Ok(AgentActionKind::CreateManagedDatabase),
        "update_managed_database" => Ok(AgentActionKind::UpdateManagedDatabase),
        "delete_managed_database" => Ok(AgentActionKind::DeleteManagedDatabase),
        "start_database_backup" => Ok(AgentActionKind::StartDatabaseBackup),
        "start_database_restore" => Ok(AgentActionKind::StartDatabaseRestore),
        other => Err(StorageError::Validation(format!(
            "unsupported agent action kind: {other}"
        ))),
    }
}

fn parse_action_status(value: &str) -> Result<AgentActionStatus, StorageError> {
    match value {
        "proposed" => Ok(AgentActionStatus::Proposed),
        "applied" => Ok(AgentActionStatus::Applied),
        "rejected" => Ok(AgentActionStatus::Rejected),
        "failed" => Ok(AgentActionStatus::Failed),
        "superseded" => Ok(AgentActionStatus::Superseded),
        other => Err(StorageError::Validation(format!(
            "unsupported agent action status: {other}"
        ))),
    }
}

fn parse_resource_kind(value: &str) -> Result<AgentResourceKind, StorageError> {
    match value {
        "sql_audit" => Ok(AgentResourceKind::SqlAudit),
        "managed_database" => Ok(AgentResourceKind::ManagedDatabase),
        "database_backup" => Ok(AgentResourceKind::DatabaseBackup),
        "database_restore" => Ok(AgentResourceKind::DatabaseRestore),
        other => Err(StorageError::Validation(format!(
            "unsupported agent resource kind: {other}"
        ))),
    }
}

fn json_storage_error(error: serde_json::Error) -> StorageError {
    StorageError::Validation(error.to_string())
}
