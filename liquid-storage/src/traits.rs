use async_trait::async_trait;
use liquid_core::{
    AgentAction, AgentActionStatus, AgentConversation, AgentEventRecord, AgentEventType,
    AgentMessage, AgentMessageRole, AgentResourceKind, AgentTurn, AgentTurnStatus,
    ApproveSqlAuditRequest, AuthResponse, CreateAgentActionRequest, CreateAgentConversationRequest,
    CreateAgentTurnRequest, CreateManagedDatabaseRequest, CreateSqlAuditRequest, LoginRequest,
    ManagedDatabase, PublicUser, RegisterRequest, RejectSqlAuditRequest, SqlAuditExecutionResult,
    SqlAuditRecord, SqlAuditReport, SqlAuditStatus, SqlStatementKind,
    UpdateAgentConversationRequest, UpdateManagedDatabaseRequest,
};
use serde_json::Value;

use crate::error::StorageError;

pub struct CreateSqlAuditRecord {
    pub request: CreateSqlAuditRequest,
    pub report: SqlAuditReport,
    pub deterministic_analysis: Value,
    pub statement_kind: Option<SqlStatementKind>,
    pub status: SqlAuditStatus,
    pub risk_score: u8,
}

#[async_trait]
pub trait LiquidStore: Send + Sync {
    async fn register_user(&self, request: RegisterRequest) -> Result<AuthResponse, StorageError>;
    async fn login_user(&self, request: LoginRequest) -> Result<AuthResponse, StorageError>;
    async fn authenticate_token(&self, token: &str) -> Result<Option<PublicUser>, StorageError>;
    async fn revoke_token(&self, token: &str) -> Result<(), StorageError>;
    async fn list_managed_databases(
        &self,
        owner_user_id: &str,
    ) -> Result<Vec<ManagedDatabase>, StorageError>;
    async fn get_current_managed_database(
        &self,
        owner_user_id: &str,
    ) -> Result<Option<ManagedDatabase>, StorageError>;
    async fn set_current_managed_database(
        &self,
        owner_user_id: &str,
        managed_database_id: &str,
    ) -> Result<ManagedDatabase, StorageError>;
    async fn clear_current_managed_database(&self, owner_user_id: &str)
    -> Result<(), StorageError>;
    async fn create_managed_database(
        &self,
        owner_user_id: &str,
        request: CreateManagedDatabaseRequest,
    ) -> Result<ManagedDatabase, StorageError>;
    async fn update_managed_database(
        &self,
        owner_user_id: &str,
        id: &str,
        request: UpdateManagedDatabaseRequest,
    ) -> Result<ManagedDatabase, StorageError>;
    async fn delete_managed_database(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<(), StorageError>;
    async fn create_sql_audit(
        &self,
        owner_user_id: &str,
        managed_database_id: &str,
        record: CreateSqlAuditRecord,
    ) -> Result<SqlAuditRecord, StorageError>;
    async fn list_sql_audits(
        &self,
        owner_user_id: &str,
        managed_database_id: Option<&str>,
        status: Option<SqlAuditStatus>,
        limit: i64,
    ) -> Result<Vec<SqlAuditRecord>, StorageError>;
    async fn get_sql_audit(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<SqlAuditRecord, StorageError>;
    async fn approve_sql_audit(
        &self,
        owner_user_id: &str,
        id: &str,
        request: ApproveSqlAuditRequest,
    ) -> Result<SqlAuditRecord, StorageError>;
    async fn reject_sql_audit(
        &self,
        owner_user_id: &str,
        id: &str,
        request: RejectSqlAuditRequest,
    ) -> Result<SqlAuditRecord, StorageError>;
    async fn start_sql_audit_execution(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<SqlAuditRecord, StorageError>;
    async fn complete_sql_audit_execution(
        &self,
        owner_user_id: &str,
        id: &str,
        result: SqlAuditExecutionResult,
    ) -> Result<SqlAuditRecord, StorageError>;
    async fn fail_sql_audit_execution(
        &self,
        owner_user_id: &str,
        id: &str,
        error: String,
    ) -> Result<SqlAuditRecord, StorageError>;
    async fn list_agent_conversations(
        &self,
        owner_user_id: &str,
        limit: i64,
    ) -> Result<Vec<AgentConversation>, StorageError>;
    async fn create_agent_conversation(
        &self,
        owner_user_id: &str,
        request: CreateAgentConversationRequest,
    ) -> Result<AgentConversation, StorageError>;
    async fn get_agent_conversation(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<AgentConversation, StorageError>;
    async fn update_agent_conversation(
        &self,
        owner_user_id: &str,
        id: &str,
        request: UpdateAgentConversationRequest,
    ) -> Result<AgentConversation, StorageError>;
    async fn list_agent_messages(
        &self,
        owner_user_id: &str,
        conversation_id: &str,
        limit: i64,
        before_message_id: Option<&str>,
    ) -> Result<Vec<AgentMessage>, StorageError>;
    async fn append_agent_message(
        &self,
        owner_user_id: &str,
        conversation_id: &str,
        turn_id: Option<&str>,
        role: AgentMessageRole,
        content: &str,
        metadata: Option<Value>,
    ) -> Result<AgentMessage, StorageError>;
    async fn create_agent_turn(
        &self,
        owner_user_id: &str,
        conversation_id: &str,
        request: CreateAgentTurnRequest,
    ) -> Result<AgentTurn, StorageError>;
    async fn get_agent_turn(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<AgentTurn, StorageError>;
    async fn update_agent_turn_status(
        &self,
        owner_user_id: &str,
        id: &str,
        status: AgentTurnStatus,
        error: Option<String>,
    ) -> Result<AgentTurn, StorageError>;
    async fn set_agent_turn_assistant_message(
        &self,
        owner_user_id: &str,
        id: &str,
        assistant_message_id: &str,
    ) -> Result<AgentTurn, StorageError>;
    async fn append_agent_turn_event(
        &self,
        owner_user_id: &str,
        turn_id: &str,
        event_type: AgentEventType,
        payload: Value,
    ) -> Result<AgentEventRecord, StorageError>;
    async fn list_agent_turn_events(
        &self,
        owner_user_id: &str,
        turn_id: &str,
        after_seq: i32,
    ) -> Result<Vec<AgentEventRecord>, StorageError>;
    async fn create_agent_action(
        &self,
        owner_user_id: &str,
        turn_id: &str,
        request: CreateAgentActionRequest,
    ) -> Result<AgentAction, StorageError>;
    async fn list_agent_actions(
        &self,
        owner_user_id: &str,
        conversation_id: Option<&str>,
        status: Option<AgentActionStatus>,
    ) -> Result<Vec<AgentAction>, StorageError>;
    async fn get_agent_action(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<AgentAction, StorageError>;
    async fn update_agent_action_status(
        &self,
        owner_user_id: &str,
        id: &str,
        status: AgentActionStatus,
        resource_kind: Option<AgentResourceKind>,
        resource_id: Option<String>,
    ) -> Result<AgentAction, StorageError>;
    async fn fail_stale_agent_turns(&self, stale_after_seconds: i64) -> Result<u64, StorageError>;
}
