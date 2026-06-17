use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    routing::get,
};
use liquid_agent::validate_backup_schedule;
use liquid_core::{
    CreateDatabaseBackupRequest, CreateDatabaseBackupScheduleRequest, CreateDatabaseRestoreRequest,
    DatabaseBackupListFilters, DatabaseBackupMetadataStoreError, DatabaseBackupRecord,
    DatabaseBackupScheduleRecord, DatabaseBackupScheduleStatus, DatabaseBackupStatus,
    DatabaseBackupTrigger, DatabaseRestoreListFilters, DatabaseRestoreRecord,
    EnqueueDatabaseBackup, EnqueueDatabaseRestore, UpdateDatabaseBackupScheduleRequest,
};
use serde::Deserialize;
use time::OffsetDateTime;

use crate::{auth::authenticated_user, error::ApiError, state::ApiState};

const MINIMUM_BACKUP_CRON_INTERVAL_SECONDS: i64 = 15 * 60;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/api/v1/database-backups",
            get(list_database_backups).post(create_database_backup),
        )
        .route("/api/v1/database-backups/{id}", get(get_database_backup))
        .route(
            "/api/v1/database-backup-schedules",
            get(list_database_backup_schedules).post(create_database_backup_schedule),
        )
        .route(
            "/api/v1/database-backup-schedules/{id}",
            get(get_database_backup_schedule)
                .patch(update_database_backup_schedule)
                .delete(delete_database_backup_schedule),
        )
        .route(
            "/api/v1/database-restores",
            get(list_database_restores).post(create_database_restore),
        )
        .route("/api/v1/database-restores/{id}", get(get_database_restore))
}

#[derive(Debug, Deserialize)]
struct ListDatabaseBackupsQuery {
    managed_database_id: Option<String>,
    status: Option<DatabaseBackupStatus>,
    trigger: Option<DatabaseBackupTrigger>,
    page: Option<i64>,
    page_size: Option<i64>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ListDatabaseBackupSchedulesQuery {
    managed_database_id: Option<String>,
    status: Option<DatabaseBackupScheduleStatus>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ListDatabaseRestoresQuery {
    backup_id: Option<String>,
    target_managed_database_id: Option<String>,
    status: Option<DatabaseBackupStatus>,
    page: Option<i64>,
    page_size: Option<i64>,
    limit: Option<i64>,
}

async fn create_database_backup(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateDatabaseBackupRequest>,
) -> Result<(StatusCode, Json<DatabaseBackupRecord>), ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let backup = state
        .database_backups
        .enqueue_database_backup(
            &user.id,
            EnqueueDatabaseBackup::immediate(
                request.managed_database_id,
                request.purpose,
                None,
                None,
            ),
        )
        .await
        .map_err(database_backup_api_error)?;

    Ok((StatusCode::ACCEPTED, Json(backup)))
}

async fn list_database_backups(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<ListDatabaseBackupsQuery>,
) -> Result<(HeaderMap, Json<Vec<DatabaseBackupRecord>>), ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let page = query.page.unwrap_or(1);
    if page < 1 {
        return Err(ApiError::bad_request(
            "page must be greater than or equal to 1",
        ));
    }
    let page_size = list_page_size(query.page_size, query.limit)?;
    let result = state
        .database_backups
        .list_database_backups_page(
            &user.id,
            DatabaseBackupListFilters {
                source_managed_database_id: query.managed_database_id.as_deref(),
                status: query.status,
                trigger: query.trigger,
                page,
                page_size,
            },
        )
        .await
        .map_err(database_backup_api_error)?;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        "x-total-count",
        header_value(result.total_count, "X-Total-Count")?,
    );
    response_headers.insert("x-page", header_value(result.page, "X-Page")?);
    response_headers.insert(
        "x-page-size",
        header_value(result.page_size, "X-Page-Size")?,
    );

    Ok((response_headers, Json(result.records)))
}

async fn get_database_backup(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<DatabaseBackupRecord>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let backup = state
        .database_backups
        .get_database_backup(&user.id, &id)
        .await
        .map_err(database_backup_api_error)?;

    Ok(Json(backup))
}

async fn create_database_backup_schedule(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(mut request): Json<CreateDatabaseBackupScheduleRequest>,
) -> Result<(StatusCode, Json<DatabaseBackupScheduleRecord>), ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    if request.timezone.is_none() {
        request.timezone = Some("UTC".to_owned());
    }
    let next_run_at = validate_schedule_request(&request)?;
    let schedule = state
        .database_backups
        .create_database_backup_schedule(&user.id, request, None, None, next_run_at)
        .await
        .map_err(database_backup_api_error)?;

    Ok((StatusCode::CREATED, Json(schedule)))
}

async fn list_database_backup_schedules(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<ListDatabaseBackupSchedulesQuery>,
) -> Result<Json<Vec<DatabaseBackupScheduleRecord>>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let schedules = state
        .database_backups
        .list_database_backup_schedules(
            &user.id,
            query.managed_database_id.as_deref(),
            query.status,
            query.limit.unwrap_or(50),
        )
        .await
        .map_err(database_backup_api_error)?;

    Ok(Json(schedules))
}

async fn get_database_backup_schedule(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<DatabaseBackupScheduleRecord>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let schedule = state
        .database_backups
        .get_database_backup_schedule(&user.id, &id)
        .await
        .map_err(database_backup_api_error)?;

    Ok(Json(schedule))
}

async fn update_database_backup_schedule(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<UpdateDatabaseBackupScheduleRequest>,
) -> Result<Json<DatabaseBackupScheduleRecord>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let current = state
        .database_backups
        .get_database_backup_schedule(&user.id, &id)
        .await
        .map_err(database_backup_api_error)?;
    let next_run_at = if request.cron_expression.is_some()
        || request.timezone.is_some()
        || request.status == Some(DatabaseBackupScheduleStatus::Active)
    {
        let cron_expression = request
            .cron_expression
            .clone()
            .unwrap_or_else(|| current.cron_expression.clone());
        let timezone = request
            .timezone
            .clone()
            .unwrap_or_else(|| current.timezone.clone());
        Some(
            validate_backup_schedule(
                &cron_expression,
                &timezone,
                OffsetDateTime::now_utc(),
                MINIMUM_BACKUP_CRON_INTERVAL_SECONDS,
            )
            .map_err(|error| ApiError::bad_request(error.to_string()))?,
        )
    } else {
        None
    };
    let schedule = state
        .database_backups
        .update_database_backup_schedule(&user.id, &id, request, next_run_at)
        .await
        .map_err(database_backup_api_error)?;

    Ok(Json(schedule))
}

async fn delete_database_backup_schedule(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<DatabaseBackupScheduleRecord>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let schedule = state
        .database_backups
        .delete_database_backup_schedule(&user.id, &id)
        .await
        .map_err(database_backup_api_error)?;

    Ok(Json(schedule))
}

async fn create_database_restore(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateDatabaseRestoreRequest>,
) -> Result<(StatusCode, Json<DatabaseRestoreRecord>), ApiError> {
    if !request.confirm_destructive_restore {
        return Err(ApiError::bad_request(
            "confirm_destructive_restore must be true",
        ));
    }
    let user = authenticated_user(&state, &headers).await?;
    let restore = state
        .database_backups
        .enqueue_database_restore(
            &user.id,
            EnqueueDatabaseRestore {
                backup_id: request.backup_id,
                target_managed_database_id: request.target_managed_database_id,
                purpose: request.purpose,
                conversation_id: None,
                created_from_turn_id: None,
            },
        )
        .await
        .map_err(database_backup_api_error)?;

    Ok((StatusCode::ACCEPTED, Json(restore)))
}

async fn list_database_restores(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<ListDatabaseRestoresQuery>,
) -> Result<(HeaderMap, Json<Vec<DatabaseRestoreRecord>>), ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let page = query.page.unwrap_or(1);
    if page < 1 {
        return Err(ApiError::bad_request(
            "page must be greater than or equal to 1",
        ));
    }
    let page_size = list_page_size(query.page_size, query.limit)?;
    let result = state
        .database_backups
        .list_database_restores_page(
            &user.id,
            DatabaseRestoreListFilters {
                backup_id: query.backup_id.as_deref(),
                target_managed_database_id: query.target_managed_database_id.as_deref(),
                status: query.status,
                page,
                page_size,
            },
        )
        .await
        .map_err(database_backup_api_error)?;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        "x-total-count",
        header_value(result.total_count, "X-Total-Count")?,
    );
    response_headers.insert("x-page", header_value(result.page, "X-Page")?);
    response_headers.insert(
        "x-page-size",
        header_value(result.page_size, "X-Page-Size")?,
    );

    Ok((response_headers, Json(result.records)))
}

async fn get_database_restore(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<DatabaseRestoreRecord>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let restore = state
        .database_backups
        .get_database_restore(&user.id, &id)
        .await
        .map_err(database_backup_api_error)?;

    Ok(Json(restore))
}

fn validate_schedule_request(
    request: &CreateDatabaseBackupScheduleRequest,
) -> Result<OffsetDateTime, ApiError> {
    validate_backup_schedule(
        &request.cron_expression,
        request.timezone.as_deref().unwrap_or("UTC"),
        OffsetDateTime::now_utc(),
        MINIMUM_BACKUP_CRON_INTERVAL_SECONDS,
    )
    .map_err(|error| ApiError::bad_request(error.to_string()))
}

fn list_page_size(page_size: Option<i64>, limit: Option<i64>) -> Result<i64, ApiError> {
    let Some(page_size) = page_size else {
        return Ok(limit.unwrap_or(50).clamp(1, 100));
    };

    if matches!(page_size, 10 | 20 | 50 | 100) {
        return Ok(page_size);
    }

    Err(ApiError::bad_request(
        "page_size must be one of 10, 20, 50, or 100",
    ))
}

fn header_value(value: i64, name: &str) -> Result<HeaderValue, ApiError> {
    HeaderValue::from_str(&value.to_string())
        .map_err(|error| ApiError::internal(anyhow::anyhow!("invalid {name} header: {error}")))
}

fn database_backup_api_error(error: DatabaseBackupMetadataStoreError) -> ApiError {
    match error {
        DatabaseBackupMetadataStoreError::NotFound => ApiError::not_found("not found"),
        DatabaseBackupMetadataStoreError::Conflict(message) => ApiError::conflict(message),
        DatabaseBackupMetadataStoreError::Validation(message) => ApiError::bad_request(message),
        DatabaseBackupMetadataStoreError::Backend(message) => {
            ApiError::internal(anyhow::anyhow!(message))
        }
    }
}
