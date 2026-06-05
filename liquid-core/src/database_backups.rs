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
pub struct DatabaseBackupObjectMetadata {
    pub bucket: String,
    pub key: String,
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
    pub object: Option<DatabaseBackupObjectMetadata>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteDatabaseBackup {
    pub bucket: String,
    pub key: String,
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
}
