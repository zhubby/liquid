use axum::http::{
    HeaderValue, Method,
    header::{AUTHORIZATION, CONTENT_TYPE},
};
use tower_http::cors::{Any, CorsLayer};

pub(crate) fn layer(cors_origin: &str) -> anyhow::Result<CorsLayer> {
    let cors = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);

    if cors_origin.trim() == "*" {
        return Ok(cors.allow_origin(Any));
    }

    Ok(cors.allow_origin(cors_origin.parse::<HeaderValue>()?))
}
