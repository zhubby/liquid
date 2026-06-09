use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use liquid_core::{
    CompleteDatabaseBackup, DatabaseBackupMetadataStore, DatabaseBackupRecord,
    DatabaseBackupStatus, DatabaseBackupStorageKind, DatabaseBackupStorageMetadata,
    DatabaseRestoreRecord, ManagedDatabaseConnectionLoader, ManagedDatabaseEngine,
    ManagedDatabasePoolKey,
};
use sha2::{Digest, Sha256};
use tokio::{process::Command, task::JoinHandle};

use super::object_store::BackupObjectStore;

const DEFAULT_STALE_AFTER_SECONDS: i64 = 15 * 60;
const DEFAULT_IDLE_POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseBackupWorkerConfig {
    pub worker_id: String,
    pub work_dir: PathBuf,
    pub object_key_prefix: String,
    pub concurrency: usize,
    pub stale_after_seconds: i64,
    pub idle_poll_interval: Duration,
}

impl DatabaseBackupWorkerConfig {
    pub fn new(worker_id: impl Into<String>, work_dir: impl Into<PathBuf>) -> Self {
        Self {
            worker_id: worker_id.into(),
            work_dir: work_dir.into(),
            object_key_prefix: "liquid/database-backups".to_owned(),
            concurrency: 1,
            stale_after_seconds: DEFAULT_STALE_AFTER_SECONDS,
            idle_poll_interval: DEFAULT_IDLE_POLL_INTERVAL,
        }
    }

    pub fn with_object_key_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.object_key_prefix = prefix.into();
        self
    }

    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseDumpResult {
    pub postgres_server_version: Option<String>,
    pub pg_dump_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseRestoreResult {
    pub pg_restore_version: Option<String>,
}

#[async_trait]
pub trait DatabaseProcessExecutor: Send + Sync {
    async fn dump_postgres(
        &self,
        spec: &liquid_core::ManagedDatabaseConnectionSpec,
        output_path: &Path,
    ) -> Result<DatabaseDumpResult>;

    async fn restore_postgres(
        &self,
        spec: &liquid_core::ManagedDatabaseConnectionSpec,
        input_path: &Path,
    ) -> Result<DatabaseRestoreResult>;
}

#[derive(Debug, Default)]
pub struct DefaultDatabaseProcessExecutor;

#[async_trait]
impl DatabaseProcessExecutor for DefaultDatabaseProcessExecutor {
    async fn dump_postgres(
        &self,
        spec: &liquid_core::ManagedDatabaseConnectionSpec,
        output_path: &Path,
    ) -> Result<DatabaseDumpResult> {
        let pg_dump_version = command_version("pg_dump").await.ok();
        let mut command = postgres_command("pg_dump", spec);
        command
            .arg("--format=custom")
            .arg("--no-owner")
            .arg("--no-acl")
            .arg("--file")
            .arg(output_path);
        run_command(command, "pg_dump", &spec.password).await?;

        Ok(DatabaseDumpResult {
            postgres_server_version: None,
            pg_dump_version,
        })
    }

    async fn restore_postgres(
        &self,
        spec: &liquid_core::ManagedDatabaseConnectionSpec,
        input_path: &Path,
    ) -> Result<DatabaseRestoreResult> {
        let pg_restore_version = command_version("pg_restore").await.ok();
        let mut command = postgres_command("pg_restore", spec);
        command
            .arg("--clean")
            .arg("--if-exists")
            .arg("--single-transaction")
            .arg("--no-owner")
            .arg("--no-acl")
            .arg("--exit-on-error")
            .arg("--dbname")
            .arg(&spec.database)
            .arg(input_path);
        run_command(command, "pg_restore", &spec.password).await?;

        Ok(DatabaseRestoreResult { pg_restore_version })
    }
}

#[derive(Clone)]
pub struct DatabaseOperationWorker {
    metadata_store: Arc<dyn DatabaseBackupMetadataStore>,
    connection_loader: Arc<dyn ManagedDatabaseConnectionLoader>,
    object_store: Option<Arc<dyn BackupObjectStore>>,
    process_executor: Arc<dyn DatabaseProcessExecutor>,
    config: DatabaseBackupWorkerConfig,
}

impl DatabaseOperationWorker {
    pub fn new(
        metadata_store: Arc<dyn DatabaseBackupMetadataStore>,
        connection_loader: Arc<dyn ManagedDatabaseConnectionLoader>,
        object_store: Option<Arc<dyn BackupObjectStore>>,
        process_executor: Arc<dyn DatabaseProcessExecutor>,
        config: DatabaseBackupWorkerConfig,
    ) -> Self {
        Self {
            metadata_store,
            connection_loader,
            object_store,
            process_executor,
            config,
        }
    }

    pub fn spawn(self) -> Vec<JoinHandle<()>> {
        let mut handles = Vec::new();
        for index in 0..self.config.concurrency.max(1) {
            let mut worker = self.clone();
            worker.config.worker_id = format!("{}-{index}", worker.config.worker_id);
            handles.push(tokio::spawn(async move {
                worker.run_forever().await;
            }));
        }

        handles
    }

    pub async fn run_forever(self) {
        if let Err(error) = self
            .metadata_store
            .fail_stale_database_jobs(self.config.stale_after_seconds)
            .await
        {
            tracing::warn!(error = %error, "failed to mark stale database backup jobs");
        }

        loop {
            match self.run_once().await {
                Ok(true) => {}
                Ok(false) => tokio::time::sleep(self.config.idle_poll_interval).await,
                Err(error) => {
                    tracing::error!(error = %error, "database operation worker iteration failed");
                    tokio::time::sleep(self.config.idle_poll_interval).await;
                }
            }
        }
    }

    pub async fn run_once(&self) -> Result<bool> {
        if let Some(backup) = self
            .metadata_store
            .claim_next_database_backup(&self.config.worker_id)
            .await?
        {
            self.process_backup(backup).await;
            return Ok(true);
        }

        if let Some(restore) = self
            .metadata_store
            .claim_next_database_restore(&self.config.worker_id)
            .await?
        {
            self.process_restore(restore).await;
            return Ok(true);
        }

        Ok(false)
    }

    async fn process_backup(&self, backup: DatabaseBackupRecord) {
        if let Err(error) = self.run_backup(&backup).await {
            let error = truncate_error(&error.to_string());
            tracing::error!(backup_id = %backup.id, error = %error, "database backup failed");
            let _ = self
                .metadata_store
                .fail_database_backup(&backup.id, error)
                .await;
        }
    }

    async fn run_backup(&self, backup: &DatabaseBackupRecord) -> Result<()> {
        ensure_postgres(backup.source.engine)?;
        ensure_status(backup.status, DatabaseBackupStatus::Running, "backup")?;
        tokio::fs::create_dir_all(&self.config.work_dir)
            .await
            .with_context(|| {
                format!(
                    "failed to create database backup work dir: {}",
                    self.config.work_dir.display()
                )
            })?;

        let file_path = self.local_backup_path(backup);
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await.with_context(|| {
                format!(
                    "failed to create backup file directory: {}",
                    parent.display()
                )
            })?;
        }
        self.metadata_store
            .update_database_backup_progress(&backup.id, "dumping", 10)
            .await?;
        let spec = self
            .connection_loader
            .load_managed_database_connection(&ManagedDatabasePoolKey::new(
                &backup.owner_user_id,
                &backup.source.id,
            ))
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        let dump = self
            .process_executor
            .dump_postgres(&spec, &file_path)
            .await
            .map_err(|error| anyhow!(redact(&error.to_string(), &spec.password)))?;
        let file_meta = tokio::fs::metadata(&file_path)
            .await
            .with_context(|| format!("failed to stat backup file: {}", file_path.display()))?;
        let size_bytes = i64::try_from(file_meta.len()).unwrap_or(i64::MAX);
        let checksum_sha256 = sha256_file(&file_path).await?;

        self.metadata_store
            .update_database_backup_progress(&backup.id, "storing", 70)
            .await?;
        let complete = if let Some(object_store) = self.object_store.as_deref() {
            let object_key = self.object_key(backup);
            let object = object_store.put_object(&object_key, &file_path).await?;
            CompleteDatabaseBackup {
                storage_kind: DatabaseBackupStorageKind::S3,
                local_path: None,
                bucket: Some(object.bucket),
                key: Some(object.key),
                version_id: object.version_id,
                etag: object.etag,
                size_bytes,
                checksum_sha256,
                postgres_server_version: dump.postgres_server_version,
                pg_dump_version: dump.pg_dump_version,
            }
        } else {
            CompleteDatabaseBackup {
                storage_kind: DatabaseBackupStorageKind::Local,
                local_path: Some(file_path.display().to_string()),
                bucket: None,
                key: None,
                version_id: None,
                etag: None,
                size_bytes,
                checksum_sha256,
                postgres_server_version: dump.postgres_server_version,
                pg_dump_version: dump.pg_dump_version,
            }
        };
        self.metadata_store
            .complete_database_backup(&backup.id, complete)
            .await?;

        Ok(())
    }

    async fn process_restore(&self, restore: DatabaseRestoreRecord) {
        if let Err(error) = self.run_restore(&restore).await {
            let error = truncate_error(&error.to_string());
            tracing::error!(restore_id = %restore.id, error = %error, "database restore failed");
            let _ = self
                .metadata_store
                .fail_database_restore(&restore.id, error)
                .await;
        }
    }

    async fn run_restore(&self, restore: &DatabaseRestoreRecord) -> Result<()> {
        ensure_postgres(restore.target.engine)?;
        ensure_status(restore.status, DatabaseBackupStatus::Running, "restore")?;
        tokio::fs::create_dir_all(&self.config.work_dir)
            .await
            .with_context(|| {
                format!(
                    "failed to create database restore work dir: {}",
                    self.config.work_dir.display()
                )
            })?;

        let backup = self
            .metadata_store
            .get_database_backup(&restore.owner_user_id, &restore.backup_id)
            .await?;
        ensure_status(backup.status, DatabaseBackupStatus::Succeeded, "backup")?;
        let storage = backup
            .storage
            .ok_or_else(|| anyhow!("backup metadata does not include storage metadata"))?;

        let file_path = self.temp_path("restore", &restore.id);
        self.metadata_store
            .update_database_restore_progress(&restore.id, "loading", 10)
            .await?;
        self.load_restore_file(&storage, &file_path).await?;

        self.metadata_store
            .update_database_restore_progress(&restore.id, "verifying", 30)
            .await?;
        let file_meta = tokio::fs::metadata(&file_path)
            .await
            .with_context(|| format!("failed to stat restore file: {}", file_path.display()))?;
        let size_bytes = i64::try_from(file_meta.len()).unwrap_or(i64::MAX);
        if let Some(expected_size) = storage.size_bytes
            && size_bytes != expected_size
        {
            bail!("downloaded backup size mismatch: expected {expected_size}, got {size_bytes}");
        }
        if let Some(expected_checksum) = storage.checksum_sha256.as_deref() {
            let actual_checksum = sha256_file(&file_path).await?;
            if actual_checksum != expected_checksum {
                bail!(
                    "downloaded backup checksum mismatch: expected {expected_checksum}, got {actual_checksum}"
                );
            }
        }

        self.metadata_store
            .update_database_restore_progress(&restore.id, "restoring", 60)
            .await?;
        let spec = self
            .connection_loader
            .load_managed_database_connection(&ManagedDatabasePoolKey::new(
                &restore.owner_user_id,
                &restore.target.id,
            ))
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        self.process_executor
            .restore_postgres(&spec, &file_path)
            .await
            .map_err(|error| anyhow!(redact(&error.to_string(), &spec.password)))?;
        self.metadata_store
            .complete_database_restore(&restore.id)
            .await?;
        let _ = tokio::fs::remove_file(file_path).await;

        Ok(())
    }

    fn object_key(&self, backup: &DatabaseBackupRecord) -> String {
        let prefix = self.config.object_key_prefix.trim_matches('/');
        if prefix.is_empty() {
            return self.relative_backup_path(backup);
        }

        format!("{}/{}", prefix, self.relative_backup_path(backup))
    }

    fn relative_backup_path(&self, backup: &DatabaseBackupRecord) -> String {
        format!(
            "{}/{}/{}.dump",
            backup.owner_user_id, backup.source.id, backup.id
        )
    }

    fn local_backup_path(&self, backup: &DatabaseBackupRecord) -> PathBuf {
        self.config.work_dir.join(self.relative_backup_path(backup))
    }

    fn temp_path(&self, kind: &str, id: &str) -> PathBuf {
        let timestamp = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
        self.config
            .work_dir
            .join(format!("{kind}-{id}-{timestamp}.dump"))
    }

    async fn load_restore_file(
        &self,
        storage: &DatabaseBackupStorageMetadata,
        file_path: &Path,
    ) -> Result<()> {
        match storage.kind {
            DatabaseBackupStorageKind::Local => {
                let local_path = storage
                    .local_path
                    .as_deref()
                    .ok_or_else(|| anyhow!("local backup metadata does not include a path"))?;
                tokio::fs::copy(local_path, file_path)
                    .await
                    .with_context(|| format!("failed to copy local backup file {local_path}"))?;
                Ok(())
            }
            DatabaseBackupStorageKind::S3 => {
                let object_store = self
                    .object_store
                    .as_deref()
                    .ok_or_else(|| anyhow!("S3 backup storage is not configured"))?;
                let key = storage
                    .key
                    .as_deref()
                    .ok_or_else(|| anyhow!("S3 backup metadata does not include an object key"))?;
                object_store
                    .get_object(key, file_path)
                    .await
                    .with_context(|| format!("failed to download backup object {key}"))?;
                Ok(())
            }
        }
    }
}

fn postgres_command(binary: &str, spec: &liquid_core::ManagedDatabaseConnectionSpec) -> Command {
    let mut command = Command::new(binary);
    command
        .env("PGHOST", &spec.host)
        .env("PGPORT", spec.port.to_string())
        .env("PGDATABASE", &spec.database)
        .env("PGUSER", &spec.username)
        .env("PGPASSWORD", &spec.password)
        .env("PGSSLMODE", spec.ssl_mode.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

async fn run_command(mut command: Command, action: &str, secret: &str) -> Result<()> {
    let output = command.output().await?;
    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = redact(&format!("{action} failed: {stdout}\n{stderr}"), secret);
    bail!("{}", truncate_error(&message))
}

async fn command_version(binary: &str) -> Result<String> {
    let output = Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .await?;
    if !output.status.success() {
        bail!("{binary} --version failed");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

async fn sha256_file(path: &Path) -> Result<String> {
    let bytes = tokio::fs::read(path)
        .await
        .with_context(|| format!("failed to read file for checksum: {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(hex::encode(hasher.finalize()))
}

fn ensure_postgres(engine: ManagedDatabaseEngine) -> Result<()> {
    match engine {
        ManagedDatabaseEngine::Postgres => Ok(()),
    }
}

fn ensure_status(
    actual: DatabaseBackupStatus,
    expected: DatabaseBackupStatus,
    record_type: &str,
) -> Result<()> {
    if actual == expected {
        return Ok(());
    }

    bail!("{record_type} must be {expected:?}; got {actual:?}")
}

fn redact(message: &str, secret: &str) -> String {
    if secret.is_empty() {
        return message.to_owned();
    }

    message.replace(secret, "[redacted]")
}

fn truncate_error(message: &str) -> String {
    const MAX_ERROR_BYTES: usize = 2_000;
    if message.len() <= MAX_ERROR_BYTES {
        return message.to_owned();
    }

    format!("{}...", &message[..MAX_ERROR_BYTES])
}

#[cfg(test)]
mod tests {
    use super::*;
    use liquid_core::ManagedDatabaseSslMode;

    #[test]
    fn worker_object_key_includes_prefix_owner_database_and_backup() {
        let worker = DatabaseOperationWorker::new(
            Arc::new(NullMetadataStore),
            Arc::new(NullConnectionLoader),
            None,
            Arc::new(NullExecutor),
            DatabaseBackupWorkerConfig::new("worker", "/tmp")
                .with_object_key_prefix("liquid/backups/"),
        );
        let backup = test_backup();

        assert_eq!(
            worker.object_key(&backup),
            "liquid/backups/user-1/db-1/backup-1.dump"
        );
    }

    #[test]
    fn local_backup_path_uses_work_dir_owner_database_and_backup() {
        let worker = DatabaseOperationWorker::new(
            Arc::new(NullMetadataStore),
            Arc::new(NullConnectionLoader),
            None,
            Arc::new(NullExecutor),
            DatabaseBackupWorkerConfig::new("worker", "/tmp/liquid-backups"),
        );
        let backup = test_backup();

        assert_eq!(
            worker.local_backup_path(&backup),
            PathBuf::from("/tmp/liquid-backups/user-1/db-1/backup-1.dump")
        );
    }

    #[tokio::test]
    async fn load_restore_file_copies_local_backup_to_temp_path() {
        let root = std::env::temp_dir().join(format!(
            "liquid-local-restore-{}",
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let source = root.join("source.dump");
        let target = root.join("restore.dump");
        tokio::fs::write(&source, b"dump").await.unwrap();
        let worker = DatabaseOperationWorker::new(
            Arc::new(NullMetadataStore),
            Arc::new(NullConnectionLoader),
            None,
            Arc::new(NullExecutor),
            DatabaseBackupWorkerConfig::new("worker", &root),
        );

        worker
            .load_restore_file(
                &DatabaseBackupStorageMetadata {
                    kind: DatabaseBackupStorageKind::Local,
                    local_path: Some(source.display().to_string()),
                    bucket: None,
                    key: None,
                    version_id: None,
                    etag: None,
                    size_bytes: Some(4),
                    checksum_sha256: None,
                },
                &target,
            )
            .await
            .unwrap();

        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"dump");
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    fn test_backup() -> DatabaseBackupRecord {
        DatabaseBackupRecord {
            id: "backup-1".to_owned(),
            owner_user_id: "user-1".to_owned(),
            source: liquid_core::ManagedDatabaseSnapshot {
                id: "db-1".to_owned(),
                name: "Warehouse".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "warehouse".to_owned(),
                username: "postgres".to_owned(),
                ssl_mode: ManagedDatabaseSslMode::Prefer,
            },
            format: liquid_core::DatabaseBackupFormat::PostgresCustom,
            status: DatabaseBackupStatus::Running,
            phase: "running".to_owned(),
            progress_percent: 1,
            storage: None,
            postgres_server_version: None,
            pg_dump_version: None,
            error: None,
            purpose: None,
            worker_id: None,
            heartbeat_at: None,
            started_at: None,
            completed_at: None,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
        }
    }

    struct NullMetadataStore;

    #[async_trait]
    impl DatabaseBackupMetadataStore for NullMetadataStore {
        async fn create_database_backup(
            &self,
            _owner_user_id: &str,
            _source_managed_database_id: &str,
            _purpose: Option<String>,
        ) -> Result<DatabaseBackupRecord, liquid_core::DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn get_database_backup(
            &self,
            _owner_user_id: &str,
            _id: &str,
        ) -> Result<DatabaseBackupRecord, liquid_core::DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn list_database_backups(
            &self,
            _owner_user_id: &str,
            _source_managed_database_id: Option<&str>,
            _status: Option<DatabaseBackupStatus>,
            _limit: i64,
        ) -> Result<Vec<DatabaseBackupRecord>, liquid_core::DatabaseBackupMetadataStoreError>
        {
            unreachable!()
        }

        async fn delete_database_backup(
            &self,
            _owner_user_id: &str,
            _id: &str,
        ) -> Result<DatabaseBackupRecord, liquid_core::DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn create_database_restore(
            &self,
            _owner_user_id: &str,
            _backup_id: &str,
            _target_managed_database_id: &str,
            _purpose: String,
        ) -> Result<DatabaseRestoreRecord, liquid_core::DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn get_database_restore(
            &self,
            _owner_user_id: &str,
            _id: &str,
        ) -> Result<DatabaseRestoreRecord, liquid_core::DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn list_database_restores(
            &self,
            _owner_user_id: &str,
            _backup_id: Option<&str>,
            _target_managed_database_id: Option<&str>,
            _status: Option<DatabaseBackupStatus>,
            _limit: i64,
        ) -> Result<Vec<DatabaseRestoreRecord>, liquid_core::DatabaseBackupMetadataStoreError>
        {
            unreachable!()
        }

        async fn claim_next_database_backup(
            &self,
            _worker_id: &str,
        ) -> Result<Option<DatabaseBackupRecord>, liquid_core::DatabaseBackupMetadataStoreError>
        {
            unreachable!()
        }

        async fn update_database_backup_progress(
            &self,
            _id: &str,
            _phase: &str,
            _progress_percent: i32,
        ) -> Result<DatabaseBackupRecord, liquid_core::DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn complete_database_backup(
            &self,
            _id: &str,
            _result: CompleteDatabaseBackup,
        ) -> Result<DatabaseBackupRecord, liquid_core::DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn fail_database_backup(
            &self,
            _id: &str,
            _error: String,
        ) -> Result<DatabaseBackupRecord, liquid_core::DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn claim_next_database_restore(
            &self,
            _worker_id: &str,
        ) -> Result<Option<DatabaseRestoreRecord>, liquid_core::DatabaseBackupMetadataStoreError>
        {
            unreachable!()
        }

        async fn update_database_restore_progress(
            &self,
            _id: &str,
            _phase: &str,
            _progress_percent: i32,
        ) -> Result<DatabaseRestoreRecord, liquid_core::DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn complete_database_restore(
            &self,
            _id: &str,
        ) -> Result<DatabaseRestoreRecord, liquid_core::DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn fail_database_restore(
            &self,
            _id: &str,
            _error: String,
        ) -> Result<DatabaseRestoreRecord, liquid_core::DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn fail_stale_database_jobs(
            &self,
            _stale_after_seconds: i64,
        ) -> Result<u64, liquid_core::DatabaseBackupMetadataStoreError> {
            unreachable!()
        }
    }

    struct NullConnectionLoader;

    #[async_trait]
    impl ManagedDatabaseConnectionLoader for NullConnectionLoader {
        async fn load_managed_database_connection(
            &self,
            _key: &ManagedDatabasePoolKey,
        ) -> Result<
            liquid_core::ManagedDatabaseConnectionSpec,
            liquid_core::ManagedDatabaseConnectionLoaderError,
        > {
            unreachable!()
        }
    }

    struct NullExecutor;

    #[async_trait]
    impl DatabaseProcessExecutor for NullExecutor {
        async fn dump_postgres(
            &self,
            _spec: &liquid_core::ManagedDatabaseConnectionSpec,
            _output_path: &Path,
        ) -> Result<DatabaseDumpResult> {
            unreachable!()
        }

        async fn restore_postgres(
            &self,
            _spec: &liquid_core::ManagedDatabaseConnectionSpec,
            _input_path: &Path,
        ) -> Result<DatabaseRestoreResult> {
            unreachable!()
        }
    }
}
