use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{
        Request, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
};
use liquid_agent::{
    AgentStream, MockSqlAuditAgent, PostgresToolExecutionMode, SqlAuditAgent, ToolRegistry,
};
use liquid_api::{ApiState, router};
use liquid_core::{
    AuditSummary, AuthResponse, CreateManagedDatabaseRequest, LoginRequest, ManagedDatabase,
    ManagedDatabaseConnectionLoader, ManagedDatabaseConnectionLoaderError,
    ManagedDatabaseConnectionSpec, ManagedDatabaseEngine, ManagedDatabasePoolKey,
    ManagedDatabasePoolPolicy, ManagedDatabaseSslMode, PublicUser, RegisterRequest, SqlAuditReport,
    SqlAuditRequest, UpdateManagedDatabaseRequest,
};
use liquid_storage::{
    LiquidStore, ManagedDatabasePoolConnector, ManagedDatabasePoolError,
    ManagedDatabasePoolManager, StorageError,
};
use serde_json::{Value, json};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use tower::ServiceExt;

const VALID_TOKEN: &str = "valid-token";

#[derive(Default)]
struct TestStore {
    revoked: Mutex<bool>,
    databases: Mutex<Vec<ManagedDatabase>>,
}

#[async_trait]
impl LiquidStore for TestStore {
    async fn register_user(&self, request: RegisterRequest) -> Result<AuthResponse, StorageError> {
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

    async fn authenticate_token(&self, token: &str) -> Result<Option<PublicUser>, StorageError> {
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

    async fn list_managed_databases(
        &self,
        _owner_user_id: &str,
    ) -> Result<Vec<ManagedDatabase>, StorageError> {
        Ok(self.databases.lock().unwrap().clone())
    }

    async fn create_managed_database(
        &self,
        _owner_user_id: &str,
        request: CreateManagedDatabaseRequest,
    ) -> Result<ManagedDatabase, StorageError> {
        let mut databases = self.databases.lock().unwrap();
        let database = ManagedDatabase {
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

    async fn update_managed_database(
        &self,
        _owner_user_id: &str,
        id: &str,
        request: UpdateManagedDatabaseRequest,
    ) -> Result<ManagedDatabase, StorageError> {
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

    async fn delete_managed_database(
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

#[async_trait]
impl ManagedDatabaseConnectionLoader for TestStore {
    async fn load_managed_database_connection(
        &self,
        key: &ManagedDatabasePoolKey,
    ) -> Result<ManagedDatabaseConnectionSpec, ManagedDatabaseConnectionLoaderError> {
        let databases = self.databases.lock().unwrap();
        let Some(database) = databases
            .iter()
            .find(|database| database.id == key.database_id)
        else {
            return Err(ManagedDatabaseConnectionLoaderError::NotFound);
        };

        Ok(ManagedDatabaseConnectionSpec {
            engine: database.engine,
            host: database.host.clone(),
            port: u16::try_from(database.port).map_err(|_| {
                ManagedDatabaseConnectionLoaderError::InvalidConnection(
                    "managed database port must be between 1 and 65535".to_owned(),
                )
            })?,
            database: database.database.clone(),
            username: database.username.clone(),
            password: "password123".to_owned(),
            ssl_mode: database.ssl_mode,
        })
    }
}

struct TestPoolConnector;

#[async_trait]
impl ManagedDatabasePoolConnector for TestPoolConnector {
    async fn connect(
        &self,
        spec: &ManagedDatabaseConnectionSpec,
        policy: &ManagedDatabasePoolPolicy,
    ) -> Result<PgPool, ManagedDatabasePoolError> {
        Ok(lazy_test_pool(spec, policy))
    }
}

#[derive(Default)]
struct CapturingSqlAuditAgent {
    tool_names: Mutex<Vec<String>>,
}

#[async_trait]
impl SqlAuditAgent for CapturingSqlAuditAgent {
    async fn audit_summary(&self) -> anyhow::Result<AuditSummary> {
        Ok(AuditSummary::sample())
    }

    async fn audit_sql(&self, request: SqlAuditRequest) -> anyhow::Result<SqlAuditReport> {
        Ok(test_audit_report(request.sql))
    }

    async fn audit_sql_with_tools(
        &self,
        request: SqlAuditRequest,
        tools: ToolRegistry,
    ) -> anyhow::Result<SqlAuditReport> {
        *self.tool_names.lock().unwrap() = tools
            .definitions()
            .into_iter()
            .map(|definition| definition.name)
            .collect();

        Ok(test_audit_report(request.sql))
    }

    async fn audit_sql_stream(&self, _request: SqlAuditRequest) -> anyhow::Result<AgentStream> {
        Err(anyhow::anyhow!("streaming is not supported in route tests"))
    }
}

fn test_app() -> Router {
    test_app_with_agent(Arc::new(MockSqlAuditAgent))
}

fn test_app_with_agent(agent: Arc<dyn SqlAuditAgent>) -> Router {
    test_app_with_agent_and_execution(agent, PostgresToolExecutionMode::Readonly)
}

fn test_app_with_agent_and_execution(
    agent: Arc<dyn SqlAuditAgent>,
    sql_execution: PostgresToolExecutionMode,
) -> Router {
    let store = Arc::new(TestStore::default());
    let loader: Arc<dyn ManagedDatabaseConnectionLoader> = store.clone();
    let pool_manager = Arc::new(ManagedDatabasePoolManager::with_connector(
        loader,
        Arc::new(TestPoolConnector),
        ManagedDatabasePoolPolicy::default(),
    ));

    router(ApiState::with_pool_manager(
        agent,
        store,
        pool_manager,
        false,
        sql_execution,
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

fn test_audit_report(sql: String) -> SqlAuditReport {
    SqlAuditReport {
        summary: format!("Audited: {sql}"),
        risk_score: 50,
        findings: Vec::new(),
    }
}

fn lazy_test_pool(
    spec: &ManagedDatabaseConnectionSpec,
    policy: &ManagedDatabasePoolPolicy,
) -> PgPool {
    let options = PgConnectOptions::new_without_pgpass()
        .host(&spec.host)
        .port(spec.port)
        .username(&spec.username)
        .password(&spec.password)
        .database(&spec.database)
        .ssl_mode(match spec.ssl_mode {
            ManagedDatabaseSslMode::Disable => sqlx::postgres::PgSslMode::Disable,
            ManagedDatabaseSslMode::Prefer => sqlx::postgres::PgSslMode::Prefer,
            ManagedDatabaseSslMode::Require => sqlx::postgres::PgSslMode::Require,
        })
        .application_name("liquid-api-route-test");

    PgPoolOptions::new()
        .max_connections(policy.max_connections.max(1))
        .min_connections(0)
        .acquire_timeout(policy.acquire_timeout)
        .idle_timeout(Some(policy.connection_idle_timeout))
        .max_lifetime(Some(policy.connection_max_lifetime))
        .test_before_acquire(true)
        .connect_lazy_with(options)
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
async fn managed_database_crud_is_bearer_protected() {
    let app = test_app();
    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases",
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
            "/api/v1/managed-databases/db-1",
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
        .oneshot(auth_request("/api/v1/managed-databases"))
        .await
        .unwrap();
    let payload = response_json(list_response).await;
    assert_eq!(payload.as_array().unwrap().len(), 1);

    let delete_response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/v1/managed-databases/db-1")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn managed_database_audit_sql_requires_authentication() {
    let response = test_app()
        .oneshot(json_request(
            "/api/v1/managed-databases/db-1/audit-sql",
            json!({
                "sql": "select * from users"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn managed_database_audit_sql_returns_not_found_for_missing_database() {
    let response = test_app()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-missing/audit-sql",
            json!({
                "sql": "select * from users"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn managed_database_audit_sql_uses_managed_database_pool() {
    let app = test_app();
    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases",
            json!({
                "name": "Warehouse",
                "engine": "postgres",
                "host": "localhost",
                "port": 5432,
                "database": "warehouse",
                "username": "readonly",
                "password": "password123",
                "ssl_mode": "disable"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(create_response.status(), StatusCode::CREATED);

    let audit_response = app
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/audit-sql",
            json!({
                "sql": "select * from users"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(audit_response.status(), StatusCode::OK);
    let payload = response_json(audit_response).await;
    assert_eq!(payload["summary"], "Mock SQL audit completed.");
    assert_eq!(payload["risk_score"], 50);
}

#[tokio::test]
async fn managed_database_audit_sql_uses_readonly_tool_registry() {
    let agent = Arc::new(CapturingSqlAuditAgent::default());
    let app =
        test_app_with_agent_and_execution(agent.clone(), PostgresToolExecutionMode::WriteGated);
    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases",
            json!({
                "name": "Warehouse",
                "engine": "postgres",
                "host": "localhost",
                "port": 5432,
                "database": "warehouse",
                "username": "readonly",
                "password": "password123",
                "ssl_mode": "disable"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(create_response.status(), StatusCode::CREATED);

    let audit_response = app
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/audit-sql",
            json!({
                "sql": "select * from users"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(audit_response.status(), StatusCode::OK);
    let tool_names = agent.tool_names.lock().unwrap().clone();
    assert!(tool_names.iter().any(|name| name == "inspect_sql_risk"));
    assert!(
        tool_names
            .iter()
            .any(|name| name == "pg_execute_readonly_sql")
    );
    assert!(!tool_names.iter().any(|name| name == "pg_execute_write_sql"));
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
    let database = ManagedDatabase {
        id: "db-1".to_owned(),
        name: "Warehouse".to_owned(),
        engine: ManagedDatabaseEngine::Postgres,
        host: "localhost".to_owned(),
        port: 5432,
        database: "warehouse".to_owned(),
        username: "readonly".to_owned(),
        ssl_mode: ManagedDatabaseSslMode::Prefer,
        has_password: true,
    };

    assert_eq!(database.engine.as_str(), "postgres");
    assert_eq!(database.ssl_mode.as_str(), "prefer");
}
