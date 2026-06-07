use axum::http::{
    HeaderName, HeaderValue, Method,
    header::{AUTHORIZATION, CONTENT_TYPE},
};
use tower_http::cors::{Any, CorsLayer};

const SQL_AUDIT_TOTAL_COUNT_HEADER: HeaderName = HeaderName::from_static("x-total-count");
const SQL_AUDIT_PAGE_HEADER: HeaderName = HeaderName::from_static("x-page");
const SQL_AUDIT_PAGE_SIZE_HEADER: HeaderName = HeaderName::from_static("x-page-size");

pub(crate) fn layer(cors_origin: &str) -> anyhow::Result<CorsLayer> {
    let cors = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE])
        .expose_headers([
            SQL_AUDIT_TOTAL_COUNT_HEADER,
            SQL_AUDIT_PAGE_HEADER,
            SQL_AUDIT_PAGE_SIZE_HEADER,
        ]);

    if cors_origin.trim() == "*" {
        return Ok(cors.allow_origin(Any));
    }

    Ok(cors.allow_origin(cors_origin.parse::<HeaderValue>()?))
}
