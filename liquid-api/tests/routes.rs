use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{
        Request, StatusCode,
        header::{
            ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
            ACCESS_CONTROL_EXPOSE_HEADERS, ACCESS_CONTROL_REQUEST_HEADERS,
            ACCESS_CONTROL_REQUEST_METHOD, AUTHORIZATION, CONTENT_TYPE, ORIGIN,
        },
    },
};
use liquid_agent::{
    AgentStream, ApprovedWriteExecutionResult, MockSqlAuditAgent, PostgresToolConfig,
    PostgresToolExecutionMode, SqlAuditAgent, ToolRegistry,
};
use liquid_api::{
    ApiState, ApprovedSqlExecutionFuture, ApprovedSqlExecutor, ChatSqlExecutionFuture,
    ChatSqlExecutionOutcome, ChatSqlExecutor, ManagedDatabaseConnectionTestFuture,
    ManagedDatabaseConnectionTester, router, router_with_cors,
};
use liquid_core::{
    AgentAction, AgentActionKind, AgentActionStatus, AgentConversation, AgentEventRecord,
    AgentEventType, AgentMessage, AgentMessageRole, AgentResourceKind, AgentTurn, AgentTurnStatus,
    ApproveSqlAuditRequest, AuditSummary, AuthResponse, CreateAgentActionRequest,
    CreateAgentConversationRequest, CreateAgentTurnRequest, CreateDatapanelCardRequest,
    CreateManagedDatabaseRequest, Datapanel, DatapanelCard, DatapanelCardLayoutUpdate,
    DatapanelExport, DatapanelPreview, DatapanelPreviewLink, DatapanelQueryResult,
    LlmProviderSettings, LoginRequest, ManagedDatabase, ManagedDatabaseConnectionLoader,
    ManagedDatabaseConnectionLoaderError, ManagedDatabaseConnectionSpec, ManagedDatabaseEngine,
    ManagedDatabasePoolKey, ManagedDatabasePoolPolicy, ManagedDatabaseSslMode, PublicUser,
    RegisterRequest, RejectSqlAuditRequest, ResolvedLlmProviderSettings, SqlAuditExecutionResult,
    SqlAuditRecord, SqlAuditReport, SqlAuditRequest, SqlAuditStatus, SqlStatementKind,
    UpdateAgentConversationRequest, UpdateCurrentUserRequest, UpdateDatapanelCardRequest,
    UpdateDatapanelRequest, UpdateLlmProviderSettingsRequest, UpdateManagedDatabaseRequest,
    UpdatePasswordRequest,
};
use liquid_sql::{PgSqlAnalysisRequest, PgSqlStatementKind, analyze_postgres_sql};
use liquid_storage::{
    CreateSqlAuditRecord, LiquidStore, ManagedDatabasePoolConnector, ManagedDatabasePoolError,
    ManagedDatabasePoolManager, SqlAuditListFilters, SqlAuditListPage, StorageError,
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
    panels: Mutex<Vec<Datapanel>>,
    panel_cards: Mutex<Vec<DatapanelCard>>,
    panel_previews: Mutex<Vec<TestDatapanelPreview>>,
    user: Mutex<PublicUser>,
    llm_settings: Mutex<Option<ResolvedLlmProviderSettings>>,
}

#[derive(Debug, Clone)]
struct TestDatapanelPreview {
    panel_id: String,
    owner_user_id: String,
    slug: String,
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
            panels: Mutex::new(Vec::new()),
            panel_cards: Mutex::new(Vec::new()),
            panel_previews: Mutex::new(Vec::new()),
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
            streaming_enabled: request.streaming_enabled.unwrap_or(true),
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
            tags: request.tags.unwrap_or_default(),
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
        if let Some(tags) = request.tags {
            database.tags = tags;
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
        filters: SqlAuditListFilters<'_>,
    ) -> Result<SqlAuditListPage, StorageError> {
        let audits = self.audits.lock().unwrap();
        let page = filters.page.max(1);
        let page_size = filters.page_size.clamp(1, 100);
        let offset = ((page - 1) * page_size) as usize;
        let filtered = audits
            .iter()
            .filter(|record| record.owner_user_id == owner_user_id)
            .filter(|record| {
                filters
                    .managed_database_id
                    .map(|id| record.managed_database_id == id)
                    .unwrap_or(true)
            })
            .filter(|record| {
                filters
                    .status
                    .map(|status| record.status == status)
                    .unwrap_or(true)
            })
            .filter(|record| {
                filters
                    .audit_status
                    .map(|status| record.status.as_str() == status.as_str())
                    .unwrap_or(true)
            })
            .filter(|record| {
                filters
                    .execution_status
                    .map(|status| match status.as_str() {
                        "not_executed" => !matches!(
                            record.status,
                            SqlAuditStatus::Executing
                                | SqlAuditStatus::Executed
                                | SqlAuditStatus::ExecutionFailed
                        ),
                        other => record.status.as_str() == other,
                    })
                    .unwrap_or(true)
            })
            .filter(|record| {
                filters
                    .created_from
                    .map(|created_from| record.created_at >= created_from)
                    .unwrap_or(true)
            })
            .filter(|record| {
                filters
                    .created_to
                    .map(|created_to| record.created_at < created_to)
                    .unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>();
        let total_count = filtered.len() as i64;
        let records = filtered
            .into_iter()
            .skip(offset)
            .take(page_size as usize)
            .collect();

        Ok(SqlAuditListPage {
            records,
            total_count,
            page,
            page_size,
        })
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
        managed_database_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AgentConversation>, StorageError> {
        Ok(self
            .conversations
            .lock()
            .unwrap()
            .iter()
            .filter(|conversation| conversation.owner_user_id == owner_user_id)
            .filter(|conversation| {
                managed_database_id
                    .map(|database_id| {
                        conversation.managed_database_id.as_deref() == Some(database_id)
                    })
                    .unwrap_or(true)
            })
            .take(limit.clamp(1, 100) as usize)
            .cloned()
            .collect())
    }

    async fn create_agent_conversation(
        &self,
        owner_user_id: &str,
        request: CreateAgentConversationRequest,
    ) -> Result<AgentConversation, StorageError> {
        let managed_database_id = request.managed_database_id;
        if let Some(database_id) = managed_database_id.as_deref() {
            let exists = self
                .databases
                .lock()
                .unwrap()
                .iter()
                .any(|database| database.id == database_id);

            if !exists {
                return Err(StorageError::NotFound);
            }
        }

        let mut conversations = self.conversations.lock().unwrap();
        let conversation = AgentConversation {
            id: format!("conversation-{}", conversations.len() + 1),
            owner_user_id: owner_user_id.to_owned(),
            title: request.title.unwrap_or_else(|| "新的会话".to_owned()),
            managed_database_id,
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
        let conversation = self
            .get_agent_conversation(owner_user_id, conversation_id)
            .await?;
        let managed_database_id = match (
            conversation.managed_database_id,
            request.managed_database_id,
        ) {
            (Some(conversation_database_id), Some(requested_database_id))
                if conversation_database_id != requested_database_id =>
            {
                return Err(StorageError::Validation(
                    "conversation belongs to a different managed database".to_owned(),
                ));
            }
            (Some(conversation_database_id), _) => Some(conversation_database_id),
            (None, Some(requested_database_id)) => {
                let exists = self
                    .databases
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|database| database.id == requested_database_id);

                if !exists {
                    return Err(StorageError::NotFound);
                }

                if let Some(conversation) =
                    self.conversations.lock().unwrap().iter_mut().find(|item| {
                        item.id == conversation_id && item.owner_user_id == owner_user_id
                    })
                {
                    conversation.managed_database_id = Some(requested_database_id.clone());
                }

                Some(requested_database_id)
            }
            (None, None) => None,
        };
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
            managed_database_id,
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
        let transition_allowed = match status {
            AgentActionStatus::Applying => {
                matches!(
                    action.status,
                    AgentActionStatus::Proposed | AgentActionStatus::Failed
                )
            }
            AgentActionStatus::Applied | AgentActionStatus::Failed => matches!(
                action.status,
                AgentActionStatus::Proposed
                    | AgentActionStatus::Failed
                    | AgentActionStatus::Applying
            ),
            AgentActionStatus::Rejected | AgentActionStatus::Superseded => {
                matches!(
                    action.status,
                    AgentActionStatus::Proposed | AgentActionStatus::Failed
                )
            }
            AgentActionStatus::Proposed => false,
        };
        if !transition_allowed {
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

    async fn get_or_create_datapanel(
        &self,
        owner_user_id: &str,
        conversation_id: &str,
    ) -> Result<Datapanel, StorageError> {
        self.get_agent_conversation(owner_user_id, conversation_id)
            .await?;
        let mut panels = self.panels.lock().unwrap();

        if let Some(panel) = panels
            .iter()
            .find(|panel| panel.conversation_id == conversation_id)
            .cloned()
        {
            let cards = self.panel_cards.lock().unwrap();
            return Ok(attach_panel_cards(panel, &cards));
        }

        let panel = Datapanel {
            id: format!("panel-{}", panels.len() + 1),
            conversation_id: conversation_id.to_owned(),
            title: "新的数据面板".to_owned(),
            description: Some("用于沉淀当前会话的数据查询结果与图表".to_owned()),
            cards: Vec::new(),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
        };
        panels.push(panel.clone());
        Ok(panel)
    }

    async fn update_datapanel(
        &self,
        owner_user_id: &str,
        panel_id: &str,
        request: UpdateDatapanelRequest,
    ) -> Result<Datapanel, StorageError> {
        let conversation_id = self
            .panels
            .lock()
            .unwrap()
            .iter()
            .find(|panel| panel.id == panel_id)
            .map(|panel| panel.conversation_id.clone())
            .ok_or(StorageError::NotFound)?;
        self.get_agent_conversation(owner_user_id, &conversation_id)
            .await?;

        let mut panels = self.panels.lock().unwrap();
        let Some(panel) = panels.iter_mut().find(|panel| panel.id == panel_id) else {
            return Err(StorageError::NotFound);
        };

        if let Some(title) = request.title {
            panel.title = title;
        }

        if request.description.is_some() {
            panel.description = request
                .description
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty());
        }

        let cards = self.panel_cards.lock().unwrap();
        Ok(attach_panel_cards(panel.clone(), &cards))
    }

    async fn create_datapanel_card(
        &self,
        owner_user_id: &str,
        panel_id: &str,
        request: CreateDatapanelCardRequest,
    ) -> Result<DatapanelCard, StorageError> {
        let conversation_id = self
            .panels
            .lock()
            .unwrap()
            .iter()
            .find(|panel| panel.id == panel_id)
            .map(|panel| panel.conversation_id.clone())
            .ok_or(StorageError::NotFound)?;
        self.get_agent_conversation(owner_user_id, &conversation_id)
            .await?;
        if !self
            .databases
            .lock()
            .unwrap()
            .iter()
            .any(|database| database.id == request.managed_database_id)
        {
            return Err(StorageError::NotFound);
        }

        let mut cards = self.panel_cards.lock().unwrap();
        let card = DatapanelCard {
            id: format!("card-{}", cards.len() + 1),
            panel_id: panel_id.to_owned(),
            managed_database_id: request.managed_database_id,
            source_action_id: request.source_action_id,
            title: request.title,
            description: request.description,
            kind: request.kind,
            sql: request.sql,
            chart: request.chart,
            layout: request.layout,
            result: request.result,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
        };
        cards.push(card.clone());
        Ok(card)
    }

    async fn get_datapanel_card(
        &self,
        owner_user_id: &str,
        panel_id: &str,
        card_id: &str,
    ) -> Result<DatapanelCard, StorageError> {
        self.get_panel_for_owner(owner_user_id, panel_id).await?;
        self.panel_cards
            .lock()
            .unwrap()
            .iter()
            .find(|card| card.id == card_id && card.panel_id == panel_id)
            .cloned()
            .ok_or(StorageError::NotFound)
    }

    async fn update_datapanel_card(
        &self,
        owner_user_id: &str,
        panel_id: &str,
        card_id: &str,
        request: UpdateDatapanelCardRequest,
    ) -> Result<DatapanelCard, StorageError> {
        self.get_panel_for_owner(owner_user_id, panel_id).await?;
        let mut cards = self.panel_cards.lock().unwrap();
        let Some(card) = cards
            .iter_mut()
            .find(|card| card.id == card_id && card.panel_id == panel_id)
        else {
            return Err(StorageError::NotFound);
        };

        if let Some(title) = request.title {
            card.title = title;
        }

        if request.description.is_some() {
            card.description = request
                .description
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty());
        }

        Ok(card.clone())
    }

    async fn update_datapanel_layout(
        &self,
        owner_user_id: &str,
        panel_id: &str,
        layouts: Vec<DatapanelCardLayoutUpdate>,
    ) -> Result<Datapanel, StorageError> {
        let panel = self.get_panel_for_owner(owner_user_id, panel_id).await?;
        let mut cards = self.panel_cards.lock().unwrap();

        for update in layouts {
            let Some(card) = cards
                .iter_mut()
                .find(|card| card.id == update.card_id && card.panel_id == panel_id)
            else {
                return Err(StorageError::NotFound);
            };

            card.layout = update.layout;
        }

        Ok(attach_panel_cards(panel, &cards))
    }

    async fn update_datapanel_card_result(
        &self,
        owner_user_id: &str,
        panel_id: &str,
        card_id: &str,
        result: DatapanelQueryResult,
    ) -> Result<DatapanelCard, StorageError> {
        self.get_panel_for_owner(owner_user_id, panel_id).await?;
        let mut cards = self.panel_cards.lock().unwrap();
        let Some(card) = cards
            .iter_mut()
            .find(|card| card.id == card_id && card.panel_id == panel_id)
        else {
            return Err(StorageError::NotFound);
        };
        card.result = result;
        Ok(card.clone())
    }

    async fn delete_datapanel_card(
        &self,
        owner_user_id: &str,
        panel_id: &str,
        card_id: &str,
    ) -> Result<(), StorageError> {
        self.get_panel_for_owner(owner_user_id, panel_id).await?;
        let mut cards = self.panel_cards.lock().unwrap();
        let before = cards.len();
        cards.retain(|card| !(card.id == card_id && card.panel_id == panel_id));

        if cards.len() == before {
            return Err(StorageError::NotFound);
        }

        Ok(())
    }

    async fn export_datapanel(
        &self,
        owner_user_id: &str,
        panel_id: &str,
    ) -> Result<DatapanelExport, StorageError> {
        let panel = self.get_panel_for_owner(owner_user_id, panel_id).await?;
        let cards = self.panel_cards.lock().unwrap();

        Ok(DatapanelExport {
            exported_at: time::OffsetDateTime::UNIX_EPOCH,
            panel: attach_panel_cards(panel, &cards),
        })
    }

    async fn create_datapanel_preview(
        &self,
        owner_user_id: &str,
        panel_id: &str,
    ) -> Result<DatapanelPreviewLink, StorageError> {
        self.get_panel_for_owner(owner_user_id, panel_id).await?;
        let mut previews = self.panel_previews.lock().unwrap();

        if let Some(preview) = previews
            .iter()
            .find(|preview| preview.panel_id == panel_id)
            .cloned()
        {
            return Ok(DatapanelPreviewLink { slug: preview.slug });
        }

        let slug = format!("preview-{}", previews.len() + 1);
        previews.push(TestDatapanelPreview {
            panel_id: panel_id.to_owned(),
            owner_user_id: owner_user_id.to_owned(),
            slug: slug.clone(),
        });

        Ok(DatapanelPreviewLink { slug })
    }

    async fn get_datapanel_preview(&self, slug: &str) -> Result<DatapanelPreview, StorageError> {
        let preview = self
            .panel_previews
            .lock()
            .unwrap()
            .iter()
            .find(|preview| preview.slug == slug)
            .cloned()
            .ok_or(StorageError::NotFound)?;
        let panel = self
            .get_panel_for_owner(&preview.owner_user_id, &preview.panel_id)
            .await?;
        let cards = self.panel_cards.lock().unwrap();

        Ok(attach_panel_cards(panel, &cards).into())
    }

    async fn fail_stale_agent_turns(&self, _stale_after_seconds: i64) -> Result<u64, StorageError> {
        Ok(0)
    }
}

impl TestStore {
    async fn get_panel_for_owner(
        &self,
        owner_user_id: &str,
        panel_id: &str,
    ) -> Result<Datapanel, StorageError> {
        let panel = self
            .panels
            .lock()
            .unwrap()
            .iter()
            .find(|panel| panel.id == panel_id)
            .cloned()
            .ok_or(StorageError::NotFound)?;
        self.get_agent_conversation(owner_user_id, &panel.conversation_id)
            .await?;
        Ok(panel)
    }
}

fn attach_panel_cards(mut panel: Datapanel, cards: &[DatapanelCard]) -> Datapanel {
    panel.cards = cards
        .iter()
        .filter(|card| card.panel_id == panel.id)
        .cloned()
        .collect();
    panel
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

struct InvalidJsonSqlAuditAgent;

#[async_trait]
impl SqlAuditAgent for InvalidJsonSqlAuditAgent {
    async fn audit_summary(&self) -> anyhow::Result<AuditSummary> {
        Ok(AuditSummary::sample())
    }

    async fn audit_sql(&self, _request: SqlAuditRequest) -> anyhow::Result<SqlAuditReport> {
        Err(anyhow::anyhow!("LLM audit report was not valid JSON"))
    }

    async fn audit_sql_with_tools(
        &self,
        _request: SqlAuditRequest,
        _tools: ToolRegistry,
    ) -> anyhow::Result<SqlAuditReport> {
        Err(anyhow::anyhow!("LLM audit report was not valid JSON"))
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

struct PendingApprovedSqlExecutor;

impl ApprovedSqlExecutor for PendingApprovedSqlExecutor {
    fn execute<'a>(
        &'a self,
        _config: PostgresToolConfig,
        _sql: &'a str,
    ) -> ApprovedSqlExecutionFuture<'a> {
        Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            Err(anyhow::anyhow!("pending test executor should not complete"))
        })
    }
}

enum FakeChatSqlOutcome {
    Ok(ChatSqlExecutionOutcome),
    Err(String),
}

#[derive(Default)]
struct FakeChatSqlExecutor {
    outcomes: Mutex<VecDeque<FakeChatSqlOutcome>>,
    sql: Mutex<Vec<String>>,
}

impl FakeChatSqlExecutor {
    fn with_outcomes(outcomes: Vec<FakeChatSqlOutcome>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into()),
            sql: Mutex::new(Vec::new()),
        }
    }
}

impl ChatSqlExecutor for FakeChatSqlExecutor {
    fn execute<'a>(&'a self, _pool: PgPool, sql: &'a str) -> ChatSqlExecutionFuture<'a> {
        Box::pin(async move {
            self.sql.lock().unwrap().push(sql.to_owned());
            match self.outcomes.lock().unwrap().pop_front() {
                Some(FakeChatSqlOutcome::Ok(outcome)) => Ok(outcome),
                Some(FakeChatSqlOutcome::Err(message)) => Err(anyhow::anyhow!(message)),
                None => Err(anyhow::anyhow!("missing fake SQL execution outcome")),
            }
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
    spawn_openai_compatible_mock_with_content(
        "{\"summary\":\"User configured model\",\"risk_score\":7,\"findings\":[]}",
    )
    .await
}

async fn spawn_openai_compatible_mock_with_content(
    content: impl Into<String>,
) -> (String, Arc<Mutex<Option<Value>>>) {
    spawn_openai_compatible_mock_with_contents([content.into()]).await
}

async fn spawn_openai_compatible_sse_mock(
    body: impl Into<String>,
) -> (String, Arc<Mutex<Option<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let captured_body = Arc::new(Mutex::new(None));
    let captured_for_task = captured_body.clone();
    let body = body.into();

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _addr)) = listener.accept().await else {
                return;
            };
            let mut buffer = vec![0; 16 * 1024];
            let Ok(read) = socket.read(&mut buffer).await else {
                return;
            };
            let request = String::from_utf8_lossy(&buffer[..read]);
            if let Some((_, body)) = request.split_once("\r\n\r\n")
                && let Ok(json) = serde_json::from_str::<Value>(body)
            {
                *captured_for_task.lock().unwrap() = Some(json);
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });

    (format!("http://{addr}/v1/chat/completions"), captured_body)
}

async fn spawn_delayed_openai_compatible_mock_with_content(
    content: impl Into<String>,
) -> (
    String,
    Arc<Mutex<Option<Value>>>,
    tokio::sync::oneshot::Sender<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let captured_body = Arc::new(Mutex::new(None));
    let captured_for_task = captured_body.clone();
    let (release_response, mut wait_for_release) = tokio::sync::oneshot::channel::<()>();
    let content = content.into();

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _addr)) = listener.accept().await else {
                return;
            };
            let mut buffer = vec![0; 16 * 1024];
            let Ok(read) = socket.read(&mut buffer).await else {
                return;
            };
            let request = String::from_utf8_lossy(&buffer[..read]);
            if let Some((_, body)) = request.split_once("\r\n\r\n")
                && let Ok(json) = serde_json::from_str::<Value>(body)
            {
                *captured_for_task.lock().unwrap() = Some(json);
            }

            let _ = (&mut wait_for_release).await;
            let body = json!({
                "choices": [{
                    "message": {
                        "content": content
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
        }
    });

    (
        format!("http://{addr}/v1/chat/completions"),
        captured_body,
        release_response,
    )
}

async fn spawn_openai_compatible_mock_with_contents<I, S>(
    contents: I,
) -> (String, Arc<Mutex<Option<Value>>>)
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let captured_body = Arc::new(Mutex::new(None));
    let captured_for_task = captured_body.clone();
    let mut contents = contents
        .into_iter()
        .map(Into::into)
        .collect::<VecDeque<_>>();
    let fallback_content = contents.back().cloned().unwrap_or_else(|| "{}".to_owned());

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _addr)) = listener.accept().await else {
                return;
            };
            let mut buffer = vec![0; 16 * 1024];
            let Ok(read) = socket.read(&mut buffer).await else {
                return;
            };
            let request = String::from_utf8_lossy(&buffer[..read]);
            if let Some((_, body)) = request.split_once("\r\n\r\n")
                && let Ok(json) = serde_json::from_str::<Value>(body)
            {
                *captured_for_task.lock().unwrap() = Some(json);
            }
            let content = contents
                .pop_front()
                .unwrap_or_else(|| fallback_content.clone());
            let body = json!({
                "choices": [{
                    "message": {
                        "content": content
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
        }
    });

    (format!("http://{addr}/v1/chat/completions"), captured_body)
}

async fn spawn_openai_compatible_mock_with_raw_responses<I>(
    responses: I,
) -> (String, Arc<Mutex<Vec<Value>>>)
where
    I: IntoIterator<Item = Value>,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let captured_bodies = Arc::new(Mutex::new(Vec::new()));
    let captured_for_task = captured_bodies.clone();
    let mut responses = responses.into_iter().collect::<VecDeque<_>>();
    let fallback_response = responses
        .back()
        .cloned()
        .unwrap_or_else(|| json!({ "choices": [{ "message": { "content": "{}" } }] }));

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _addr)) = listener.accept().await else {
                return;
            };
            let mut buffer = vec![0; 128 * 1024];
            let Ok(read) = socket.read(&mut buffer).await else {
                return;
            };
            let request = String::from_utf8_lossy(&buffer[..read]);
            if let Some((_, body)) = request.split_once("\r\n\r\n")
                && let Ok(json) = serde_json::from_str::<Value>(body)
            {
                captured_for_task.lock().unwrap().push(json);
            }
            let body = responses
                .pop_front()
                .unwrap_or_else(|| fallback_response.clone())
                .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });

    (
        format!("http://{addr}/v1/chat/completions"),
        captured_bodies,
    )
}

async fn configure_workbench_llm_provider(
    app: &Router,
    content: impl Into<String>,
) -> Arc<Mutex<Option<Value>>> {
    configure_workbench_llm_provider_with_contents(app, [content.into()]).await
}

async fn configure_workbench_llm_provider_with_contents<I, S>(
    app: &Router,
    contents: I,
) -> Arc<Mutex<Option<Value>>>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let (base_url, captured_body) = spawn_openai_compatible_mock_with_contents(contents).await;
    let settings_response = app
        .clone()
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/settings/llm-provider",
            json!({
                "provider": "openai_compatible",
                "base_url": base_url,
                "model": "chat-model",
                "api_mode": "chat_completions",
                "api_key": "sk-user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(settings_response.status(), StatusCode::OK);

    captured_body
}

async fn configure_workbench_llm_provider_with_raw_responses<I>(
    app: &Router,
    responses: I,
) -> Arc<Mutex<Vec<Value>>>
where
    I: IntoIterator<Item = Value>,
{
    let (base_url, captured_bodies) =
        spawn_openai_compatible_mock_with_raw_responses(responses).await;
    let settings_response = app
        .clone()
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/settings/llm-provider",
            json!({
                "provider": "openai_compatible",
                "base_url": base_url,
                "model": "chat-model",
                "api_mode": "chat_completions",
                "api_key": "sk-user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(settings_response.status(), StatusCode::OK);

    captured_bodies
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
    test_app_with_agent_store_execution_and_executor(agent, store, sql_execution, executor)
}

fn test_app_with_agent_store_execution_and_executor(
    agent: Arc<dyn SqlAuditAgent>,
    store: Arc<TestStore>,
    sql_execution: PostgresToolExecutionMode,
    executor: Arc<dyn ApprovedSqlExecutor>,
) -> Router {
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

fn test_app_with_agent_store_executors(
    agent: Arc<dyn SqlAuditAgent>,
    store: Arc<TestStore>,
    sql_execution: PostgresToolExecutionMode,
    executor: Arc<dyn ApprovedSqlExecutor>,
    chat_sql_executor: Arc<dyn ChatSqlExecutor>,
) -> Router {
    let loader: Arc<dyn ManagedDatabaseConnectionLoader> = store.clone();
    let pool_manager = Arc::new(ManagedDatabasePoolManager::with_connector(
        loader,
        Arc::new(TestPoolConnector),
        ManagedDatabasePoolPolicy::default(),
    ));

    router(ApiState::with_pool_manager_executors_and_connection_tester(
        agent,
        store,
        pool_manager,
        false,
        sql_execution,
        executor,
        chat_sql_executor,
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

async fn create_sql_mode_workspace(store: &TestStore) -> (ManagedDatabase, AgentConversation) {
    let database = store
        .create_managed_database(
            "user-1",
            CreateManagedDatabaseRequest {
                name: "Warehouse".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "warehouse".to_owned(),
                username: "readonly".to_owned(),
                password: "password123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();
    let conversation = store
        .create_agent_conversation(
            "user-1",
            CreateAgentConversationRequest {
                title: Some("SQL workspace".to_owned()),
                managed_database_id: Some(database.id.clone()),
            },
        )
        .await
        .unwrap();

    (database, conversation)
}

fn public_llm_settings(settings: &ResolvedLlmProviderSettings) -> LlmProviderSettings {
    LlmProviderSettings {
        provider: settings.provider,
        base_url: settings.base_url.clone(),
        model: settings.model.clone(),
        api_mode: settings.api_mode,
        streaming_enabled: settings.streaming_enabled,
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
async fn cors_exposes_sql_audit_pagination_headers() {
    let response = test_app_with_cors()
        .oneshot(
            Request::builder()
                .uri("/api/v1/sql-audits")
                .header(ORIGIN, "http://localhost:3000")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
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
    let exposed_headers = response
        .headers()
        .get(ACCESS_CONTROL_EXPOSE_HEADERS)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(exposed_headers.contains("x-total-count"));
    assert!(exposed_headers.contains("x-page"));
    assert!(exposed_headers.contains("x-page-size"));
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
    assert_eq!(payload["settings"]["streaming_enabled"], true);
    assert_eq!(payload["settings"]["has_api_key"], true);
    assert!(payload["settings"].get("api_key").is_none());

    let response = app
        .clone()
        .oneshot(auth_request("/api/v1/settings/llm-provider"))
        .await
        .unwrap();
    let payload = response_json(response).await;
    assert_eq!(payload["settings"]["has_api_key"], true);
    assert_eq!(payload["settings"]["streaming_enabled"], true);
    assert!(payload["settings"].get("api_key").is_none());

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
                "streaming_enabled": false
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["settings"]["streaming_enabled"], false);
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
async fn chat_conversations_require_authentication() {
    let response = test_app()
        .oneshot(json_request(
            "/api/v1/chat/conversations",
            json!({ "title": "Ops" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn legacy_agent_routes_are_not_mounted() {
    let app = test_app();

    let list_response = app
        .clone()
        .oneshot(auth_request("/api/v1/agent/conversations"))
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::NOT_FOUND);

    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/agent/conversations",
            json!({ "title": "Legacy workspace" }),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::NOT_FOUND);

    let capabilities_response = app
        .oneshot(auth_request("/api/v1/agent/capabilities"))
        .await
        .unwrap();
    assert_eq!(capabilities_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn chat_conversation_can_be_deleted() {
    let app = test_app();
    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
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
                .uri("/api/v1/chat/conversations/conversation-1")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

    let list_response = app
        .oneshot(auth_request("/api/v1/chat/conversations"))
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let conversations = response_json(list_response).await;
    assert_eq!(conversations.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn chat_conversations_are_scoped_to_managed_database() {
    let app = test_app();
    create_test_database(&app).await;

    let second_database_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases",
            json!({
                "name": "Doro",
                "engine": "postgres",
                "host": "localhost",
                "port": 5432,
                "database": "doro",
                "username": "readonly",
                "password": "password123",
                "ssl_mode": "disable"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(second_database_response.status(), StatusCode::CREATED);

    let warehouse_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
            json!({
                "title": "Warehouse workspace",
                "managed_database_id": "db-1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(warehouse_response.status(), StatusCode::OK);
    let warehouse = response_json(warehouse_response).await;
    assert_eq!(warehouse["managed_database_id"], "db-1");
    assert_eq!(warehouse["selected_database"]["id"], "db-1");

    let doro_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
            json!({
                "title": "Doro workspace",
                "managed_database_id": "db-2"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(doro_response.status(), StatusCode::OK);
    let doro = response_json(doro_response).await;
    assert_eq!(doro["managed_database_id"], "db-2");
    assert_eq!(doro["selected_database"]["id"], "db-2");

    let warehouse_list_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/chat/conversations?managed_database_id=db-1",
        ))
        .await
        .unwrap();
    assert_eq!(warehouse_list_response.status(), StatusCode::OK);
    let warehouse_conversations = response_json(warehouse_list_response).await;
    assert_eq!(warehouse_conversations.as_array().unwrap().len(), 1);
    assert_eq!(warehouse_conversations[0]["title"], "Warehouse workspace");
    assert_eq!(warehouse_conversations[0]["managed_database_id"], "db-1");

    let doro_list_response = app
        .oneshot(auth_request(
            "/api/v1/chat/conversations?managed_database_id=db-2",
        ))
        .await
        .unwrap();
    assert_eq!(doro_list_response.status(), StatusCode::OK);
    let doro_conversations = response_json(doro_list_response).await;
    assert_eq!(doro_conversations.as_array().unwrap().len(), 1);
    assert_eq!(doro_conversations[0]["title"], "Doro workspace");
    assert_eq!(doro_conversations[0]["managed_database_id"], "db-2");
}

#[tokio::test]
async fn chat_turn_streams_typed_events_and_action() {
    let app = test_app();
    create_test_database(&app).await;
    configure_workbench_llm_provider(
        &app,
        r#"{
            "message": "I prepared a Markdown audit.\n\n```sql\nselect * from users\n```",
            "actions": [{
                "kind": "create_sql_audit",
                "title": "Create SQL audit",
                "description": "Review SQL from chat",
                "sql": "select * from users",
                "context": "chat requested review"
            }]
        }"#,
    )
    .await;

    let conversation_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
            json!({ "title": "Chat review" }),
        ))
        .await
        .unwrap();
    assert_eq!(conversation_response.status(), StatusCode::OK);
    let conversation = response_json(conversation_response).await;
    assert_eq!(conversation["title"], "Chat review");
    assert_eq!(conversation["selected_database"]["id"], "db-1");

    let turn_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations/conversation-1/turns",
            json!({
                "message": "audit this query",
                "managed_database_id": "db-1",
                "dashboard_context": {
                    "active_view": "ai",
                    "date_range": "last_7_days"
                },
                "client_request_id": "client-chat-1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(turn_response.status(), StatusCode::OK);
    let turn = response_json(turn_response).await;
    assert_eq!(turn["status"], "queued");
    assert_eq!(turn["input_message_id"], "message-1");

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream_response = app
        .clone()
        .oneshot(auth_request("/api/v1/chat/turns/turn-1/stream?after_seq=0"))
        .await
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);
    let stream_body = axum::body::to_bytes(stream_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stream_body = String::from_utf8(stream_body.to_vec()).unwrap();
    assert!(stream_body.contains("event: chat.event"));
    assert!(stream_body.contains(r#""type":"status_changed""#));
    assert!(stream_body.contains(r#""type":"assistant_delta""#));
    assert!(stream_body.contains(r#""type":"assistant_done""#));
    assert!(stream_body.contains(r#""type":"action_proposed""#));
    assert!(stream_body.contains(r#""type":"turn_waiting_for_user""#));
    assert!(stream_body.contains(r#""status":"waiting_for_user""#));
    assert!(stream_body.contains(r#""preview":{"kind":"sql_audit""#));

    let actions_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/chat/conversations/conversation-1/actions",
        ))
        .await
        .unwrap();
    assert_eq!(actions_response.status(), StatusCode::OK);
    let actions = response_json(actions_response).await;
    assert_eq!(actions.as_array().unwrap().len(), 1);
    assert_eq!(actions[0]["kind"], "create_sql_audit");
    assert_eq!(actions[0]["preview"]["kind"], "sql_audit");
    assert_eq!(actions[0]["preview"]["sql"], "select * from users");

    let messages_response = app
        .oneshot(auth_request(
            "/api/v1/chat/conversations/conversation-1/messages",
        ))
        .await
        .unwrap();
    assert_eq!(messages_response.status(), StatusCode::OK);
    let messages = response_json(messages_response).await;
    assert_eq!(messages.as_array().unwrap().len(), 2);
    assert_eq!(messages[1]["parts"][0]["kind"], "markdown");
}

#[tokio::test]
async fn chat_turn_runs_readonly_tool_without_action_or_audit() {
    let app = test_app();
    create_test_database(&app).await;
    let captured = configure_workbench_llm_provider_with_raw_responses(
        &app,
        [
            json!({
                "choices": [{
                    "message": {
                        "content": "",
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "pg_execute_readonly_sql",
                                "arguments": "{\"sql\":\"select datname from pg_database where datistemplate = false order by datname\",\"limit\":100}"
                            }
                        }]
                    }
                }]
            }),
            json!({
                "choices": [{
                    "message": {
                        "content": r#"{
                            "message": "我尝试查询数据库列表，但当前测试连接不可用。请检查数据库连接后重试。",
                            "actions": []
                        }"#
                    }
                }]
            }),
        ],
    )
    .await;

    let _conversation = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
            json!({ "title": "Readonly chat" }),
        ))
        .await
        .unwrap();
    let turn_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations/conversation-1/turns",
            json!({
                "message": "现在有哪些数据库",
                "managed_database_id": "db-1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(turn_response.status(), StatusCode::OK);

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let stream_response = app
        .clone()
        .oneshot(auth_request("/api/v1/chat/turns/turn-1/stream?after_seq=0"))
        .await
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);
    let stream_body = axum::body::to_bytes(stream_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stream_body = String::from_utf8(stream_body.to_vec()).unwrap();
    assert!(stream_body.contains(r#""type":"tool_started""#));
    assert!(stream_body.contains(r#""name":"pg_execute_readonly_sql""#));
    assert!(stream_body.contains(r#""type":"tool_finished""#));
    assert!(stream_body.contains(r#""type":"assistant_delta""#));
    assert!(stream_body.contains(r#""type":"turn_completed""#));
    assert!(!stream_body.contains(r#""type":"action_proposed""#));
    assert!(!stream_body.contains(r#""type":"turn_waiting_for_user""#));

    let actions_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/chat/conversations/conversation-1/actions",
        ))
        .await
        .unwrap();
    assert_eq!(actions_response.status(), StatusCode::OK);
    let actions = response_json(actions_response).await;
    assert_eq!(actions.as_array().unwrap().len(), 0);

    let messages_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/chat/conversations/conversation-1/messages",
        ))
        .await
        .unwrap();
    assert_eq!(messages_response.status(), StatusCode::OK);
    let messages = response_json(messages_response).await;
    assert_eq!(messages.as_array().unwrap().len(), 2);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["role"], "assistant");

    let captured = captured.lock().unwrap().clone();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0]["model"], "chat-model");
    assert!(
        captured[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["function"]["name"] == "pg_execute_readonly_sql")
    );
    assert!(
        captured[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["function"]["name"] == "propose_sql_operation")
    );
    assert!(
        captured[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool")
    );
}

#[tokio::test]
async fn chat_messages_and_stream_include_query_result_table_parts() {
    let store = Arc::new(TestStore::default());
    let app = test_app_with_agent_store_execution_and_executor(
        Arc::new(MockSqlAuditAgent),
        store.clone(),
        PostgresToolExecutionMode::Readonly,
        Arc::new(FakeApprovedSqlExecutor::default()),
    );
    let database = store
        .create_managed_database(
            "user-1",
            CreateManagedDatabaseRequest {
                name: "Warehouse".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "warehouse".to_owned(),
                username: "readonly".to_owned(),
                password: "password123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();
    let conversation = store
        .create_agent_conversation(
            "user-1",
            CreateAgentConversationRequest {
                title: Some("Query workspace".to_owned()),
                managed_database_id: Some(database.id.clone()),
            },
        )
        .await
        .unwrap();
    let turn = store
        .create_agent_turn(
            "user-1",
            &conversation.id,
            CreateAgentTurnRequest {
                message: "查询 agent_events 表的数据".to_owned(),
                managed_database_id: Some(database.id.clone()),
                dashboard_context: None,
                client_request_id: None,
            },
        )
        .await
        .unwrap();
    let assistant = store
        .append_agent_message(
            "user-1",
            &conversation.id,
            Some(&turn.id),
            AgentMessageRole::Assistant,
            "这是查询结果。",
            Some(json!({
                "kind": "assistant_response",
                "query_result_tables": [{
                    "managed_database_id": database.id,
                    "sql": "select id, event_type from agent_events order by id limit 2",
                    "result": {
                        "columns": ["id", "event_type"],
                        "rows": [
                            { "id": 1, "event_type": "turn_started" },
                            { "id": 2, "event_type": "message_created" }
                        ],
                        "row_count": 2,
                        "truncated": false,
                        "elapsed_ms": 4,
                        "refreshed_at": "1970-01-01T00:00:00Z"
                    }
                }]
            })),
        )
        .await
        .unwrap();
    store
        .set_agent_turn_assistant_message("user-1", &turn.id, &assistant.id)
        .await
        .unwrap();
    store
        .append_agent_turn_event(
            "user-1",
            &turn.id,
            AgentEventType::MessageCreated,
            json!({
                "message_id": assistant.id,
                "role": "assistant",
            }),
        )
        .await
        .unwrap();
    store
        .update_agent_turn_status("user-1", &turn.id, AgentTurnStatus::Completed, None)
        .await
        .unwrap();
    store
        .append_agent_turn_event(
            "user-1",
            &turn.id,
            AgentEventType::TurnCompleted,
            json!({ "status": "completed" }),
        )
        .await
        .unwrap();

    let messages_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/chat/conversations/conversation-1/messages",
        ))
        .await
        .unwrap();
    assert_eq!(messages_response.status(), StatusCode::OK);
    let messages = response_json(messages_response).await;
    assert_eq!(messages[1]["parts"][0]["kind"], "markdown");
    assert_eq!(messages[1]["parts"][1]["kind"], "query_result_table");
    assert_eq!(messages[1]["parts"][1]["result"]["row_count"], 2);

    let stream_response = app
        .oneshot(auth_request("/api/v1/chat/turns/turn-1/stream?after_seq=0"))
        .await
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);
    let stream_body = axum::body::to_bytes(stream_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stream_body = String::from_utf8(stream_body.to_vec()).unwrap();
    assert!(stream_body.contains(r#""type":"assistant_done""#));
    assert!(stream_body.contains(r#""kind":"query_result_table""#));
}

#[tokio::test]
async fn chat_sql_execution_persists_select_result_table_without_llm_provider() {
    let store = Arc::new(TestStore::default());
    let executor = Arc::new(FakeChatSqlExecutor::with_outcomes(vec![
        FakeChatSqlOutcome::Ok(ChatSqlExecutionOutcome::Query {
            statement_kind: SqlStatementKind::Select,
            result: DatapanelQueryResult {
                columns: vec!["id".to_owned(), "event_type".to_owned()],
                rows: vec![json!({ "id": 1, "event_type": "turn_started" })],
                row_count: 1,
                truncated: false,
                elapsed_ms: 4,
                refreshed_at: time::OffsetDateTime::UNIX_EPOCH,
            },
            saveable: true,
        }),
    ]));
    let app = test_app_with_agent_store_executors(
        Arc::new(MockSqlAuditAgent),
        store.clone(),
        PostgresToolExecutionMode::Readonly,
        Arc::new(FakeApprovedSqlExecutor::default()),
        executor.clone(),
    );
    let (_database, conversation) = create_sql_mode_workspace(&store).await;

    let response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            &format!(
                "/api/v1/chat/conversations/{}/sql-executions",
                conversation.id
            ),
            json!({
                "sql": "select id, event_type from agent_events",
                "client_request_id": "client-select-1"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["turn"]["status"], "completed");
    assert_eq!(
        payload["user_message"]["content"],
        "select id, event_type from agent_events"
    );
    assert_eq!(
        payload["assistant_message"]["parts"][1]["kind"],
        "query_result_table"
    );
    assert_eq!(
        payload["assistant_message"]["parts"][1]["result"]["row_count"],
        1
    );
    assert!(
        payload["assistant_message"]["parts"][1]
            .get("saveable")
            .is_none()
    );
    assert_eq!(
        executor.sql.lock().unwrap().as_slice(),
        &["select id, event_type from agent_events".to_owned()]
    );

    let messages_response = app
        .oneshot(auth_request(&format!(
            "/api/v1/chat/conversations/{}/messages",
            conversation.id
        )))
        .await
        .unwrap();
    assert_eq!(messages_response.status(), StatusCode::OK);
    let messages = response_json(messages_response).await;
    assert_eq!(messages[1]["parts"][1]["kind"], "query_result_table");
    assert_eq!(messages[1]["parts"][1]["result"]["columns"][0], "id");
}

#[tokio::test]
async fn chat_sql_execution_persists_update_summary() {
    let store = Arc::new(TestStore::default());
    let executor = Arc::new(FakeChatSqlExecutor::with_outcomes(vec![
        FakeChatSqlOutcome::Ok(ChatSqlExecutionOutcome::Summary {
            statement_kind: SqlStatementKind::Update,
            affected_rows: Some(3),
            elapsed_ms: 9,
        }),
    ]));
    let app = test_app_with_agent_store_executors(
        Arc::new(MockSqlAuditAgent),
        store.clone(),
        PostgresToolExecutionMode::Readonly,
        Arc::new(FakeApprovedSqlExecutor::default()),
        executor,
    );
    let (_database, conversation) = create_sql_mode_workspace(&store).await;

    let response = app
        .oneshot(auth_json_request(
            "POST",
            &format!(
                "/api/v1/chat/conversations/{}/sql-executions",
                conversation.id
            ),
            json!({ "sql": "update accounts set active = false where stale" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["turn"]["status"], "completed");
    assert_eq!(
        payload["assistant_message"]["parts"][1]["kind"],
        "sql_execution_summary"
    );
    assert_eq!(
        payload["assistant_message"]["parts"][1]["statement_kind"],
        "update"
    );
    assert_eq!(payload["assistant_message"]["parts"][1]["affected_rows"], 3);
    assert_eq!(payload["assistant_message"]["parts"][1]["elapsed_ms"], 9);
}

#[tokio::test]
async fn chat_sql_execution_marks_returning_results_not_saveable() {
    let store = Arc::new(TestStore::default());
    let executor = Arc::new(FakeChatSqlExecutor::with_outcomes(vec![
        FakeChatSqlOutcome::Ok(ChatSqlExecutionOutcome::Query {
            statement_kind: SqlStatementKind::Insert,
            result: DatapanelQueryResult {
                columns: vec!["id".to_owned()],
                rows: vec![json!({ "id": 42 })],
                row_count: 1,
                truncated: false,
                elapsed_ms: 5,
                refreshed_at: time::OffsetDateTime::UNIX_EPOCH,
            },
            saveable: false,
        }),
    ]));
    let app = test_app_with_agent_store_executors(
        Arc::new(MockSqlAuditAgent),
        store.clone(),
        PostgresToolExecutionMode::Readonly,
        Arc::new(FakeApprovedSqlExecutor::default()),
        executor,
    );
    let (_database, conversation) = create_sql_mode_workspace(&store).await;

    let response = app
        .oneshot(auth_json_request(
            "POST",
            &format!(
                "/api/v1/chat/conversations/{}/sql-executions",
                conversation.id
            ),
            json!({ "sql": "insert into accounts(name) values ('a') returning id" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(
        payload["assistant_message"]["parts"][1]["kind"],
        "query_result_table"
    );
    assert_eq!(payload["assistant_message"]["parts"][1]["saveable"], false);
}

#[tokio::test]
async fn chat_sql_execution_persists_executor_failure() {
    let store = Arc::new(TestStore::default());
    let executor = Arc::new(FakeChatSqlExecutor::with_outcomes(vec![
        FakeChatSqlOutcome::Err("database rejected statement".to_owned()),
    ]));
    let app = test_app_with_agent_store_executors(
        Arc::new(MockSqlAuditAgent),
        store.clone(),
        PostgresToolExecutionMode::Readonly,
        Arc::new(FakeApprovedSqlExecutor::default()),
        executor.clone(),
    );
    let (_database, conversation) = create_sql_mode_workspace(&store).await;

    let response = app
        .oneshot(auth_json_request(
            "POST",
            &format!(
                "/api/v1/chat/conversations/{}/sql-executions",
                conversation.id
            ),
            json!({ "sql": "select * from missing_table" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["turn"]["status"], "failed");
    assert!(
        payload["assistant_message"]["content"]
            .as_str()
            .unwrap()
            .contains("database rejected statement")
    );
    assert_eq!(payload["assistant_message"]["parts"][0]["kind"], "markdown");
    assert_eq!(
        payload["assistant_message"]["parts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        executor.sql.lock().unwrap().as_slice(),
        &["select * from missing_table".to_owned()]
    );
}

#[tokio::test]
async fn chat_sql_execution_persists_validation_failure_for_multiple_statements() {
    let store = Arc::new(TestStore::default());
    let app = test_app_with_agent_store_execution_and_executor(
        Arc::new(MockSqlAuditAgent),
        store.clone(),
        PostgresToolExecutionMode::Readonly,
        Arc::new(FakeApprovedSqlExecutor::default()),
    );
    let (_database, conversation) = create_sql_mode_workspace(&store).await;

    let response = app
        .oneshot(auth_json_request(
            "POST",
            &format!(
                "/api/v1/chat/conversations/{}/sql-executions",
                conversation.id
            ),
            json!({ "sql": "select 1; select 2" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["turn"]["status"], "failed");
    assert!(
        payload["assistant_message"]["content"]
            .as_str()
            .unwrap()
            .contains("exactly one")
    );
}

#[tokio::test]
async fn chat_sql_execution_persists_validation_failure_for_transaction_control() {
    let store = Arc::new(TestStore::default());
    let app = test_app_with_agent_store_execution_and_executor(
        Arc::new(MockSqlAuditAgent),
        store.clone(),
        PostgresToolExecutionMode::Readonly,
        Arc::new(FakeApprovedSqlExecutor::default()),
    );
    let (_database, conversation) = create_sql_mode_workspace(&store).await;

    let response = app
        .oneshot(auth_json_request(
            "POST",
            &format!(
                "/api/v1/chat/conversations/{}/sql-executions",
                conversation.id
            ),
            json!({ "sql": "begin" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["turn"]["status"], "failed");
    assert!(
        payload["assistant_message"]["content"]
            .as_str()
            .unwrap()
            .contains("transaction and control")
    );
}

#[tokio::test]
async fn chat_turn_reports_provider_not_configured_without_assistant_message() {
    let app = test_app();
    create_test_database(&app).await;

    let settings_response = app
        .clone()
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/settings/llm-provider",
            json!({
                "provider": "openai_compatible",
                "base_url": "http://127.0.0.1:9/v1/chat/completions",
                "model": "chat-model",
                "api_mode": "chat_completions"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(settings_response.status(), StatusCode::OK);

    let _conversation = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
            json!({ "title": "Provider required chat" }),
        ))
        .await
        .unwrap();
    let turn_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations/conversation-1/turns",
            json!({
                "message": "hello",
                "managed_database_id": "db-1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(turn_response.status(), StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream_response = app
        .clone()
        .oneshot(auth_request("/api/v1/chat/turns/turn-1/stream?after_seq=0"))
        .await
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);
    let stream_body = axum::body::to_bytes(stream_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stream_body = String::from_utf8(stream_body.to_vec()).unwrap();
    assert!(stream_body.contains(r#""type":"turn_failed""#));
    assert!(stream_body.contains(r#""error_code":"provider_not_configured""#));
    assert!(stream_body.contains("workspace.providerNotConfigured"));

    let messages_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/chat/conversations/conversation-1/messages",
        ))
        .await
        .unwrap();
    let messages = response_json(messages_response).await;
    assert_eq!(messages.as_array().unwrap().len(), 1);
    assert_eq!(messages[0]["role"], "user");
}

#[tokio::test]
async fn chat_turn_cancel_prevents_late_assistant_message_and_action() {
    let app = test_app();
    create_test_database(&app).await;
    let (base_url, captured_body, release_response) =
        spawn_delayed_openai_compatible_mock_with_content(
            r#"{
                "message": "This late reply should not be persisted.",
                "actions": [{
                    "kind": "create_sql_audit",
                    "title": "Create SQL audit",
                    "description": "Late audit",
                    "sql": "select 1"
                }]
            }"#,
        )
        .await;

    let settings_response = app
        .clone()
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/settings/llm-provider",
            json!({
                "provider": "openai_compatible",
                "base_url": base_url,
                "model": "chat-model",
                "api_mode": "chat_completions",
                "api_key": "sk-user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(settings_response.status(), StatusCode::OK);

    let _conversation = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
            json!({ "title": "Cancel chat" }),
        ))
        .await
        .unwrap();
    let turn_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations/conversation-1/turns",
            json!({
                "message": "please audit later",
                "managed_database_id": "db-1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(turn_response.status(), StatusCode::OK);

    for _ in 0..20 {
        if captured_body.lock().unwrap().is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(captured_body.lock().unwrap().is_some());

    let cancel_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/turns/turn-1/cancel",
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(cancel_response.status(), StatusCode::OK);
    let cancelled_turn = response_json(cancel_response).await;
    assert_eq!(cancelled_turn["status"], "cancelled");

    let _ = release_response.send(());
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let messages_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/chat/conversations/conversation-1/messages",
        ))
        .await
        .unwrap();
    let messages = response_json(messages_response).await;
    assert_eq!(messages.as_array().unwrap().len(), 1);
    assert_eq!(messages[0]["role"], "user");

    let actions_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/chat/conversations/conversation-1/actions",
        ))
        .await
        .unwrap();
    let actions = response_json(actions_response).await;
    assert_eq!(actions.as_array().unwrap().len(), 0);

    let stream_response = app
        .clone()
        .oneshot(auth_request("/api/v1/chat/turns/turn-1/stream?after_seq=0"))
        .await
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);
    let stream_body = axum::body::to_bytes(stream_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stream_body = String::from_utf8(stream_body.to_vec()).unwrap();
    assert!(stream_body.contains(r#""type":"turn_failed""#));
    assert!(stream_body.contains(r#""error_code":"turn_cancelled""#));
    assert!(!stream_body.contains(r#""type":"assistant_done""#));
    assert!(!stream_body.contains(r#""type":"action_proposed""#));
}

#[tokio::test]
async fn chat_turn_uses_user_llm_provider_settings_when_configured() {
    let app = test_app();
    create_test_database(&app).await;
    let (base_url, captured_body) = spawn_openai_compatible_mock_with_content(
        r#"{
            "message": "I prepared this audit from the configured provider.",
            "actions": [{
                "kind": "create_sql_audit",
                "title": "Create SQL audit",
                "description": "Review SQL from chat",
                "sql": "select * from users",
                "context": "chat requested review"
            }]
        }"#,
    )
    .await;

    let settings_response = app
        .clone()
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/settings/llm-provider",
            json!({
                "provider": "openai_compatible",
                "base_url": base_url,
                "model": "chat-model",
                "api_mode": "chat_completions",
                "api_key": "sk-user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(settings_response.status(), StatusCode::OK);

    let conversation_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
            json!({ "title": "Provider chat" }),
        ))
        .await
        .unwrap();
    assert_eq!(conversation_response.status(), StatusCode::OK);

    let turn_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations/conversation-1/turns",
            json!({
                "message": "audit this query",
                "managed_database_id": "db-1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(turn_response.status(), StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream_response = app
        .clone()
        .oneshot(auth_request("/api/v1/chat/turns/turn-1/stream?after_seq=0"))
        .await
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);
    let stream_body = axum::body::to_bytes(stream_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stream_body = String::from_utf8(stream_body.to_vec()).unwrap();
    assert!(stream_body.contains("I prepared this audit from the configured provider."));
    assert!(stream_body.contains(r#""type":"action_proposed""#));
    assert!(stream_body.contains(r#""type":"turn_waiting_for_user""#));

    let actions_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/chat/conversations/conversation-1/actions?status=proposed",
        ))
        .await
        .unwrap();
    assert_eq!(actions_response.status(), StatusCode::OK);
    let actions = response_json(actions_response).await;
    assert_eq!(actions.as_array().unwrap().len(), 1);
    assert_eq!(actions[0]["kind"], "create_sql_audit");
    assert_eq!(actions[0]["preview"]["kind"], "sql_audit");
    assert_eq!(actions[0]["preview"]["sql"], "select * from users");

    let captured = captured_body.lock().unwrap().clone().unwrap();
    assert_eq!(captured["model"], "chat-model");
    assert_eq!(captured["stream"], true);
    assert_eq!(captured["messages"][0]["role"], "system");
    assert!(
        captured["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("\"write_sql_execution\": false")
    );
}

#[tokio::test]
async fn chat_turn_streams_provider_text_deltas_to_chat_sse() {
    let app = test_app();
    create_test_database(&app).await;
    let (base_url, captured_body) = spawn_openai_compatible_sse_mock(
        r#"data: {"choices":[{"delta":{"content":"hel"}}]}

data: {"choices":[{"delta":{"content":"lo"}}]}

data: [DONE]

"#,
    )
    .await;

    let settings_response = app
        .clone()
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/settings/llm-provider",
            json!({
                "provider": "openai_compatible",
                "base_url": base_url,
                "model": "chat-model",
                "api_mode": "chat_completions",
                "api_key": "sk-user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(settings_response.status(), StatusCode::OK);

    let conversation_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
            json!({ "title": "Provider chat" }),
        ))
        .await
        .unwrap();
    assert_eq!(conversation_response.status(), StatusCode::OK);

    let turn_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations/conversation-1/turns",
            json!({
                "message": "answer directly",
                "managed_database_id": "db-1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(turn_response.status(), StatusCode::OK);

    let stream_response = app
        .clone()
        .oneshot(auth_request("/api/v1/chat/turns/turn-1/stream?after_seq=0"))
        .await
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);
    let stream_body = axum::body::to_bytes(stream_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stream_body = String::from_utf8(stream_body.to_vec()).unwrap();

    let captured = captured_body.lock().unwrap().clone().unwrap();
    assert_eq!(captured["stream"], true);
    assert!(stream_body.matches(r#""type":"assistant_delta""#).count() >= 2);
    assert!(stream_body.contains("hel"));
    assert!(stream_body.contains("hello"));
    assert!(stream_body.contains(r#""type":"assistant_done""#));
}

#[tokio::test]
async fn chat_turn_respects_disabled_llm_provider_streaming_setting() {
    let app = test_app();
    create_test_database(&app).await;
    let (base_url, captured_body) =
        spawn_openai_compatible_mock_with_content("Configured complete reply.").await;

    let settings_response = app
        .clone()
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/settings/llm-provider",
            json!({
                "provider": "openai_compatible",
                "base_url": base_url,
                "model": "chat-model",
                "api_mode": "chat_completions",
                "streaming_enabled": false,
                "api_key": "sk-user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(settings_response.status(), StatusCode::OK);

    let conversation_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
            json!({ "title": "Provider chat" }),
        ))
        .await
        .unwrap();
    assert_eq!(conversation_response.status(), StatusCode::OK);

    let turn_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations/conversation-1/turns",
            json!({
                "message": "answer directly",
                "managed_database_id": "db-1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(turn_response.status(), StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let captured = captured_body.lock().unwrap().clone().unwrap();
    assert_eq!(captured["model"], "chat-model");
    assert_eq!(captured["stream"], false);
}

#[tokio::test]
async fn chat_turn_blocks_without_llm_provider_key() {
    let app = test_app();
    create_test_database(&app).await;

    let settings_response = app
        .clone()
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/settings/llm-provider",
            json!({
                "provider": "openai_compatible",
                "base_url": "http://127.0.0.1:9/v1/chat/completions",
                "model": "chat-model",
                "api_mode": "chat_completions"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(settings_response.status(), StatusCode::OK);

    let _conversation = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
            json!({ "title": "Provider required chat" }),
        ))
        .await
        .unwrap();
    let turn_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations/conversation-1/turns",
            json!({
                "message": "select * from users",
                "managed_database_id": "db-1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(turn_response.status(), StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream_response = app
        .clone()
        .oneshot(auth_request("/api/v1/chat/turns/turn-1/stream?after_seq=0"))
        .await
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);
    let stream_body = axum::body::to_bytes(stream_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stream_body = String::from_utf8(stream_body.to_vec()).unwrap();
    assert!(stream_body.contains(r#""type":"turn_failed""#));
    assert!(stream_body.contains(r#""error_code":"provider_not_configured""#));
    assert!(stream_body.contains("workspace.providerNotConfigured"));

    let actions_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/chat/conversations/conversation-1/actions?status=proposed",
        ))
        .await
        .unwrap();
    assert_eq!(actions_response.status(), StatusCode::OK);
    let actions = response_json(actions_response).await;
    assert_eq!(actions.as_array().unwrap().len(), 0);

    let messages_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/chat/conversations/conversation-1/messages",
        ))
        .await
        .unwrap();
    assert_eq!(messages_response.status(), StatusCode::OK);
    let messages = response_json(messages_response).await;
    assert_eq!(messages.as_array().unwrap().len(), 1);
    assert_eq!(messages[0]["role"], "user");
}

#[tokio::test]
async fn chat_turn_fails_when_llm_returns_invalid_json() {
    let app = test_app();
    create_test_database(&app).await;
    let (base_url, _captured_body) =
        spawn_openai_compatible_mock_with_content(r#"{"message":"missing end""#).await;

    let settings_response = app
        .clone()
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/settings/llm-provider",
            json!({
                "provider": "openai_compatible",
                "base_url": base_url,
                "model": "chat-model",
                "api_mode": "chat_completions",
                "api_key": "sk-user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(settings_response.status(), StatusCode::OK);

    let _conversation = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
            json!({ "title": "Invalid provider chat" }),
        ))
        .await
        .unwrap();
    let turn_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations/conversation-1/turns",
            json!({
                "message": "hello",
                "managed_database_id": "db-1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(turn_response.status(), StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream_response = app
        .clone()
        .oneshot(auth_request("/api/v1/chat/turns/turn-1/stream?after_seq=0"))
        .await
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);
    let stream_body = axum::body::to_bytes(stream_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stream_body = String::from_utf8(stream_body.to_vec()).unwrap();
    assert!(stream_body.contains(r#""type":"turn_failed""#));
    assert!(stream_body.contains(r#""error_code":"invalid_model_response""#));
    assert!(stream_body.contains("workspace.invalidModelResponse"));

    let turn_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/chat/turns/turn-1/stream?after_seq=999",
        ))
        .await
        .unwrap();
    assert_eq!(turn_response.status(), StatusCode::OK);

    let actions_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/chat/conversations/conversation-1/actions?status=proposed",
        ))
        .await
        .unwrap();
    let actions = response_json(actions_response).await;
    assert_eq!(actions.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn chat_turn_accepts_plain_text_llm_final_message() {
    let app = test_app();
    create_test_database(&app).await;
    let (base_url, _captured_body) =
        spawn_openai_compatible_mock_with_content("我可以帮你查询数据库状态。").await;

    let settings_response = app
        .clone()
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/settings/llm-provider",
            json!({
                "provider": "openai_compatible",
                "base_url": base_url,
                "model": "chat-model",
                "api_mode": "chat_completions",
                "api_key": "sk-user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(settings_response.status(), StatusCode::OK);

    let _conversation = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
            json!({ "title": "Plain text chat" }),
        ))
        .await
        .unwrap();
    let turn_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations/conversation-1/turns",
            json!({
                "message": "你好",
                "managed_database_id": "db-1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(turn_response.status(), StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream_response = app
        .clone()
        .oneshot(auth_request("/api/v1/chat/turns/turn-1/stream?after_seq=0"))
        .await
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);
    let stream_body = axum::body::to_bytes(stream_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stream_body = String::from_utf8(stream_body.to_vec()).unwrap();
    assert!(stream_body.contains(r#""type":"assistant_delta""#));
    assert!(stream_body.contains("我可以帮你查询数据库状态。"));
    assert!(stream_body.contains(r#""type":"turn_completed""#));
    assert!(!stream_body.contains(r#""type":"turn_failed""#));
}

#[tokio::test]
async fn chat_turn_fails_when_llm_proposes_unknown_sql_audit_id() {
    let app = test_app();
    create_test_database(&app).await;
    let (base_url, _captured_body) = spawn_openai_compatible_mock_with_content(
        r#"{
            "message": "I will execute it.",
            "actions": [{
                "kind": "execute_sql_audit",
                "title": "Execute SQL audit",
                "description": "Execute missing audit",
                "sql_audit_id": "audit-missing"
            }]
        }"#,
    )
    .await;

    let settings_response = app
        .clone()
        .oneshot(auth_json_request(
            "PUT",
            "/api/v1/settings/llm-provider",
            json!({
                "provider": "openai_compatible",
                "base_url": base_url,
                "model": "chat-model",
                "api_mode": "chat_completions",
                "api_key": "sk-user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(settings_response.status(), StatusCode::OK);

    let _conversation = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
            json!({ "title": "Unknown audit chat" }),
        ))
        .await
        .unwrap();
    let turn_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations/conversation-1/turns",
            json!({
                "message": "execute it",
                "managed_database_id": "db-1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(turn_response.status(), StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream_response = app
        .clone()
        .oneshot(auth_request("/api/v1/chat/turns/turn-1/stream?after_seq=0"))
        .await
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);
    let stream_body = axum::body::to_bytes(stream_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stream_body = String::from_utf8(stream_body.to_vec()).unwrap();
    assert!(stream_body.contains(r#""type":"turn_failed""#));
    assert!(stream_body.contains(r#""error_code":"invalid_action_intent""#));
    assert!(stream_body.contains("workspace.invalidActionIntent"));

    let actions_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/chat/conversations/conversation-1/actions?status=proposed",
        ))
        .await
        .unwrap();
    let actions = response_json(actions_response).await;
    assert_eq!(actions.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn applying_chat_sql_audit_action_uses_existing_audit_flow() {
    let app = test_app();
    create_test_database(&app).await;
    configure_workbench_llm_provider_with_contents(
        &app,
        [
            r#"{
                "message": "I prepared this audit from the configured provider.",
                "actions": [{
                    "kind": "create_sql_audit",
                    "title": "Create SQL audit",
                    "description": "Review SQL from chat",
                    "sql": "select * from users",
                    "context": "chat requested review"
                }]
            }"#,
            r#"{
                "summary": "Provider SQL audit completed.",
                "risk_score": 7,
                "findings": []
            }"#,
            r#"{
                "message": "SQL 审计已完成，审计记录 audit-1 已生成。",
                "actions": []
            }"#,
        ],
    )
    .await;

    let _conversation = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
            json!({ "title": "SQL review" }),
        ))
        .await
        .unwrap();
    let turn_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations/conversation-1/turns",
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
            "/api/v1/chat/actions/action-1/apply",
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(apply_response.status(), StatusCode::OK);
    let action = response_json(apply_response).await;
    assert_eq!(action["status"], "applying");
    let stream_after_seq = action["stream_after_seq"].as_i64().unwrap();
    assert!(stream_after_seq > 0);

    let action = wait_for_chat_action_status(
        app.clone(),
        "conversation-1",
        "action-1",
        AgentActionStatus::Applied,
    )
    .await;
    assert_eq!(action["resource_kind"], "sql_audit");
    assert_eq!(action["resource_id"], "audit-1");

    let final_message =
        wait_for_chat_message_containing(app.clone(), "conversation-1", "SQL 审计已完成").await;
    assert_eq!(final_message["role"], "assistant");

    let messages_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/chat/conversations/conversation-1/messages",
        ))
        .await
        .unwrap();
    assert_eq!(messages_response.status(), StatusCode::OK);
    let messages = response_json(messages_response).await;
    assert_eq!(messages.as_array().unwrap().len(), 3);
    assert_eq!(messages[2]["role"], "assistant");
    assert!(messages[2]["content"].as_str().unwrap().contains("audit-1"));
    assert_eq!(messages[2]["parts"][0]["kind"], "markdown");

    let stream_response = app
        .clone()
        .oneshot(auth_request(&format!(
            "/api/v1/chat/turns/turn-1/stream?after_seq={stream_after_seq}"
        )))
        .await
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);
    let stream_body = axum::body::to_bytes(stream_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stream_body = String::from_utf8(stream_body.to_vec()).unwrap();
    assert!(stream_body.contains(r#""type":"tool_started""#));
    assert!(stream_body.contains(r#""type":"tool_finished""#));
    assert_eq!(
        stream_body
            .matches("Checking SQL safety and policy")
            .count(),
        1
    );
    assert!(stream_body.contains(r#""name":"sql_audit""#));
    assert!(stream_body.contains(r#""synthesizing""#));
    assert!(stream_body.contains(r#""type":"assistant_delta""#));
    assert!(stream_body.contains(r#""type":"assistant_done""#));
    assert!(!stream_body.contains(r#""role":"tool""#));
    assert!(!stream_body.contains(r#""type":"action_proposed""#));
    assert!(!stream_body.contains(r#""type":"turn_waiting_for_user""#));
    assert!(stream_body.contains(r#""type":"action_updated""#));

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
async fn applying_chat_sql_execution_action_executes_after_audit_approval() {
    let app = test_app_with_agent_execution_and_executor(
        Arc::new(MockSqlAuditAgent),
        PostgresToolExecutionMode::WriteGated,
        Arc::new(FakeApprovedSqlExecutor::default()),
    );
    create_test_database(&app).await;
    configure_workbench_llm_provider_with_contents(
        &app,
        [
            r#"{
                "message": "我准备好执行创建 test1 数据库的操作。确认后系统会先完成安全检查再执行。",
                "actions": [{
                    "kind": "create_sql_audit",
                    "title": "创建 test1 数据库",
                    "description": "执行创建 test1 数据库的 DDL 语句",
                    "sql": "CREATE DATABASE test1;",
                    "context": "用户请求新建一个名为 test1 的数据库",
                    "execution_purpose": "用户确认从聊天中创建 test1 数据库"
                }]
            }"#,
            r#"{
                "summary": "DDL statement passed the configured audit checks.",
                "risk_score": 5,
                "findings": []
            }"#,
            r#"{
                "message": "test1 数据库已创建成功。",
                "actions": []
            }"#,
        ],
    )
    .await;

    let _conversation = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations",
            json!({ "title": "Create database" }),
        ))
        .await
        .unwrap();
    let turn_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/chat/conversations/conversation-1/turns",
            json!({
                "message": "帮我创建一个test1 的数据库",
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
            "/api/v1/chat/actions/action-1/apply",
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(apply_response.status(), StatusCode::OK);
    let action = response_json(apply_response).await;
    assert_eq!(action["status"], "applying");
    let stream_after_seq = action["stream_after_seq"].as_i64().unwrap();
    assert!(stream_after_seq > 0);

    let action = wait_for_chat_action_status(
        app.clone(),
        "conversation-1",
        "action-1",
        AgentActionStatus::Applied,
    )
    .await;
    assert_eq!(action["resource_kind"], "sql_audit");
    assert_eq!(action["resource_id"], "audit-1");

    let message =
        wait_for_chat_message_containing(app.clone(), "conversation-1", "test1 数据库已创建成功")
            .await;
    let content = message["content"].as_str().unwrap();
    assert_eq!(message["role"], "assistant");
    assert!(!content.contains("Audit summary"));
    assert!(!content.contains("Findings"));

    let stream_response = app
        .clone()
        .oneshot(auth_request(&format!(
            "/api/v1/chat/turns/turn-1/stream?after_seq={stream_after_seq}"
        )))
        .await
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);
    let stream_body = axum::body::to_bytes(stream_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stream_body = String::from_utf8(stream_body.to_vec()).unwrap();
    assert!(stream_body.contains(r#""type":"tool_started""#));
    assert!(stream_body.contains(r#""type":"tool_finished""#));
    assert_eq!(
        stream_body
            .matches("Checking SQL safety and policy")
            .count(),
        1
    );
    assert_eq!(
        stream_body
            .matches("Executing the approved SQL operation")
            .count(),
        1
    );
    assert!(stream_body.contains(r#""name":"sql_audit""#));
    assert!(stream_body.contains(r#""name":"sql_execute""#));
    assert!(stream_body.contains(r#""synthesizing""#));
    assert!(stream_body.contains(r#""type":"assistant_delta""#));
    assert!(stream_body.contains(r#""type":"assistant_done""#));
    assert!(!stream_body.contains(r#""role":"tool""#));
    assert!(!stream_body.contains(r#""type":"action_proposed""#));
    assert!(!stream_body.contains(r#""type":"turn_waiting_for_user""#));

    let audit_response = app
        .oneshot(auth_request("/api/v1/sql-audits/audit-1"))
        .await
        .unwrap();
    assert_eq!(audit_response.status(), StatusCode::OK);
    let audit = response_json(audit_response).await;
    assert_eq!(audit["sql"], "CREATE DATABASE test1;");
    assert_eq!(audit["status"], "executed");
    assert_eq!(audit["execution_result"]["affected_rows"], 1);
}

#[tokio::test]
async fn applying_chat_datapanel_card_action_imports_card_into_workspace_panel() {
    let store = Arc::new(TestStore::default());
    let app = test_app_with_agent_store_execution_and_executor(
        Arc::new(MockSqlAuditAgent),
        store.clone(),
        PostgresToolExecutionMode::Readonly,
        Arc::new(FakeApprovedSqlExecutor::default()),
    );
    let (conversation, action) = create_datapanel_card_action_fixture(&store).await;

    let apply_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            &format!("/api/v1/chat/actions/{}/apply", action.id),
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(apply_response.status(), StatusCode::OK);
    let apply_payload = response_json(apply_response).await;
    assert_eq!(apply_payload["status"], "applying");
    assert_eq!(apply_payload["preview"]["kind"], "datapanel_card");
    assert_eq!(apply_payload["preview"]["title"], "Daily revenue");

    let final_action =
        wait_for_store_action_status(&store, &action.id, AgentActionStatus::Applied).await;
    assert_eq!(
        final_action.resource_kind,
        Some(AgentResourceKind::DatapanelCard)
    );

    let panel_response = app
        .oneshot(auth_request(&format!(
            "/api/v1/chat/conversations/{}/datapanel",
            conversation.id
        )))
        .await
        .unwrap();
    assert_eq!(panel_response.status(), StatusCode::OK);
    let panel = response_json(panel_response).await;
    assert_eq!(panel["cards"].as_array().unwrap().len(), 1);
    assert_eq!(panel["cards"][0]["title"], "Daily revenue");
}

#[tokio::test]
async fn applying_terminal_chat_action_returns_diagnostic_details() {
    let store = Arc::new(TestStore::default());
    let app = test_app_with_agent_store_execution_and_executor(
        Arc::new(MockSqlAuditAgent),
        store.clone(),
        PostgresToolExecutionMode::Readonly,
        Arc::new(FakeApprovedSqlExecutor::default()),
    );
    let (_conversation, action) = create_datapanel_card_action_fixture(&store).await;

    let first_apply = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            &format!("/api/v1/chat/actions/{}/apply", action.id),
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(first_apply.status(), StatusCode::OK);
    let apply_payload = response_json(first_apply).await;
    assert_eq!(apply_payload["status"], "applying");
    wait_for_store_action_status(&store, &action.id, AgentActionStatus::Applied).await;

    let repeat_apply = app
        .oneshot(auth_json_request(
            "POST",
            &format!("/api/v1/chat/actions/{}/apply", action.id),
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(repeat_apply.status(), StatusCode::CONFLICT);
    let payload = response_json(repeat_apply).await;
    assert_eq!(
        payload["error"],
        "agent action cannot be applied from applied status"
    );
    assert_eq!(payload["details"]["action_id"], action.id);
    assert_eq!(payload["details"]["action_kind"], "create_datapanel_card");
    assert_eq!(payload["details"]["action_status"], "applied");
}

#[tokio::test]
async fn applying_sql_action_requires_earlier_same_turn_sql_actions_first() {
    let store = Arc::new(TestStore::default());
    let app = test_app_with_agent_store_execution_and_executor(
        Arc::new(CapturingSqlAuditAgent::default()),
        store.clone(),
        PostgresToolExecutionMode::WriteGated,
        Arc::new(PendingApprovedSqlExecutor),
    );
    let (_conversation, create_action, insert_action) =
        create_dependent_sql_actions_fixture(&store).await;

    let blocked_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            &format!("/api/v1/chat/actions/{}/apply", insert_action.id),
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(blocked_response.status(), StatusCode::CONFLICT);
    let payload = response_json(blocked_response).await;
    assert_eq!(
        payload["error"],
        "earlier SQL action from this turn must be applied before this action"
    );
    assert_eq!(payload["details"]["action_id"], insert_action.id);
    assert_eq!(payload["details"]["blocked_by_action_id"], create_action.id);
    assert_eq!(payload["details"]["blocked_by_action_status"], "proposed");

    let apply_create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            &format!("/api/v1/chat/actions/{}/apply", create_action.id),
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(apply_create_response.status(), StatusCode::OK);
    let applying_create = response_json(apply_create_response).await;
    assert_eq!(applying_create["status"], "applying");

    let still_blocked_response = app
        .oneshot(auth_json_request(
            "POST",
            &format!("/api/v1/chat/actions/{}/apply", insert_action.id),
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(still_blocked_response.status(), StatusCode::CONFLICT);
    let payload = response_json(still_blocked_response).await;
    assert_eq!(payload["details"]["blocked_by_action_status"], "applying");
}

#[tokio::test]
async fn applying_failed_chat_action_retries_action() {
    let store = Arc::new(TestStore::default());
    let app = test_app_with_agent_store_execution_and_executor(
        Arc::new(MockSqlAuditAgent),
        store.clone(),
        PostgresToolExecutionMode::Readonly,
        Arc::new(FakeApprovedSqlExecutor::default()),
    );
    let (_conversation, action) = create_datapanel_card_action_fixture(&store).await;
    store
        .update_agent_action_status("user-1", &action.id, AgentActionStatus::Failed, None, None)
        .await
        .unwrap();

    let apply_response = app
        .oneshot(auth_json_request(
            "POST",
            &format!("/api/v1/chat/actions/{}/apply", action.id),
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(apply_response.status(), StatusCode::OK);
    let apply_payload = response_json(apply_response).await;
    assert_eq!(apply_payload["status"], "applying");

    let final_action =
        wait_for_store_action_status(&store, &action.id, AgentActionStatus::Applied).await;
    assert_eq!(final_action.status, AgentActionStatus::Applied);
    assert_eq!(
        final_action.resource_kind,
        Some(AgentResourceKind::DatapanelCard)
    );
}

async fn wait_for_chat_action_status(
    app: Router,
    conversation_id: &str,
    action_id: &str,
    status: AgentActionStatus,
) -> Value {
    for _ in 0..50 {
        let response = app
            .clone()
            .oneshot(auth_request(&format!(
                "/api/v1/chat/conversations/{conversation_id}/actions"
            )))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let actions = response_json(response).await;

        if let Some(action) = actions
            .as_array()
            .unwrap()
            .iter()
            .find(|action| action["id"] == action_id && action["status"] == status.as_str())
        {
            return action.clone();
        }

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    panic!("timed out waiting for chat action {action_id} to become {status:?}");
}

async fn wait_for_chat_message_containing(
    app: Router,
    conversation_id: &str,
    needle: &str,
) -> Value {
    for _ in 0..50 {
        let response = app
            .clone()
            .oneshot(auth_request(&format!(
                "/api/v1/chat/conversations/{conversation_id}/messages"
            )))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let messages = response_json(response).await;

        if let Some(message) = messages.as_array().unwrap().iter().find(|message| {
            message["content"]
                .as_str()
                .is_some_and(|content| content.contains(needle))
        }) {
            return message.clone();
        }

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    panic!("timed out waiting for chat message containing {needle:?}");
}

async fn wait_for_store_action_status(
    store: &Arc<TestStore>,
    action_id: &str,
    status: AgentActionStatus,
) -> AgentAction {
    for _ in 0..50 {
        let action = store.get_agent_action("user-1", action_id).await.unwrap();

        if action.status == status {
            return action;
        }

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    panic!("timed out waiting for store action {action_id} to become {status:?}");
}

async fn create_datapanel_card_action_fixture(
    store: &Arc<TestStore>,
) -> (AgentConversation, AgentAction) {
    let database = store
        .create_managed_database(
            "user-1",
            CreateManagedDatabaseRequest {
                name: "Warehouse".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "warehouse".to_owned(),
                username: "readonly".to_owned(),
                password: "password123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();
    let conversation = store
        .create_agent_conversation(
            "user-1",
            CreateAgentConversationRequest {
                title: Some("datapanel workspace".to_owned()),
                managed_database_id: Some(database.id.clone()),
            },
        )
        .await
        .unwrap();
    let turn = store
        .create_agent_turn(
            "user-1",
            &conversation.id,
            CreateAgentTurnRequest {
                message: "show revenue".to_owned(),
                managed_database_id: Some(database.id.clone()),
                dashboard_context: None,
                client_request_id: None,
            },
        )
        .await
        .unwrap();
    let action = store
        .create_agent_action(
            "user-1",
            &turn.id,
            CreateAgentActionRequest {
                kind: AgentActionKind::CreateDatapanelCard,
                title: "Create Datapanel card".to_owned(),
                description: "Import revenue table".to_owned(),
                payload: json!({
                    "managed_database_id": database.id,
                    "title": "Daily revenue",
                    "description": "Revenue by day",
                    "kind": "table",
                    "sql": "select '2026-06-06' as day, 42 as revenue",
                    "layout": { "x": 0, "y": 0, "w": 12, "h": 5 },
                    "result": {
                        "columns": ["day", "revenue"],
                        "rows": [{ "day": "2026-06-06", "revenue": 42 }],
                        "row_count": 1,
                        "truncated": false,
                        "elapsed_ms": 2,
                        "refreshed_at": "1970-01-01T00:00:00Z"
                    }
                }),
                resource_kind: Some(AgentResourceKind::DatapanelCard),
                resource_id: None,
                requires_confirmation: true,
            },
        )
        .await
        .unwrap();

    (conversation, action)
}

async fn create_dependent_sql_actions_fixture(
    store: &Arc<TestStore>,
) -> (AgentConversation, AgentAction, AgentAction) {
    let database = store
        .create_managed_database(
            "user-1",
            CreateManagedDatabaseRequest {
                name: "Warehouse".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "warehouse".to_owned(),
                username: "readonly".to_owned(),
                password: "password123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();
    let conversation = store
        .create_agent_conversation(
            "user-1",
            CreateAgentConversationRequest {
                title: Some("dependent sql workspace".to_owned()),
                managed_database_id: Some(database.id.clone()),
            },
        )
        .await
        .unwrap();
    let turn = store
        .create_agent_turn(
            "user-1",
            &conversation.id,
            CreateAgentTurnRequest {
                message: "create test table and insert data".to_owned(),
                managed_database_id: Some(database.id.clone()),
                dashboard_context: None,
                client_request_id: None,
            },
        )
        .await
        .unwrap();
    let create_action = store
        .create_agent_action(
            "user-1",
            &turn.id,
            CreateAgentActionRequest {
                kind: AgentActionKind::CreateSqlAudit,
                title: "Create test table".to_owned(),
                description: "Create the table before inserting rows.".to_owned(),
                payload: json!({
                    "managed_database_id": database.id,
                    "request": {
                        "sql": "create table test (id integer primary key)",
                        "execution_purpose": "Create test table"
                    }
                }),
                resource_kind: Some(AgentResourceKind::SqlAudit),
                resource_id: None,
                requires_confirmation: true,
            },
        )
        .await
        .unwrap();
    let insert_action = store
        .create_agent_action(
            "user-1",
            &turn.id,
            CreateAgentActionRequest {
                kind: AgentActionKind::CreateSqlAudit,
                title: "Insert test rows".to_owned(),
                description: "Insert rows after the test table exists.".to_owned(),
                payload: json!({
                    "managed_database_id": database.id,
                    "request": {
                        "sql": "insert into test (id) values (1)",
                        "execution_purpose": "Insert test rows"
                    }
                }),
                resource_kind: Some(AgentResourceKind::SqlAudit),
                resource_id: None,
                requires_confirmation: true,
            },
        )
        .await
        .unwrap();

    (conversation, create_action, insert_action)
}

#[tokio::test]
async fn refreshing_datapanel_card_rejects_non_select_sql_before_pool_use() {
    let store = Arc::new(TestStore::default());
    let app = test_app_with_agent_store_execution_and_executor(
        Arc::new(MockSqlAuditAgent),
        store.clone(),
        PostgresToolExecutionMode::Readonly,
        Arc::new(FakeApprovedSqlExecutor::default()),
    );
    let database = store
        .create_managed_database(
            "user-1",
            CreateManagedDatabaseRequest {
                name: "Warehouse".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "warehouse".to_owned(),
                username: "readonly".to_owned(),
                password: "password123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();
    let conversation = store
        .create_agent_conversation(
            "user-1",
            CreateAgentConversationRequest {
                title: Some("datapanel workspace".to_owned()),
                managed_database_id: Some(database.id.clone()),
            },
        )
        .await
        .unwrap();
    let panel = store
        .get_or_create_datapanel("user-1", &conversation.id)
        .await
        .unwrap();
    let card = store
        .create_datapanel_card(
            "user-1",
            &panel.id,
            CreateDatapanelCardRequest {
                managed_database_id: database.id,
                source_action_id: None,
                title: "Bad card".to_owned(),
                description: None,
                kind: liquid_core::DatapanelCardKind::Table,
                sql: "delete from users".to_owned(),
                chart: None,
                layout: liquid_core::DatapanelCardLayout {
                    x: 0,
                    y: 0,
                    w: 12,
                    h: 5,
                },
                result: DatapanelQueryResult {
                    columns: vec![],
                    rows: vec![],
                    row_count: 0,
                    truncated: false,
                    elapsed_ms: 0,
                    refreshed_at: time::OffsetDateTime::UNIX_EPOCH,
                },
            },
        )
        .await
        .unwrap();

    let response = app
        .oneshot(auth_json_request(
            "POST",
            &format!("/api/v1/datapanels/{}/cards/{}/refresh", panel.id, card.id),
            json!({}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload = response_json(response).await;
    assert!(payload["error"].as_str().unwrap().contains("SELECT"));
}

#[tokio::test]
async fn saving_chat_query_result_creates_table_datapanel_card() {
    let store = Arc::new(TestStore::default());
    let app = test_app_with_agent_store_execution_and_executor(
        Arc::new(MockSqlAuditAgent),
        store.clone(),
        PostgresToolExecutionMode::Readonly,
        Arc::new(FakeApprovedSqlExecutor::default()),
    );
    let database = store
        .create_managed_database(
            "user-1",
            CreateManagedDatabaseRequest {
                name: "Warehouse".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "warehouse".to_owned(),
                username: "readonly".to_owned(),
                password: "password123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();
    let conversation = store
        .create_agent_conversation(
            "user-1",
            CreateAgentConversationRequest {
                title: Some("datapanel workspace".to_owned()),
                managed_database_id: Some(database.id.clone()),
            },
        )
        .await
        .unwrap();

    let response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            &format!(
                "/api/v1/chat/conversations/{}/datapanel/cards",
                conversation.id
            ),
            json!({
                "managed_database_id": database.id,
                "title": "Agent events",
                "description": "Recent agent events",
                "sql": "select id, event_type from agent_events order by id limit 2;",
                "result": {
                    "columns": ["id", "event_type"],
                    "rows": [
                        { "id": 1, "event_type": "turn_started" },
                        { "id": 2, "event_type": "message_created" }
                    ],
                    "row_count": 2,
                    "truncated": false,
                    "elapsed_ms": 4,
                    "refreshed_at": "1970-01-01T00:00:00Z"
                }
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let card = response_json(response).await;
    assert_eq!(card["kind"], "table");
    assert_eq!(card["title"], "Agent events");
    assert_eq!(
        card["sql"],
        "select id, event_type from agent_events order by id limit 2"
    );
    assert_eq!(card["result"]["row_count"], 2);
    assert_eq!(card["layout"]["w"], 12);
}

#[tokio::test]
async fn saving_chat_query_result_rejects_non_select_sql() {
    let store = Arc::new(TestStore::default());
    let app = test_app_with_agent_store_execution_and_executor(
        Arc::new(MockSqlAuditAgent),
        store.clone(),
        PostgresToolExecutionMode::Readonly,
        Arc::new(FakeApprovedSqlExecutor::default()),
    );
    let database = store
        .create_managed_database(
            "user-1",
            CreateManagedDatabaseRequest {
                name: "Warehouse".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "warehouse".to_owned(),
                username: "readonly".to_owned(),
                password: "password123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();
    let conversation = store
        .create_agent_conversation(
            "user-1",
            CreateAgentConversationRequest {
                title: Some("datapanel workspace".to_owned()),
                managed_database_id: Some(database.id.clone()),
            },
        )
        .await
        .unwrap();

    let response = app
        .oneshot(auth_json_request(
            "POST",
            &format!(
                "/api/v1/chat/conversations/{}/datapanel/cards",
                conversation.id
            ),
            json!({
                "managed_database_id": database.id,
                "title": "Bad card",
                "sql": "delete from agent_events",
                "result": {
                    "columns": [],
                    "rows": [],
                    "row_count": 0,
                    "truncated": false,
                    "elapsed_ms": 0,
                    "refreshed_at": "1970-01-01T00:00:00Z"
                }
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload = response_json(response).await;
    assert!(payload["error"].as_str().unwrap().contains("SELECT"));
}

#[tokio::test]
async fn creating_datapanel_preview_requires_bearer_token() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/datapanels/panel-1/preview")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn datapanel_preview_reuses_slug_and_public_response_excludes_private_fields() {
    let store = Arc::new(TestStore::default());
    let app = test_app_with_agent_store_execution_and_executor(
        Arc::new(MockSqlAuditAgent),
        store.clone(),
        PostgresToolExecutionMode::Readonly,
        Arc::new(FakeApprovedSqlExecutor::default()),
    );
    let database = store
        .create_managed_database(
            "user-1",
            CreateManagedDatabaseRequest {
                name: "Warehouse".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "warehouse".to_owned(),
                username: "readonly".to_owned(),
                password: "password123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();
    let conversation = store
        .create_agent_conversation(
            "user-1",
            CreateAgentConversationRequest {
                title: Some("datapanel workspace".to_owned()),
                managed_database_id: Some(database.id.clone()),
            },
        )
        .await
        .unwrap();
    let panel = store
        .get_or_create_datapanel("user-1", &conversation.id)
        .await
        .unwrap();
    store
        .create_datapanel_card(
            "user-1",
            &panel.id,
            CreateDatapanelCardRequest {
                managed_database_id: database.id,
                source_action_id: Some("action-1".to_owned()),
                title: "Agent events".to_owned(),
                description: Some("Recent agent events".to_owned()),
                kind: liquid_core::DatapanelCardKind::Table,
                sql: "select id, event_type from agent_events".to_owned(),
                chart: None,
                layout: liquid_core::DatapanelCardLayout {
                    x: 0,
                    y: 0,
                    w: 12,
                    h: 5,
                },
                result: DatapanelQueryResult {
                    columns: vec!["id".to_owned(), "event_type".to_owned()],
                    rows: vec![json!({ "id": 1, "event_type": "turn_started" })],
                    row_count: 1,
                    truncated: false,
                    elapsed_ms: 3,
                    refreshed_at: time::OffsetDateTime::UNIX_EPOCH,
                },
            },
        )
        .await
        .unwrap();

    let first_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            &format!("/api/v1/datapanels/{}/preview", panel.id),
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(first_response.status(), StatusCode::OK);
    let first = response_json(first_response).await;

    let second_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            &format!("/api/v1/datapanels/{}/preview", panel.id),
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(second_response.status(), StatusCode::OK);
    let second = response_json(second_response).await;
    assert_eq!(second["slug"], first["slug"]);

    let slug = first["slug"].as_str().unwrap();
    let public_response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/datapanel-previews/{slug}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(public_response.status(), StatusCode::OK);
    let preview = response_json(public_response).await;
    assert_eq!(preview["title"], "新的数据面板");
    assert_eq!(preview["cards"][0]["title"], "Agent events");
    assert_eq!(preview["cards"][0]["result"]["row_count"], 1);
    assert!(preview["cards"][0].get("sql").is_none());
    assert!(preview["cards"][0].get("managed_database_id").is_none());
    assert!(preview["cards"][0].get("source_action_id").is_none());
    assert!(preview.get("owner_user_id").is_none());
}

#[tokio::test]
async fn unknown_datapanel_preview_slug_returns_not_found() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/datapanel-previews/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
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
                "tags": ["prod", "finance"],
                "ssl_mode": "prefer"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(create_response.status(), StatusCode::CREATED);
    let payload = response_json(create_response).await;
    assert_eq!(payload["name"], "Warehouse");
    assert_eq!(payload["tags"], json!(["prod", "finance"]));
    assert_eq!(payload["has_password"], true);
    assert!(payload.get("password").is_none());

    let update_response = app
        .clone()
        .oneshot(auth_json_request(
            "PATCH",
            "/api/v1/managed-databases/db-1",
            json!({
                "name": "Warehouse Replica",
                "tags": ["replica"],
                "ssl_mode": "require"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(update_response.status(), StatusCode::OK);
    let payload = response_json(update_response).await;
    assert_eq!(payload["name"], "Warehouse Replica");
    assert_eq!(payload["tags"], json!(["replica"]));
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
async fn managed_database_audit_sql_exposes_write_tool_when_write_gated() {
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
    assert!(tool_names.iter().any(|name| name == "pg_execute_write_sql"));
}

#[tokio::test]
async fn managed_database_audit_sql_keeps_write_tool_hidden_when_readonly() {
    let agent = Arc::new(CapturingSqlAuditAgent::default());
    let app = test_app_with_agent_and_execution(agent.clone(), PostgresToolExecutionMode::Readonly);
    create_test_database(&app).await;

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
async fn sql_audit_list_supports_filters_pagination_and_headers() {
    let app = test_app_with_agent_and_execution(
        Arc::new(CapturingSqlAuditAgent::default()),
        PostgresToolExecutionMode::WriteGated,
    );
    create_test_database(&app).await;

    let select_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "select * from users"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(select_response.status(), StatusCode::CREATED);

    let update_response = app
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
    assert_eq!(update_response.status(), StatusCode::CREATED);

    let drop_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "drop table users"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(drop_response.status(), StatusCode::CREATED);

    let approve_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/sql-audits/audit-2/approve",
            json!({
                "comment": "approved"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(approve_response.status(), StatusCode::OK);

    let execute_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sql-audits/audit-2/execute")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(execute_response.status(), StatusCode::OK);

    let first_page_response = app
        .clone()
        .oneshot(auth_request("/api/v1/sql-audits?page=1&page_size=10"))
        .await
        .unwrap();
    assert_eq!(first_page_response.status(), StatusCode::OK);
    assert_eq!(
        first_page_response
            .headers()
            .get("x-total-count")
            .unwrap()
            .to_str()
            .unwrap(),
        "3"
    );
    assert_eq!(
        first_page_response
            .headers()
            .get("x-page-size")
            .unwrap()
            .to_str()
            .unwrap(),
        "10"
    );
    let payload = response_json(first_page_response).await;
    assert_eq!(payload.as_array().unwrap().len(), 3);

    let second_page_response = app
        .clone()
        .oneshot(auth_request("/api/v1/sql-audits?page=2&page_size=10"))
        .await
        .unwrap();
    assert_eq!(second_page_response.status(), StatusCode::OK);
    let payload = response_json(second_page_response).await;
    assert_eq!(payload.as_array().unwrap().len(), 0);

    let blocked_response = app
        .clone()
        .oneshot(auth_request("/api/v1/sql-audits?audit_status=blocked"))
        .await
        .unwrap();
    assert_eq!(blocked_response.status(), StatusCode::OK);
    let payload = response_json(blocked_response).await;
    assert_eq!(payload.as_array().unwrap().len(), 1);
    assert_eq!(payload[0]["status"], "blocked");

    let executed_response = app
        .clone()
        .oneshot(auth_request("/api/v1/sql-audits?execution_status=executed"))
        .await
        .unwrap();
    assert_eq!(executed_response.status(), StatusCode::OK);
    let payload = response_json(executed_response).await;
    assert_eq!(payload.as_array().unwrap().len(), 1);
    assert_eq!(payload[0]["status"], "executed");

    let not_executed_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/sql-audits?execution_status=not_executed",
        ))
        .await
        .unwrap();
    assert_eq!(not_executed_response.status(), StatusCode::OK);
    let payload = response_json(not_executed_response).await;
    assert_eq!(payload.as_array().unwrap().len(), 2);

    let time_range_response = app
        .clone()
        .oneshot(auth_request(
            "/api/v1/sql-audits?created_from=1969-01-01T00%3A00%3A00Z&created_to=1971-01-01T00%3A00%3A00Z",
        ))
        .await
        .unwrap();
    assert_eq!(time_range_response.status(), StatusCode::OK);
    assert_eq!(
        time_range_response
            .headers()
            .get("x-total-count")
            .unwrap()
            .to_str()
            .unwrap(),
        "3"
    );

    let invalid_page_size_response = app
        .oneshot(auth_request("/api/v1/sql-audits?page_size=25"))
        .await
        .unwrap();
    assert_eq!(invalid_page_size_response.status(), StatusCode::BAD_REQUEST);
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
async fn sql_audit_falls_back_to_deterministic_report_when_llm_report_is_invalid_json() {
    let app = test_app_with_agent_and_execution(
        Arc::new(InvalidJsonSqlAuditAgent),
        PostgresToolExecutionMode::WriteGated,
    );
    create_test_database(&app).await;

    let response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "create table test (id integer primary key)",
                "execution_purpose": "Create test table from chat"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = response_json(response).await;
    assert_eq!(payload["status"], "pending_approval");
    assert_eq!(payload["statement_kind"], "create");
    assert_eq!(payload["risk_score"], 25);
    assert_eq!(payload["report"]["risk_score"], 25);
    assert!(
        payload["report"]["summary"]
            .as_str()
            .unwrap()
            .contains("deterministic PostgreSQL parser")
    );
    assert!(
        payload["report"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["title"] == "LLM audit report unavailable")
    );
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
        tags: vec![],
        ssl_mode: ManagedDatabaseSslMode::Prefer,
        has_password: true,
    };

    assert_eq!(database.engine.as_str(), "postgres");
    assert_eq!(database.ssl_mode.as_str(), "prefer");
}
