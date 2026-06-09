use std::{future::Future, pin::Pin, sync::Arc};

use liquid_agent::{
    ApprovedWriteExecutionResult, PostgresToolConfig, PostgresToolExecutionMode, SqlAuditAgent,
    execute_approved_write_sql_with_config,
};
use liquid_config::WorkbenchConfig;
use liquid_core::{ManagedDatabaseConnectionLoader, ManagedDatabasePoolPolicy};
use liquid_storage::{LiquidStore, ManagedDatabasePoolManager};
use sqlx::PgPool;

use crate::chat_sql::{ChatSqlExecutor, DefaultChatSqlExecutor};

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
    pub(crate) managed_database_pools: Arc<ManagedDatabasePoolManager>,
    pub(crate) sql_metadata_required: bool,
    pub(crate) sql_execution: PostgresToolExecutionMode,
    pub(crate) approved_write_execution_enabled: bool,
    pub(crate) approved_sql_executor: Arc<dyn ApprovedSqlExecutor>,
    pub(crate) chat_sql_executor: Arc<dyn ChatSqlExecutor>,
    pub(crate) managed_database_connection_tester: Arc<dyn ManagedDatabaseConnectionTester>,
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
            workbench: WorkbenchConfig::default(),
        }
    }

    pub fn with_workbench_config(mut self, workbench: WorkbenchConfig) -> Self {
        self.workbench = workbench;
        self
    }
}
