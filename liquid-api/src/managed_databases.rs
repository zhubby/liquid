use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, patch, post},
};
use liquid_agent::{PostgresToolConfig, tools::sets::sql_audit_tools};
use liquid_core::{
    CreateManagedDatabaseRequest, CurrentManagedDatabaseResponse, ManagedDatabase,
    ManagedDatabaseConnectionTestResponse, ManagedDatabasePoolKey,
    SetCurrentManagedDatabaseRequest, SqlAuditReport, SqlAuditRequest,
    UpdateManagedDatabaseRequest,
};

use crate::{auth::authenticated_user, error::ApiError, state::ApiState};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/api/v1/managed-databases/current",
            get(current_managed_database)
                .put(set_current_managed_database)
                .delete(clear_current_managed_database),
        )
        .route(
            "/api/v1/managed-databases",
            get(list_managed_databases).post(create_managed_database),
        )
        .route(
            "/api/v1/managed-databases/{id}/test-connection",
            post(test_managed_database_connection),
        )
        .route(
            "/api/v1/managed-databases/{id}",
            patch(update_managed_database).delete(delete_managed_database),
        )
        .route(
            "/api/v1/managed-databases/{id}/audit-sql",
            post(audit_managed_database_sql),
        )
}

async fn current_managed_database(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<CurrentManagedDatabaseResponse>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let database = state.store.get_current_managed_database(&user.id).await?;

    Ok(Json(CurrentManagedDatabaseResponse { database }))
}

async fn set_current_managed_database(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<SetCurrentManagedDatabaseRequest>,
) -> Result<Json<CurrentManagedDatabaseResponse>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let database = state
        .store
        .set_current_managed_database(&user.id, &request.managed_database_id)
        .await?;

    Ok(Json(CurrentManagedDatabaseResponse {
        database: Some(database),
    }))
}

async fn clear_current_managed_database(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    state.store.clear_current_managed_database(&user.id).await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn list_managed_databases(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ManagedDatabase>>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let databases = state.store.list_managed_databases(&user.id).await?;

    Ok(Json(databases))
}

async fn create_managed_database(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateManagedDatabaseRequest>,
) -> Result<(StatusCode, Json<ManagedDatabase>), ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let database = state
        .store
        .create_managed_database(&user.id, request)
        .await?;

    Ok((StatusCode::CREATED, Json(database)))
}

async fn update_managed_database(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<UpdateManagedDatabaseRequest>,
) -> Result<Json<ManagedDatabase>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let database = state
        .store
        .update_managed_database(&user.id, &id, request)
        .await?;
    state
        .managed_database_pools
        .invalidate(&ManagedDatabasePoolKey::new(user.id, id))
        .await;

    Ok(Json(database))
}

async fn delete_managed_database(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    state.store.delete_managed_database(&user.id, &id).await?;
    state
        .managed_database_pools
        .invalidate(&ManagedDatabasePoolKey::new(user.id, id))
        .await;

    Ok(StatusCode::NO_CONTENT)
}

async fn test_managed_database_connection(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ManagedDatabaseConnectionTestResponse>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let pool = state
        .managed_database_pools
        .create_pool(ManagedDatabasePoolKey::new(user.id, id))
        .await?;
    state
        .managed_database_connection_tester
        .test(pool)
        .await
        .map_err(|error| {
            ApiError::bad_request(format!("managed database connection test failed: {error}"))
        })?;

    Ok(Json(ManagedDatabaseConnectionTestResponse {
        ok: true,
        message: "连接测试通过".to_owned(),
    }))
}

async fn audit_managed_database_sql(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<SqlAuditRequest>,
) -> Result<Json<SqlAuditReport>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let pool = state
        .managed_database_pools
        .get_pool(ManagedDatabasePoolKey::new(user.id, id))
        .await?;
    let tools = sql_audit_tools(PostgresToolConfig::new(
        Some(pool),
        state.sql_metadata_required,
        state.sql_execution,
    ));
    let report = state
        .agent
        .audit_sql_with_tools(request, tools)
        .await
        .map_err(ApiError::internal)?;

    Ok(Json(report))
}
