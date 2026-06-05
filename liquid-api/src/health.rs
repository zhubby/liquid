use axum::{Json, Router, routing::get};
use serde::Serialize;

use crate::state::ApiState;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new().route("/healthz", get(healthz))
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
}

async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "liquid-api",
    })
}
