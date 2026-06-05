use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, patch},
};
use liquid_core::{CreateManagedDatabaseRequest, ManagedDatabase, UpdateManagedDatabaseRequest};

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

    Ok(Json(database))
}

async fn delete_managed_database(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    state.store.delete_managed_database(&user.id, &id).await?;

    Ok(StatusCode::NO_CONTENT)
}
