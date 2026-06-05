use axum::{Json, Router, extract::State, http::HeaderMap, routing::get};
use liquid_core::AuditSummary;

use crate::{auth::authenticated_user, error::ApiError, state::ApiState};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new().route("/api/v1/audit/summary", get(audit_summary))
}

async fn audit_summary(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<AuditSummary>, ApiError> {
    let _user = authenticated_user(&state, &headers).await?;
    let summary = state
        .agent
        .audit_summary()
        .await
        .map_err(ApiError::internal)?;

    Ok(Json(summary))
}
