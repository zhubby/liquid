use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::{ManagedDatabaseEngine, ManagedDatabaseSslMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DatabaseBackupStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Deleted,
}

impl DatabaseBackupStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Deleted => "deleted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DatabaseBackupFormat {
    PostgresCustom,
}

impl DatabaseBackupFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PostgresCustom => "postgres_custom",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DatabaseBackupTrigger {
    #[default]
    Immediate,
    Cron,
}

impl DatabaseBackupTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Immediate => "immediate",
            Self::Cron => "cron",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DatabaseBackupScheduleStatus {
    Active,
    Paused,
    Deleted,
}

impl DatabaseBackupScheduleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Deleted => "deleted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DatabaseOperationKind {
    Backup,
    Restore,
}

impl DatabaseOperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Backup => "backup",
            Self::Restore => "restore",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DatabaseOperationEventType {
    Queued,
    Succeeded,
    Failed,
}

impl DatabaseOperationEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ManagedDatabaseSnapshot {
    pub id: String,
    pub name: String,
    pub engine: ManagedDatabaseEngine,
    pub host: String,
    pub port: i32,
    pub database: String,
    pub username: String,
    pub ssl_mode: ManagedDatabaseSslMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseBackupStorageMetadata {
    pub kind: DatabaseBackupStorageKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub local_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub bucket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub version_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub size_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub checksum_sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DatabaseBackupStorageKind {
    Local,
    S3,
}

impl DatabaseBackupStorageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::S3 => "s3",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseBackupRecord {
    pub id: String,
    pub owner_user_id: String,
    pub source: ManagedDatabaseSnapshot,
    pub format: DatabaseBackupFormat,
    pub status: DatabaseBackupStatus,
    pub phase: String,
    pub progress_percent: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub schedule_id: Option<String>,
    pub trigger: DatabaseBackupTrigger,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "string")]
    pub scheduled_for: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub created_from_turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub storage: Option<DatabaseBackupStorageMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub postgres_server_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pg_dump_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub worker_id: Option<String>,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "string")]
    pub heartbeat_at: Option<OffsetDateTime>,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "string")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "string")]
    pub completed_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseRestoreRecord {
    pub id: String,
    pub owner_user_id: String,
    pub backup_id: String,
    pub target: ManagedDatabaseSnapshot,
    pub format: DatabaseBackupFormat,
    pub status: DatabaseBackupStatus,
    pub phase: String,
    pub progress_percent: i32,
    #[ts(type = "unknown")]
    pub restore_options: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub created_from_turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub worker_id: Option<String>,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "string")]
    pub heartbeat_at: Option<OffsetDateTime>,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "string")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "string")]
    pub completed_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateDatabaseBackupRequest {
    pub managed_database_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub client_request_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnqueueDatabaseBackup {
    pub managed_database_id: String,
    pub purpose: Option<String>,
    pub schedule_id: Option<String>,
    pub trigger: DatabaseBackupTrigger,
    pub scheduled_for: Option<OffsetDateTime>,
    pub conversation_id: Option<String>,
    pub created_from_turn_id: Option<String>,
}

impl EnqueueDatabaseBackup {
    pub fn immediate(
        managed_database_id: impl Into<String>,
        purpose: Option<String>,
        conversation_id: Option<String>,
        created_from_turn_id: Option<String>,
    ) -> Self {
        Self {
            managed_database_id: managed_database_id.into(),
            purpose,
            schedule_id: None,
            trigger: DatabaseBackupTrigger::Immediate,
            scheduled_for: None,
            conversation_id,
            created_from_turn_id,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateDatabaseRestoreRequest {
    pub backup_id: String,
    pub target_managed_database_id: String,
    pub purpose: String,
    pub confirm_destructive_restore: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub client_request_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnqueueDatabaseRestore {
    pub backup_id: String,
    pub target_managed_database_id: String,
    pub purpose: String,
    pub conversation_id: Option<String>,
    pub created_from_turn_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseBackupScheduleRecord {
    pub id: String,
    pub owner_user_id: String,
    pub source: ManagedDatabaseSnapshot,
    pub cron_expression: String,
    pub timezone: String,
    pub status: DatabaseBackupScheduleStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub keep_last: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub retention_days: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub created_from_turn_id: Option<String>,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "string")]
    pub last_enqueued_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub next_run_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateDatabaseBackupScheduleRequest {
    pub managed_database_id: String,
    pub cron_expression: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub keep_last: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub retention_days: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateDatabaseBackupScheduleRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cron_expression: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status: Option<DatabaseBackupScheduleStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub keep_last: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub retention_days: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseOperationEventRecord {
    pub id: String,
    pub owner_user_id: String,
    pub operation_kind: DatabaseOperationKind,
    pub operation_id: String,
    pub event_type: DatabaseOperationEventType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub turn_id: Option<String>,
    #[ts(type = "unknown")]
    pub payload: serde_json::Value,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "string")]
    pub delivered_at: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub delivered_message_id: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteDatabaseBackup {
    pub storage_kind: DatabaseBackupStorageKind,
    pub local_path: Option<String>,
    pub bucket: Option<String>,
    pub key: Option<String>,
    pub version_id: Option<String>,
    pub etag: Option<String>,
    pub size_bytes: i64,
    pub checksum_sha256: String,
    pub postgres_server_version: Option<String>,
    pub pg_dump_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseBackupMetadataStoreError {
    NotFound,
    Conflict(String),
    Validation(String),
    Backend(String),
}

impl std::fmt::Display for DatabaseBackupMetadataStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(formatter, "database backup record not found"),
            Self::Conflict(message) | Self::Validation(message) | Self::Backend(message) => {
                write!(formatter, "{message}")
            }
        }
    }
}

impl std::error::Error for DatabaseBackupMetadataStoreError {}

#[async_trait]
pub trait DatabaseBackupMetadataStore: Send + Sync {
    async fn create_database_backup(
        &self,
        owner_user_id: &str,
        source_managed_database_id: &str,
        purpose: Option<String>,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError>;

    async fn enqueue_database_backup(
        &self,
        owner_user_id: &str,
        request: EnqueueDatabaseBackup,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
        self.create_database_backup(owner_user_id, &request.managed_database_id, request.purpose)
            .await
    }

    async fn get_database_backup(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError>;

    async fn list_database_backups(
        &self,
        owner_user_id: &str,
        source_managed_database_id: Option<&str>,
        status: Option<DatabaseBackupStatus>,
        limit: i64,
    ) -> Result<Vec<DatabaseBackupRecord>, DatabaseBackupMetadataStoreError>;

    async fn delete_database_backup(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError>;

    async fn create_database_restore(
        &self,
        owner_user_id: &str,
        backup_id: &str,
        target_managed_database_id: &str,
        purpose: String,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError>;

    async fn enqueue_database_restore(
        &self,
        owner_user_id: &str,
        request: EnqueueDatabaseRestore,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
        self.create_database_restore(
            owner_user_id,
            &request.backup_id,
            &request.target_managed_database_id,
            request.purpose,
        )
        .await
    }

    async fn get_database_restore(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError>;

    async fn list_database_restores(
        &self,
        owner_user_id: &str,
        backup_id: Option<&str>,
        target_managed_database_id: Option<&str>,
        status: Option<DatabaseBackupStatus>,
        limit: i64,
    ) -> Result<Vec<DatabaseRestoreRecord>, DatabaseBackupMetadataStoreError>;

    async fn claim_next_database_backup(
        &self,
        worker_id: &str,
    ) -> Result<Option<DatabaseBackupRecord>, DatabaseBackupMetadataStoreError>;

    async fn update_database_backup_progress(
        &self,
        id: &str,
        phase: &str,
        progress_percent: i32,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError>;

    async fn complete_database_backup(
        &self,
        id: &str,
        result: CompleteDatabaseBackup,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError>;

    async fn fail_database_backup(
        &self,
        id: &str,
        error: String,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError>;

    async fn claim_next_database_restore(
        &self,
        worker_id: &str,
    ) -> Result<Option<DatabaseRestoreRecord>, DatabaseBackupMetadataStoreError>;

    async fn update_database_restore_progress(
        &self,
        id: &str,
        phase: &str,
        progress_percent: i32,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError>;

    async fn complete_database_restore(
        &self,
        id: &str,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError>;

    async fn fail_database_restore(
        &self,
        id: &str,
        error: String,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError>;

    async fn fail_stale_database_jobs(
        &self,
        stale_after_seconds: i64,
    ) -> Result<u64, DatabaseBackupMetadataStoreError>;

    async fn create_database_backup_schedule(
        &self,
        owner_user_id: &str,
        request: CreateDatabaseBackupScheduleRequest,
        conversation_id: Option<String>,
        created_from_turn_id: Option<String>,
        next_run_at: OffsetDateTime,
    ) -> Result<DatabaseBackupScheduleRecord, DatabaseBackupMetadataStoreError> {
        let _ = (
            owner_user_id,
            request,
            conversation_id,
            created_from_turn_id,
            next_run_at,
        );
        Err(DatabaseBackupMetadataStoreError::Backend(
            "database backup schedules are not supported by this store".to_owned(),
        ))
    }

    async fn get_database_backup_schedule(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<DatabaseBackupScheduleRecord, DatabaseBackupMetadataStoreError> {
        let _ = (owner_user_id, id);
        Err(DatabaseBackupMetadataStoreError::NotFound)
    }

    async fn list_database_backup_schedules(
        &self,
        owner_user_id: &str,
        managed_database_id: Option<&str>,
        status: Option<DatabaseBackupScheduleStatus>,
        limit: i64,
    ) -> Result<Vec<DatabaseBackupScheduleRecord>, DatabaseBackupMetadataStoreError> {
        let _ = (owner_user_id, managed_database_id, status, limit);
        Ok(Vec::new())
    }

    async fn update_database_backup_schedule(
        &self,
        owner_user_id: &str,
        id: &str,
        request: UpdateDatabaseBackupScheduleRequest,
        next_run_at: Option<OffsetDateTime>,
    ) -> Result<DatabaseBackupScheduleRecord, DatabaseBackupMetadataStoreError> {
        let _ = (owner_user_id, id, request, next_run_at);
        Err(DatabaseBackupMetadataStoreError::NotFound)
    }

    async fn delete_database_backup_schedule(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<DatabaseBackupScheduleRecord, DatabaseBackupMetadataStoreError> {
        let _ = (owner_user_id, id);
        Err(DatabaseBackupMetadataStoreError::NotFound)
    }

    async fn claim_due_database_backup_schedule(
        &self,
        scheduler_id: &str,
        now: OffsetDateTime,
    ) -> Result<Option<DatabaseBackupScheduleRecord>, DatabaseBackupMetadataStoreError> {
        let _ = (scheduler_id, now);
        Ok(None)
    }

    async fn complete_database_backup_schedule_enqueue(
        &self,
        owner_user_id: &str,
        id: &str,
        scheduled_for: OffsetDateTime,
        next_run_at: OffsetDateTime,
    ) -> Result<DatabaseBackupScheduleRecord, DatabaseBackupMetadataStoreError> {
        let _ = (owner_user_id, id, scheduled_for, next_run_at);
        Err(DatabaseBackupMetadataStoreError::NotFound)
    }

    async fn append_database_operation_event(
        &self,
        operation_kind: DatabaseOperationKind,
        operation_id: &str,
        event_type: DatabaseOperationEventType,
        payload: serde_json::Value,
    ) -> Result<DatabaseOperationEventRecord, DatabaseBackupMetadataStoreError> {
        let _ = (operation_kind, operation_id, event_type, payload);
        Err(DatabaseBackupMetadataStoreError::Backend(
            "database operation events are not supported by this store".to_owned(),
        ))
    }

    async fn claim_next_database_operation_event(
        &self,
    ) -> Result<Option<DatabaseOperationEventRecord>, DatabaseBackupMetadataStoreError> {
        Ok(None)
    }

    async fn mark_database_operation_event_delivered(
        &self,
        event_id: &str,
        delivered_message_id: &str,
    ) -> Result<DatabaseOperationEventRecord, DatabaseBackupMetadataStoreError> {
        let _ = (event_id, delivered_message_id);
        Err(DatabaseBackupMetadataStoreError::NotFound)
    }
}
