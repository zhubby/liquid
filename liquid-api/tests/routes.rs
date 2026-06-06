use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{
        Request, StatusCode,
        header::{
            ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
            ACCESS_CONTROL_REQUEST_HEADERS, ACCESS_CONTROL_REQUEST_METHOD, AUTHORIZATION,
            CONTENT_TYPE, ORIGIN,
        },
    },
};
use liquid_agent::{
    AgentStream, ApprovedWriteExecutionResult, MockSqlAuditAgent, PostgresToolConfig,
    PostgresToolExecutionMode, SqlAuditAgent, ToolRegistry,
};
use liquid_api::{
    ApiState, ApprovedSqlExecutionFuture, ApprovedSqlExecutor, ManagedDatabaseConnectionTestFuture,
    ManagedDatabaseConnectionTester, router, router_with_cors,
};
use liquid_core::{
    AgentAction, AgentActionStatus, AgentConversation, AgentEventRecord, AgentEventType,
    AgentMessage, AgentMessageRole, AgentResourceKind, AgentTurn, AgentTurnStatus,
    ApproveSqlAuditRequest, AuditSummary, AuthResponse, CreateAgentActionRequest,
    CreateAgentConversationRequest, CreateAgentTurnRequest, CreateManagedDatabaseRequest,
    LlmProviderApiMode, LlmProviderKind, LlmProviderSettings, LoginRequest, ManagedDatabase,
    ManagedDatabaseConnectionLoader, ManagedDatabaseConnectionLoaderError,
    ManagedDatabaseConnectionSpec, ManagedDatabaseEngine, ManagedDatabasePoolKey,
    ManagedDatabasePoolPolicy, ManagedDatabaseSslMode, PublicUser, RegisterRequest,
    RejectSqlAuditRequest, ResolvedLlmProviderSettings, SqlAuditExecutionResult, SqlAuditRecord,
    SqlAuditReport, SqlAuditRequest, SqlAuditStatus, UpdateAgentConversationRequest,
    UpdateCurrentUserRequest, UpdateLlmProviderSettingsRequest, UpdateManagedDatabaseRequest,
    UpdatePasswordRequest,
};
use liquid_sql::{PgSqlAnalysisRequest, PgSqlStatementKind, analyze_postgres_sql};
use liquid_storage::{
    CreateSqlAuditRecord, LiquidStore, ManagedDatabasePoolConnector, ManagedDatabasePoolError,
    ManagedDatabasePoolManager, StorageError,
};
use serde_json::{Value, json};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tower::ServiceExt;

const VALID_TOKEN: &str = "valid-token";

struct TestStore {
    revoked: Mutex<bool>,
    databases: Mutex<Vec<ManagedDatabase>>,
    current_database_id: Mutex<Option<String>>,
    audits: Mutex<Vec<SqlAuditRecord>>,
    conversations: Mutex<Vec<AgentConversation>>,
    messages: Mutex<Vec<AgentMessage>>,
    turns: Mutex<Vec<AgentTurn>>,
    events: Mutex<Vec<AgentEventRecord>>,
    actions: Mutex<Vec<AgentAction>>,
    user: Mutex<PublicUser>,
    llm_settings: Mutex<Option<ResolvedLlmProviderSettings>>,
}

impl Default for TestStore {
    fn default() -> Self {
        Self {
            revoked: Mutex::new(false),
            databases: Mutex::new(Vec::new()),
            current_database_id: Mutex::new(None),
            audits: Mutex::new(Vec::new()),
            conversations: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
            turns: Mutex::new(Vec::new()),
            events: Mutex::new(Vec::new()),
            actions: Mutex::new(Vec::new()),
            user: Mutex::new(test_user()),
            llm_settings: Mutex::new(None),
        }
    }
}

#[async_trait]
impl LiquidStore for TestStore {
    async fn register_user(&self, request: RegisterRequest) -> Result<AuthResponse, StorageError> {
        Ok(test_auth_response(request.email, request.display_name))
    }

    async fn login_user(&self, request: LoginRequest) -> Result<AuthResponse, StorageError> {
        if request.email == "user@test.local" && request.password == "password123" {
            Ok(test_auth_response(
                "user@test.local".to_owned(),
                "Test User".to_owned(),
            ))
        } else {
            Err(StorageError::InvalidCredentials)
        }
    }

    async fn authenticate_token(&self, token: &str) -> Result<Option<PublicUser>, StorageError> {
        if token == VALID_TOKEN && !*self.revoked.lock().unwrap() {
            Ok(Some(self.user.lock().unwrap().clone()))
        } else {
            Ok(None)
        }
    }

    async fn update_current_user(
        &self,
        _owner_user_id: &str,
        request: UpdateCurrentUserRequest,
    ) -> Result<PublicUser, StorageError> {
        if request.display_name.trim().is_empty() {
            return Err(StorageError::Validation(
                "display_name is required".to_owned(),
            ));
        }

        let mut user = self.user.lock().unwrap();
        user.display_name = request.display_name.trim().to_owned();

        Ok(user.clone())
    }

    async fn update_password(
        &self,
        _owner_user_id: &str,
        request: UpdatePasswordRequest,
    ) -> Result<(), StorageError> {
        if request.current_password != "password123" {
            return Err(StorageError::InvalidCredentials);
        }
        if request.new_password.len() < 8 {
            return Err(StorageError::Validation(
                "password must be at least 8 characters".to_owned(),
            ));
        }

        Ok(())
    }

    async fn revoke_token(&self, token: &str) -> Result<(), StorageError> {
        if token == VALID_TOKEN {
            *self.revoked.lock().unwrap() = true;
        }

        Ok(())
    }

    async fn get_llm_provider_settings(
        &self,
        _owner_user_id: &str,
    ) -> Result<Option<LlmProviderSettings>, StorageError> {
        Ok(self
            .llm_settings
            .lock()
            .unwrap()
            .as_ref()
            .map(public_llm_settings))
    }

    async fn upsert_llm_provider_settings(
        &self,
        _owner_user_id: &str,
        request: UpdateLlmProviderSettingsRequest,
    ) -> Result<LlmProviderSettings, StorageError> {
        let mut settings = self.llm_settings.lock().unwrap();
        let api_key = request
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                settings
                    .as_ref()
                    .and_then(|settings| settings.api_key.clone())
            });
        let resolved = ResolvedLlmProviderSettings {
            provider: request.provider,
            base_url: request.base_url,
            model: request.model,
            api_mode: request.api_mode,
            api_key,
        };
        let public = public_llm_settings(&resolved);
        *settings = Some(resolved);

        Ok(public)
    }

    async fn resolve_llm_provider_settings(
        &self,
        _owner_user_id: &str,
    ) -> Result<Option<ResolvedLlmProviderSettings>, StorageError> {
        Ok(self.llm_settings.lock().unwrap().clone())
    }

    async fn list_managed_databases(
        &self,
        _owner_user_id: &str,
    ) -> Result<Vec<ManagedDatabase>, StorageError> {
        Ok(self.databases.lock().unwrap().clone())
    }

    async fn get_current_managed_database(
        &self,
        _owner_user_id: &str,
    ) -> Result<Option<ManagedDatabase>, StorageError> {
        let current_id = self.current_database_id.lock().unwrap().clone();
        let Some(current_id) = current_id else {
            return Ok(None);
        };

        Ok(self
            .databases
            .lock()
            .unwrap()
            .iter()
            .find(|database| database.id == current_id)
            .cloned())
    }

    async fn set_current_managed_database(
        &self,
        _owner_user_id: &str,
        managed_database_id: &str,
    ) -> Result<ManagedDatabase, StorageError> {
        let database = self
            .databases
            .lock()
            .unwrap()
            .iter()
            .find(|database| database.id == managed_database_id)
            .cloned()
            .ok_or(StorageError::NotFound)?;

        *self.current_database_id.lock().unwrap() = Some(managed_database_id.to_owned());

        Ok(database)
    }

    async fn clear_current_managed_database(
        &self,
        _owner_user_id: &str,
    ) -> Result<(), StorageError> {
        *self.current_database_id.lock().unwrap() = None;

        Ok(())
    }

    async fn create_managed_database(
        &self,
        _owner_user_id: &str,
        request: CreateManagedDatabaseRequest,
    ) -> Result<ManagedDatabase, StorageError> {
        let mut databases = self.databases.lock().unwrap();
        let database = ManagedDatabase {
            id: format!("db-{}", databases.len() + 1),
            name: request.name,
            engine: request.engine,
            host: request.host,
            port: request.port,
            database: request.database,
            username: request.username,
            ssl_mode: request.ssl_mode,
            has_password: true,
        };
        databases.push(database.clone());
        Ok(database)
    }

    async fn update_managed_database(
        &self,
        _owner_user_id: &str,
        id: &str,
        request: UpdateManagedDatabaseRequest,
    ) -> Result<ManagedDatabase, StorageError> {
        let mut databases = self.databases.lock().unwrap();
        let Some(database) = databases.iter_mut().find(|database| database.id == id) else {
            return Err(StorageError::NotFound);
        };

        if let Some(name) = request.name {
            database.name = name;
        }
        if let Some(host) = request.host {
            database.host = host;
        }
        if let Some(port) = request.port {
            database.port = port;
        }
        if let Some(database_name) = request.database {
            database.database = database_name;
        }
        if let Some(username) = request.username {
            database.username = username;
        }
        if let Some(ssl_mode) = request.ssl_mode {
            database.ssl_mode = ssl_mode;
        }

        Ok(database.clone())
    }

    async fn delete_managed_database(
        &self,
        _owner_user_id: &str,
        id: &str,
    ) -> Result<(), StorageError> {
        let mut databases = self.databases.lock().unwrap();
        let before = databases.len();
        databases.retain(|database| database.id != id);

        if databases.len() == before {
            return Err(StorageError::NotFound);
        }

        if self.current_database_id.lock().unwrap().as_deref() == Some(id) {
            *self.current_database_id.lock().unwrap() = None;
        }

        Ok(())
    }

    async fn create_sql_audit(
        &self,
        owner_user_id: &str,
        managed_database_id: &str,
        record: CreateSqlAuditRecord,
    ) -> Result<SqlAuditRecord, StorageError> {
        let CreateSqlAuditRecord {
            request,
            report,
            deterministic_analysis,
            statement_kind,
            status,
            risk_score,
        } = record;
        let database = self
            .databases
            .lock()
            .unwrap()
            .iter()
            .find(|database| database.id == managed_database_id)
            .cloned()
            .ok_or(StorageError::NotFound)?;
        let mut audits = self.audits.lock().unwrap();
        let record = SqlAuditRecord {
            id: format!("audit-{}", audits.len() + 1),
            owner_user_id: owner_user_id.to_owned(),
            managed_database_id: managed_database_id.to_owned(),
            managed_database_name: database.name,
            managed_database_engine: database.engine.as_str().to_owned(),
            managed_database_host: database.host,
            managed_database_port: database.port,
            managed_database_database: database.database,
            managed_database_username: database.username,
            managed_database_ssl_mode: database.ssl_mode.as_str().to_owned(),
            sql: request.sql,
            schema: request.schema,
            context: request.context,
            execution_purpose: request.execution_purpose,
            status,
            statement_kind,
            risk_score,
            report: Some(report),
            deterministic_analysis: Some(deterministic_analysis),
            approved_by_user_id: None,
            approved_at: None,
            approval_comment: None,
            rejected_by_user_id: None,
            rejected_at: None,
            rejection_comment: None,
            execution_result: None,
            execution_error: None,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
            executed_at: None,
        };
        audits.push(record.clone());
        Ok(record)
    }

    async fn list_sql_audits(
        &self,
        owner_user_id: &str,
        managed_database_id: Option<&str>,
        status: Option<SqlAuditStatus>,
        limit: i64,
    ) -> Result<Vec<SqlAuditRecord>, StorageError> {
        let audits = self.audits.lock().unwrap();
        Ok(audits
            .iter()
            .filter(|record| record.owner_user_id == owner_user_id)
            .filter(|record| {
                managed_database_id
                    .map(|id| record.managed_database_id == id)
                    .unwrap_or(true)
            })
            .filter(|record| status.map(|status| record.status == status).unwrap_or(true))
            .take(limit.clamp(1, 100) as usize)
            .cloned()
            .collect())
    }

    async fn get_sql_audit(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<SqlAuditRecord, StorageError> {
        self.audits
            .lock()
            .unwrap()
            .iter()
            .find(|record| record.id == id && record.owner_user_id == owner_user_id)
            .cloned()
            .ok_or(StorageError::NotFound)
    }

    async fn approve_sql_audit(
        &self,
        owner_user_id: &str,
        id: &str,
        request: ApproveSqlAuditRequest,
    ) -> Result<SqlAuditRecord, StorageError> {
        let mut audits = self.audits.lock().unwrap();
        let Some(record) = audits
            .iter_mut()
            .find(|record| record.id == id && record.owner_user_id == owner_user_id)
        else {
            return Err(StorageError::NotFound);
        };
        if !matches!(record.status, SqlAuditStatus::PendingApproval) {
            return Err(StorageError::Conflict(
                "only pending approval audits can be approved".to_owned(),
            ));
        }
        record.status = SqlAuditStatus::Approved;
        record.approved_by_user_id = Some(owner_user_id.to_owned());
        record.approval_comment = request.comment;
        Ok(record.clone())
    }

    async fn reject_sql_audit(
        &self,
        owner_user_id: &str,
        id: &str,
        request: RejectSqlAuditRequest,
    ) -> Result<SqlAuditRecord, StorageError> {
        let mut audits = self.audits.lock().unwrap();
        let Some(record) = audits
            .iter_mut()
            .find(|record| record.id == id && record.owner_user_id == owner_user_id)
        else {
            return Err(StorageError::NotFound);
        };
        if !matches!(record.status, SqlAuditStatus::PendingApproval) {
            return Err(StorageError::Conflict(
                "only pending approval audits can be rejected".to_owned(),
            ));
        }
        record.status = SqlAuditStatus::Rejected;
        record.rejected_by_user_id = Some(owner_user_id.to_owned());
        record.rejection_comment = request.comment;
        Ok(record.clone())
    }

    async fn start_sql_audit_execution(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<SqlAuditRecord, StorageError> {
        let mut audits = self.audits.lock().unwrap();
        let Some(record) = audits
            .iter_mut()
            .find(|record| record.id == id && record.owner_user_id == owner_user_id)
        else {
            return Err(StorageError::NotFound);
        };
        if !matches!(record.status, SqlAuditStatus::Approved) {
            return Err(StorageError::Conflict(
                "only approved audits can be executed".to_owned(),
            ));
        }
        record.status = SqlAuditStatus::Executing;
        Ok(record.clone())
    }

    async fn complete_sql_audit_execution(
        &self,
        owner_user_id: &str,
        id: &str,
        result: SqlAuditExecutionResult,
    ) -> Result<SqlAuditRecord, StorageError> {
        let mut audits = self.audits.lock().unwrap();
        let Some(record) = audits
            .iter_mut()
            .find(|record| record.id == id && record.owner_user_id == owner_user_id)
        else {
            return Err(StorageError::NotFound);
        };
        if !matches!(record.status, SqlAuditStatus::Executing) {
            return Err(StorageError::Conflict(
                "only executing audits can be completed".to_owned(),
            ));
        }
        record.status = SqlAuditStatus::Executed;
        record.execution_result = Some(result);
        record.executed_at = Some(time::OffsetDateTime::UNIX_EPOCH);
        Ok(record.clone())
    }

    async fn fail_sql_audit_execution(
        &self,
        owner_user_id: &str,
        id: &str,
        error: String,
    ) -> Result<SqlAuditRecord, StorageError> {
        let mut audits = self.audits.lock().unwrap();
        let Some(record) = audits
            .iter_mut()
            .find(|record| record.id == id && record.owner_user_id == owner_user_id)
        else {
            return Err(StorageError::NotFound);
        };
        if !matches!(record.status, SqlAuditStatus::Executing) {
            return Err(StorageError::Conflict(
                "only executing audits can fail".to_owned(),
            ));
        }
        record.status = SqlAuditStatus::ExecutionFailed;
        record.execution_error = Some(error);
        Ok(record.clone())
    }

    async fn list_agent_conversations(
        &self,
        owner_user_id: &str,
        limit: i64,
    ) -> Result<Vec<AgentConversation>, StorageError> {
        Ok(self
            .conversations
            .lock()
            .unwrap()
            .iter()
            .filter(|conversation| conversation.owner_user_id == owner_user_id)
            .take(limit.clamp(1, 100) as usize)
            .cloned()
            .collect())
    }

    async fn create_agent_conversation(
        &self,
        owner_user_id: &str,
        request: CreateAgentConversationRequest,
    ) -> Result<AgentConversation, StorageError> {
        let mut conversations = self.conversations.lock().unwrap();
        let conversation = AgentConversation {
            id: format!("conversation-{}", conversations.len() + 1),
            owner_user_id: owner_user_id.to_owned(),
            title: request
                .title
                .unwrap_or_else(|| "New conversation".to_owned()),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
        };
        conversations.push(conversation.clone());
        Ok(conversation)
    }

    async fn get_agent_conversation(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<AgentConversation, StorageError> {
        self.conversations
            .lock()
            .unwrap()
            .iter()
            .find(|conversation| {
                conversation.id == id && conversation.owner_user_id == owner_user_id
            })
            .cloned()
            .ok_or(StorageError::NotFound)
    }

    async fn update_agent_conversation(
        &self,
        owner_user_id: &str,
        id: &str,
        request: UpdateAgentConversationRequest,
    ) -> Result<AgentConversation, StorageError> {
        let mut conversations = self.conversations.lock().unwrap();
        let Some(conversation) = conversations.iter_mut().find(|conversation| {
            conversation.id == id && conversation.owner_user_id == owner_user_id
        }) else {
            return Err(StorageError::NotFound);
        };

        if let Some(title) = request.title {
            conversation.title = title;
        }

        Ok(conversation.clone())
    }

    async fn delete_agent_conversation(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<(), StorageError> {
        let mut conversations = self.conversations.lock().unwrap();
        let before = conversations.len();

        conversations.retain(|conversation| {
            !(conversation.id == id && conversation.owner_user_id == owner_user_id)
        });

        if conversations.len() == before {
            return Err(StorageError::NotFound);
        }

        Ok(())
    }

    async fn list_agent_messages(
        &self,
        owner_user_id: &str,
        conversation_id: &str,
        limit: i64,
        _before_message_id: Option<&str>,
    ) -> Result<Vec<AgentMessage>, StorageError> {
        self.get_agent_conversation(owner_user_id, conversation_id)
            .await?;
        Ok(self
            .messages
            .lock()
            .unwrap()
            .iter()
            .filter(|message| message.conversation_id == conversation_id)
            .take(limit.clamp(1, 200) as usize)
            .cloned()
            .collect())
    }

    async fn append_agent_message(
        &self,
        owner_user_id: &str,
        conversation_id: &str,
        turn_id: Option<&str>,
        role: AgentMessageRole,
        content: &str,
        metadata: Option<Value>,
    ) -> Result<AgentMessage, StorageError> {
        self.get_agent_conversation(owner_user_id, conversation_id)
            .await?;
        let mut messages = self.messages.lock().unwrap();
        let message = AgentMessage {
            id: format!("message-{}", messages.len() + 1),
            conversation_id: conversation_id.to_owned(),
            turn_id: turn_id.map(str::to_owned),
            role,
            content: content.to_owned(),
            metadata,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
        };
        messages.push(message.clone());
        Ok(message)
    }

    async fn create_agent_turn(
        &self,
        owner_user_id: &str,
        conversation_id: &str,
        request: CreateAgentTurnRequest,
    ) -> Result<AgentTurn, StorageError> {
        self.get_agent_conversation(owner_user_id, conversation_id)
            .await?;
        let user_message = self
            .append_agent_message(
                owner_user_id,
                conversation_id,
                None,
                AgentMessageRole::User,
                &request.message,
                None,
            )
            .await?;
        let mut turns = self.turns.lock().unwrap();
        let turn = AgentTurn {
            id: format!("turn-{}", turns.len() + 1),
            conversation_id: conversation_id.to_owned(),
            status: AgentTurnStatus::Queued,
            user_message_id: user_message.id.clone(),
            assistant_message_id: None,
            error: None,
            client_request_id: request.client_request_id,
            managed_database_id: request.managed_database_id,
            dashboard_context: request.dashboard_context,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
            completed_at: None,
        };
        turns.push(turn.clone());
        self.messages
            .lock()
            .unwrap()
            .iter_mut()
            .find(|message| message.id == user_message.id)
            .unwrap()
            .turn_id = Some(turn.id.clone());
        Ok(turn)
    }

    async fn get_agent_turn(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<AgentTurn, StorageError> {
        let turn = self
            .turns
            .lock()
            .unwrap()
            .iter()
            .find(|turn| turn.id == id)
            .cloned()
            .ok_or(StorageError::NotFound)?;
        self.get_agent_conversation(owner_user_id, &turn.conversation_id)
            .await?;
        Ok(turn)
    }

    async fn update_agent_turn_status(
        &self,
        owner_user_id: &str,
        id: &str,
        status: AgentTurnStatus,
        error: Option<String>,
    ) -> Result<AgentTurn, StorageError> {
        let conversation_id = self
            .turns
            .lock()
            .unwrap()
            .iter()
            .find(|turn| turn.id == id)
            .map(|turn| turn.conversation_id.clone())
            .ok_or(StorageError::NotFound)?;
        self.get_agent_conversation(owner_user_id, &conversation_id)
            .await?;
        let mut turns = self.turns.lock().unwrap();
        let Some(turn) = turns.iter_mut().find(|turn| turn.id == id) else {
            return Err(StorageError::NotFound);
        };
        turn.status = status;
        turn.error = error;
        if status.is_terminal() {
            turn.completed_at = Some(time::OffsetDateTime::UNIX_EPOCH);
        }
        Ok(turn.clone())
    }

    async fn set_agent_turn_assistant_message(
        &self,
        owner_user_id: &str,
        id: &str,
        assistant_message_id: &str,
    ) -> Result<AgentTurn, StorageError> {
        let conversation_id = self
            .turns
            .lock()
            .unwrap()
            .iter()
            .find(|turn| turn.id == id)
            .map(|turn| turn.conversation_id.clone())
            .ok_or(StorageError::NotFound)?;
        self.get_agent_conversation(owner_user_id, &conversation_id)
            .await?;
        let mut turns = self.turns.lock().unwrap();
        let Some(turn) = turns.iter_mut().find(|turn| turn.id == id) else {
            return Err(StorageError::NotFound);
        };
        turn.assistant_message_id = Some(assistant_message_id.to_owned());
        Ok(turn.clone())
    }

    async fn append_agent_turn_event(
        &self,
        owner_user_id: &str,
        turn_id: &str,
        event_type: AgentEventType,
        payload: Value,
    ) -> Result<AgentEventRecord, StorageError> {
        self.get_agent_turn(owner_user_id, turn_id).await?;
        let mut events = self.events.lock().unwrap();
        let seq = events
            .iter()
            .filter(|event| event.turn_id == turn_id)
            .map(|event| event.seq)
            .max()
            .unwrap_or(0)
            + 1;
        let event = AgentEventRecord {
            seq,
            turn_id: turn_id.to_owned(),
            event_type,
            payload,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
        };
        events.push(event.clone());
        Ok(event)
    }

    async fn list_agent_turn_events(
        &self,
        owner_user_id: &str,
        turn_id: &str,
        after_seq: i32,
    ) -> Result<Vec<AgentEventRecord>, StorageError> {
        self.get_agent_turn(owner_user_id, turn_id).await?;
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.turn_id == turn_id && event.seq > after_seq)
            .cloned()
            .collect())
    }

    async fn create_agent_action(
        &self,
        owner_user_id: &str,
        turn_id: &str,
        request: CreateAgentActionRequest,
    ) -> Result<AgentAction, StorageError> {
        let turn = self.get_agent_turn(owner_user_id, turn_id).await?;
        let mut actions = self.actions.lock().unwrap();
        let action = AgentAction {
            id: format!("action-{}", actions.len() + 1),
            conversation_id: turn.conversation_id,
            turn_id: turn_id.to_owned(),
            kind: request.kind,
            status: AgentActionStatus::Proposed,
            title: request.title,
            description: request.description,
            payload: request.payload,
            resource_kind: request.resource_kind,
            resource_id: request.resource_id,
            requires_confirmation: request.requires_confirmation,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
        };
        actions.push(action.clone());
        Ok(action)
    }

    async fn list_agent_actions(
        &self,
        owner_user_id: &str,
        conversation_id: Option<&str>,
        status: Option<AgentActionStatus>,
    ) -> Result<Vec<AgentAction>, StorageError> {
        if let Some(conversation_id) = conversation_id {
            self.get_agent_conversation(owner_user_id, conversation_id)
                .await?;
        }

        Ok(self
            .actions
            .lock()
            .unwrap()
            .iter()
            .filter(|action| {
                conversation_id
                    .map(|conversation_id| action.conversation_id == conversation_id)
                    .unwrap_or(true)
            })
            .filter(|action| status.map(|status| action.status == status).unwrap_or(true))
            .cloned()
            .collect())
    }

    async fn get_agent_action(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<AgentAction, StorageError> {
        let action = self
            .actions
            .lock()
            .unwrap()
            .iter()
            .find(|action| action.id == id)
            .cloned()
            .ok_or(StorageError::NotFound)?;
        self.get_agent_conversation(owner_user_id, &action.conversation_id)
            .await?;
        Ok(action)
    }

    async fn update_agent_action_status(
        &self,
        owner_user_id: &str,
        id: &str,
        status: AgentActionStatus,
        resource_kind: Option<AgentResourceKind>,
        resource_id: Option<String>,
    ) -> Result<AgentAction, StorageError> {
        let conversation_id = self
            .actions
            .lock()
            .unwrap()
            .iter()
            .find(|action| action.id == id)
            .map(|action| action.conversation_id.clone())
            .ok_or(StorageError::NotFound)?;
        self.get_agent_conversation(owner_user_id, &conversation_id)
            .await?;
        let mut actions = self.actions.lock().unwrap();
        let Some(action) = actions.iter_mut().find(|action| action.id == id) else {
            return Err(StorageError::NotFound);
        };
        if action.status != AgentActionStatus::Proposed {
            return Err(StorageError::Conflict(format!(
                "agent action is already {}",
                action.status.as_str()
            )));
        }
        action.status = status;
        if resource_kind.is_some() {
            action.resource_kind = resource_kind;
        }
        if resource_id.is_some() {
            action.resource_id = resource_id;
        }
        Ok(action.clone())
    }

    async fn fail_stale_agent_turns(&self, _stale_after_seconds: i64) -> Result<u64, StorageError> {
        Ok(0)
    }
}

#[async_trait]
impl ManagedDatabaseConnectionLoader for TestStore {
    async fn load_managed_database_connection(
        &self,
        key: &ManagedDatabasePoolKey,
    ) -> Result<ManagedDatabaseConnectionSpec, ManagedDatabaseConnectionLoaderError> {
        let databases = self.databases.lock().unwrap();
        let Some(database) = databases
            .iter()
            .find(|database| database.id == key.database_id)
        else {
            return Err(ManagedDatabaseConnectionLoaderError::NotFound);
        };

        Ok(ManagedDatabaseConnectionSpec {
            engine: database.engine,
            host: database.host.clone(),
            port: u16::try_from(database.port).map_err(|_| {
                ManagedDatabaseConnectionLoaderError::InvalidConnection(
                    "managed database port must be between 1 and 65535".to_owned(),
                )
            })?,
            database: database.database.clone(),
            username: database.username.clone(),
            password: "password123".to_owned(),
            ssl_mode: database.ssl_mode,
        })
    }
}

struct TestPoolConnector;

#[async_trait]
impl ManagedDatabasePoolConnector for TestPoolConnector {
    async fn connect(
        &self,
        spec: &ManagedDatabaseConnectionSpec,
        policy: &ManagedDatabasePoolPolicy,
    ) -> Result<PgPool, ManagedDatabasePoolError> {
        Ok(lazy_test_pool(spec, policy))
    }
}

#[derive(Default)]
struct CapturingSqlAuditAgent {
    tool_names: Mutex<Vec<String>>,
}

#[async_trait]
impl SqlAuditAgent for CapturingSqlAuditAgent {
    async fn audit_summary(&self) -> anyhow::Result<AuditSummary> {
        Ok(AuditSummary::sample())
    }

    async fn audit_sql(&self, request: SqlAuditRequest) -> anyhow::Result<SqlAuditReport> {
        Ok(test_audit_report(request.sql))
    }

    async fn audit_sql_with_tools(
        &self,
        request: SqlAuditRequest,
        tools: ToolRegistry,
    ) -> anyhow::Result<SqlAuditReport> {
        *self.tool_names.lock().unwrap() = tools
            .definitions()
            .into_iter()
            .map(|definition| definition.name)
            .collect();

        Ok(test_audit_report(request.sql))
    }

    async fn audit_sql_stream(&self, _request: SqlAuditRequest) -> anyhow::Result<AgentStream> {
        Err(anyhow::anyhow!("streaming is not supported in route tests"))
    }
}

#[derive(Default)]
struct FakeApprovedSqlExecutor {
    fail_with: Mutex<Option<String>>,
}

impl ApprovedSqlExecutor for FakeApprovedSqlExecutor {
    fn execute<'a>(
        &'a self,
        _config: PostgresToolConfig,
        sql: &'a str,
    ) -> ApprovedSqlExecutionFuture<'a> {
        Box::pin(async move {
            if let Some(message) = self.fail_with.lock().unwrap().clone() {
                return Err(anyhow::anyhow!(message));
            }

            let analysis = analyze_postgres_sql(PgSqlAnalysisRequest::new(sql));
            Ok(ApprovedWriteExecutionResult {
                statement_kind: analysis
                    .statements
                    .first()
                    .map(|statement| statement.kind.clone())
                    .unwrap_or(PgSqlStatementKind::Other),
                affected_rows: 1,
                elapsed_ms: 7,
                risk_floor: analysis.risk_floor(),
                analysis,
            })
        })
    }
}

#[derive(Default)]
struct FakeManagedDatabaseConnectionTester;

impl ManagedDatabaseConnectionTester for FakeManagedDatabaseConnectionTester {
    fn test<'a>(&'a self, _pool: PgPool) -> ManagedDatabaseConnectionTestFuture<'a> {
        Box::pin(async { Ok(()) })
    }
}

async fn spawn_openai_compatible_mock() -> (String, Arc<Mutex<Option<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let captured_body = Arc::new(Mutex::new(None));
    let captured_for_task = captured_body.clone();

    tokio::spawn(async move {
        let Ok((mut socket, _addr)) = listener.accept().await else {
            return;
        };
        let mut buffer = vec![0; 16 * 1024];
        let Ok(read) = socket.read(&mut buffer).await else {
            return;
        };
        let request = String::from_utf8_lossy(&buffer[..read]);
        if let Some((_, body)) = request.split_once("\r\n\r\n") {
            if let Ok(json) = serde_json::from_str::<Value>(body) {
                *captured_for_task.lock().unwrap() = Some(json);
            }
        }
        let body = json!({
            "choices": [{
                "message": {
                    "content": "{\"summary\":\"User configured model\",\"risk_score\":7,\"findings\":[]}"
                }
            }]
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = socket.write_all(response.as_bytes()).await;
    });

    (format!("http://{addr}/v1/chat/completions"), captured_body)
}

fn test_app() -> Router {
    test_app_with_agent(Arc::new(MockSqlAuditAgent))
}

fn test_app_with_cors() -> Router {
    let store = Arc::new(TestStore::default());
    let loader: Arc<dyn ManagedDatabaseConnectionLoader> = store.clone();
    let pool_manager = Arc::new(ManagedDatabasePoolManager::with_connector(
        loader,
        Arc::new(TestPoolConnector),
        ManagedDatabasePoolPolicy::default(),
    ));

    router_with_cors(
        ApiState::with_pool_manager_executor_and_connection_tester(
            Arc::new(MockSqlAuditAgent),
            store,
            pool_manager,
            false,
            PostgresToolExecutionMode::Readonly,
            Arc::new(FakeApprovedSqlExecutor::default()),
            Arc::new(FakeManagedDatabaseConnectionTester),
        ),
        "http://localhost:3000",
    )
    .unwrap()
}

fn test_app_with_agent(agent: Arc<dyn SqlAuditAgent>) -> Router {
    test_app_with_agent_and_execution(agent, PostgresToolExecutionMode::Readonly)
}

fn test_app_with_agent_and_execution(
    agent: Arc<dyn SqlAuditAgent>,
    sql_execution: PostgresToolExecutionMode,
) -> Router {
    test_app_with_agent_execution_and_executor(
        agent,
        sql_execution,
        Arc::new(FakeApprovedSqlExecutor::default()),
    )
}

fn test_app_with_agent_execution_and_executor(
    agent: Arc<dyn SqlAuditAgent>,
    sql_execution: PostgresToolExecutionMode,
    executor: Arc<dyn ApprovedSqlExecutor>,
) -> Router {
    let store = Arc::new(TestStore::default());
    let loader: Arc<dyn ManagedDatabaseConnectionLoader> = store.clone();
    let pool_manager = Arc::new(ManagedDatabasePoolManager::with_connector(
        loader,
        Arc::new(TestPoolConnector),
        ManagedDatabasePoolPolicy::default(),
    ));

    router(ApiState::with_pool_manager_executor_and_connection_tester(
        agent,
        store,
        pool_manager,
        false,
        sql_execution,
        executor,
        Arc::new(FakeManagedDatabaseConnectionTester),
    ))
}

fn test_auth_response(email: String, display_name: String) -> AuthResponse {
    AuthResponse {
        token: VALID_TOKEN.to_owned(),
        token_type: "Bearer".to_owned(),
        expires_in_seconds: 3600,
        user: PublicUser {
            id: "user-1".to_owned(),
            email,
            display_name,
        },
    }
}

fn test_user() -> PublicUser {
    PublicUser {
        id: "user-1".to_owned(),
        email: "user@test.local".to_owned(),
        display_name: "Test User".to_owned(),
    }
}

fn public_llm_settings(settings: &ResolvedLlmProviderSettings) -> LlmProviderSettings {
    LlmProviderSettings {
        provider: settings.provider,
        base_url: settings.base_url.clone(),
        model: settings.model.clone(),
        api_mode: settings.api_mode,
        has_api_key: settings.api_key.is_some(),
    }
}

fn test_audit_report(sql: String) -> SqlAuditReport {
    SqlAuditReport {
        summary: format!("Audited: {sql}"),
        risk_score: 50,
        findings: Vec::new(),
    }
}

fn lazy_test_pool(
    spec: &ManagedDatabaseConnectionSpec,
    policy: &ManagedDatabasePoolPolicy,
) -> PgPool {
    let options = PgConnectOptions::new_without_pgpass()
        .host(&spec.host)
        .port(spec.port)
        .username(&spec.username)
        .password(&spec.password)
        .database(&spec.database)
        .ssl_mode(match spec.ssl_mode {
            ManagedDatabaseSslMode::Disable => sqlx::postgres::PgSslMode::Disable,
            ManagedDatabaseSslMode::Prefer => sqlx::postgres::PgSslMode::Prefer,
            ManagedDatabaseSslMode::Require => sqlx::postgres::PgSslMode::Require,
        })
        .application_name("liquid-api-route-test");

    PgPoolOptions::new()
        .max_connections(policy.max_connections.max(1))
        .min_connections(0)
        .acquire_timeout(policy.acquire_timeout)
        .idle_timeout(Some(policy.connection_idle_timeout))
        .max_lifetime(Some(policy.connection_max_lifetime))
        .test_before_acquire(true)
        .connect_lazy_with(options)
}

async fn create_test_database(app: &Router) {
    let response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases",
            json!({
                "name": "Warehouse",
                "engine": "postgres",
                "host": "localhost",
                "port": 5432,
                "database": "warehouse",
                "username": "readonly",
                "password": "password123",
                "ssl_mode": "disable"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn healthz_returns_ok() {
    let response = test_app()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn cors_preflight_allows_setting_current_managed_database() {
    let response = test_app_with_cors()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/api/v1/managed-databases/current")
                .header(ORIGIN, "http://localhost:3000")
                .header(ACCESS_CONTROL_REQUEST_METHOD, "PUT")
                .header(ACCESS_CONTROL_REQUEST_HEADERS, "authorization,content-type")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
        "http://localhost:3000"
    );
    let allowed_methods = response
        .headers()
        .get(ACCESS_CONTROL_ALLOW_METHODS)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(allowed_methods.contains("PUT"));
}

#[tokio::test]
async fn register_returns_bearer_token() {
    let response = test_app()
        .oneshot(json_request(
            "/api/v1/auth/register",
            json!({
                "email": "user@test.local",
                "display_name": "Test User",
                "password": "password123"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let payload = response_json(response).await;
    assert_eq!(payload["token"], VALID_TOKEN);
    assert_eq!(payload["user"]["email"], "user@test.local");
}

#[tokio::test]
async fn login_rejects_invalid_credentials() {
    let response = test_app()
        .oneshot(json_request(
            "/api/v1/auth/login",
            json!({
                "email": "user@test.local",
                "password": "wrong-password"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn me_requires_bearer_token() {
    let response = test_app()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn me_returns_current_user_for_valid_token() {
    let response = test_app()
        .oneshot(auth_request("/api/v1/auth/me"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let payload = response_json(response).await;
    assert_eq!(payload["user"]["email"], "user@test.local");
}

#[tokio::test]
async fn update_me_changes_display_name() {
    let app = test_app();
    let response = app
        .clone()
        .oneshot(auth_json_request(
            "PATCH",
            "/api/v1/auth/me",
            json!({ "display_name": "Renamed User" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["user"]["display_name"], "Renamed User");

    let response = app.oneshot(auth_request("/api/v1/auth/me")).await.unwrap();
    let payload = response_json(response).await;
    assert_eq!(payload["user"]["display_name"], "Renamed User");
}

#[tokio::test]
async fn update_password_rejects_wrong_current_password() {
    let response = test_app()
        .oneshot(auth_json_request(
            "PATCH",
            "/api/v1/auth/password",
            json!({
                "current_password": "wrong-password",
                "new_password": "new-password123"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn update_password_accepts_current_password() {
    let response = test_app()
        .oneshot(auth_json_request(
            "PATCH",
            "/api/v1/auth/password",
            json!({
                "current_password": "password123",
                "new_password": "new-password123"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn llm_provider_settings_round_trip_without_api_key_echo() {
    let app = test_app();
    let response = app
        .clone()
        .oneshot(auth_request("/api/v1/settings/llm-provider"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert!(payload["settings"].is_null());

    let response = app
        .clone()
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/settings/llm-provider",
            json!({
                "provider": "openai_compatible",
                "base_url": "https://api.openai.com/v1/chat/completions",
                "model": "gpt-4.1",
                "api_mode": "chat_completions",
                "api_key": "sk-test"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["settings"]["provider"], "openai_compatible");
    assert_eq!(payload["settings"]["model"], "gpt-4.1");
    assert_eq!(payload["settings"]["has_api_key"], true);
    assert!(payload["settings"].get("api_key").is_none());

    let response = app
        .oneshot(auth_request("/api/v1/settings/llm-provider"))
        .await
        .unwrap();
    let payload = response_json(response).await;
    assert_eq!(payload["settings"]["has_api_key"], true);
    assert!(payload["settings"].get("api_key").is_none());
}

#[tokio::test]
async fn logout_revokes_token() {
    let app = test_app();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/logout")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = app.oneshot(auth_request("/api/v1/auth/me")).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn audit_summary_requires_authentication() {
    let response = test_app()
        .oneshot(
            Request::builder()
                .uri("/api/v1/audit/summary")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn audit_summary_returns_sample_payload_for_authenticated_user() {
    let response = test_app()
        .oneshot(auth_request("/api/v1/audit/summary"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let payload = response_json(response).await;
    assert_eq!(payload["audit_score"], 92);
    assert!(payload["risk_breakdown"].is_array());
}

#[tokio::test]
async fn agent_conversations_require_authentication() {
    let response = test_app()
        .oneshot(json_request(
            "/api/v1/agent/conversations",
            json!({ "title": "Ops" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn agent_conversation_can_be_deleted() {
    let app = test_app();
    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/agent/conversations",
            json!({ "title": "Disposable workspace" }),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);

    let delete_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/v1/agent/conversations/conversation-1")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

    let list_response = app
        .oneshot(auth_request("/api/v1/agent/conversations"))
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let conversations = response_json(list_response).await;
    assert_eq!(conversations.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn agent_turn_persists_events_and_proposed_action() {
    let app = test_app();
    create_test_database(&app).await;

    let conversation_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/agent/conversations",
            json!({ "title": "SQL review" }),
        ))
        .await
        .unwrap();
    assert_eq!(conversation_response.status(), StatusCode::OK);
    let conversation = response_json(conversation_response).await;
    assert_eq!(conversation["title"], "SQL review");

    let turn_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/agent/conversations/conversation-1/turns",
            json!({
                "message": "select * from users",
                "managed_database_id": "db-1",
                "dashboard_context": {
                    "active_view": "ai",
                    "date_range": "last_7_days"
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(turn_response.status(), StatusCode::OK);
    let turn = response_json(turn_response).await;
    assert_eq!(turn["status"], "queued");

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let events_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/agent/turns/turn-1/events?after_seq=0",
        ))
        .await
        .unwrap();
    assert_eq!(events_response.status(), StatusCode::OK);
    let events_body = axum::body::to_bytes(events_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let events_body = String::from_utf8(events_body.to_vec()).unwrap();
    assert!(events_body.contains("action_proposed"));
    assert!(events_body.contains("turn_completed"));

    let actions_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/agent/actions?conversation_id=conversation-1&status=proposed",
        ))
        .await
        .unwrap();
    assert_eq!(actions_response.status(), StatusCode::OK);
    let actions = response_json(actions_response).await;
    assert_eq!(actions.as_array().unwrap().len(), 1);
    assert_eq!(actions[0]["kind"], "create_sql_audit");
    assert_eq!(actions[0]["payload"]["managed_database_id"], "db-1");
}

#[tokio::test]
async fn applying_agent_sql_audit_action_uses_existing_audit_flow() {
    let app = test_app();
    create_test_database(&app).await;

    let _conversation = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/agent/conversations",
            json!({ "title": "SQL review" }),
        ))
        .await
        .unwrap();
    let turn_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/agent/conversations/conversation-1/turns",
            json!({
                "message": "select * from users",
                "managed_database_id": "db-1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(turn_response.status(), StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let apply_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/agent/actions/action-1/apply",
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(apply_response.status(), StatusCode::OK);
    let action = response_json(apply_response).await;
    assert_eq!(action["status"], "applied");
    assert_eq!(action["resource_kind"], "sql_audit");
    assert_eq!(action["resource_id"], "audit-1");

    let audit_response = app
        .oneshot(auth_request("/api/v1/sql-audits/audit-1"))
        .await
        .unwrap();
    assert_eq!(audit_response.status(), StatusCode::OK);
    let audit = response_json(audit_response).await;
    assert_eq!(audit["sql"], "select * from users");
    assert_eq!(audit["status"], "audited");
}

#[tokio::test]
async fn managed_database_crud_is_bearer_protected() {
    let app = test_app();
    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases",
            json!({
                "name": "Warehouse",
                "engine": "postgres",
                "host": "localhost",
                "port": 5432,
                "database": "warehouse",
                "username": "readonly",
                "password": "password123",
                "ssl_mode": "prefer"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(create_response.status(), StatusCode::CREATED);
    let payload = response_json(create_response).await;
    assert_eq!(payload["name"], "Warehouse");
    assert_eq!(payload["has_password"], true);
    assert!(payload.get("password").is_none());

    let update_response = app
        .clone()
        .oneshot(auth_json_request(
            "PATCH",
            "/api/v1/managed-databases/db-1",
            json!({
                "name": "Warehouse Replica",
                "ssl_mode": "require"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(update_response.status(), StatusCode::OK);
    let payload = response_json(update_response).await;
    assert_eq!(payload["name"], "Warehouse Replica");
    assert_eq!(payload["ssl_mode"], "require");

    let list_response = app
        .clone()
        .oneshot(auth_request("/api/v1/managed-databases"))
        .await
        .unwrap();
    let payload = response_json(list_response).await;
    assert_eq!(payload.as_array().unwrap().len(), 1);

    let delete_response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/v1/managed-databases/db-1")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn current_managed_database_can_be_set_read_and_cleared() {
    let app = test_app();
    create_test_database(&app).await;

    let initial_response = app
        .clone()
        .oneshot(auth_request("/api/v1/managed-databases/current"))
        .await
        .unwrap();
    assert_eq!(initial_response.status(), StatusCode::OK);
    let payload = response_json(initial_response).await;
    assert!(payload["database"].is_null());

    let set_response = app
        .clone()
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/managed-databases/current",
            json!({
                "managed_database_id": "db-1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(set_response.status(), StatusCode::OK);
    let payload = response_json(set_response).await;
    assert_eq!(payload["database"]["id"], "db-1");

    let current_response = app
        .clone()
        .oneshot(auth_request("/api/v1/managed-databases/current"))
        .await
        .unwrap();
    assert_eq!(current_response.status(), StatusCode::OK);
    let payload = response_json(current_response).await;
    assert_eq!(payload["database"]["name"], "Warehouse");

    let clear_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/v1/managed-databases/current")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(clear_response.status(), StatusCode::NO_CONTENT);

    let current_response = app
        .oneshot(auth_request("/api/v1/managed-databases/current"))
        .await
        .unwrap();
    let payload = response_json(current_response).await;
    assert!(payload["database"].is_null());
}

#[tokio::test]
async fn current_managed_database_requires_authentication_and_existing_database() {
    let app = test_app();

    let unauthenticated_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/managed-databases/current")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "managed_database_id": "db-1"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthenticated_response.status(), StatusCode::UNAUTHORIZED);

    let missing_response = app
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/managed-databases/current",
            json!({
                "managed_database_id": "db-missing"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_current_managed_database_clears_current_selection() {
    let app = test_app();
    create_test_database(&app).await;

    let set_response = app
        .clone()
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/managed-databases/current",
            json!({
                "managed_database_id": "db-1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(set_response.status(), StatusCode::OK);

    let delete_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/v1/managed-databases/db-1")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

    let current_response = app
        .oneshot(auth_request("/api/v1/managed-databases/current"))
        .await
        .unwrap();
    let payload = response_json(current_response).await;
    assert!(payload["database"].is_null());
}

#[tokio::test]
async fn managed_database_test_connection_uses_managed_database_pool() {
    let app = test_app();
    create_test_database(&app).await;

    let response = app
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/test-connection",
            json!({}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["ok"], true);
}

#[tokio::test]
async fn managed_database_test_connection_returns_not_found_for_missing_database() {
    let response = test_app()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-missing/test-connection",
            json!({}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn managed_database_audit_sql_requires_authentication() {
    let response = test_app()
        .oneshot(json_request(
            "/api/v1/managed-databases/db-1/audit-sql",
            json!({
                "sql": "select * from users"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn managed_database_audit_sql_returns_not_found_for_missing_database() {
    let response = test_app()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-missing/audit-sql",
            json!({
                "sql": "select * from users"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn managed_database_audit_sql_uses_managed_database_pool() {
    let app = test_app();
    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases",
            json!({
                "name": "Warehouse",
                "engine": "postgres",
                "host": "localhost",
                "port": 5432,
                "database": "warehouse",
                "username": "readonly",
                "password": "password123",
                "ssl_mode": "disable"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(create_response.status(), StatusCode::CREATED);

    let audit_response = app
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/audit-sql",
            json!({
                "sql": "select * from users"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(audit_response.status(), StatusCode::OK);
    let payload = response_json(audit_response).await;
    assert_eq!(payload["summary"], "Mock SQL audit completed.");
    assert_eq!(payload["risk_score"], 50);
}

#[tokio::test]
async fn managed_database_audit_sql_uses_readonly_tool_registry() {
    let agent = Arc::new(CapturingSqlAuditAgent::default());
    let app =
        test_app_with_agent_and_execution(agent.clone(), PostgresToolExecutionMode::WriteGated);
    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases",
            json!({
                "name": "Warehouse",
                "engine": "postgres",
                "host": "localhost",
                "port": 5432,
                "database": "warehouse",
                "username": "readonly",
                "password": "password123",
                "ssl_mode": "disable"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(create_response.status(), StatusCode::CREATED);

    let audit_response = app
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/audit-sql",
            json!({
                "sql": "select * from users"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(audit_response.status(), StatusCode::OK);
    let tool_names = agent.tool_names.lock().unwrap().clone();
    assert!(tool_names.iter().any(|name| name == "inspect_sql_risk"));
    assert!(
        tool_names
            .iter()
            .any(|name| name == "pg_execute_readonly_sql")
    );
    assert!(!tool_names.iter().any(|name| name == "pg_execute_write_sql"));
}

#[tokio::test]
async fn sql_audit_persistence_requires_authentication() {
    let response = test_app()
        .oneshot(json_request(
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "select * from users"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sql_audit_persistence_creates_audited_select_record() {
    let app = test_app();
    create_test_database(&app).await;

    let response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "select * from users",
                "context": "read-only review"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = response_json(response).await;
    assert_eq!(payload["id"], "audit-1");
    assert_eq!(payload["status"], "audited");
    assert_eq!(payload["statement_kind"], "select");
    assert_eq!(payload["managed_database_id"], "db-1");
    assert_eq!(payload["sql"], "select * from users");
    assert_eq!(payload["report"]["summary"], "Mock SQL audit completed.");
    assert_eq!(payload["report"]["risk_score"], 50);

    let list_response = app
        .oneshot(auth_request(
            "/api/v1/sql-audits?managed_database_id=db-1&status=audited",
        ))
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let payload = response_json(list_response).await;
    assert_eq!(payload.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn sql_audit_uses_user_llm_provider_settings_when_configured() {
    let app = test_app();
    create_test_database(&app).await;
    let (base_url, captured_body) = spawn_openai_compatible_mock().await;

    let settings_response = app
        .clone()
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/settings/llm-provider",
            json!({
                "provider": "openai_compatible",
                "base_url": base_url,
                "model": "user-model",
                "api_mode": "chat_completions",
                "api_key": "sk-user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(settings_response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "select * from users",
                "context": "user configured model"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = response_json(response).await;
    assert_eq!(payload["report"]["summary"], "User configured model");
    assert_eq!(payload["report"]["risk_score"], 7);

    let captured = captured_body.lock().unwrap().clone().unwrap();
    assert_eq!(captured["model"], "user-model");
}

#[tokio::test]
async fn sql_audit_approve_and_execute_runs_once_when_write_gated() {
    let app = test_app_with_agent_and_execution(
        Arc::new(CapturingSqlAuditAgent::default()),
        PostgresToolExecutionMode::WriteGated,
    );
    create_test_database(&app).await;

    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "update users set active = false where id = 1",
                "execution_purpose": "Deactivate test user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let payload = response_json(create_response).await;
    assert_eq!(payload["status"], "pending_approval");

    let approve_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/sql-audits/audit-1/approve",
            json!({
                "comment": "approved"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(approve_response.status(), StatusCode::OK);
    let payload = response_json(approve_response).await;
    assert_eq!(payload["status"], "approved");

    let execute_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sql-audits/audit-1/execute")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(execute_response.status(), StatusCode::OK);
    let payload = response_json(execute_response).await;
    assert_eq!(payload["status"], "executed");
    assert_eq!(payload["execution_result"]["affected_rows"], 1);

    let repeat_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sql-audits/audit-1/execute")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(repeat_response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn sql_audit_reject_blocks_execution() {
    let app = test_app_with_agent_and_execution(
        Arc::new(CapturingSqlAuditAgent::default()),
        PostgresToolExecutionMode::WriteGated,
    );
    create_test_database(&app).await;

    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "delete from users where id = 1",
                "execution_purpose": "Remove test user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let reject_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/sql-audits/audit-1/reject",
            json!({
                "comment": "too risky"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(reject_response.status(), StatusCode::OK);
    let payload = response_json(reject_response).await;
    assert_eq!(payload["status"], "rejected");

    let execute_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sql-audits/audit-1/execute")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(execute_response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn sql_audit_blocks_critical_sql() {
    let app = test_app();
    create_test_database(&app).await;

    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "drop table users",
                "execution_purpose": "Dangerous migration"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let payload = response_json(create_response).await;
    assert_eq!(payload["status"], "blocked");

    let approve_response = app
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/sql-audits/audit-1/approve",
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(approve_response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn sql_audit_execute_requires_write_gated_config() {
    let app = test_app();
    create_test_database(&app).await;

    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "update users set active = false where id = 1",
                "execution_purpose": "Deactivate test user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let execute_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sql-audits/audit-1/execute")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(execute_response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn sql_audit_execute_rejects_managed_database_drift() {
    let app = test_app_with_agent_and_execution(
        Arc::new(CapturingSqlAuditAgent::default()),
        PostgresToolExecutionMode::WriteGated,
    );
    create_test_database(&app).await;

    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "update users set active = false where id = 1",
                "execution_purpose": "Deactivate test user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let approve_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/sql-audits/audit-1/approve",
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(approve_response.status(), StatusCode::OK);

    let update_response = app
        .clone()
        .oneshot(auth_json_request(
            "PATCH",
            "/api/v1/managed-databases/db-1",
            json!({
                "host": "other-host"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(update_response.status(), StatusCode::OK);

    let execute_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sql-audits/audit-1/execute")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(execute_response.status(), StatusCode::CONFLICT);
}

fn json_request(uri: &str, payload: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap()
}

fn auth_request(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
        .body(Body::empty())
        .unwrap()
}

fn auth_json_request(method: &str, uri: &str, payload: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap()
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();

    serde_json::from_slice(&body).unwrap()
}

#[test]
fn fake_store_uses_expected_enum_values() {
    let database = ManagedDatabase {
        id: "db-1".to_owned(),
        name: "Warehouse".to_owned(),
        engine: ManagedDatabaseEngine::Postgres,
        host: "localhost".to_owned(),
        port: 5432,
        database: "warehouse".to_owned(),
        username: "readonly".to_owned(),
        ssl_mode: ManagedDatabaseSslMode::Prefer,
        has_password: true,
    };

    assert_eq!(database.engine.as_str(), "postgres");
    assert_eq!(database.ssl_mode.as_str(), "prefer");
}
