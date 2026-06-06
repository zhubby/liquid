use axum::{
    Json, Router,
    extract::State,
    http::HeaderMap,
    routing::{get, put},
};
use liquid_core::{LlmProviderSettingsResponse, UpdateLlmProviderSettingsRequest};

use crate::{auth::authenticated_user, error::ApiError, state::ApiState};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new().route(
        "/api/v1/settings/llm-provider",
        get(get_llm_provider_settings).put(update_llm_provider_settings),
    )
}

async fn get_llm_provider_settings(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<LlmProviderSettingsResponse>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let settings = state.store.get_llm_provider_settings(&user.id).await?;

    Ok(Json(LlmProviderSettingsResponse { settings }))
}

async fn update_llm_provider_settings(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<UpdateLlmProviderSettingsRequest>,
) -> Result<Json<LlmProviderSettingsResponse>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let settings = state
        .store
        .upsert_llm_provider_settings(&user.id, request)
        .await?;

    Ok(Json(LlmProviderSettingsResponse {
        settings: Some(settings),
    }))
}
