use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use liquid_agent::SqlAuditAgent;
use liquid_config::LiquidConfig;
use liquid_core::AuditSummary;
use serde::Serialize;
use tokio::net::TcpListener;

#[derive(Clone)]
pub struct ApiState {
    agent: Arc<dyn SqlAuditAgent>,
}

impl ApiState {
    pub fn new(agent: Arc<dyn SqlAuditAgent>) -> Self {
        Self { agent }
    }
}

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/audit/summary", get(audit_summary))
        .with_state(state)
}

pub async fn serve(config: LiquidConfig, agent: Arc<dyn SqlAuditAgent>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(config.api_addr).await?;

    axum::serve(listener, router(ApiState::new(agent))).await?;
    Ok(())
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

async fn audit_summary(State(state): State<ApiState>) -> Result<Json<AuditSummary>, ApiError> {
    let summary = state.agent.audit_summary().await?;

    Ok(Json(summary))
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

struct ApiError(anyhow::Error);

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(error: E) -> Self {
        Self(error.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: self.0.to_string(),
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use liquid_agent::MockSqlAuditAgent;
    use serde_json::Value;
    use tower::ServiceExt;

    use super::*;

    fn test_app() -> Router {
        router(ApiState::new(Arc::new(MockSqlAuditAgent)))
    }

    #[tokio::test]
    async fn healthz_returns_ok() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn audit_summary_returns_sample_payload() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/audit/summary")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(payload["audit_score"], 92);
        assert!(payload["risk_breakdown"].is_array());
    }
}
