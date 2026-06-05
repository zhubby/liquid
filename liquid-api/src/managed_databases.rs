use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, patch, post},
};
use liquid_agent::{PostgresToolConfig, ToolRegistry};
use liquid_core::{
    CreateManagedDatabaseRequest, ManagedDatabase, ManagedDatabasePoolKey, SqlAuditReport,
    SqlAuditRequest, UpdateManagedDatabaseRequest,
};

use crate::{auth::authenticated_user, error::ApiError, state::ApiState};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/api/v1/managed-databases",
            get(list_managed_databases).post(create_managed_database),
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
    let tools = ToolRegistry::with_postgres_tools(PostgresToolConfig::new(
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
