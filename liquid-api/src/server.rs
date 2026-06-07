use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::Context;
use liquid_agent::{
    DatabaseBackupWorkerConfig, DatabaseOperationWorker, DefaultDatabaseProcessExecutor,
    PostgresToolExecutionMode, S3BackupObjectStore, S3BackupObjectStoreConfig, SqlAuditAgent,
};
use liquid_config::{LiquidConfig, SqlExecutionMode, SqlMetadataMode};
use liquid_core::{
    DatabaseBackupMetadataStore, ManagedDatabaseConnectionLoader, ManagedDatabasePoolPolicy,
};
use liquid_storage::LiquidStore;
use liquid_storage::{ManagedDatabasePoolManager, Storage};
use tokio::net::TcpListener;

use crate::{ApiState, router_with_cors};

pub async fn serve(
    config: LiquidConfig,
    agent: Arc<dyn SqlAuditAgent>,
    store: Arc<Storage>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(config.api_addr)
        .await
        .with_context(|| format!("failed to bind Liquid API listener at {}", config.api_addr))?;
    tracing::info!(addr = %config.api_addr, "liquid api listener bound");
    let loader: Arc<dyn ManagedDatabaseConnectionLoader> = store.clone();
    let managed_database_pools = Arc::new(ManagedDatabasePoolManager::new(
        loader,
        managed_database_pool_policy(&config),
    ));
    managed_database_pools.spawn_reaper();
    store.fail_stale_agent_turns(1).await?;
    spawn_database_operation_worker(&config, store.clone())?;
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

fn spawn_database_operation_worker(
    config: &LiquidConfig,
    store: Arc<Storage>,
) -> anyhow::Result<()> {
    let Some(bucket) = config.database_backup.s3_bucket.clone() else {
        return Ok(());
    };

    let s3_config =
        S3BackupObjectStoreConfig::new(bucket, config.database_backup.s3_region.clone())
            .with_prefix(config.database_backup.s3_prefix.clone())
            .with_endpoint(config.database_backup.s3_endpoint.clone())
            .with_path_style(config.database_backup.s3_path_style);
    let object_store = Arc::new(S3BackupObjectStore::from_env(s3_config)?);
    let metadata_store: Arc<dyn DatabaseBackupMetadataStore> = store.clone();
    let connection_loader: Arc<dyn ManagedDatabaseConnectionLoader> = store;
    let worker_config = DatabaseBackupWorkerConfig::new(
        format!("liquid-{}", std::process::id()),
        PathBuf::from(&config.database_backup.work_dir),
    )
    .with_object_key_prefix(config.database_backup.s3_prefix.clone())
    .with_concurrency(config.database_backup.worker_concurrency);
    let worker = DatabaseOperationWorker::new(
        metadata_store,
        connection_loader,
        object_store,
        Arc::new(DefaultDatabaseProcessExecutor),
        worker_config,
    );
    let _handles = worker.spawn();

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
        SqlExecutionMode::Readonly => PostgresToolExecutionMode::Readonly,
        SqlExecutionMode::WriteGated => PostgresToolExecutionMode::WriteGated,
    }
}
