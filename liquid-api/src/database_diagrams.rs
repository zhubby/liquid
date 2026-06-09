use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use liquid_core::{CreateDatabaseDiagramRequest, DatabaseDiagram, UpdateDatabaseDiagramRequest};

use crate::{auth::authenticated_user, error::ApiError, state::ApiState};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/api/v1/database-diagrams",
            get(list_database_diagrams).post(create_database_diagram),
        )
        .route(
            "/api/v1/database-diagrams/{id}",
            get(get_database_diagram)
                .patch(update_database_diagram)
                .delete(delete_database_diagram),
        )
}

async fn list_database_diagrams(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DatabaseDiagram>>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let diagrams = state.store.list_database_diagrams(&user.id).await?;

    Ok(Json(diagrams))
}

async fn create_database_diagram(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateDatabaseDiagramRequest>,
) -> Result<(StatusCode, Json<DatabaseDiagram>), ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let diagram = state
        .store
        .create_database_diagram(&user.id, request)
        .await?;

    Ok((StatusCode::CREATED, Json(diagram)))
}

async fn get_database_diagram(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<DatabaseDiagram>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let diagram = state.store.get_database_diagram(&user.id, &id).await?;

    Ok(Json(diagram))
}

async fn update_database_diagram(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<UpdateDatabaseDiagramRequest>,
) -> Result<Json<DatabaseDiagram>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let diagram = state
        .store
        .update_database_diagram(&user.id, &id, request)
        .await?;

    Ok(Json(diagram))
}

async fn delete_database_diagram(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    state.store.delete_database_diagram(&user.id, &id).await?;

    Ok(StatusCode::NO_CONTENT)
}
