use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use liquid_core::{
    AppendDatabaseOperationDiagnostic, CompleteDatabaseBackup, DatabaseBackupMetadataStore,
    DatabaseBackupRecord, DatabaseBackupStatus, DatabaseBackupStorageKind,
    DatabaseBackupStorageMetadata, DatabaseOperationEventType, DatabaseOperationKind,
    DatabaseRestoreRecord, ManagedDatabaseConnectionLoader, ManagedDatabaseEngine,
    ManagedDatabasePoolKey,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::{process::Command, task::JoinHandle};

use super::object_store::BackupObjectStore;

const DEFAULT_STALE_AFTER_SECONDS: i64 = 15 * 60;
const DEFAULT_IDLE_POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_ERROR_BYTES: usize = 2_000;
const MAX_DIAGNOSTIC_OUTPUT_BYTES: usize = 64 * 1024;

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
            if let Ok(failed) = self
                .metadata_store
                .fail_database_backup(&backup.id, error)
                .await
            {
                self.append_operation_event(
                    DatabaseOperationKind::Backup,
                    &failed.id,
                    DatabaseOperationEventType::Failed,
                    json!({ "backup": failed }),
                )
                .await;
            }
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
        let dump = match self.process_executor.dump_postgres(&spec, &file_path).await {
            Ok(dump) => dump,
            Err(error) => {
                let diagnostic = process_failure_diagnostic(error, &spec.password);
                self.append_operation_diagnostic(
                    DatabaseOperationKind::Backup,
                    &backup.owner_user_id,
                    &backup.id,
                    "dumping",
                    diagnostic.clone(),
                )
                .await;
                bail!("{}", diagnostic.message);
            }
        };
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
        let completed = self
            .metadata_store
            .complete_database_backup(&backup.id, complete)
            .await?;
        self.append_operation_event(
            DatabaseOperationKind::Backup,
            &completed.id,
            DatabaseOperationEventType::Succeeded,
            json!({ "backup": completed }),
        )
        .await;

        Ok(())
    }

    async fn process_restore(&self, restore: DatabaseRestoreRecord) {
        if let Err(error) = self.run_restore(&restore).await {
            let error = truncate_error(&error.to_string());
            tracing::error!(restore_id = %restore.id, error = %error, "database restore failed");
            if let Ok(failed) = self
                .metadata_store
                .fail_database_restore(&restore.id, error)
                .await
            {
                self.append_operation_event(
                    DatabaseOperationKind::Restore,
                    &failed.id,
                    DatabaseOperationEventType::Failed,
                    json!({ "restore": failed }),
                )
                .await;
            }
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
        if let Err(error) = self
            .process_executor
            .restore_postgres(&spec, &file_path)
            .await
        {
            let diagnostic = process_failure_diagnostic(error, &spec.password);
            self.append_operation_diagnostic(
                DatabaseOperationKind::Restore,
                &restore.owner_user_id,
                &restore.id,
                "restoring",
                diagnostic.clone(),
            )
            .await;
            bail!("{}", diagnostic.message);
        }
        let completed = self
            .metadata_store
            .complete_database_restore(&restore.id)
            .await?;
        self.append_operation_event(
            DatabaseOperationKind::Restore,
            &completed.id,
            DatabaseOperationEventType::Succeeded,
            json!({ "restore": completed }),
        )
        .await;
        let _ = tokio::fs::remove_file(file_path).await;

        Ok(())
    }

    async fn append_operation_event(
        &self,
        operation_kind: DatabaseOperationKind,
        operation_id: &str,
        event_type: DatabaseOperationEventType,
        payload: serde_json::Value,
    ) {
        if let Err(error) = self
            .metadata_store
            .append_database_operation_event(operation_kind, operation_id, event_type, payload)
            .await
        {
            tracing::warn!(
                operation_kind = %operation_kind.as_str(),
                operation_id,
                event_type = %event_type.as_str(),
                error = %error,
                "failed to append database operation event"
            );
        }
    }

    async fn append_operation_diagnostic(
        &self,
        operation_kind: DatabaseOperationKind,
        owner_user_id: &str,
        operation_id: &str,
        phase: &str,
        diagnostic: ProcessFailureDiagnostic,
    ) {
        if let Err(error) = self
            .metadata_store
            .append_database_operation_diagnostic(
                owner_user_id,
                AppendDatabaseOperationDiagnostic {
                    operation_kind,
                    operation_id: operation_id.to_owned(),
                    phase: phase.to_owned(),
                    message: diagnostic.message,
                    command_name: diagnostic.command_name,
                    exit_code: diagnostic.exit_code,
                    stdout: diagnostic.stdout,
                    stderr: diagnostic.stderr,
                    stdout_truncated: diagnostic.stdout_truncated,
                    stderr_truncated: diagnostic.stderr_truncated,
                },
            )
            .await
        {
            tracing::warn!(
                operation_kind = %operation_kind.as_str(),
                operation_id,
                phase,
                error = %error,
                "failed to append database operation diagnostic"
            );
        }
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

    let stdout = redact(&String::from_utf8_lossy(&output.stdout), secret);
    let stderr = redact(&String::from_utf8_lossy(&output.stderr), secret);
    Err(DatabaseCommandFailure {
        action: action.to_owned(),
        exit_code: output.status.code(),
        stdout,
        stderr,
    }
    .into())
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessFailureDiagnostic {
    message: String,
    command_name: Option<String>,
    exit_code: Option<i32>,
    stdout: Option<String>,
    stderr: Option<String>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DatabaseCommandFailure {
    action: String,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl DatabaseCommandFailure {
    fn message(&self) -> String {
        command_failure_message(&self.action, &self.stdout, &self.stderr)
    }
}

impl std::fmt::Display for DatabaseCommandFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message())
    }
}

impl std::error::Error for DatabaseCommandFailure {}

fn process_failure_diagnostic(error: anyhow::Error, secret: &str) -> ProcessFailureDiagnostic {
    if let Some(command_failure) = error.downcast_ref::<DatabaseCommandFailure>() {
        let stdout = redact(&command_failure.stdout, secret);
        let stderr = redact(&command_failure.stderr, secret);
        let (stdout, stdout_truncated) = diagnostic_output(stdout);
        let (stderr, stderr_truncated) = diagnostic_output(stderr);

        return ProcessFailureDiagnostic {
            message: truncate_error(&redact(&command_failure.message(), secret)),
            command_name: Some(command_failure.action.clone()),
            exit_code: command_failure.exit_code,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
        };
    }

    ProcessFailureDiagnostic {
        message: truncate_error(&redact(&error.to_string(), secret)),
        command_name: None,
        exit_code: None,
        stdout: None,
        stderr: None,
        stdout_truncated: false,
        stderr_truncated: false,
    }
}

fn command_failure_message(action: &str, stdout: &str, stderr: &str) -> String {
    truncate_error(&format!("{action} failed: {stdout}\n{stderr}"))
}

fn diagnostic_output(value: String) -> (Option<String>, bool) {
    if value.is_empty() {
        return (None, false);
    }

    let (value, truncated) = truncate_to_bytes(&value, MAX_DIAGNOSTIC_OUTPUT_BYTES);
    (Some(value), truncated)
}

fn truncate_error(message: &str) -> String {
    truncate_to_bytes(message, MAX_ERROR_BYTES).0
}

fn truncate_to_bytes(message: &str, max_bytes: usize) -> (String, bool) {
    if message.len() <= max_bytes {
        return (message.to_owned(), false);
    }

    let suffix = "...";
    let mut end = max_bytes.saturating_sub(suffix.len());
    while end > 0 && !message.is_char_boundary(end) {
        end -= 1;
    }

    (format!("{}{}", &message[..end], suffix), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use liquid_core::{ManagedDatabaseConnectionSpec, ManagedDatabaseSslMode};
    use std::sync::Mutex;

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

    #[tokio::test]
    async fn process_backup_persists_pg_dump_failure_diagnostic() {
        let root = temp_root("liquid-backup-diagnostic").await;
        let backup = test_backup();
        let store = Arc::new(RecordingMetadataStore::new(backup.clone(), None));
        let worker = DatabaseOperationWorker::new(
            store.clone(),
            Arc::new(StaticConnectionLoader),
            None,
            Arc::new(FailingExecutor::dump(long_secret_error("pg_dump"))),
            DatabaseBackupWorkerConfig::new("worker", &root),
        );

        worker.process_backup(backup).await;

        {
            let failed = store.failed_backups.lock().unwrap();
            assert_eq!(failed.len(), 1);
            assert!(failed[0].contains("pg_dump failed"));
            assert!(!failed[0].contains("secret-password"));
        }

        {
            let diagnostics = store.diagnostics.lock().unwrap();
            assert_eq!(diagnostics.len(), 1);
            let diagnostic = &diagnostics[0];
            assert_eq!(diagnostic.operation_kind, DatabaseOperationKind::Backup);
            assert_eq!(diagnostic.operation_id, "backup-1");
            assert_eq!(diagnostic.phase, "dumping");
            assert_eq!(diagnostic.command_name.as_deref(), Some("pg_dump"));
            assert_eq!(diagnostic.exit_code, Some(1));
            assert!(diagnostic.stderr.as_ref().unwrap().contains("[redacted]"));
            assert!(
                !diagnostic
                    .stderr
                    .as_ref()
                    .unwrap()
                    .contains("secret-password")
            );
            assert!(diagnostic.stderr_truncated);
            assert!(diagnostic.stderr.as_ref().unwrap().len() <= MAX_DIAGNOSTIC_OUTPUT_BYTES);
        }

        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn process_restore_persists_pg_restore_failure_diagnostic() {
        let root = temp_root("liquid-restore-diagnostic").await;
        let source = root.join("source.dump");
        tokio::fs::write(&source, b"dump").await.unwrap();
        let mut backup = test_backup();
        backup.status = DatabaseBackupStatus::Succeeded;
        backup.storage = Some(DatabaseBackupStorageMetadata {
            kind: DatabaseBackupStorageKind::Local,
            local_path: Some(source.display().to_string()),
            bucket: None,
            key: None,
            version_id: None,
            etag: None,
            size_bytes: Some(4),
            checksum_sha256: None,
        });
        let restore = test_restore();
        let store = Arc::new(RecordingMetadataStore::new(
            backup.clone(),
            Some(restore.clone()),
        ));
        let worker = DatabaseOperationWorker::new(
            store.clone(),
            Arc::new(StaticConnectionLoader),
            None,
            Arc::new(FailingExecutor::restore(long_secret_error("pg_restore"))),
            DatabaseBackupWorkerConfig::new("worker", &root),
        );

        worker.process_restore(restore).await;

        {
            let failed = store.failed_restores.lock().unwrap();
            assert_eq!(failed.len(), 1);
            assert!(failed[0].contains("pg_restore failed"));
            assert!(!failed[0].contains("secret-password"));
        }

        {
            let diagnostics = store.diagnostics.lock().unwrap();
            assert_eq!(diagnostics.len(), 1);
            let diagnostic = &diagnostics[0];
            assert_eq!(diagnostic.operation_kind, DatabaseOperationKind::Restore);
            assert_eq!(diagnostic.operation_id, "restore-1");
            assert_eq!(diagnostic.phase, "restoring");
            assert_eq!(diagnostic.command_name.as_deref(), Some("pg_restore"));
            assert_eq!(diagnostic.exit_code, Some(1));
            assert!(diagnostic.stderr.as_ref().unwrap().contains("[redacted]"));
            assert!(
                !diagnostic
                    .stderr
                    .as_ref()
                    .unwrap()
                    .contains("secret-password")
            );
            assert!(diagnostic.stderr_truncated);
            assert!(diagnostic.stderr.as_ref().unwrap().len() <= MAX_DIAGNOSTIC_OUTPUT_BYTES);
        }

        let _ = tokio::fs::remove_dir_all(root).await;
    }

    async fn temp_root(prefix: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "{}-{}",
            prefix,
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        tokio::fs::create_dir_all(&root).await.unwrap();
        root
    }

    fn long_secret_error(action: &str) -> DatabaseCommandFailure {
        DatabaseCommandFailure {
            action: action.to_owned(),
            exit_code: Some(1),
            stdout: format!("{action} stdout secret-password"),
            stderr: format!(
                "{action} stderr secret-password {}",
                "x".repeat(MAX_DIAGNOSTIC_OUTPUT_BYTES + 1024)
            ),
        }
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
            schedule_id: None,
            trigger: liquid_core::DatabaseBackupTrigger::Immediate,
            scheduled_for: None,
            conversation_id: None,
            created_from_turn_id: None,
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

    fn test_restore() -> DatabaseRestoreRecord {
        DatabaseRestoreRecord {
            id: "restore-1".to_owned(),
            owner_user_id: "user-1".to_owned(),
            backup_id: "backup-1".to_owned(),
            target: liquid_core::ManagedDatabaseSnapshot {
                id: "target-db-1".to_owned(),
                name: "Target".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "target".to_owned(),
                username: "postgres".to_owned(),
                ssl_mode: ManagedDatabaseSslMode::Prefer,
            },
            format: liquid_core::DatabaseBackupFormat::PostgresCustom,
            status: DatabaseBackupStatus::Running,
            phase: "running".to_owned(),
            progress_percent: 1,
            restore_options: serde_json::json!({}),
            conversation_id: None,
            created_from_turn_id: None,
            error: None,
            purpose: Some("restore backup".to_owned()),
            worker_id: None,
            heartbeat_at: None,
            started_at: None,
            completed_at: None,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
        }
    }

    struct RecordingMetadataStore {
        backup: Mutex<DatabaseBackupRecord>,
        restore: Mutex<Option<DatabaseRestoreRecord>>,
        diagnostics: Mutex<Vec<AppendDatabaseOperationDiagnostic>>,
        failed_backups: Mutex<Vec<String>>,
        failed_restores: Mutex<Vec<String>>,
    }

    impl RecordingMetadataStore {
        fn new(backup: DatabaseBackupRecord, restore: Option<DatabaseRestoreRecord>) -> Self {
            Self {
                backup: Mutex::new(backup),
                restore: Mutex::new(restore),
                diagnostics: Mutex::new(Vec::new()),
                failed_backups: Mutex::new(Vec::new()),
                failed_restores: Mutex::new(Vec::new()),
            }
        }
    }

    struct StaticConnectionLoader;

    #[async_trait]
    impl ManagedDatabaseConnectionLoader for StaticConnectionLoader {
        async fn load_managed_database_connection(
            &self,
            _key: &ManagedDatabasePoolKey,
        ) -> Result<ManagedDatabaseConnectionSpec, liquid_core::ManagedDatabaseConnectionLoaderError>
        {
            Ok(ManagedDatabaseConnectionSpec {
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "warehouse".to_owned(),
                username: "postgres".to_owned(),
                password: "secret-password".to_owned(),
                ssl_mode: ManagedDatabaseSslMode::Prefer,
            })
        }
    }

    enum FailingExecutor {
        Dump(DatabaseCommandFailure),
        Restore(DatabaseCommandFailure),
    }

    impl FailingExecutor {
        fn dump(failure: DatabaseCommandFailure) -> Self {
            Self::Dump(failure)
        }

        fn restore(failure: DatabaseCommandFailure) -> Self {
            Self::Restore(failure)
        }
    }

    #[async_trait]
    impl DatabaseProcessExecutor for FailingExecutor {
        async fn dump_postgres(
            &self,
            _spec: &liquid_core::ManagedDatabaseConnectionSpec,
            _output_path: &Path,
        ) -> Result<DatabaseDumpResult> {
            match self {
                Self::Dump(failure) => Err(failure.clone().into()),
                Self::Restore(_) => unreachable!(),
            }
        }

        async fn restore_postgres(
            &self,
            _spec: &liquid_core::ManagedDatabaseConnectionSpec,
            _input_path: &Path,
        ) -> Result<DatabaseRestoreResult> {
            match self {
                Self::Restore(failure) => Err(failure.clone().into()),
                Self::Dump(_) => unreachable!(),
            }
        }
    }

    #[async_trait]
    impl DatabaseBackupMetadataStore for RecordingMetadataStore {
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
            Ok(self.backup.lock().unwrap().clone())
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
            self.restore
                .lock()
                .unwrap()
                .clone()
                .ok_or(liquid_core::DatabaseBackupMetadataStoreError::NotFound)
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
            phase: &str,
            progress_percent: i32,
        ) -> Result<DatabaseBackupRecord, liquid_core::DatabaseBackupMetadataStoreError> {
            let mut backup = self.backup.lock().unwrap();
            backup.phase = phase.to_owned();
            backup.progress_percent = progress_percent;
            Ok(backup.clone())
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
            error: String,
        ) -> Result<DatabaseBackupRecord, liquid_core::DatabaseBackupMetadataStoreError> {
            self.failed_backups.lock().unwrap().push(error.clone());
            let mut backup = self.backup.lock().unwrap();
            backup.status = DatabaseBackupStatus::Failed;
            backup.phase = "failed".to_owned();
            backup.error = Some(error);
            Ok(backup.clone())
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
            phase: &str,
            progress_percent: i32,
        ) -> Result<DatabaseRestoreRecord, liquid_core::DatabaseBackupMetadataStoreError> {
            let mut restore = self.restore.lock().unwrap();
            let restore = restore
                .as_mut()
                .ok_or(liquid_core::DatabaseBackupMetadataStoreError::NotFound)?;
            restore.phase = phase.to_owned();
            restore.progress_percent = progress_percent;
            Ok(restore.clone())
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
            error: String,
        ) -> Result<DatabaseRestoreRecord, liquid_core::DatabaseBackupMetadataStoreError> {
            self.failed_restores.lock().unwrap().push(error.clone());
            let mut restore = self.restore.lock().unwrap();
            let restore = restore
                .as_mut()
                .ok_or(liquid_core::DatabaseBackupMetadataStoreError::NotFound)?;
            restore.status = DatabaseBackupStatus::Failed;
            restore.phase = "failed".to_owned();
            restore.error = Some(error);
            Ok(restore.clone())
        }

        async fn fail_stale_database_jobs(
            &self,
            _stale_after_seconds: i64,
        ) -> Result<u64, liquid_core::DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn append_database_operation_diagnostic(
            &self,
            owner_user_id: &str,
            diagnostic: AppendDatabaseOperationDiagnostic,
        ) -> Result<
            liquid_core::DatabaseOperationDiagnosticRecord,
            liquid_core::DatabaseBackupMetadataStoreError,
        > {
            self.diagnostics.lock().unwrap().push(diagnostic.clone());
            Ok(liquid_core::DatabaseOperationDiagnosticRecord {
                id: "diagnostic-1".to_owned(),
                owner_user_id: owner_user_id.to_owned(),
                operation_kind: diagnostic.operation_kind,
                operation_id: diagnostic.operation_id,
                phase: diagnostic.phase,
                message: diagnostic.message,
                command_name: diagnostic.command_name,
                exit_code: diagnostic.exit_code,
                stdout: diagnostic.stdout,
                stderr: diagnostic.stderr,
                stdout_truncated: diagnostic.stdout_truncated,
                stderr_truncated: diagnostic.stderr_truncated,
                created_at: time::OffsetDateTime::UNIX_EPOCH,
            })
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
