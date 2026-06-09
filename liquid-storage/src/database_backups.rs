use liquid_core::{
    CompleteDatabaseBackup, DatabaseBackupFormat, DatabaseBackupMetadataStoreError,
    DatabaseBackupRecord, DatabaseBackupStatus, DatabaseBackupStorageKind,
    DatabaseBackupStorageMetadata, DatabaseRestoreRecord, ManagedDatabaseSnapshot,
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

pub(crate) async fn create_database_backup(
    storage: &Storage,
    owner_user_id: &str,
    source_managed_database_id: &str,
    purpose: Option<String>,
) -> Result<DatabaseBackupRecord, StorageError> {
    let source =
        load_managed_database_snapshot(storage, owner_user_id, source_managed_database_id).await?;
    let purpose = optional_string("purpose", purpose)?;

    let row = sqlx::query_as::<_, DatabaseBackupRow>(
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
            purpose
        )
        values (
            $1::uuid, $2::uuid, $3, $4, $5, $6, $7, $8, $9,
            'postgres_custom', 'queued', 'queued', 0, $10
        )
        returning
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
            worker_id,
            heartbeat_at,
            started_at,
            completed_at,
            error,
            purpose,
            created_at,
            updated_at
        "#,
    )
    .bind(owner_user_id)
    .bind(&source.id)
    .bind(&source.name)
    .bind(source.engine.as_str())
    .bind(&source.host)
    .bind(source.port)
    .bind(&source.database)
    .bind(&source.username)
    .bind(source.ssl_mode.as_str())
    .bind(purpose)
    .fetch_one(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.try_into()
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
    let rows = sqlx::query_as::<_, DatabaseBackupRow>(
        r#"
        select
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
            worker_id,
            heartbeat_at,
            started_at,
            completed_at,
            error,
            purpose,
            created_at,
            updated_at
        from database_backups
        where owner_user_id = $1::uuid
          and ($2::uuid is null or source_managed_database_id = $2::uuid)
          and ($3::text is null or status = $3)
        order by created_at desc
        limit $4
        "#,
    )
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

pub(crate) async fn delete_database_backup(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<DatabaseBackupRecord, StorageError> {
    let row = sqlx::query_as::<_, DatabaseBackupRow>(
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
        returning
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
            worker_id,
            heartbeat_at,
            started_at,
            completed_at,
            error,
            purpose,
            created_at,
            updated_at
        "#,
    )
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
    let backup = get_database_backup(storage, owner_user_id, backup_id).await?;
    if backup.status != DatabaseBackupStatus::Succeeded {
        return Err(StorageError::Conflict(
            "only succeeded database backups can be restored".to_owned(),
        ));
    }
    let target =
        load_managed_database_snapshot(storage, owner_user_id, target_managed_database_id).await?;
    let purpose = required_string("purpose", &purpose)?;

    let row = sqlx::query_as::<_, DatabaseRestoreRow>(
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
            purpose
        )
        values (
            $1::uuid, $2::uuid, $3::uuid, $4, $5, $6, $7, $8, $9, $10,
            'postgres_custom', '{}'::jsonb, 'queued', 'queued', 0, $11
        )
        returning
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
            worker_id,
            heartbeat_at,
            started_at,
            completed_at,
            error,
            purpose,
            created_at,
            updated_at
        "#,
    )
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
    .bind(purpose)
    .fetch_one(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.try_into()
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
    let rows = sqlx::query_as::<_, DatabaseRestoreRow>(
        r#"
        select
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
            worker_id,
            heartbeat_at,
            started_at,
            completed_at,
            error,
            purpose,
            created_at,
            updated_at
        from database_restore_jobs
        where owner_user_id = $1::uuid
          and ($2::uuid is null or backup_id = $2::uuid)
          and ($3::uuid is null or target_managed_database_id = $3::uuid)
          and ($4::text is null or status = $4)
        order by created_at desc
        limit $5
        "#,
    )
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
    let row = sqlx::query_as::<_, DatabaseBackupRow>(
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
        returning
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
            worker_id,
            heartbeat_at,
            started_at,
            completed_at,
            error,
            purpose,
            created_at,
            updated_at
        "#,
    )
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
    let row = sqlx::query_as::<_, DatabaseBackupRow>(
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
        returning
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
            worker_id,
            heartbeat_at,
            started_at,
            completed_at,
            error,
            purpose,
            created_at,
            updated_at
        "#,
    )
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
    let row = sqlx::query_as::<_, DatabaseBackupRow>(
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
        returning
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
            worker_id,
            heartbeat_at,
            started_at,
            completed_at,
            error,
            purpose,
            created_at,
            updated_at
        "#,
    )
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
    let row = sqlx::query_as::<_, DatabaseRestoreRow>(
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
        returning
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
            worker_id,
            heartbeat_at,
            started_at,
            completed_at,
            error,
            purpose,
            created_at,
            updated_at
        "#,
    )
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
    let row = sqlx::query_as::<_, DatabaseRestoreRow>(
        r#"
        update database_restore_jobs
        set phase = $2,
            progress_percent = $3,
            heartbeat_at = now(),
            updated_at = now()
        where id = $1::uuid
          and status = 'running'
        returning
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
            worker_id,
            heartbeat_at,
            started_at,
            completed_at,
            error,
            purpose,
            created_at,
            updated_at
        "#,
    )
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
    let row = sqlx::query_as::<_, DatabaseRestoreRow>(
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
        returning
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
            worker_id,
            heartbeat_at,
            started_at,
            completed_at,
            error,
            purpose,
            created_at,
            updated_at
        "#,
    )
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
    let row = sqlx::query_as::<_, DatabaseRestoreRow>(
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
        returning
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
            worker_id,
            heartbeat_at,
            started_at,
            completed_at,
            error,
            purpose,
            created_at,
            updated_at
        "#,
    )
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

async fn update_backup_progress_row(
    storage: &Storage,
    id: &str,
    phase: &str,
    progress_percent: i32,
) -> Result<DatabaseBackupRow, StorageError> {
    let row = sqlx::query_as::<_, DatabaseBackupRow>(
        r#"
        update database_backups
        set phase = $2,
            progress_percent = $3,
            heartbeat_at = now(),
            updated_at = now()
        where id = $1::uuid
          and status = 'running'
        returning
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
            worker_id,
            heartbeat_at,
            started_at,
            completed_at,
            error,
            purpose,
            created_at,
            updated_at
        "#,
    )
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
    let row = sqlx::query_as::<_, DatabaseBackupRow>(
        r#"
        select
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
            worker_id,
            heartbeat_at,
            started_at,
            completed_at,
            error,
            purpose,
            created_at,
            updated_at
        from database_backups
        where id = $1::uuid
          and ($2::uuid is null or owner_user_id = $2::uuid)
        "#,
    )
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
    let row = sqlx::query_as::<_, DatabaseRestoreRow>(
        r#"
        select
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
            worker_id,
            heartbeat_at,
            started_at,
            completed_at,
            error,
            purpose,
            created_at,
            updated_at
        from database_restore_jobs
        where id = $1::uuid
          and ($2::uuid is null or owner_user_id = $2::uuid)
        "#,
    )
    .bind(id)
    .bind(owner_user_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
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
