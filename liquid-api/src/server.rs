use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::Context;
use liquid_agent::{
    DatabaseBackupScheduler, DatabaseBackupSchedulerConfig, DatabaseBackupWorkerConfig,
    DatabaseOperationWorker, DefaultDatabaseProcessExecutor, PostgresToolExecutionMode,
    S3BackupObjectStore, S3BackupObjectStoreConfig, SqlAuditAgent,
};
use liquid_config::{LiquidConfig, SqlExecutionMode, SqlMetadataMode};
use liquid_core::{
    AgentMessageRole, DatabaseBackupMetadataStore, DatabaseBackupRecord,
    DatabaseOperationEventType, DatabaseOperationKind, DatabaseRestoreRecord,
    ManagedDatabaseConnectionLoader, ManagedDatabasePoolPolicy,
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
    spawn_database_backup_scheduler(store.clone());
    spawn_database_operation_event_deliverer(store.clone());
    let state = ApiState::with_pool_manager(
        agent,
        store.clone(),
        managed_database_pools,
        matches!(config.sql_metadata, SqlMetadataMode::Required),
        managed_database_audit_execution(config.sql_execution),
    )
    .with_database_backup_store(store)
    .with_workbench_config(config.workbench.clone());
    let app = router_with_cors(state, &config.cors_origin)?;

    axum::serve(listener, app).await?;
    Ok(())
}

fn spawn_database_operation_worker(
    config: &LiquidConfig,
    store: Arc<Storage>,
) -> anyhow::Result<()> {
    let object_store = config
        .database_backup
        .s3_bucket
        .clone()
        .map(|bucket| {
            let s3_config =
                S3BackupObjectStoreConfig::new(bucket, config.database_backup.s3_region.clone())
                    .with_prefix(config.database_backup.s3_prefix.clone())
                    .with_endpoint(config.database_backup.s3_endpoint.clone())
                    .with_path_style(config.database_backup.s3_path_style);
            S3BackupObjectStore::from_env(s3_config)
                .map(|store| Arc::new(store) as Arc<dyn liquid_agent::BackupObjectStore>)
        })
        .transpose()?;
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

fn spawn_database_backup_scheduler(store: Arc<Storage>) {
    let metadata_store: Arc<dyn DatabaseBackupMetadataStore> = store;
    let scheduler = DatabaseBackupScheduler::new(
        metadata_store,
        DatabaseBackupSchedulerConfig::new(format!("liquid-scheduler-{}", std::process::id())),
    );
    let _handle = scheduler.spawn();
}

fn spawn_database_operation_event_deliverer(store: Arc<Storage>) {
    tokio::spawn(async move {
        loop {
            match deliver_next_database_operation_event(&store).await {
                Ok(true) => {}
                Ok(false) => tokio::time::sleep(Duration::from_secs(2)).await,
                Err(error) => {
                    tracing::warn!(error = %error, "database operation event delivery failed");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    });
}

async fn deliver_next_database_operation_event(store: &Storage) -> anyhow::Result<bool> {
    let Some(event) = store.claim_next_database_operation_event().await? else {
        return Ok(false);
    };
    let Some(conversation_id) = event.conversation_id.as_deref() else {
        return Ok(true);
    };
    let (content, metadata) = match event.operation_kind {
        DatabaseOperationKind::Backup => {
            let backup = event
                .payload
                .get("backup")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("database backup event missing backup payload"))
                .and_then(|value| Ok(serde_json::from_value::<DatabaseBackupRecord>(value)?))?;
            let content = match event.event_type {
                DatabaseOperationEventType::Succeeded => {
                    format!("Database backup completed for {}.", backup.source.name)
                }
                DatabaseOperationEventType::Failed => {
                    format!("Database backup failed for {}.", backup.source.name)
                }
                DatabaseOperationEventType::Queued => return Ok(true),
            };
            (
                content,
                serde_json::json!({
                    "kind": "database_operation_status",
                    "database_backup": backup,
                }),
            )
        }
        DatabaseOperationKind::Restore => {
            let restore = event
                .payload
                .get("restore")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("database restore event missing restore payload"))
                .and_then(|value| Ok(serde_json::from_value::<DatabaseRestoreRecord>(value)?))?;
            let content = match event.event_type {
                DatabaseOperationEventType::Succeeded => {
                    format!("Database restore completed for {}.", restore.target.name)
                }
                DatabaseOperationEventType::Failed => {
                    format!("Database restore failed for {}.", restore.target.name)
                }
                DatabaseOperationEventType::Queued => return Ok(true),
            };
            (
                content,
                serde_json::json!({
                    "kind": "database_operation_status",
                    "database_restore": restore,
                }),
            )
        }
    };

    let message = store
        .append_agent_message(
            &event.owner_user_id,
            conversation_id,
            event.turn_id.as_deref(),
            AgentMessageRole::Assistant,
            &content,
            Some(metadata),
        )
        .await?;
    store
        .mark_database_operation_event_delivered(&event.id, &message.id)
        .await?;

    Ok(true)
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
