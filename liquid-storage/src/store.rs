use async_trait::async_trait;
use liquid_core::{
    AgentAction, AgentActionStatus, AgentConversation, AgentEventRecord, AgentEventType,
    AgentMessage, AgentMessageRole, AgentResourceKind, AgentTurn, AgentTurnStatus,
    ApproveSqlAuditRequest, AuthResponse, BiCardLayoutUpdate, BiPanel, BiPanelCard, BiPanelExport,
    BiQueryResult, CompleteDatabaseBackup, CreateAgentActionRequest,
    CreateAgentConversationRequest, CreateAgentTurnRequest, CreateBiPanelCardRequest,
    CreateManagedDatabaseRequest, DatabaseBackupMetadataStore, DatabaseBackupMetadataStoreError,
    DatabaseBackupRecord, DatabaseBackupStatus, DatabaseRestoreRecord, LlmProviderSettings,
    LoginRequest, ManagedDatabase, ManagedDatabaseConnectionLoader,
    ManagedDatabaseConnectionLoaderError, ManagedDatabaseConnectionSpec, ManagedDatabasePoolKey,
    PublicUser, RegisterRequest, RejectSqlAuditRequest, ResolvedLlmProviderSettings,
    SqlAuditExecutionResult, SqlAuditRecord, SqlAuditStatus, UpdateAgentConversationRequest,
    UpdateBiPanelCardRequest, UpdateBiPanelRequest, UpdateCurrentUserRequest,
    UpdateLlmProviderSettingsRequest, UpdateManagedDatabaseRequest, UpdatePasswordRequest,
};
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::{
    agent_workbench, auth, bi_panels,
    crypto::PasswordCipher,
    database_backups,
    error::StorageError,
    managed_databases,
    options::StorageOptions,
    settings, sql_audits,
    traits::{CreateSqlAuditRecord, LiquidStore},
};

#[derive(Debug, Clone)]
pub struct Storage {
    pub(crate) pool: PgPool,
    pub(crate) token_ttl_seconds: i64,
    pub(crate) cipher: PasswordCipher,
}

impl Storage {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        Self::connect_with_options(StorageOptions::new(database_url)).await
    }

    pub async fn connect_with_options(options: StorageOptions) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(options.max_connections)
            .connect(&options.database_url)
            .await?;

        Ok(Self {
            pool,
            token_ttl_seconds: options.token_ttl_seconds,
            cipher: PasswordCipher::new(&options.encryption_key),
        })
    }

    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations").run(&self.pool).await
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn decrypt_managed_database_password(
        &self,
        encrypted_password: &str,
    ) -> Result<String, StorageError> {
        self.cipher.decrypt(encrypted_password)
    }
}

#[async_trait]
impl ManagedDatabaseConnectionLoader for Storage {
    async fn load_managed_database_connection(
        &self,
        key: &ManagedDatabasePoolKey,
    ) -> Result<ManagedDatabaseConnectionSpec, ManagedDatabaseConnectionLoaderError> {
        managed_databases::load_managed_database_connection(self, key)
            .await
            .map_err(managed_database_loader_error)
    }
}

#[async_trait]
impl DatabaseBackupMetadataStore for Storage {
    async fn create_database_backup(
        &self,
        owner_user_id: &str,
        source_managed_database_id: &str,
        purpose: Option<String>,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
        database_backups::create_database_backup(
            self,
            owner_user_id,
            source_managed_database_id,
            purpose,
        )
        .await
        .map_err(database_backups::metadata_store_error)
    }

    async fn get_database_backup(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
        database_backups::get_database_backup(self, owner_user_id, id)
            .await
            .map_err(database_backups::metadata_store_error)
    }

    async fn list_database_backups(
        &self,
        owner_user_id: &str,
        source_managed_database_id: Option<&str>,
        status: Option<DatabaseBackupStatus>,
        limit: i64,
    ) -> Result<Vec<DatabaseBackupRecord>, DatabaseBackupMetadataStoreError> {
        database_backups::list_database_backups(
            self,
            owner_user_id,
            source_managed_database_id,
            status,
            limit,
        )
        .await
        .map_err(database_backups::metadata_store_error)
    }

    async fn delete_database_backup(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
        database_backups::delete_database_backup(self, owner_user_id, id)
            .await
            .map_err(database_backups::metadata_store_error)
    }

    async fn create_database_restore(
        &self,
        owner_user_id: &str,
        backup_id: &str,
        target_managed_database_id: &str,
        purpose: String,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
        database_backups::create_database_restore(
            self,
            owner_user_id,
            backup_id,
            target_managed_database_id,
            purpose,
        )
        .await
        .map_err(database_backups::metadata_store_error)
    }

    async fn get_database_restore(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
        database_backups::get_database_restore(self, owner_user_id, id)
            .await
            .map_err(database_backups::metadata_store_error)
    }

    async fn list_database_restores(
        &self,
        owner_user_id: &str,
        backup_id: Option<&str>,
        target_managed_database_id: Option<&str>,
        status: Option<DatabaseBackupStatus>,
        limit: i64,
    ) -> Result<Vec<DatabaseRestoreRecord>, DatabaseBackupMetadataStoreError> {
        database_backups::list_database_restores(
            self,
            owner_user_id,
            backup_id,
            target_managed_database_id,
            status,
            limit,
        )
        .await
        .map_err(database_backups::metadata_store_error)
    }

    async fn claim_next_database_backup(
        &self,
        worker_id: &str,
    ) -> Result<Option<DatabaseBackupRecord>, DatabaseBackupMetadataStoreError> {
        database_backups::claim_next_database_backup(self, worker_id)
            .await
            .map_err(database_backups::metadata_store_error)
    }

    async fn update_database_backup_progress(
        &self,
        id: &str,
        phase: &str,
        progress_percent: i32,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
        database_backups::update_database_backup_progress(self, id, phase, progress_percent)
            .await
            .map_err(database_backups::metadata_store_error)
    }

    async fn complete_database_backup(
        &self,
        id: &str,
        result: CompleteDatabaseBackup,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
        database_backups::complete_database_backup(self, id, result)
            .await
            .map_err(database_backups::metadata_store_error)
    }

    async fn fail_database_backup(
        &self,
        id: &str,
        error: String,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
        database_backups::fail_database_backup(self, id, error)
            .await
            .map_err(database_backups::metadata_store_error)
    }

    async fn claim_next_database_restore(
        &self,
        worker_id: &str,
    ) -> Result<Option<DatabaseRestoreRecord>, DatabaseBackupMetadataStoreError> {
        database_backups::claim_next_database_restore(self, worker_id)
            .await
            .map_err(database_backups::metadata_store_error)
    }

    async fn update_database_restore_progress(
        &self,
        id: &str,
        phase: &str,
        progress_percent: i32,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
        database_backups::update_database_restore_progress(self, id, phase, progress_percent)
            .await
            .map_err(database_backups::metadata_store_error)
    }

    async fn complete_database_restore(
        &self,
        id: &str,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
        database_backups::complete_database_restore(self, id)
            .await
            .map_err(database_backups::metadata_store_error)
    }

    async fn fail_database_restore(
        &self,
        id: &str,
        error: String,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
        database_backups::fail_database_restore(self, id, error)
            .await
            .map_err(database_backups::metadata_store_error)
    }

    async fn fail_stale_database_jobs(
        &self,
        stale_after_seconds: i64,
    ) -> Result<u64, DatabaseBackupMetadataStoreError> {
        database_backups::fail_stale_database_jobs(self, stale_after_seconds)
            .await
            .map_err(database_backups::metadata_store_error)
    }
}

#[async_trait]
impl LiquidStore for Storage {
    async fn register_user(&self, request: RegisterRequest) -> Result<AuthResponse, StorageError> {
        auth::register_user(self, request).await
    }

    async fn login_user(&self, request: LoginRequest) -> Result<AuthResponse, StorageError> {
        auth::login_user(self, request).await
    }

    async fn authenticate_token(&self, token: &str) -> Result<Option<PublicUser>, StorageError> {
        auth::authenticate_token(self, token).await
    }

    async fn update_current_user(
        &self,
        owner_user_id: &str,
        request: UpdateCurrentUserRequest,
    ) -> Result<PublicUser, StorageError> {
        auth::update_current_user(self, owner_user_id, request).await
    }

    async fn update_password(
        &self,
        owner_user_id: &str,
        request: UpdatePasswordRequest,
    ) -> Result<(), StorageError> {
        auth::update_password(self, owner_user_id, request).await
    }

    async fn revoke_token(&self, token: &str) -> Result<(), StorageError> {
        auth::revoke_token(self, token).await
    }

    async fn get_llm_provider_settings(
        &self,
        owner_user_id: &str,
    ) -> Result<Option<LlmProviderSettings>, StorageError> {
        settings::get_llm_provider_settings(self, owner_user_id).await
    }

    async fn upsert_llm_provider_settings(
        &self,
        owner_user_id: &str,
        request: UpdateLlmProviderSettingsRequest,
    ) -> Result<LlmProviderSettings, StorageError> {
        settings::upsert_llm_provider_settings(self, owner_user_id, request).await
    }

    async fn resolve_llm_provider_settings(
        &self,
        owner_user_id: &str,
    ) -> Result<Option<ResolvedLlmProviderSettings>, StorageError> {
        settings::resolve_llm_provider_settings(self, owner_user_id).await
    }

    async fn list_managed_databases(
        &self,
        owner_user_id: &str,
    ) -> Result<Vec<ManagedDatabase>, StorageError> {
        managed_databases::list_managed_databases(self, owner_user_id).await
    }

    async fn get_current_managed_database(
        &self,
        owner_user_id: &str,
    ) -> Result<Option<ManagedDatabase>, StorageError> {
        managed_databases::get_current_managed_database(self, owner_user_id).await
    }

    async fn set_current_managed_database(
        &self,
        owner_user_id: &str,
        managed_database_id: &str,
    ) -> Result<ManagedDatabase, StorageError> {
        managed_databases::set_current_managed_database(self, owner_user_id, managed_database_id)
            .await
    }

    async fn clear_current_managed_database(
        &self,
        owner_user_id: &str,
    ) -> Result<(), StorageError> {
        managed_databases::clear_current_managed_database(self, owner_user_id).await
    }

    async fn create_managed_database(
        &self,
        owner_user_id: &str,
        request: CreateManagedDatabaseRequest,
    ) -> Result<ManagedDatabase, StorageError> {
        managed_databases::create_managed_database(self, owner_user_id, request).await
    }

    async fn update_managed_database(
        &self,
        owner_user_id: &str,
        id: &str,
        request: UpdateManagedDatabaseRequest,
    ) -> Result<ManagedDatabase, StorageError> {
        managed_databases::update_managed_database(self, owner_user_id, id, request).await
    }

    async fn delete_managed_database(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<(), StorageError> {
        managed_databases::delete_managed_database(self, owner_user_id, id).await
    }

    async fn create_sql_audit(
        &self,
        owner_user_id: &str,
        managed_database_id: &str,
        record: CreateSqlAuditRecord,
    ) -> Result<SqlAuditRecord, StorageError> {
        sql_audits::create_sql_audit(self, owner_user_id, managed_database_id, record).await
    }

    async fn list_sql_audits(
        &self,
        owner_user_id: &str,
        managed_database_id: Option<&str>,
        status: Option<SqlAuditStatus>,
        limit: i64,
    ) -> Result<Vec<SqlAuditRecord>, StorageError> {
        sql_audits::list_sql_audits(self, owner_user_id, managed_database_id, status, limit).await
    }

    async fn get_sql_audit(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<SqlAuditRecord, StorageError> {
        sql_audits::get_sql_audit(self, owner_user_id, id).await
    }

    async fn approve_sql_audit(
        &self,
        owner_user_id: &str,
        id: &str,
        request: ApproveSqlAuditRequest,
    ) -> Result<SqlAuditRecord, StorageError> {
        sql_audits::approve_sql_audit(self, owner_user_id, id, request).await
    }

    async fn reject_sql_audit(
        &self,
        owner_user_id: &str,
        id: &str,
        request: RejectSqlAuditRequest,
    ) -> Result<SqlAuditRecord, StorageError> {
        sql_audits::reject_sql_audit(self, owner_user_id, id, request).await
    }

    async fn start_sql_audit_execution(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<SqlAuditRecord, StorageError> {
        sql_audits::start_sql_audit_execution(self, owner_user_id, id).await
    }

    async fn complete_sql_audit_execution(
        &self,
        owner_user_id: &str,
        id: &str,
        result: SqlAuditExecutionResult,
    ) -> Result<SqlAuditRecord, StorageError> {
        sql_audits::complete_sql_audit_execution(self, owner_user_id, id, result).await
    }

    async fn fail_sql_audit_execution(
        &self,
        owner_user_id: &str,
        id: &str,
        error: String,
    ) -> Result<SqlAuditRecord, StorageError> {
        sql_audits::fail_sql_audit_execution(self, owner_user_id, id, error).await
    }

    async fn list_agent_conversations(
        &self,
        owner_user_id: &str,
        limit: i64,
    ) -> Result<Vec<AgentConversation>, StorageError> {
        agent_workbench::list_agent_conversations(self, owner_user_id, limit).await
    }

    async fn create_agent_conversation(
        &self,
        owner_user_id: &str,
        request: CreateAgentConversationRequest,
    ) -> Result<AgentConversation, StorageError> {
        agent_workbench::create_agent_conversation(self, owner_user_id, request).await
    }

    async fn get_agent_conversation(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<AgentConversation, StorageError> {
        agent_workbench::get_agent_conversation(self, owner_user_id, id).await
    }

    async fn update_agent_conversation(
        &self,
        owner_user_id: &str,
        id: &str,
        request: UpdateAgentConversationRequest,
    ) -> Result<AgentConversation, StorageError> {
        agent_workbench::update_agent_conversation(self, owner_user_id, id, request).await
    }

    async fn delete_agent_conversation(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<(), StorageError> {
        agent_workbench::delete_agent_conversation(self, owner_user_id, id).await
    }

    async fn list_agent_messages(
        &self,
        owner_user_id: &str,
        conversation_id: &str,
        limit: i64,
        before_message_id: Option<&str>,
    ) -> Result<Vec<AgentMessage>, StorageError> {
        agent_workbench::list_agent_messages(
            self,
            owner_user_id,
            conversation_id,
            limit,
            before_message_id,
        )
        .await
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
        agent_workbench::append_agent_message(
            self,
            owner_user_id,
            conversation_id,
            turn_id,
            role,
            content,
            metadata,
        )
        .await
    }

    async fn create_agent_turn(
        &self,
        owner_user_id: &str,
        conversation_id: &str,
        request: CreateAgentTurnRequest,
    ) -> Result<AgentTurn, StorageError> {
        agent_workbench::create_agent_turn(self, owner_user_id, conversation_id, request).await
    }

    async fn get_agent_turn(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<AgentTurn, StorageError> {
        agent_workbench::get_agent_turn(self, owner_user_id, id).await
    }

    async fn update_agent_turn_status(
        &self,
        owner_user_id: &str,
        id: &str,
        status: AgentTurnStatus,
        error: Option<String>,
    ) -> Result<AgentTurn, StorageError> {
        agent_workbench::update_agent_turn_status(self, owner_user_id, id, status, error).await
    }

    async fn set_agent_turn_assistant_message(
        &self,
        owner_user_id: &str,
        id: &str,
        assistant_message_id: &str,
    ) -> Result<AgentTurn, StorageError> {
        agent_workbench::set_agent_turn_assistant_message(
            self,
            owner_user_id,
            id,
            assistant_message_id,
        )
        .await
    }

    async fn append_agent_turn_event(
        &self,
        owner_user_id: &str,
        turn_id: &str,
        event_type: AgentEventType,
        payload: Value,
    ) -> Result<AgentEventRecord, StorageError> {
        agent_workbench::append_agent_turn_event(self, owner_user_id, turn_id, event_type, payload)
            .await
    }

    async fn list_agent_turn_events(
        &self,
        owner_user_id: &str,
        turn_id: &str,
        after_seq: i32,
    ) -> Result<Vec<AgentEventRecord>, StorageError> {
        agent_workbench::list_agent_turn_events(self, owner_user_id, turn_id, after_seq).await
    }

    async fn create_agent_action(
        &self,
        owner_user_id: &str,
        turn_id: &str,
        request: CreateAgentActionRequest,
    ) -> Result<AgentAction, StorageError> {
        agent_workbench::create_agent_action(self, owner_user_id, turn_id, request).await
    }

    async fn list_agent_actions(
        &self,
        owner_user_id: &str,
        conversation_id: Option<&str>,
        status: Option<AgentActionStatus>,
    ) -> Result<Vec<AgentAction>, StorageError> {
        agent_workbench::list_agent_actions(self, owner_user_id, conversation_id, status).await
    }

    async fn get_agent_action(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<AgentAction, StorageError> {
        agent_workbench::get_agent_action(self, owner_user_id, id).await
    }

    async fn update_agent_action_status(
        &self,
        owner_user_id: &str,
        id: &str,
        status: AgentActionStatus,
        resource_kind: Option<AgentResourceKind>,
        resource_id: Option<String>,
    ) -> Result<AgentAction, StorageError> {
        agent_workbench::update_agent_action_status(
            self,
            owner_user_id,
            id,
            status,
            resource_kind,
            resource_id,
        )
        .await
    }

    async fn get_or_create_bi_panel(
        &self,
        owner_user_id: &str,
        conversation_id: &str,
    ) -> Result<BiPanel, StorageError> {
        bi_panels::get_or_create_bi_panel(self, owner_user_id, conversation_id).await
    }

    async fn update_bi_panel(
        &self,
        owner_user_id: &str,
        panel_id: &str,
        request: UpdateBiPanelRequest,
    ) -> Result<BiPanel, StorageError> {
        bi_panels::update_bi_panel(self, owner_user_id, panel_id, request).await
    }

    async fn create_bi_panel_card(
        &self,
        owner_user_id: &str,
        panel_id: &str,
        request: CreateBiPanelCardRequest,
    ) -> Result<BiPanelCard, StorageError> {
        bi_panels::create_bi_panel_card(self, owner_user_id, panel_id, request).await
    }

    async fn get_bi_panel_card(
        &self,
        owner_user_id: &str,
        panel_id: &str,
        card_id: &str,
    ) -> Result<BiPanelCard, StorageError> {
        bi_panels::get_bi_panel_card(self, owner_user_id, panel_id, card_id).await
    }

    async fn update_bi_panel_card(
        &self,
        owner_user_id: &str,
        panel_id: &str,
        card_id: &str,
        request: UpdateBiPanelCardRequest,
    ) -> Result<BiPanelCard, StorageError> {
        bi_panels::update_bi_panel_card(self, owner_user_id, panel_id, card_id, request).await
    }

    async fn update_bi_panel_layout(
        &self,
        owner_user_id: &str,
        panel_id: &str,
        layouts: Vec<BiCardLayoutUpdate>,
    ) -> Result<BiPanel, StorageError> {
        bi_panels::update_bi_panel_layout(self, owner_user_id, panel_id, layouts).await
    }

    async fn update_bi_panel_card_result(
        &self,
        owner_user_id: &str,
        panel_id: &str,
        card_id: &str,
        result: BiQueryResult,
    ) -> Result<BiPanelCard, StorageError> {
        bi_panels::update_bi_panel_card_result(self, owner_user_id, panel_id, card_id, result).await
    }

    async fn delete_bi_panel_card(
        &self,
        owner_user_id: &str,
        panel_id: &str,
        card_id: &str,
    ) -> Result<(), StorageError> {
        bi_panels::delete_bi_panel_card(self, owner_user_id, panel_id, card_id).await
    }

    async fn export_bi_panel(
        &self,
        owner_user_id: &str,
        panel_id: &str,
    ) -> Result<BiPanelExport, StorageError> {
        bi_panels::export_bi_panel(self, owner_user_id, panel_id).await
    }

    async fn fail_stale_agent_turns(&self, stale_after_seconds: i64) -> Result<u64, StorageError> {
        agent_workbench::fail_stale_agent_turns(self, stale_after_seconds).await
    }
}

fn managed_database_loader_error(error: StorageError) -> ManagedDatabaseConnectionLoaderError {
    match error {
        StorageError::NotFound => ManagedDatabaseConnectionLoaderError::NotFound,
        StorageError::Conflict(message) => ManagedDatabaseConnectionLoaderError::Backend(message),
        StorageError::Validation(message) => {
            ManagedDatabaseConnectionLoaderError::InvalidConnection(message)
        }
        StorageError::Crypto(message) => ManagedDatabaseConnectionLoaderError::Secret(message),
        StorageError::Database(error) => {
            ManagedDatabaseConnectionLoaderError::Backend(error.to_string())
        }
        other => ManagedDatabaseConnectionLoaderError::Backend(other.to_string()),
    }
}
