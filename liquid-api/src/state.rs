use std::sync::Arc;

use liquid_agent::{PostgresToolExecutionMode, SqlAuditAgent};
use liquid_core::{ManagedDatabaseConnectionLoader, ManagedDatabasePoolPolicy};
use liquid_storage::{LiquidStore, ManagedDatabasePoolManager};

#[derive(Clone)]
pub struct ApiState {
    pub(crate) agent: Arc<dyn SqlAuditAgent>,
    pub(crate) store: Arc<dyn LiquidStore>,
    pub(crate) managed_database_pools: Arc<ManagedDatabasePoolManager>,
    pub(crate) sql_metadata_required: bool,
    pub(crate) sql_execution: PostgresToolExecutionMode,
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
        Self {
            agent,
            store,
            managed_database_pools,
            sql_metadata_required,
            sql_execution: managed_database_audit_execution(sql_execution),
        }
    }
}

fn managed_database_audit_execution(mode: PostgresToolExecutionMode) -> PostgresToolExecutionMode {
    match mode {
        PostgresToolExecutionMode::Off => PostgresToolExecutionMode::Off,
        PostgresToolExecutionMode::Readonly | PostgresToolExecutionMode::WriteGated => {
            PostgresToolExecutionMode::Readonly
        }
    }
}
