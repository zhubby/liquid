use async_trait::async_trait;
use liquid_core::{
    ApproveSqlAuditRequest, AuthResponse, CreateManagedDatabaseRequest, CreateSqlAuditRequest,
    LoginRequest, ManagedDatabase, PublicUser, RegisterRequest, RejectSqlAuditRequest,
    SqlAuditExecutionResult, SqlAuditRecord, SqlAuditReport, SqlAuditStatus, SqlStatementKind,
    UpdateManagedDatabaseRequest,
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
}
