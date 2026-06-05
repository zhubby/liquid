use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{
        HeaderMap, HeaderValue, Method, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use liquid_agent::SqlAuditAgent;
use liquid_config::LiquidConfig;
use liquid_core::{
    AuditSummary, AuditedDatabase, AuthResponse, CreateAuditedDatabaseRequest, CurrentUserResponse,
    LoginRequest, PublicUser, RegisterRequest, UpdateAuditedDatabaseRequest,
};
use liquid_storage::{LiquidStore, StorageError, current_user_response};
use serde::Serialize;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
pub struct ApiState {
    agent: Arc<dyn SqlAuditAgent>,
    store: Arc<dyn LiquidStore>,
}

impl ApiState {
    pub fn new(agent: Arc<dyn SqlAuditAgent>, store: Arc<dyn LiquidStore>) -> Self {
        Self { agent, store }
    }
}

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/auth/register", post(register))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/auth/me", get(me))
        .route("/api/v1/audit/summary", get(audit_summary))
        .route(
            "/api/v1/audited-databases",
            get(list_audited_databases).post(create_audited_database),
        )
        .route(
            "/api/v1/audited-databases/{id}",
            patch(update_audited_database).delete(delete_audited_database),
        )
        .with_state(state)
}

pub fn router_with_cors(state: ApiState, cors_origin: &str) -> anyhow::Result<Router> {
    Ok(router(state).layer(cors_layer(cors_origin)?))
}

pub async fn serve(
    config: LiquidConfig,
    agent: Arc<dyn SqlAuditAgent>,
    store: Arc<dyn LiquidStore>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(config.api_addr).await?;
    let app = router_with_cors(ApiState::new(agent, store), &config.cors_origin)?;

    axum::serve(listener, app).await?;
    Ok(())
}

fn cors_layer(cors_origin: &str) -> anyhow::Result<CorsLayer> {
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);

    if cors_origin.trim() == "*" {
        return Ok(cors.allow_origin(Any));
    }

    Ok(cors.allow_origin(cors_origin.parse::<HeaderValue>()?))
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

async fn list_audited_databases(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<AuditedDatabase>>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let databases = state.store.list_audited_databases(&user.id).await?;

    Ok(Json(databases))
}

async fn create_audited_database(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateAuditedDatabaseRequest>,
) -> Result<(StatusCode, Json<AuditedDatabase>), ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let database = state
        .store
        .create_audited_database(&user.id, request)
        .await?;

    Ok((StatusCode::CREATED, Json(database)))
}

async fn update_audited_database(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<UpdateAuditedDatabaseRequest>,
) -> Result<Json<AuditedDatabase>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let database = state
        .store
        .update_audited_database(&user.id, &id, request)
        .await?;

    Ok(Json(database))
}

async fn delete_audited_database(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    state.store.delete_audited_database(&user.id, &id).await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn authenticated_user(state: &ApiState, headers: &HeaderMap) -> Result<PublicUser, ApiError> {
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

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    fn internal(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }
}

impl From<StorageError> for ApiError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::DuplicateEmail | StorageError::DuplicateAuditedDatabaseName => Self {
                status: StatusCode::CONFLICT,
                message: error.to_string(),
            },
            StorageError::InvalidCredentials => Self {
                status: StatusCode::UNAUTHORIZED,
                message: error.to_string(),
            },
            StorageError::NotFound => Self {
                status: StatusCode::NOT_FOUND,
                message: error.to_string(),
            },
            StorageError::Validation(_) => Self {
                status: StatusCode::BAD_REQUEST,
                message: error.to_string(),
            },
            StorageError::Database(_) | StorageError::Crypto(_) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "internal storage error".to_owned(),
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use liquid_agent::MockSqlAuditAgent;
    use liquid_core::{AuditedDatabaseEngine, AuditedDatabaseSslMode};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::*;

    const VALID_TOKEN: &str = "valid-token";

    #[derive(Default)]
    struct TestStore {
        revoked: Mutex<bool>,
        databases: Mutex<Vec<AuditedDatabase>>,
    }

    #[async_trait]
    impl LiquidStore for TestStore {
        async fn register_user(
            &self,
            request: RegisterRequest,
        ) -> Result<AuthResponse, StorageError> {
            Ok(test_auth_response(request.email, request.display_name))
        }

        async fn login_user(&self, request: LoginRequest) -> Result<AuthResponse, StorageError> {
            if request.email == "user@test.local" && request.password == "password123" {
                Ok(test_auth_response(
                    "user@test.local".to_owned(),
                    "Test User".to_owned(),
                ))
            } else {
                Err(StorageError::InvalidCredentials)
            }
        }

        async fn authenticate_token(
            &self,
            token: &str,
        ) -> Result<Option<PublicUser>, StorageError> {
            if token == VALID_TOKEN && !*self.revoked.lock().unwrap() {
                Ok(Some(test_user()))
            } else {
                Ok(None)
            }
        }

        async fn revoke_token(&self, token: &str) -> Result<(), StorageError> {
            if token == VALID_TOKEN {
                *self.revoked.lock().unwrap() = true;
            }

            Ok(())
        }

        async fn list_audited_databases(
            &self,
            _owner_user_id: &str,
        ) -> Result<Vec<AuditedDatabase>, StorageError> {
            Ok(self.databases.lock().unwrap().clone())
        }

        async fn create_audited_database(
            &self,
            _owner_user_id: &str,
            request: CreateAuditedDatabaseRequest,
        ) -> Result<AuditedDatabase, StorageError> {
            let mut databases = self.databases.lock().unwrap();
            let database = AuditedDatabase {
                id: format!("db-{}", databases.len() + 1),
                name: request.name,
                engine: request.engine,
                host: request.host,
                port: request.port,
                database: request.database,
                username: request.username,
                ssl_mode: request.ssl_mode,
                has_password: true,
            };
            databases.push(database.clone());
            Ok(database)
        }

        async fn update_audited_database(
            &self,
            _owner_user_id: &str,
            id: &str,
            request: UpdateAuditedDatabaseRequest,
        ) -> Result<AuditedDatabase, StorageError> {
            let mut databases = self.databases.lock().unwrap();
            let Some(database) = databases.iter_mut().find(|database| database.id == id) else {
                return Err(StorageError::NotFound);
            };

            if let Some(name) = request.name {
                database.name = name;
            }
            if let Some(host) = request.host {
                database.host = host;
            }
            if let Some(port) = request.port {
                database.port = port;
            }
            if let Some(database_name) = request.database {
                database.database = database_name;
            }
            if let Some(username) = request.username {
                database.username = username;
            }
            if let Some(ssl_mode) = request.ssl_mode {
                database.ssl_mode = ssl_mode;
            }

            Ok(database.clone())
        }

        async fn delete_audited_database(
            &self,
            _owner_user_id: &str,
            id: &str,
        ) -> Result<(), StorageError> {
            let mut databases = self.databases.lock().unwrap();
            let before = databases.len();
            databases.retain(|database| database.id != id);

            if databases.len() == before {
                return Err(StorageError::NotFound);
            }

            Ok(())
        }
    }

    fn test_app() -> Router {
        router(ApiState::new(
            Arc::new(MockSqlAuditAgent),
            Arc::new(TestStore::default()),
        ))
    }

    fn test_auth_response(email: String, display_name: String) -> AuthResponse {
        AuthResponse {
            token: VALID_TOKEN.to_owned(),
            token_type: "Bearer".to_owned(),
            expires_in_seconds: 3600,
            user: PublicUser {
                id: "user-1".to_owned(),
                email,
                display_name,
            },
        }
    }

    fn test_user() -> PublicUser {
        PublicUser {
            id: "user-1".to_owned(),
            email: "user@test.local".to_owned(),
            display_name: "Test User".to_owned(),
        }
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
    async fn register_returns_bearer_token() {
        let response = test_app()
            .oneshot(json_request(
                "/api/v1/auth/register",
                json!({
                    "email": "user@test.local",
                    "display_name": "Test User",
                    "password": "password123"
                }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let payload = response_json(response).await;
        assert_eq!(payload["token"], VALID_TOKEN);
        assert_eq!(payload["user"]["email"], "user@test.local");
    }

    #[tokio::test]
    async fn login_rejects_invalid_credentials() {
        let response = test_app()
            .oneshot(json_request(
                "/api/v1/auth/login",
                json!({
                    "email": "user@test.local",
                    "password": "wrong-password"
                }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn me_requires_bearer_token() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/auth/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn me_returns_current_user_for_valid_token() {
        let response = test_app()
            .oneshot(auth_request("/api/v1/auth/me"))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let payload = response_json(response).await;
        assert_eq!(payload["user"]["email"], "user@test.local");
    }

    #[tokio::test]
    async fn logout_revokes_token() {
        let app = test_app();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/logout")
                    .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let response = app.oneshot(auth_request("/api/v1/auth/me")).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn audit_summary_requires_authentication() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/audit/summary")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn audit_summary_returns_sample_payload_for_authenticated_user() {
        let response = test_app()
            .oneshot(auth_request("/api/v1/audit/summary"))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let payload = response_json(response).await;
        assert_eq!(payload["audit_score"], 92);
        assert!(payload["risk_breakdown"].is_array());
    }

    #[tokio::test]
    async fn audited_database_crud_is_bearer_protected() {
        let app = test_app();
        let create_response = app
            .clone()
            .oneshot(auth_json_request(
                "POST",
                "/api/v1/audited-databases",
                json!({
                    "name": "Warehouse",
                    "engine": "postgres",
                    "host": "localhost",
                    "port": 5432,
                    "database": "warehouse",
                    "username": "readonly",
                    "password": "password123",
                    "ssl_mode": "prefer"
                }),
            ))
            .await
            .unwrap();

        assert_eq!(create_response.status(), StatusCode::CREATED);
        let payload = response_json(create_response).await;
        assert_eq!(payload["name"], "Warehouse");
        assert_eq!(payload["has_password"], true);
        assert!(payload.get("password").is_none());

        let update_response = app
            .clone()
            .oneshot(auth_json_request(
                "PATCH",
                "/api/v1/audited-databases/db-1",
                json!({
                    "name": "Warehouse Replica",
                    "ssl_mode": "require"
                }),
            ))
            .await
            .unwrap();

        assert_eq!(update_response.status(), StatusCode::OK);
        let payload = response_json(update_response).await;
        assert_eq!(payload["name"], "Warehouse Replica");
        assert_eq!(payload["ssl_mode"], "require");

        let list_response = app
            .clone()
            .oneshot(auth_request("/api/v1/audited-databases"))
            .await
            .unwrap();
        let payload = response_json(list_response).await;
        assert_eq!(payload.as_array().unwrap().len(), 1);

        let delete_response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/audited-databases/db-1")
                    .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);
    }

    fn json_request(uri: &str, payload: Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap()
    }

    fn auth_request(uri: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
            .body(Body::empty())
            .unwrap()
    }

    fn auth_json_request(method: &str, uri: &str, payload: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap()
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        serde_json::from_slice(&body).unwrap()
    }

    #[test]
    fn fake_store_uses_expected_enum_values() {
        let database = AuditedDatabase {
            id: "db-1".to_owned(),
            name: "Warehouse".to_owned(),
            engine: AuditedDatabaseEngine::Postgres,
            host: "localhost".to_owned(),
            port: 5432,
            database: "warehouse".to_owned(),
            username: "readonly".to_owned(),
            ssl_mode: AuditedDatabaseSslMode::Prefer,
            has_password: true,
        };

        assert_eq!(database.engine.as_str(), "postgres");
        assert_eq!(database.ssl_mode.as_str(), "prefer");
    }
}
