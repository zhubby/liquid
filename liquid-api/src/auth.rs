use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    routing::{get, post},
};
use liquid_core::{AuthResponse, CurrentUserResponse, LoginRequest, PublicUser, RegisterRequest};
use liquid_storage::current_user_response;

use crate::{error::ApiError, state::ApiState};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/v1/auth/register", post(register))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/auth/me", get(me))
}

async fn register(
    State(state): State<ApiState>,
    Json(request): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), ApiError> {
    let response = state.store.register_user(request).await?;

    Ok((StatusCode::CREATED, Json(response)))
}

async fn login(
    State(state): State<ApiState>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let response = state.store.login_user(request).await?;

    Ok(Json(response))
}

async fn logout(State(state): State<ApiState>, headers: HeaderMap) -> Result<StatusCode, ApiError> {
    let token = bearer_token(&headers)?;
    state.store.revoke_token(&token).await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn me(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<CurrentUserResponse>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;

    Ok(Json(current_user_response(user)))
}

pub(crate) async fn authenticated_user(
    state: &ApiState,
    headers: &HeaderMap,
) -> Result<PublicUser, ApiError> {
    let token = bearer_token(headers)?;
    let Some(user) = state.store.authenticate_token(&token).await? else {
        return Err(ApiError::unauthorized("invalid bearer token"));
    };

    Ok(user)
}

fn bearer_token(headers: &HeaderMap) -> Result<String, ApiError> {
    let Some(value) = headers.get(AUTHORIZATION) else {
        return Err(ApiError::unauthorized("missing bearer token"));
    };
    let value = value
        .to_str()
        .map_err(|_| ApiError::unauthorized("invalid authorization header"))?;
    let Some(token) = value.strip_prefix("Bearer ") else {
        return Err(ApiError::unauthorized("invalid authorization scheme"));
    };
    let token = token.trim();

    if token.is_empty() {
        return Err(ApiError::unauthorized("missing bearer token"));
    }

    Ok(token.to_owned())
}
