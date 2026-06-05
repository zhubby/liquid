use std::sync::Arc;
use std::time::Duration;

use liquid_agent::{PostgresToolExecutionMode, SqlAuditAgent};
use liquid_config::{LiquidConfig, SqlExecutionMode, SqlMetadataMode};
use liquid_core::{ManagedDatabaseConnectionLoader, ManagedDatabasePoolPolicy};
use liquid_storage::{ManagedDatabasePoolManager, Storage};
use tokio::net::TcpListener;

use crate::{ApiState, router_with_cors};

pub async fn serve(
    config: LiquidConfig,
    agent: Arc<dyn SqlAuditAgent>,
    store: Arc<Storage>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(config.api_addr).await?;
    let loader: Arc<dyn ManagedDatabaseConnectionLoader> = store.clone();
    let managed_database_pools = Arc::new(ManagedDatabasePoolManager::new(
        loader,
        managed_database_pool_policy(&config),
    ));
    managed_database_pools.spawn_reaper();
    let app = router_with_cors(
        ApiState::with_pool_manager(
            agent,
            store,
            managed_database_pools,
            matches!(config.sql_metadata, SqlMetadataMode::Required),
            managed_database_audit_execution(config.sql_execution),
        ),
        &config.cors_origin,
    )?;

    axum::serve(listener, app).await?;
    Ok(())
}

fn managed_database_pool_policy(config: &LiquidConfig) -> ManagedDatabasePoolPolicy {
    ManagedDatabasePoolPolicy {
        max_connections: config.managed_database_pool.max_connections,
        pool_idle_ttl: Duration::from_secs(config.managed_database_pool.idle_ttl_seconds),
        reap_interval: Duration::from_secs(config.managed_database_pool.reap_interval_seconds),
        acquire_timeout: Duration::from_secs(config.managed_database_pool.acquire_timeout_seconds),
        ..ManagedDatabasePoolPolicy::default()
    }
}

fn managed_database_audit_execution(mode: SqlExecutionMode) -> PostgresToolExecutionMode {
    match mode {
        SqlExecutionMode::Off => PostgresToolExecutionMode::Off,
        SqlExecutionMode::Readonly | SqlExecutionMode::WriteGated => {
            PostgresToolExecutionMode::Readonly
        }
    }
}
