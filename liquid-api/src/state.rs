use std::{future::Future, pin::Pin, sync::Arc};

use liquid_agent::{
    ApprovedWriteExecutionResult, PostgresToolConfig, PostgresToolExecutionMode, SqlAuditAgent,
    execute_approved_write_sql_with_config,
};
use liquid_config::WorkbenchConfig;
use liquid_core::{
    CompleteDatabaseBackup, DatabaseBackupMetadataStore, DatabaseBackupMetadataStoreError,
    DatabaseBackupRecord, DatabaseBackupStatus, DatabaseRestoreRecord,
};
use liquid_core::{ManagedDatabaseConnectionLoader, ManagedDatabasePoolPolicy};
use liquid_storage::{LiquidStore, ManagedDatabasePoolManager};
use sqlx::PgPool;

use crate::chat_sql::{ChatSqlExecutor, DefaultChatSqlExecutor};
use crate::database_diagram_generation::{
    DatabaseDiagramGenerator, PostgresDatabaseDiagramGenerator,
};

pub type ApprovedSqlExecutionFuture<'a> =
    Pin<Box<dyn Future<Output = anyhow::Result<ApprovedWriteExecutionResult>> + Send + 'a>>;

pub type ManagedDatabaseConnectionTestFuture<'a> =
    Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>;

pub trait ApprovedSqlExecutor: Send + Sync {
    fn execute<'a>(
        &'a self,
        config: PostgresToolConfig,
        sql: &'a str,
    ) -> ApprovedSqlExecutionFuture<'a>;
}

pub trait ManagedDatabaseConnectionTester: Send + Sync {
    fn test<'a>(&'a self, pool: PgPool) -> ManagedDatabaseConnectionTestFuture<'a>;
}

#[derive(Debug, Default)]
pub struct DefaultApprovedSqlExecutor;

impl ApprovedSqlExecutor for DefaultApprovedSqlExecutor {
    fn execute<'a>(
        &'a self,
        config: PostgresToolConfig,
        sql: &'a str,
    ) -> ApprovedSqlExecutionFuture<'a> {
        Box::pin(async move { execute_approved_write_sql_with_config(config, sql).await })
    }
}

#[derive(Debug, Default)]
pub struct DefaultManagedDatabaseConnectionTester;

impl ManagedDatabaseConnectionTester for DefaultManagedDatabaseConnectionTester {
    fn test<'a>(&'a self, pool: PgPool) -> ManagedDatabaseConnectionTestFuture<'a> {
        Box::pin(async move {
            let mut connection = pool.acquire().await?;
            sqlx::query_scalar::<_, i32>("select 1")
                .fetch_one(&mut *connection)
                .await?;

            Ok(())
        })
    }
}

#[derive(Clone)]
pub struct ApiState {
    pub(crate) agent: Arc<dyn SqlAuditAgent>,
    pub(crate) store: Arc<dyn LiquidStore>,
    pub(crate) database_backups: Arc<dyn DatabaseBackupMetadataStore>,
    pub(crate) managed_database_pools: Arc<ManagedDatabasePoolManager>,
    pub(crate) sql_metadata_required: bool,
    pub(crate) sql_execution: PostgresToolExecutionMode,
    pub(crate) approved_write_execution_enabled: bool,
    pub(crate) approved_sql_executor: Arc<dyn ApprovedSqlExecutor>,
    pub(crate) chat_sql_executor: Arc<dyn ChatSqlExecutor>,
    pub(crate) managed_database_connection_tester: Arc<dyn ManagedDatabaseConnectionTester>,
    pub(crate) database_diagram_generator: Arc<dyn DatabaseDiagramGenerator>,
    pub(crate) workbench: WorkbenchConfig,
}

impl ApiState {
    pub fn new<S>(agent: Arc<dyn SqlAuditAgent>, store: Arc<S>) -> Self
    where
        S: LiquidStore + ManagedDatabaseConnectionLoader + 'static,
    {
        let pool_manager = Arc::new(ManagedDatabasePoolManager::new(
            store.clone(),
            ManagedDatabasePoolPolicy::default(),
        ));

        Self::with_pool_manager(
            agent,
            store,
            pool_manager,
            false,
            PostgresToolExecutionMode::Readonly,
        )
    }

    pub fn with_pool_manager<S>(
        agent: Arc<dyn SqlAuditAgent>,
        store: Arc<S>,
        managed_database_pools: Arc<ManagedDatabasePoolManager>,
        sql_metadata_required: bool,
        sql_execution: PostgresToolExecutionMode,
    ) -> Self
    where
        S: LiquidStore + 'static,
    {
        Self::with_pool_manager_and_executor(
            agent,
            store,
            managed_database_pools,
            sql_metadata_required,
            sql_execution,
            Arc::new(DefaultApprovedSqlExecutor),
        )
    }

    pub fn with_pool_manager_and_executor<S>(
        agent: Arc<dyn SqlAuditAgent>,
        store: Arc<S>,
        managed_database_pools: Arc<ManagedDatabasePoolManager>,
        sql_metadata_required: bool,
        sql_execution: PostgresToolExecutionMode,
        approved_sql_executor: Arc<dyn ApprovedSqlExecutor>,
    ) -> Self
    where
        S: LiquidStore + 'static,
    {
        Self::with_pool_manager_executor_and_connection_tester(
            agent,
            store,
            managed_database_pools,
            sql_metadata_required,
            sql_execution,
            approved_sql_executor,
            Arc::new(DefaultManagedDatabaseConnectionTester),
        )
    }

    pub fn with_pool_manager_executor_and_connection_tester<S>(
        agent: Arc<dyn SqlAuditAgent>,
        store: Arc<S>,
        managed_database_pools: Arc<ManagedDatabasePoolManager>,
        sql_metadata_required: bool,
        sql_execution: PostgresToolExecutionMode,
        approved_sql_executor: Arc<dyn ApprovedSqlExecutor>,
        managed_database_connection_tester: Arc<dyn ManagedDatabaseConnectionTester>,
    ) -> Self
    where
        S: LiquidStore + 'static,
    {
        Self::with_pool_manager_executors_and_connection_tester(
            agent,
            store,
            managed_database_pools,
            sql_metadata_required,
            sql_execution,
            approved_sql_executor,
            Arc::new(DefaultChatSqlExecutor),
            managed_database_connection_tester,
        )
    }

    pub fn with_pool_manager_executors_and_connection_tester<S>(
        agent: Arc<dyn SqlAuditAgent>,
        store: Arc<S>,
        managed_database_pools: Arc<ManagedDatabasePoolManager>,
        sql_metadata_required: bool,
        sql_execution: PostgresToolExecutionMode,
        approved_sql_executor: Arc<dyn ApprovedSqlExecutor>,
        chat_sql_executor: Arc<dyn ChatSqlExecutor>,
        managed_database_connection_tester: Arc<dyn ManagedDatabaseConnectionTester>,
    ) -> Self
    where
        S: LiquidStore + 'static,
    {
        Self {
            agent,
            store,
            managed_database_pools,
            sql_metadata_required,
            approved_write_execution_enabled: matches!(
                sql_execution,
                PostgresToolExecutionMode::WriteGated
            ),
            sql_execution,
            approved_sql_executor,
            chat_sql_executor,
            managed_database_connection_tester,
            database_diagram_generator: Arc::new(PostgresDatabaseDiagramGenerator),
            workbench: WorkbenchConfig::default(),
            database_backups: Arc::new(UnsupportedDatabaseBackupStore),
        }
    }

    pub fn with_workbench_config(mut self, workbench: WorkbenchConfig) -> Self {
        self.workbench = workbench;
        self
    }

    pub fn with_database_backup_store(
        mut self,
        database_backups: Arc<dyn DatabaseBackupMetadataStore>,
    ) -> Self {
        self.database_backups = database_backups;
        self
    }

    pub fn with_database_diagram_generator(
        mut self,
        database_diagram_generator: Arc<dyn DatabaseDiagramGenerator>,
    ) -> Self {
        self.database_diagram_generator = database_diagram_generator;
        self
    }
}

#[derive(Debug)]
struct UnsupportedDatabaseBackupStore;

#[async_trait::async_trait]
impl DatabaseBackupMetadataStore for UnsupportedDatabaseBackupStore {
    async fn create_database_backup(
        &self,
        _owner_user_id: &str,
        _source_managed_database_id: &str,
        _purpose: Option<String>,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
        Err(DatabaseBackupMetadataStoreError::Backend(
            "database backups are not configured".to_owned(),
        ))
    }

    async fn get_database_backup(
        &self,
        _owner_user_id: &str,
        _id: &str,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
        Err(DatabaseBackupMetadataStoreError::NotFound)
    }

    async fn list_database_backups(
        &self,
        _owner_user_id: &str,
        _source_managed_database_id: Option<&str>,
        _status: Option<DatabaseBackupStatus>,
        _limit: i64,
    ) -> Result<Vec<DatabaseBackupRecord>, DatabaseBackupMetadataStoreError> {
        Ok(Vec::new())
    }

    async fn delete_database_backup(
        &self,
        _owner_user_id: &str,
        _id: &str,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
        Err(DatabaseBackupMetadataStoreError::NotFound)
    }

    async fn create_database_restore(
        &self,
        _owner_user_id: &str,
        _backup_id: &str,
        _target_managed_database_id: &str,
        _purpose: String,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
        Err(DatabaseBackupMetadataStoreError::Backend(
            "database restores are not configured".to_owned(),
        ))
    }

    async fn get_database_restore(
        &self,
        _owner_user_id: &str,
        _id: &str,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
        Err(DatabaseBackupMetadataStoreError::NotFound)
    }

    async fn list_database_restores(
        &self,
        _owner_user_id: &str,
        _backup_id: Option<&str>,
        _target_managed_database_id: Option<&str>,
        _status: Option<DatabaseBackupStatus>,
        _limit: i64,
    ) -> Result<Vec<DatabaseRestoreRecord>, DatabaseBackupMetadataStoreError> {
        Ok(Vec::new())
    }

    async fn claim_next_database_backup(
        &self,
        _worker_id: &str,
    ) -> Result<Option<DatabaseBackupRecord>, DatabaseBackupMetadataStoreError> {
        Ok(None)
    }

    async fn update_database_backup_progress(
        &self,
        _id: &str,
        _phase: &str,
        _progress_percent: i32,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
        Err(DatabaseBackupMetadataStoreError::NotFound)
    }

    async fn complete_database_backup(
        &self,
        _id: &str,
        _result: CompleteDatabaseBackup,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
        Err(DatabaseBackupMetadataStoreError::NotFound)
    }

    async fn fail_database_backup(
        &self,
        _id: &str,
        _error: String,
    ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
        Err(DatabaseBackupMetadataStoreError::NotFound)
    }

    async fn claim_next_database_restore(
        &self,
        _worker_id: &str,
    ) -> Result<Option<DatabaseRestoreRecord>, DatabaseBackupMetadataStoreError> {
        Ok(None)
    }

    async fn update_database_restore_progress(
        &self,
        _id: &str,
        _phase: &str,
        _progress_percent: i32,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
        Err(DatabaseBackupMetadataStoreError::NotFound)
    }

    async fn complete_database_restore(
        &self,
        _id: &str,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
        Err(DatabaseBackupMetadataStoreError::NotFound)
    }

    async fn fail_database_restore(
        &self,
        _id: &str,
        _error: String,
    ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
        Err(DatabaseBackupMetadataStoreError::NotFound)
    }

    async fn fail_stale_database_jobs(
        &self,
        _stale_after_seconds: i64,
    ) -> Result<u64, DatabaseBackupMetadataStoreError> {
        Ok(0)
    }
}
