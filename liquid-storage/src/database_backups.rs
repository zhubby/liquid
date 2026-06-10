use liquid_core::{
    AppendDatabaseOperationDiagnostic, CompleteDatabaseBackup, CreateDatabaseBackupScheduleRequest,
    DatabaseBackupFormat, DatabaseBackupListFilters, DatabaseBackupListPage,
    DatabaseBackupMetadataStoreError, DatabaseBackupRecord, DatabaseBackupScheduleRecord,
    DatabaseBackupScheduleStatus, DatabaseBackupStatus, DatabaseBackupStorageKind,
    DatabaseBackupStorageMetadata, DatabaseBackupTrigger, DatabaseOperationDiagnosticFilters,
    DatabaseOperationDiagnosticRecord, DatabaseOperationEventRecord, DatabaseOperationEventType,
    DatabaseOperationKind, DatabaseRestoreRecord, EnqueueDatabaseBackup, EnqueueDatabaseRestore,
    ManagedDatabaseSnapshot, UpdateDatabaseBackupScheduleRequest,
};
use serde_json::Value;
use sqlx::Row;
use time::OffsetDateTime;

use crate::{
    error::{StorageError, map_database_error},
    managed_databases::{load_managed_database_snapshot, parse_engine, parse_ssl_mode},
    store::Storage,
    validation::{optional_string, required_string},
};

const DATABASE_BACKUP_COLUMNS: &str = r#"
id::text,
owner_user_id::text,
source_managed_database_id::text,
source_managed_database_name,
source_managed_database_engine,
source_managed_database_host,
source_managed_database_port,
source_managed_database_database,
source_managed_database_username,
source_managed_database_ssl_mode,
format,
storage_kind,
local_path,
s3_bucket,
s3_key,
s3_version_id,
s3_etag,
size_bytes,
checksum_sha256,
postgres_server_version,
pg_dump_version,
status,
phase,
progress_percent,
schedule_id::text,
trigger,
scheduled_for,
conversation_id::text,
created_from_turn_id::text,
worker_id,
heartbeat_at,
started_at,
completed_at,
error,
purpose,
created_at,
updated_at
"#;

const DATABASE_RESTORE_COLUMNS: &str = r#"
id::text,
owner_user_id::text,
backup_id::text,
target_managed_database_id::text,
target_managed_database_name,
target_managed_database_engine,
target_managed_database_host,
target_managed_database_port,
target_managed_database_database,
target_managed_database_username,
target_managed_database_ssl_mode,
format,
restore_options,
status,
phase,
progress_percent,
conversation_id::text,
created_from_turn_id::text,
worker_id,
heartbeat_at,
started_at,
completed_at,
error,
purpose,
created_at,
updated_at
"#;

const DATABASE_BACKUP_SCHEDULE_COLUMNS: &str = r#"
id::text,
owner_user_id::text,
source_managed_database_id::text,
source_managed_database_name,
source_managed_database_engine,
source_managed_database_host,
source_managed_database_port,
source_managed_database_database,
source_managed_database_username,
source_managed_database_ssl_mode,
cron_expression,
timezone,
status,
purpose,
keep_last,
retention_days,
conversation_id::text,
created_from_turn_id::text,
last_enqueued_at,
next_run_at,
created_at,
updated_at
"#;

const DATABASE_OPERATION_EVENT_COLUMNS: &str = r#"
id::text,
owner_user_id::text,
operation_kind,
operation_id::text,
event_type,
conversation_id::text,
turn_id::text,
payload,
delivered_at,
delivered_message_id::text,
created_at
"#;

const DATABASE_OPERATION_DIAGNOSTIC_COLUMNS: &str = r#"
id::text,
owner_user_id::text,
operation_kind,
operation_id::text,
phase,
message,
command_name,
exit_code,
stdout,
stderr,
stdout_truncated,
stderr_truncated,
created_at
"#;

pub(crate) async fn create_database_backup(
    storage: &Storage,
    owner_user_id: &str,
    source_managed_database_id: &str,
    purpose: Option<String>,
) -> Result<DatabaseBackupRecord, StorageError> {
    enqueue_database_backup(
        storage,
        owner_user_id,
        EnqueueDatabaseBackup::immediate(
            source_managed_database_id.to_owned(),
            purpose,
            None,
            None,
        ),
    )
    .await
}

pub(crate) async fn enqueue_database_backup(
    storage: &Storage,
    owner_user_id: &str,
    request: EnqueueDatabaseBackup,
) -> Result<DatabaseBackupRecord, StorageError> {
    let source =
        load_managed_database_snapshot(storage, owner_user_id, &request.managed_database_id)
            .await?;
    let purpose = optional_string("purpose", request.purpose)?;
    let conversation_id = optional_string("conversation_id", request.conversation_id)?;
    let created_from_turn_id =
        optional_string("created_from_turn_id", request.created_from_turn_id)?;

    let row = sqlx::query_as::<_, DatabaseBackupRow>(&format!(
        r#"
        insert into database_backups (
            owner_user_id,
            source_managed_database_id,
            source_managed_database_name,
            source_managed_database_engine,
            source_managed_database_host,
            source_managed_database_port,
            source_managed_database_database,
            source_managed_database_username,
            source_managed_database_ssl_mode,
            format,
            status,
            phase,
            progress_percent,
            schedule_id,
            trigger,
            scheduled_for,
            conversation_id,
            created_from_turn_id,
            purpose
        )
        values (
            $1::uuid, $2::uuid, $3, $4, $5, $6, $7, $8, $9,
            'postgres_custom', 'queued', 'queued', 0, $10::uuid, $11, $12, $13::uuid, $14::uuid, $15
        )
        returning {DATABASE_BACKUP_COLUMNS}
        "#
    ))
    .bind(owner_user_id)
    .bind(&source.id)
    .bind(&source.name)
    .bind(source.engine.as_str())
    .bind(&source.host)
    .bind(source.port)
    .bind(&source.database)
    .bind(&source.username)
    .bind(source.ssl_mode.as_str())
    .bind(request.schedule_id)
    .bind(request.trigger.as_str())
    .bind(request.scheduled_for)
    .bind(conversation_id)
    .bind(created_from_turn_id)
    .bind(purpose)
    .fetch_one(&storage.pool)
    .await
    .map_err(map_database_error)?;

    let backup = DatabaseBackupRecord::try_from(row)?;
    let _ = append_database_operation_event(
        storage,
        DatabaseOperationKind::Backup,
        &backup.id,
        DatabaseOperationEventType::Queued,
        serde_json::json!({ "backup": backup }),
    )
    .await;

    Ok(backup)
}

pub(crate) async fn get_database_backup(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<DatabaseBackupRecord, StorageError> {
    fetch_database_backup(storage, Some(owner_user_id), id).await
}

pub(crate) async fn list_database_backups(
    storage: &Storage,
    owner_user_id: &str,
    source_managed_database_id: Option<&str>,
    status: Option<DatabaseBackupStatus>,
    limit: i64,
) -> Result<Vec<DatabaseBackupRecord>, StorageError> {
    let limit = limit.clamp(1, 100);
    let rows = sqlx::query_as::<_, DatabaseBackupRow>(&format!(
        r#"
        select {DATABASE_BACKUP_COLUMNS}
        from database_backups
        where owner_user_id = $1::uuid
          and ($2::uuid is null or source_managed_database_id = $2::uuid)
          and ($3::text is null or status = $3)
        order by created_at desc
        limit $4
        "#
    ))
    .bind(owner_user_id)
    .bind(source_managed_database_id)
    .bind(status.map(DatabaseBackupStatus::as_str))
    .bind(limit)
    .fetch_all(&storage.pool)
    .await
    .map_err(map_database_error)?;

    rows.into_iter()
        .map(DatabaseBackupRecord::try_from)
        .collect()
}

pub(crate) async fn list_database_backups_page(
    storage: &Storage,
    owner_user_id: &str,
    filters: DatabaseBackupListFilters<'_>,
) -> Result<DatabaseBackupListPage, StorageError> {
    let page = filters.page.max(1);
    let page_size = filters.page_size.clamp(1, 100);
    let offset = (page - 1) * page_size;
    let status = filters.status.map(DatabaseBackupStatus::as_str);
    let trigger = filters.trigger.map(DatabaseBackupTrigger::as_str);
    let total_count = sqlx::query_scalar::<_, i64>(
        r#"
        select count(*)
        from database_backups
        where owner_user_id = $1::uuid
          and ($2::uuid is null or source_managed_database_id = $2::uuid)
          and ($3::text is null or status = $3)
          and ($4::text is null or trigger = $4)
        "#,
    )
    .bind(owner_user_id)
    .bind(filters.source_managed_database_id)
    .bind(status)
    .bind(trigger)
    .fetch_one(&storage.pool)
    .await
    .map_err(map_database_error)?;

    let rows = sqlx::query_as::<_, DatabaseBackupRow>(&format!(
        r#"
        select {DATABASE_BACKUP_COLUMNS}
        from database_backups
        where owner_user_id = $1::uuid
          and ($2::uuid is null or source_managed_database_id = $2::uuid)
          and ($3::text is null or status = $3)
          and ($4::text is null or trigger = $4)
        order by created_at desc
        limit $5
        offset $6
        "#
    ))
    .bind(owner_user_id)
    .bind(filters.source_managed_database_id)
    .bind(status)
    .bind(trigger)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&storage.pool)
    .await
    .map_err(map_database_error)?;
    let records = rows
        .into_iter()
        .map(DatabaseBackupRecord::try_from)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(DatabaseBackupListPage {
        records,
        total_count,
        page,
        page_size,
    })
}

pub(crate) async fn delete_database_backup(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<DatabaseBackupRecord, StorageError> {
    let row = sqlx::query_as::<_, DatabaseBackupRow>(&format!(
        r#"
        update database_backups
        set status = 'deleted',
            phase = 'deleted',
            progress_percent = 100,
            updated_at = now(),
            completed_at = coalesce(completed_at, now())
        where id = $1::uuid
          and owner_user_id = $2::uuid
          and status <> 'running'
        returning {DATABASE_BACKUP_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(owner_user_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    let Some(row) = row else {
        let existing = fetch_database_backup(storage, Some(owner_user_id), id).await?;
        if existing.status == DatabaseBackupStatus::Running {
            return Err(StorageError::Conflict(
                "running database backups cannot be deleted".to_owned(),
            ));
        }
        return Err(StorageError::NotFound);
    };

    row.try_into()
}

pub(crate) async fn create_database_restore(
    storage: &Storage,
    owner_user_id: &str,
    backup_id: &str,
    target_managed_database_id: &str,
    purpose: String,
) -> Result<DatabaseRestoreRecord, StorageError> {
    enqueue_database_restore(
        storage,
        owner_user_id,
        EnqueueDatabaseRestore {
            backup_id: backup_id.to_owned(),
            target_managed_database_id: target_managed_database_id.to_owned(),
            purpose,
            conversation_id: None,
            created_from_turn_id: None,
        },
    )
    .await
}

pub(crate) async fn enqueue_database_restore(
    storage: &Storage,
    owner_user_id: &str,
    request: EnqueueDatabaseRestore,
) -> Result<DatabaseRestoreRecord, StorageError> {
    let backup = get_database_backup(storage, owner_user_id, &request.backup_id).await?;
    if backup.status != DatabaseBackupStatus::Succeeded {
        return Err(StorageError::Conflict(
            "only succeeded database backups can be restored".to_owned(),
        ));
    }
    let target =
        load_managed_database_snapshot(storage, owner_user_id, &request.target_managed_database_id)
            .await?;
    let purpose = required_string("purpose", &request.purpose)?;
    let conversation_id = optional_string("conversation_id", request.conversation_id)?;
    let created_from_turn_id =
        optional_string("created_from_turn_id", request.created_from_turn_id)?;

    let row = sqlx::query_as::<_, DatabaseRestoreRow>(&format!(
        r#"
        insert into database_restore_jobs (
            owner_user_id,
            backup_id,
            target_managed_database_id,
            target_managed_database_name,
            target_managed_database_engine,
            target_managed_database_host,
            target_managed_database_port,
            target_managed_database_database,
            target_managed_database_username,
            target_managed_database_ssl_mode,
            format,
            restore_options,
            status,
            phase,
            progress_percent,
            conversation_id,
            created_from_turn_id,
            purpose
        )
        values (
            $1::uuid, $2::uuid, $3::uuid, $4, $5, $6, $7, $8, $9, $10,
            'postgres_custom', '{{}}'::jsonb, 'queued', 'queued', 0, $11::uuid, $12::uuid, $13
        )
        returning {DATABASE_RESTORE_COLUMNS}
        "#
    ))
    .bind(owner_user_id)
    .bind(&backup.id)
    .bind(&target.id)
    .bind(&target.name)
    .bind(target.engine.as_str())
    .bind(&target.host)
    .bind(target.port)
    .bind(&target.database)
    .bind(&target.username)
    .bind(target.ssl_mode.as_str())
    .bind(conversation_id)
    .bind(created_from_turn_id)
    .bind(purpose)
    .fetch_one(&storage.pool)
    .await
    .map_err(map_database_error)?;

    let restore = DatabaseRestoreRecord::try_from(row)?;
    let _ = append_database_operation_event(
        storage,
        DatabaseOperationKind::Restore,
        &restore.id,
        DatabaseOperationEventType::Queued,
        serde_json::json!({ "restore": restore }),
    )
    .await;

    Ok(restore)
}

pub(crate) async fn get_database_restore(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<DatabaseRestoreRecord, StorageError> {
    fetch_database_restore(storage, Some(owner_user_id), id).await
}

pub(crate) async fn list_database_restores(
    storage: &Storage,
    owner_user_id: &str,
    backup_id: Option<&str>,
    target_managed_database_id: Option<&str>,
    status: Option<DatabaseBackupStatus>,
    limit: i64,
) -> Result<Vec<DatabaseRestoreRecord>, StorageError> {
    let limit = limit.clamp(1, 100);
    let rows = sqlx::query_as::<_, DatabaseRestoreRow>(&format!(
        r#"
        select {DATABASE_RESTORE_COLUMNS}
        from database_restore_jobs
        where owner_user_id = $1::uuid
          and ($2::uuid is null or backup_id = $2::uuid)
          and ($3::uuid is null or target_managed_database_id = $3::uuid)
          and ($4::text is null or status = $4)
        order by created_at desc
        limit $5
        "#
    ))
    .bind(owner_user_id)
    .bind(backup_id)
    .bind(target_managed_database_id)
    .bind(status.map(DatabaseBackupStatus::as_str))
    .bind(limit)
    .fetch_all(&storage.pool)
    .await
    .map_err(map_database_error)?;

    rows.into_iter()
        .map(DatabaseRestoreRecord::try_from)
        .collect()
}

pub(crate) async fn claim_next_database_backup(
    storage: &Storage,
    worker_id: &str,
) -> Result<Option<DatabaseBackupRecord>, StorageError> {
    let worker_id = required_string("worker_id", worker_id)?;
    let row = sqlx::query_as::<_, DatabaseBackupRow>(&format!(
        r#"
        update database_backups
        set status = 'running',
            phase = 'claimed',
            progress_percent = greatest(progress_percent, 1),
            worker_id = $1,
            heartbeat_at = now(),
            started_at = coalesce(started_at, now()),
            error = null,
            updated_at = now()
        where id = (
            select id
            from database_backups
            where status = 'queued'
            order by created_at
            for update skip locked
            limit 1
        )
        returning {DATABASE_BACKUP_COLUMNS}
        "#
    ))
    .bind(worker_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.map(DatabaseBackupRecord::try_from).transpose()
}

pub(crate) async fn update_database_backup_progress(
    storage: &Storage,
    id: &str,
    phase: &str,
    progress_percent: i32,
) -> Result<DatabaseBackupRecord, StorageError> {
    let phase = required_string("phase", phase)?;
    let progress_percent = progress_percent.clamp(0, 100);
    let row = update_backup_progress_row(storage, id, &phase, progress_percent).await?;

    row.try_into()
}

pub(crate) async fn complete_database_backup(
    storage: &Storage,
    id: &str,
    result: CompleteDatabaseBackup,
) -> Result<DatabaseBackupRecord, StorageError> {
    let row = sqlx::query_as::<_, DatabaseBackupRow>(&format!(
        r#"
        update database_backups
        set status = 'succeeded',
            phase = 'succeeded',
            progress_percent = 100,
            storage_kind = $2,
            local_path = $3,
            s3_bucket = $4,
            s3_key = $5,
            s3_version_id = $6,
            s3_etag = $7,
            size_bytes = $8,
            checksum_sha256 = $9,
            postgres_server_version = $10,
            pg_dump_version = $11,
            heartbeat_at = now(),
            completed_at = now(),
            error = null,
            updated_at = now()
        where id = $1::uuid
          and status = 'running'
        returning {DATABASE_BACKUP_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(result.storage_kind.as_str())
    .bind(result.local_path)
    .bind(result.bucket)
    .bind(result.key)
    .bind(result.version_id)
    .bind(result.etag)
    .bind(result.size_bytes)
    .bind(result.checksum_sha256)
    .bind(result.postgres_server_version)
    .bind(result.pg_dump_version)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or_else(|| {
        StorageError::Conflict("only running database backups can be completed".to_owned())
    })?
    .try_into()
}

pub(crate) async fn fail_database_backup(
    storage: &Storage,
    id: &str,
    error: String,
) -> Result<DatabaseBackupRecord, StorageError> {
    let error = required_string("error", &error)?;
    let row = sqlx::query_as::<_, DatabaseBackupRow>(&format!(
        r#"
        update database_backups
        set status = 'failed',
            phase = 'failed',
            error = $2,
            heartbeat_at = now(),
            completed_at = now(),
            updated_at = now()
        where id = $1::uuid
          and status in ('queued', 'running')
        returning {DATABASE_BACKUP_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(error)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or_else(|| {
        StorageError::Conflict("only queued or running database backups can fail".to_owned())
    })?
    .try_into()
}

pub(crate) async fn claim_next_database_restore(
    storage: &Storage,
    worker_id: &str,
) -> Result<Option<DatabaseRestoreRecord>, StorageError> {
    let worker_id = required_string("worker_id", worker_id)?;
    let row = sqlx::query_as::<_, DatabaseRestoreRow>(&format!(
        r#"
        update database_restore_jobs
        set status = 'running',
            phase = 'claimed',
            progress_percent = greatest(progress_percent, 1),
            worker_id = $1,
            heartbeat_at = now(),
            started_at = coalesce(started_at, now()),
            error = null,
            updated_at = now()
        where id = (
            select id
            from database_restore_jobs
            where status = 'queued'
            order by created_at
            for update skip locked
            limit 1
        )
        returning {DATABASE_RESTORE_COLUMNS}
        "#
    ))
    .bind(worker_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.map(DatabaseRestoreRecord::try_from).transpose()
}

pub(crate) async fn update_database_restore_progress(
    storage: &Storage,
    id: &str,
    phase: &str,
    progress_percent: i32,
) -> Result<DatabaseRestoreRecord, StorageError> {
    let phase = required_string("phase", phase)?;
    let progress_percent = progress_percent.clamp(0, 100);
    let row = sqlx::query_as::<_, DatabaseRestoreRow>(&format!(
        r#"
        update database_restore_jobs
        set phase = $2,
            progress_percent = $3,
            heartbeat_at = now(),
            updated_at = now()
        where id = $1::uuid
          and status = 'running'
        returning {DATABASE_RESTORE_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(phase)
    .bind(progress_percent)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or_else(|| {
        StorageError::Conflict("only running database restore jobs can update progress".to_owned())
    })?
    .try_into()
}

pub(crate) async fn complete_database_restore(
    storage: &Storage,
    id: &str,
) -> Result<DatabaseRestoreRecord, StorageError> {
    let row = sqlx::query_as::<_, DatabaseRestoreRow>(&format!(
        r#"
        update database_restore_jobs
        set status = 'succeeded',
            phase = 'succeeded',
            progress_percent = 100,
            heartbeat_at = now(),
            completed_at = now(),
            error = null,
            updated_at = now()
        where id = $1::uuid
          and status = 'running'
        returning {DATABASE_RESTORE_COLUMNS}
        "#
    ))
    .bind(id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or_else(|| {
        StorageError::Conflict("only running database restore jobs can be completed".to_owned())
    })?
    .try_into()
}

pub(crate) async fn fail_database_restore(
    storage: &Storage,
    id: &str,
    error: String,
) -> Result<DatabaseRestoreRecord, StorageError> {
    let error = required_string("error", &error)?;
    let row = sqlx::query_as::<_, DatabaseRestoreRow>(&format!(
        r#"
        update database_restore_jobs
        set status = 'failed',
            phase = 'failed',
            error = $2,
            heartbeat_at = now(),
            completed_at = now(),
            updated_at = now()
        where id = $1::uuid
          and status in ('queued', 'running')
        returning {DATABASE_RESTORE_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(error)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or_else(|| {
        StorageError::Conflict("only queued or running database restore jobs can fail".to_owned())
    })?
    .try_into()
}

pub(crate) async fn fail_stale_database_jobs(
    storage: &Storage,
    stale_after_seconds: i64,
) -> Result<u64, StorageError> {
    let stale_after_seconds = stale_after_seconds.max(1);
    let backup_result = sqlx::query(
        r#"
        update database_backups
        set status = 'failed',
            phase = 'failed',
            error = 'database backup worker heartbeat expired',
            completed_at = now(),
            updated_at = now()
        where status = 'running'
          and heartbeat_at < now() - ($1::text || ' seconds')::interval
        "#,
    )
    .bind(stale_after_seconds)
    .execute(&storage.pool)
    .await
    .map_err(map_database_error)?;
    let restore_result = sqlx::query(
        r#"
        update database_restore_jobs
        set status = 'failed',
            phase = 'failed',
            error = 'database restore worker heartbeat expired',
            completed_at = now(),
            updated_at = now()
        where status = 'running'
          and heartbeat_at < now() - ($1::text || ' seconds')::interval
        "#,
    )
    .bind(stale_after_seconds)
    .execute(&storage.pool)
    .await
    .map_err(map_database_error)?;

    Ok(backup_result.rows_affected() + restore_result.rows_affected())
}

pub(crate) async fn create_database_backup_schedule(
    storage: &Storage,
    owner_user_id: &str,
    request: CreateDatabaseBackupScheduleRequest,
    conversation_id: Option<String>,
    created_from_turn_id: Option<String>,
    next_run_at: OffsetDateTime,
) -> Result<DatabaseBackupScheduleRecord, StorageError> {
    let source =
        load_managed_database_snapshot(storage, owner_user_id, &request.managed_database_id)
            .await?;
    let cron_expression = required_string("cron_expression", &request.cron_expression)?;
    let timezone = required_string("timezone", request.timezone.as_deref().unwrap_or("UTC"))?;
    let purpose = optional_string("purpose", request.purpose)?;
    let conversation_id = optional_string("conversation_id", conversation_id)?;
    let created_from_turn_id = optional_string("created_from_turn_id", created_from_turn_id)?;
    let keep_last = positive_optional_i32("keep_last", request.keep_last)?;
    let retention_days = positive_optional_i32("retention_days", request.retention_days)?;

    let row = sqlx::query_as::<_, DatabaseBackupScheduleRow>(&format!(
        r#"
        insert into database_backup_schedules (
            owner_user_id,
            source_managed_database_id,
            source_managed_database_name,
            source_managed_database_engine,
            source_managed_database_host,
            source_managed_database_port,
            source_managed_database_database,
            source_managed_database_username,
            source_managed_database_ssl_mode,
            cron_expression,
            timezone,
            status,
            purpose,
            keep_last,
            retention_days,
            conversation_id,
            created_from_turn_id,
            next_run_at
        )
        values (
            $1::uuid, $2::uuid, $3, $4, $5, $6, $7, $8, $9,
            $10, $11, 'active', $12, $13, $14, $15::uuid, $16::uuid, $17
        )
        returning {DATABASE_BACKUP_SCHEDULE_COLUMNS}
        "#
    ))
    .bind(owner_user_id)
    .bind(&source.id)
    .bind(&source.name)
    .bind(source.engine.as_str())
    .bind(&source.host)
    .bind(source.port)
    .bind(&source.database)
    .bind(&source.username)
    .bind(source.ssl_mode.as_str())
    .bind(cron_expression)
    .bind(timezone)
    .bind(purpose)
    .bind(keep_last)
    .bind(retention_days)
    .bind(conversation_id)
    .bind(created_from_turn_id)
    .bind(next_run_at)
    .fetch_one(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.try_into()
}

pub(crate) async fn get_database_backup_schedule(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<DatabaseBackupScheduleRecord, StorageError> {
    fetch_database_backup_schedule(storage, owner_user_id, id).await
}

pub(crate) async fn list_database_backup_schedules(
    storage: &Storage,
    owner_user_id: &str,
    managed_database_id: Option<&str>,
    status: Option<DatabaseBackupScheduleStatus>,
    limit: i64,
) -> Result<Vec<DatabaseBackupScheduleRecord>, StorageError> {
    let rows = sqlx::query_as::<_, DatabaseBackupScheduleRow>(&format!(
        r#"
        select {DATABASE_BACKUP_SCHEDULE_COLUMNS}
        from database_backup_schedules
        where owner_user_id = $1::uuid
          and ($2::uuid is null or source_managed_database_id = $2::uuid)
          and ($3::text is null or status = $3)
        order by created_at desc
        limit $4
        "#
    ))
    .bind(owner_user_id)
    .bind(managed_database_id)
    .bind(status.map(DatabaseBackupScheduleStatus::as_str))
    .bind(limit.clamp(1, 100))
    .fetch_all(&storage.pool)
    .await
    .map_err(map_database_error)?;

    rows.into_iter()
        .map(DatabaseBackupScheduleRecord::try_from)
        .collect()
}

pub(crate) async fn update_database_backup_schedule(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
    request: UpdateDatabaseBackupScheduleRequest,
    next_run_at: Option<OffsetDateTime>,
) -> Result<DatabaseBackupScheduleRecord, StorageError> {
    let cron_expression = request
        .cron_expression
        .as_deref()
        .map(|value| required_string("cron_expression", value))
        .transpose()?;
    let timezone = request
        .timezone
        .as_deref()
        .map(|value| required_string("timezone", value))
        .transpose()?;
    let purpose = optional_string("purpose", request.purpose)?;
    let keep_last = positive_optional_i32("keep_last", request.keep_last)?;
    let retention_days = positive_optional_i32("retention_days", request.retention_days)?;

    let row = sqlx::query_as::<_, DatabaseBackupScheduleRow>(&format!(
        r#"
        update database_backup_schedules
        set cron_expression = coalesce($3, cron_expression),
            timezone = coalesce($4, timezone),
            status = coalesce($5, status),
            purpose = coalesce($6, purpose),
            keep_last = coalesce($7, keep_last),
            retention_days = coalesce($8, retention_days),
            next_run_at = coalesce($9, next_run_at),
            updated_at = now()
        where id = $1::uuid
          and owner_user_id = $2::uuid
          and status <> 'deleted'
        returning {DATABASE_BACKUP_SCHEDULE_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(owner_user_id)
    .bind(cron_expression)
    .bind(timezone)
    .bind(request.status.map(DatabaseBackupScheduleStatus::as_str))
    .bind(purpose)
    .bind(keep_last)
    .bind(retention_days)
    .bind(next_run_at)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

pub(crate) async fn delete_database_backup_schedule(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<DatabaseBackupScheduleRecord, StorageError> {
    let row = sqlx::query_as::<_, DatabaseBackupScheduleRow>(&format!(
        r#"
        update database_backup_schedules
        set status = 'deleted',
            updated_at = now()
        where id = $1::uuid
          and owner_user_id = $2::uuid
        returning {DATABASE_BACKUP_SCHEDULE_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(owner_user_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

pub(crate) async fn claim_due_database_backup_schedule(
    storage: &Storage,
    scheduler_id: &str,
    now: OffsetDateTime,
) -> Result<Option<DatabaseBackupScheduleRecord>, StorageError> {
    let scheduler_id = required_string("scheduler_id", scheduler_id)?;
    let row = sqlx::query_as::<_, DatabaseBackupScheduleRow>(&format!(
        r#"
        update database_backup_schedules
        set scheduler_id = $1,
            claimed_at = now(),
            updated_at = now()
        where id = (
            select id
            from database_backup_schedules
            where status = 'active'
              and next_run_at <= $2
            order by next_run_at, created_at
            for update skip locked
            limit 1
        )
        returning {DATABASE_BACKUP_SCHEDULE_COLUMNS}
        "#
    ))
    .bind(scheduler_id)
    .bind(now)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.map(DatabaseBackupScheduleRecord::try_from).transpose()
}

pub(crate) async fn complete_database_backup_schedule_enqueue(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
    scheduled_for: OffsetDateTime,
    next_run_at: OffsetDateTime,
) -> Result<DatabaseBackupScheduleRecord, StorageError> {
    let row = sqlx::query_as::<_, DatabaseBackupScheduleRow>(&format!(
        r#"
        update database_backup_schedules
        set last_enqueued_at = $3,
            next_run_at = $4,
            scheduler_id = null,
            claimed_at = null,
            updated_at = now()
        where id = $1::uuid
          and owner_user_id = $2::uuid
        returning {DATABASE_BACKUP_SCHEDULE_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(owner_user_id)
    .bind(scheduled_for)
    .bind(next_run_at)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

pub(crate) async fn append_database_operation_event(
    storage: &Storage,
    operation_kind: DatabaseOperationKind,
    operation_id: &str,
    event_type: DatabaseOperationEventType,
    payload: Value,
) -> Result<DatabaseOperationEventRecord, StorageError> {
    let row = match operation_kind {
        DatabaseOperationKind::Backup => sqlx::query_as::<_, DatabaseOperationEventRow>(&format!(
            r#"
                insert into database_operation_events (
                    owner_user_id,
                    operation_kind,
                    operation_id,
                    event_type,
                    conversation_id,
                    turn_id,
                    payload
                )
                select
                    owner_user_id,
                    $2,
                    id,
                    $3,
                    conversation_id,
                    created_from_turn_id,
                    $4
                from database_backups
                where id = $1::uuid
                on conflict (operation_kind, operation_id, event_type) do update
                set payload = excluded.payload
                returning {DATABASE_OPERATION_EVENT_COLUMNS}
                "#
        ))
        .bind(operation_id)
        .bind(operation_kind.as_str())
        .bind(event_type.as_str())
        .bind(payload)
        .fetch_optional(&storage.pool)
        .await
        .map_err(map_database_error)?,
        DatabaseOperationKind::Restore => sqlx::query_as::<_, DatabaseOperationEventRow>(&format!(
            r#"
                insert into database_operation_events (
                    owner_user_id,
                    operation_kind,
                    operation_id,
                    event_type,
                    conversation_id,
                    turn_id,
                    payload
                )
                select
                    owner_user_id,
                    $2,
                    id,
                    $3,
                    conversation_id,
                    created_from_turn_id,
                    $4
                from database_restore_jobs
                where id = $1::uuid
                on conflict (operation_kind, operation_id, event_type) do update
                set payload = excluded.payload
                returning {DATABASE_OPERATION_EVENT_COLUMNS}
                "#
        ))
        .bind(operation_id)
        .bind(operation_kind.as_str())
        .bind(event_type.as_str())
        .bind(payload)
        .fetch_optional(&storage.pool)
        .await
        .map_err(map_database_error)?,
    };

    row.ok_or(StorageError::NotFound)?.try_into()
}

pub(crate) async fn append_database_operation_diagnostic(
    storage: &Storage,
    owner_user_id: &str,
    diagnostic: AppendDatabaseOperationDiagnostic,
) -> Result<DatabaseOperationDiagnosticRecord, StorageError> {
    let phase = required_string("phase", &diagnostic.phase)?;
    let message = required_string("message", &diagnostic.message)?;
    let command_name = optional_string("command_name", diagnostic.command_name)?;
    let stdout = optional_diagnostic_text("stdout", diagnostic.stdout)?;
    let stderr = optional_diagnostic_text("stderr", diagnostic.stderr)?;

    let row = match diagnostic.operation_kind {
        DatabaseOperationKind::Backup => {
            sqlx::query_as::<_, DatabaseOperationDiagnosticRow>(&format!(
                r#"
                insert into database_operation_diagnostics (
                    owner_user_id,
                    operation_kind,
                    operation_id,
                    phase,
                    message,
                    command_name,
                    exit_code,
                    stdout,
                    stderr,
                    stdout_truncated,
                    stderr_truncated
                )
                select
                    owner_user_id,
                    $3,
                    id,
                    $4,
                    $5,
                    $6,
                    $7,
                    $8,
                    $9,
                    $10,
                    $11
                from database_backups
                where id = $1::uuid
                  and owner_user_id = $2::uuid
                returning {DATABASE_OPERATION_DIAGNOSTIC_COLUMNS}
                "#
            ))
            .bind(&diagnostic.operation_id)
            .bind(owner_user_id)
            .bind(diagnostic.operation_kind.as_str())
            .bind(phase)
            .bind(message)
            .bind(command_name)
            .bind(diagnostic.exit_code)
            .bind(stdout)
            .bind(stderr)
            .bind(diagnostic.stdout_truncated)
            .bind(diagnostic.stderr_truncated)
            .fetch_optional(&storage.pool)
            .await
            .map_err(map_database_error)?
        }
        DatabaseOperationKind::Restore => {
            sqlx::query_as::<_, DatabaseOperationDiagnosticRow>(&format!(
                r#"
                insert into database_operation_diagnostics (
                    owner_user_id,
                    operation_kind,
                    operation_id,
                    phase,
                    message,
                    command_name,
                    exit_code,
                    stdout,
                    stderr,
                    stdout_truncated,
                    stderr_truncated
                )
                select
                    owner_user_id,
                    $3,
                    id,
                    $4,
                    $5,
                    $6,
                    $7,
                    $8,
                    $9,
                    $10,
                    $11
                from database_restore_jobs
                where id = $1::uuid
                  and owner_user_id = $2::uuid
                returning {DATABASE_OPERATION_DIAGNOSTIC_COLUMNS}
                "#
            ))
            .bind(&diagnostic.operation_id)
            .bind(owner_user_id)
            .bind(diagnostic.operation_kind.as_str())
            .bind(phase)
            .bind(message)
            .bind(command_name)
            .bind(diagnostic.exit_code)
            .bind(stdout)
            .bind(stderr)
            .bind(diagnostic.stdout_truncated)
            .bind(diagnostic.stderr_truncated)
            .fetch_optional(&storage.pool)
            .await
            .map_err(map_database_error)?
        }
    };

    row.ok_or(StorageError::NotFound)?.try_into()
}

pub(crate) async fn list_database_operation_diagnostics(
    storage: &Storage,
    owner_user_id: &str,
    filters: DatabaseOperationDiagnosticFilters<'_>,
) -> Result<Vec<DatabaseOperationDiagnosticRecord>, StorageError> {
    let limit = filters.limit.clamp(1, 100);
    let rows = sqlx::query_as::<_, DatabaseOperationDiagnosticRow>(&format!(
        r#"
        select {DATABASE_OPERATION_DIAGNOSTIC_COLUMNS}
        from database_operation_diagnostics
        where owner_user_id = $1::uuid
          and operation_kind = $2
          and operation_id = $3::uuid
        order by created_at desc, id desc
        limit $4
        "#
    ))
    .bind(owner_user_id)
    .bind(filters.operation_kind.as_str())
    .bind(filters.operation_id)
    .bind(limit)
    .fetch_all(&storage.pool)
    .await
    .map_err(map_database_error)?;

    rows.into_iter()
        .map(DatabaseOperationDiagnosticRecord::try_from)
        .collect()
}

pub(crate) async fn claim_next_database_operation_event(
    storage: &Storage,
) -> Result<Option<DatabaseOperationEventRecord>, StorageError> {
    let row = sqlx::query_as::<_, DatabaseOperationEventRow>(&format!(
        r#"
        select {DATABASE_OPERATION_EVENT_COLUMNS}
        from database_operation_events
        where delivered_at is null
          and conversation_id is not null
          and event_type <> 'queued'
        order by created_at, id
        limit 1
        "#
    ))
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.map(DatabaseOperationEventRecord::try_from).transpose()
}

pub(crate) async fn mark_database_operation_event_delivered(
    storage: &Storage,
    event_id: &str,
    delivered_message_id: &str,
) -> Result<DatabaseOperationEventRecord, StorageError> {
    let row = sqlx::query_as::<_, DatabaseOperationEventRow>(&format!(
        r#"
        update database_operation_events
        set delivered_at = now(),
            delivered_message_id = $2::uuid
        where id = $1::uuid
        returning {DATABASE_OPERATION_EVENT_COLUMNS}
        "#
    ))
    .bind(event_id)
    .bind(delivered_message_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

async fn update_backup_progress_row(
    storage: &Storage,
    id: &str,
    phase: &str,
    progress_percent: i32,
) -> Result<DatabaseBackupRow, StorageError> {
    let row = sqlx::query_as::<_, DatabaseBackupRow>(&format!(
        r#"
        update database_backups
        set phase = $2,
            progress_percent = $3,
            heartbeat_at = now(),
            updated_at = now()
        where id = $1::uuid
          and status = 'running'
        returning {DATABASE_BACKUP_COLUMNS}
        "#
    ))
    .bind(id)
    .bind(phase)
    .bind(progress_percent)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or_else(|| {
        StorageError::Conflict("only running database backups can update progress".to_owned())
    })
}

async fn fetch_database_backup(
    storage: &Storage,
    owner_user_id: Option<&str>,
    id: &str,
) -> Result<DatabaseBackupRecord, StorageError> {
    let row = sqlx::query_as::<_, DatabaseBackupRow>(&format!(
        r#"
        select {DATABASE_BACKUP_COLUMNS}
        from database_backups
        where id = $1::uuid
          and ($2::uuid is null or owner_user_id = $2::uuid)
        "#
    ))
    .bind(id)
    .bind(owner_user_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

async fn fetch_database_restore(
    storage: &Storage,
    owner_user_id: Option<&str>,
    id: &str,
) -> Result<DatabaseRestoreRecord, StorageError> {
    let row = sqlx::query_as::<_, DatabaseRestoreRow>(&format!(
        r#"
        select {DATABASE_RESTORE_COLUMNS}
        from database_restore_jobs
        where id = $1::uuid
          and ($2::uuid is null or owner_user_id = $2::uuid)
        "#
    ))
    .bind(id)
    .bind(owner_user_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

async fn fetch_database_backup_schedule(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<DatabaseBackupScheduleRecord, StorageError> {
    let row = sqlx::query_as::<_, DatabaseBackupScheduleRow>(&format!(
        r#"
        select {DATABASE_BACKUP_SCHEDULE_COLUMNS}
        from database_backup_schedules
        where id = $1::uuid
          and owner_user_id = $2::uuid
        "#
    ))
    .bind(id)
    .bind(owner_user_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

fn positive_optional_i32(name: &str, value: Option<i32>) -> Result<Option<i32>, StorageError> {
    if let Some(value) = value
        && value <= 0
    {
        return Err(StorageError::Validation(format!("{name} must be positive")));
    }

    Ok(value)
}

fn optional_diagnostic_text(
    name: &str,
    value: Option<String>,
) -> Result<Option<String>, StorageError> {
    const MAX_DIAGNOSTIC_BYTES: usize = 64 * 1024;

    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > MAX_DIAGNOSTIC_BYTES {
        return Err(StorageError::Validation(format!(
            "{name} must be at most {MAX_DIAGNOSTIC_BYTES} bytes"
        )));
    }

    Ok(Some(value))
}

#[derive(Debug)]
struct DatabaseBackupScheduleRow {
    id: String,
    owner_user_id: String,
    source_managed_database_id: String,
    source_managed_database_name: String,
    source_managed_database_engine: String,
    source_managed_database_host: String,
    source_managed_database_port: i32,
    source_managed_database_database: String,
    source_managed_database_username: String,
    source_managed_database_ssl_mode: String,
    cron_expression: String,
    timezone: String,
    status: String,
    purpose: Option<String>,
    keep_last: Option<i32>,
    retention_days: Option<i32>,
    conversation_id: Option<String>,
    created_from_turn_id: Option<String>,
    last_enqueued_at: Option<OffsetDateTime>,
    next_run_at: OffsetDateTime,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for DatabaseBackupScheduleRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            owner_user_id: row.try_get("owner_user_id")?,
            source_managed_database_id: row.try_get("source_managed_database_id")?,
            source_managed_database_name: row.try_get("source_managed_database_name")?,
            source_managed_database_engine: row.try_get("source_managed_database_engine")?,
            source_managed_database_host: row.try_get("source_managed_database_host")?,
            source_managed_database_port: row.try_get("source_managed_database_port")?,
            source_managed_database_database: row.try_get("source_managed_database_database")?,
            source_managed_database_username: row.try_get("source_managed_database_username")?,
            source_managed_database_ssl_mode: row.try_get("source_managed_database_ssl_mode")?,
            cron_expression: row.try_get("cron_expression")?,
            timezone: row.try_get("timezone")?,
            status: row.try_get("status")?,
            purpose: row.try_get("purpose")?,
            keep_last: row.try_get("keep_last")?,
            retention_days: row.try_get("retention_days")?,
            conversation_id: row.try_get("conversation_id")?,
            created_from_turn_id: row.try_get("created_from_turn_id")?,
            last_enqueued_at: row.try_get("last_enqueued_at")?,
            next_run_at: row.try_get("next_run_at")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

impl TryFrom<DatabaseBackupScheduleRow> for DatabaseBackupScheduleRecord {
    type Error = StorageError;

    fn try_from(row: DatabaseBackupScheduleRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            owner_user_id: row.owner_user_id,
            source: ManagedDatabaseSnapshot {
                id: row.source_managed_database_id,
                name: row.source_managed_database_name,
                engine: parse_engine(&row.source_managed_database_engine)?,
                host: row.source_managed_database_host,
                port: row.source_managed_database_port,
                database: row.source_managed_database_database,
                username: row.source_managed_database_username,
                ssl_mode: parse_ssl_mode(&row.source_managed_database_ssl_mode)?,
            },
            cron_expression: row.cron_expression,
            timezone: row.timezone,
            status: parse_backup_schedule_status(&row.status)?,
            purpose: row.purpose,
            keep_last: row.keep_last,
            retention_days: row.retention_days,
            conversation_id: row.conversation_id,
            created_from_turn_id: row.created_from_turn_id,
            last_enqueued_at: row.last_enqueued_at,
            next_run_at: row.next_run_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(Debug)]
struct DatabaseOperationEventRow {
    id: String,
    owner_user_id: String,
    operation_kind: String,
    operation_id: String,
    event_type: String,
    conversation_id: Option<String>,
    turn_id: Option<String>,
    payload: Value,
    delivered_at: Option<OffsetDateTime>,
    delivered_message_id: Option<String>,
    created_at: OffsetDateTime,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for DatabaseOperationEventRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            owner_user_id: row.try_get("owner_user_id")?,
            operation_kind: row.try_get("operation_kind")?,
            operation_id: row.try_get("operation_id")?,
            event_type: row.try_get("event_type")?,
            conversation_id: row.try_get("conversation_id")?,
            turn_id: row.try_get("turn_id")?,
            payload: row.try_get("payload")?,
            delivered_at: row.try_get("delivered_at")?,
            delivered_message_id: row.try_get("delivered_message_id")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

impl TryFrom<DatabaseOperationEventRow> for DatabaseOperationEventRecord {
    type Error = StorageError;

    fn try_from(row: DatabaseOperationEventRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            owner_user_id: row.owner_user_id,
            operation_kind: parse_database_operation_kind(&row.operation_kind)?,
            operation_id: row.operation_id,
            event_type: parse_database_operation_event_type(&row.event_type)?,
            conversation_id: row.conversation_id,
            turn_id: row.turn_id,
            payload: row.payload,
            delivered_at: row.delivered_at,
            delivered_message_id: row.delivered_message_id,
            created_at: row.created_at,
        })
    }
}

#[derive(Debug)]
struct DatabaseOperationDiagnosticRow {
    id: String,
    owner_user_id: String,
    operation_kind: String,
    operation_id: String,
    phase: String,
    message: String,
    command_name: Option<String>,
    exit_code: Option<i32>,
    stdout: Option<String>,
    stderr: Option<String>,
    stdout_truncated: bool,
    stderr_truncated: bool,
    created_at: OffsetDateTime,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for DatabaseOperationDiagnosticRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            owner_user_id: row.try_get("owner_user_id")?,
            operation_kind: row.try_get("operation_kind")?,
            operation_id: row.try_get("operation_id")?,
            phase: row.try_get("phase")?,
            message: row.try_get("message")?,
            command_name: row.try_get("command_name")?,
            exit_code: row.try_get("exit_code")?,
            stdout: row.try_get("stdout")?,
            stderr: row.try_get("stderr")?,
            stdout_truncated: row.try_get("stdout_truncated")?,
            stderr_truncated: row.try_get("stderr_truncated")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

impl TryFrom<DatabaseOperationDiagnosticRow> for DatabaseOperationDiagnosticRecord {
    type Error = StorageError;

    fn try_from(row: DatabaseOperationDiagnosticRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            owner_user_id: row.owner_user_id,
            operation_kind: parse_database_operation_kind(&row.operation_kind)?,
            operation_id: row.operation_id,
            phase: row.phase,
            message: row.message,
            command_name: row.command_name,
            exit_code: row.exit_code,
            stdout: row.stdout,
            stderr: row.stderr,
            stdout_truncated: row.stdout_truncated,
            stderr_truncated: row.stderr_truncated,
            created_at: row.created_at,
        })
    }
}

#[derive(Debug)]
struct DatabaseBackupRow {
    id: String,
    owner_user_id: String,
    source_managed_database_id: String,
    source_managed_database_name: String,
    source_managed_database_engine: String,
    source_managed_database_host: String,
    source_managed_database_port: i32,
    source_managed_database_database: String,
    source_managed_database_username: String,
    source_managed_database_ssl_mode: String,
    format: String,
    storage_kind: Option<String>,
    local_path: Option<String>,
    s3_bucket: Option<String>,
    s3_key: Option<String>,
    s3_version_id: Option<String>,
    s3_etag: Option<String>,
    size_bytes: Option<i64>,
    checksum_sha256: Option<String>,
    postgres_server_version: Option<String>,
    pg_dump_version: Option<String>,
    status: String,
    phase: String,
    progress_percent: i32,
    schedule_id: Option<String>,
    trigger: String,
    scheduled_for: Option<OffsetDateTime>,
    conversation_id: Option<String>,
    created_from_turn_id: Option<String>,
    worker_id: Option<String>,
    heartbeat_at: Option<OffsetDateTime>,
    started_at: Option<OffsetDateTime>,
    completed_at: Option<OffsetDateTime>,
    error: Option<String>,
    purpose: Option<String>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for DatabaseBackupRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            owner_user_id: row.try_get("owner_user_id")?,
            source_managed_database_id: row.try_get("source_managed_database_id")?,
            source_managed_database_name: row.try_get("source_managed_database_name")?,
            source_managed_database_engine: row.try_get("source_managed_database_engine")?,
            source_managed_database_host: row.try_get("source_managed_database_host")?,
            source_managed_database_port: row.try_get("source_managed_database_port")?,
            source_managed_database_database: row.try_get("source_managed_database_database")?,
            source_managed_database_username: row.try_get("source_managed_database_username")?,
            source_managed_database_ssl_mode: row.try_get("source_managed_database_ssl_mode")?,
            format: row.try_get("format")?,
            storage_kind: row.try_get("storage_kind")?,
            local_path: row.try_get("local_path")?,
            s3_bucket: row.try_get("s3_bucket")?,
            s3_key: row.try_get("s3_key")?,
            s3_version_id: row.try_get("s3_version_id")?,
            s3_etag: row.try_get("s3_etag")?,
            size_bytes: row.try_get("size_bytes")?,
            checksum_sha256: row.try_get("checksum_sha256")?,
            postgres_server_version: row.try_get("postgres_server_version")?,
            pg_dump_version: row.try_get("pg_dump_version")?,
            status: row.try_get("status")?,
            phase: row.try_get("phase")?,
            progress_percent: row.try_get("progress_percent")?,
            schedule_id: row
                .try_get::<Option<String>, _>("schedule_id")
                .ok()
                .flatten(),
            trigger: row
                .try_get::<String, _>("trigger")
                .unwrap_or_else(|_| DatabaseBackupTrigger::Immediate.as_str().to_owned()),
            scheduled_for: row
                .try_get::<Option<OffsetDateTime>, _>("scheduled_for")
                .ok()
                .flatten(),
            conversation_id: row
                .try_get::<Option<String>, _>("conversation_id")
                .ok()
                .flatten(),
            created_from_turn_id: row
                .try_get::<Option<String>, _>("created_from_turn_id")
                .ok()
                .flatten(),
            worker_id: row.try_get("worker_id")?,
            heartbeat_at: row.try_get("heartbeat_at")?,
            started_at: row.try_get("started_at")?,
            completed_at: row.try_get("completed_at")?,
            error: row.try_get("error")?,
            purpose: row.try_get("purpose")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

impl TryFrom<DatabaseBackupRow> for DatabaseBackupRecord {
    type Error = StorageError;

    fn try_from(row: DatabaseBackupRow) -> Result<Self, Self::Error> {
        let storage = database_backup_storage_metadata(
            row.storage_kind,
            row.local_path,
            row.s3_bucket,
            row.s3_key,
            row.s3_version_id,
            row.s3_etag,
            row.size_bytes,
            row.checksum_sha256,
        )?;

        Ok(Self {
            id: row.id,
            owner_user_id: row.owner_user_id,
            source: ManagedDatabaseSnapshot {
                id: row.source_managed_database_id,
                name: row.source_managed_database_name,
                engine: parse_engine(&row.source_managed_database_engine)?,
                host: row.source_managed_database_host,
                port: row.source_managed_database_port,
                database: row.source_managed_database_database,
                username: row.source_managed_database_username,
                ssl_mode: parse_ssl_mode(&row.source_managed_database_ssl_mode)?,
            },
            format: parse_backup_format(&row.format)?,
            status: parse_backup_status(&row.status)?,
            phase: row.phase,
            progress_percent: row.progress_percent,
            schedule_id: row.schedule_id,
            trigger: parse_backup_trigger(&row.trigger)?,
            scheduled_for: row.scheduled_for,
            conversation_id: row.conversation_id,
            created_from_turn_id: row.created_from_turn_id,
            storage,
            postgres_server_version: row.postgres_server_version,
            pg_dump_version: row.pg_dump_version,
            error: row.error,
            purpose: row.purpose,
            worker_id: row.worker_id,
            heartbeat_at: row.heartbeat_at,
            started_at: row.started_at,
            completed_at: row.completed_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(Debug)]
struct DatabaseRestoreRow {
    id: String,
    owner_user_id: String,
    backup_id: String,
    target_managed_database_id: String,
    target_managed_database_name: String,
    target_managed_database_engine: String,
    target_managed_database_host: String,
    target_managed_database_port: i32,
    target_managed_database_database: String,
    target_managed_database_username: String,
    target_managed_database_ssl_mode: String,
    format: String,
    restore_options: Value,
    status: String,
    phase: String,
    progress_percent: i32,
    conversation_id: Option<String>,
    created_from_turn_id: Option<String>,
    worker_id: Option<String>,
    heartbeat_at: Option<OffsetDateTime>,
    started_at: Option<OffsetDateTime>,
    completed_at: Option<OffsetDateTime>,
    error: Option<String>,
    purpose: String,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for DatabaseRestoreRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            owner_user_id: row.try_get("owner_user_id")?,
            backup_id: row.try_get("backup_id")?,
            target_managed_database_id: row.try_get("target_managed_database_id")?,
            target_managed_database_name: row.try_get("target_managed_database_name")?,
            target_managed_database_engine: row.try_get("target_managed_database_engine")?,
            target_managed_database_host: row.try_get("target_managed_database_host")?,
            target_managed_database_port: row.try_get("target_managed_database_port")?,
            target_managed_database_database: row.try_get("target_managed_database_database")?,
            target_managed_database_username: row.try_get("target_managed_database_username")?,
            target_managed_database_ssl_mode: row.try_get("target_managed_database_ssl_mode")?,
            format: row.try_get("format")?,
            restore_options: row.try_get("restore_options")?,
            status: row.try_get("status")?,
            phase: row.try_get("phase")?,
            progress_percent: row.try_get("progress_percent")?,
            conversation_id: row
                .try_get::<Option<String>, _>("conversation_id")
                .ok()
                .flatten(),
            created_from_turn_id: row
                .try_get::<Option<String>, _>("created_from_turn_id")
                .ok()
                .flatten(),
            worker_id: row.try_get("worker_id")?,
            heartbeat_at: row.try_get("heartbeat_at")?,
            started_at: row.try_get("started_at")?,
            completed_at: row.try_get("completed_at")?,
            error: row.try_get("error")?,
            purpose: row.try_get("purpose")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

impl TryFrom<DatabaseRestoreRow> for DatabaseRestoreRecord {
    type Error = StorageError;

    fn try_from(row: DatabaseRestoreRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            owner_user_id: row.owner_user_id,
            backup_id: row.backup_id,
            target: ManagedDatabaseSnapshot {
                id: row.target_managed_database_id,
                name: row.target_managed_database_name,
                engine: parse_engine(&row.target_managed_database_engine)?,
                host: row.target_managed_database_host,
                port: row.target_managed_database_port,
                database: row.target_managed_database_database,
                username: row.target_managed_database_username,
                ssl_mode: parse_ssl_mode(&row.target_managed_database_ssl_mode)?,
            },
            format: parse_backup_format(&row.format)?,
            status: parse_backup_status(&row.status)?,
            phase: row.phase,
            progress_percent: row.progress_percent,
            restore_options: row.restore_options,
            conversation_id: row.conversation_id,
            created_from_turn_id: row.created_from_turn_id,
            error: row.error,
            purpose: Some(row.purpose),
            worker_id: row.worker_id,
            heartbeat_at: row.heartbeat_at,
            started_at: row.started_at,
            completed_at: row.completed_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

fn parse_backup_trigger(value: &str) -> Result<DatabaseBackupTrigger, StorageError> {
    match value {
        "immediate" => Ok(DatabaseBackupTrigger::Immediate),
        "cron" => Ok(DatabaseBackupTrigger::Cron),
        other => Err(StorageError::Validation(format!(
            "unsupported database backup trigger: {other}"
        ))),
    }
}

fn parse_backup_schedule_status(value: &str) -> Result<DatabaseBackupScheduleStatus, StorageError> {
    match value {
        "active" => Ok(DatabaseBackupScheduleStatus::Active),
        "paused" => Ok(DatabaseBackupScheduleStatus::Paused),
        "deleted" => Ok(DatabaseBackupScheduleStatus::Deleted),
        other => Err(StorageError::Validation(format!(
            "unsupported database backup schedule status: {other}"
        ))),
    }
}

fn parse_database_operation_kind(value: &str) -> Result<DatabaseOperationKind, StorageError> {
    match value {
        "backup" => Ok(DatabaseOperationKind::Backup),
        "restore" => Ok(DatabaseOperationKind::Restore),
        other => Err(StorageError::Validation(format!(
            "unsupported database operation kind: {other}"
        ))),
    }
}

fn parse_database_operation_event_type(
    value: &str,
) -> Result<DatabaseOperationEventType, StorageError> {
    match value {
        "queued" => Ok(DatabaseOperationEventType::Queued),
        "succeeded" => Ok(DatabaseOperationEventType::Succeeded),
        "failed" => Ok(DatabaseOperationEventType::Failed),
        other => Err(StorageError::Validation(format!(
            "unsupported database operation event type: {other}"
        ))),
    }
}

fn parse_backup_format(value: &str) -> Result<DatabaseBackupFormat, StorageError> {
    match value {
        "postgres_custom" => Ok(DatabaseBackupFormat::PostgresCustom),
        other => Err(StorageError::Validation(format!(
            "unsupported database backup format: {other}"
        ))),
    }
}

fn parse_backup_status(value: &str) -> Result<DatabaseBackupStatus, StorageError> {
    match value {
        "queued" => Ok(DatabaseBackupStatus::Queued),
        "running" => Ok(DatabaseBackupStatus::Running),
        "succeeded" => Ok(DatabaseBackupStatus::Succeeded),
        "failed" => Ok(DatabaseBackupStatus::Failed),
        "deleted" => Ok(DatabaseBackupStatus::Deleted),
        other => Err(StorageError::Validation(format!(
            "unsupported database backup status: {other}"
        ))),
    }
}

fn parse_backup_storage_kind(value: &str) -> Result<DatabaseBackupStorageKind, StorageError> {
    match value {
        "local" => Ok(DatabaseBackupStorageKind::Local),
        "s3" => Ok(DatabaseBackupStorageKind::S3),
        other => Err(StorageError::Validation(format!(
            "unsupported database backup storage kind: {other}"
        ))),
    }
}

fn database_backup_storage_metadata(
    storage_kind: Option<String>,
    local_path: Option<String>,
    bucket: Option<String>,
    key: Option<String>,
    version_id: Option<String>,
    etag: Option<String>,
    size_bytes: Option<i64>,
    checksum_sha256: Option<String>,
) -> Result<Option<DatabaseBackupStorageMetadata>, StorageError> {
    let Some(kind) = storage_kind else {
        return Ok(None);
    };
    let kind = parse_backup_storage_kind(&kind)?;

    match kind {
        DatabaseBackupStorageKind::Local => {
            let Some(local_path) = local_path else {
                return Ok(None);
            };
            Ok(Some(DatabaseBackupStorageMetadata {
                kind,
                local_path: Some(local_path),
                bucket: None,
                key: None,
                version_id: None,
                etag: None,
                size_bytes,
                checksum_sha256,
            }))
        }
        DatabaseBackupStorageKind::S3 => {
            let (Some(bucket), Some(key)) = (bucket, key) else {
                return Ok(None);
            };
            Ok(Some(DatabaseBackupStorageMetadata {
                kind,
                local_path,
                bucket: Some(bucket),
                key: Some(key),
                version_id,
                etag,
                size_bytes,
                checksum_sha256,
            }))
        }
    }
}

pub(crate) fn metadata_store_error(error: StorageError) -> DatabaseBackupMetadataStoreError {
    match error {
        StorageError::NotFound => DatabaseBackupMetadataStoreError::NotFound,
        StorageError::Conflict(message) => DatabaseBackupMetadataStoreError::Conflict(message),
        StorageError::Validation(message) => DatabaseBackupMetadataStoreError::Validation(message),
        StorageError::Database(error) => {
            DatabaseBackupMetadataStoreError::Backend(error.to_string())
        }
        StorageError::Crypto(message) => DatabaseBackupMetadataStoreError::Backend(message),
        other => DatabaseBackupMetadataStoreError::Backend(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_database_backup_status_values() {
        assert_eq!(
            parse_backup_status("queued").unwrap(),
            DatabaseBackupStatus::Queued
        );
        assert!(parse_backup_status("unknown").is_err());
    }

    #[test]
    fn backup_record_omits_incomplete_storage_metadata() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let record = DatabaseBackupRecord::try_from(DatabaseBackupRow {
            id: "backup-1".to_owned(),
            owner_user_id: "user-1".to_owned(),
            source_managed_database_id: "db-1".to_owned(),
            source_managed_database_name: "Warehouse".to_owned(),
            source_managed_database_engine: "postgres".to_owned(),
            source_managed_database_host: "localhost".to_owned(),
            source_managed_database_port: 5432,
            source_managed_database_database: "warehouse".to_owned(),
            source_managed_database_username: "postgres".to_owned(),
            source_managed_database_ssl_mode: "prefer".to_owned(),
            format: "postgres_custom".to_owned(),
            storage_kind: Some("s3".to_owned()),
            local_path: None,
            s3_bucket: Some("bucket".to_owned()),
            s3_key: None,
            s3_version_id: None,
            s3_etag: None,
            size_bytes: None,
            checksum_sha256: None,
            postgres_server_version: None,
            pg_dump_version: None,
            status: "queued".to_owned(),
            phase: "queued".to_owned(),
            progress_percent: 0,
            schedule_id: None,
            trigger: "immediate".to_owned(),
            scheduled_for: None,
            conversation_id: None,
            created_from_turn_id: None,
            worker_id: None,
            heartbeat_at: None,
            started_at: None,
            completed_at: None,
            error: None,
            purpose: None,
            created_at: now,
            updated_at: now,
        })
        .unwrap();

        assert!(record.storage.is_none());
    }
}
