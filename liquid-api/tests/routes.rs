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
    AgentStream, ApprovedWriteExecutionResult, MockSqlAuditAgent, PostgresToolConfig,
    PostgresToolExecutionMode, SqlAuditAgent, ToolRegistry,
};
use liquid_api::{ApiState, ApprovedSqlExecutionFuture, ApprovedSqlExecutor, router};
use liquid_core::{
    ApproveSqlAuditRequest, AuditSummary, AuthResponse, CreateManagedDatabaseRequest, LoginRequest,
    ManagedDatabase, ManagedDatabaseConnectionLoader, ManagedDatabaseConnectionLoaderError,
    ManagedDatabaseConnectionSpec, ManagedDatabaseEngine, ManagedDatabasePoolKey,
    ManagedDatabasePoolPolicy, ManagedDatabaseSslMode, PublicUser, RegisterRequest,
    RejectSqlAuditRequest, SqlAuditExecutionResult, SqlAuditRecord, SqlAuditReport,
    SqlAuditRequest, SqlAuditStatus, UpdateManagedDatabaseRequest,
};
use liquid_sql::{PgSqlAnalysisRequest, PgSqlStatementKind, analyze_postgres_sql};
use liquid_storage::{
    CreateSqlAuditRecord, LiquidStore, ManagedDatabasePoolConnector, ManagedDatabasePoolError,
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
    audits: Mutex<Vec<SqlAuditRecord>>,
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

    async fn create_sql_audit(
        &self,
        owner_user_id: &str,
        managed_database_id: &str,
        record: CreateSqlAuditRecord,
    ) -> Result<SqlAuditRecord, StorageError> {
        let CreateSqlAuditRecord {
            request,
            report,
            deterministic_analysis,
            statement_kind,
            status,
            risk_score,
        } = record;
        let database = self
            .databases
            .lock()
            .unwrap()
            .iter()
            .find(|database| database.id == managed_database_id)
            .cloned()
            .ok_or(StorageError::NotFound)?;
        let mut audits = self.audits.lock().unwrap();
        let record = SqlAuditRecord {
            id: format!("audit-{}", audits.len() + 1),
            owner_user_id: owner_user_id.to_owned(),
            managed_database_id: managed_database_id.to_owned(),
            managed_database_name: database.name,
            managed_database_engine: database.engine.as_str().to_owned(),
            managed_database_host: database.host,
            managed_database_port: database.port,
            managed_database_database: database.database,
            managed_database_username: database.username,
            managed_database_ssl_mode: database.ssl_mode.as_str().to_owned(),
            sql: request.sql,
            schema: request.schema,
            context: request.context,
            execution_purpose: request.execution_purpose,
            status,
            statement_kind,
            risk_score,
            report: Some(report),
            deterministic_analysis: Some(deterministic_analysis),
            approved_by_user_id: None,
            approved_at: None,
            approval_comment: None,
            rejected_by_user_id: None,
            rejected_at: None,
            rejection_comment: None,
            execution_result: None,
            execution_error: None,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
            executed_at: None,
        };
        audits.push(record.clone());
        Ok(record)
    }

    async fn list_sql_audits(
        &self,
        owner_user_id: &str,
        managed_database_id: Option<&str>,
        status: Option<SqlAuditStatus>,
        limit: i64,
    ) -> Result<Vec<SqlAuditRecord>, StorageError> {
        let audits = self.audits.lock().unwrap();
        Ok(audits
            .iter()
            .filter(|record| record.owner_user_id == owner_user_id)
            .filter(|record| {
                managed_database_id
                    .map(|id| record.managed_database_id == id)
                    .unwrap_or(true)
            })
            .filter(|record| status.map(|status| record.status == status).unwrap_or(true))
            .take(limit.clamp(1, 100) as usize)
            .cloned()
            .collect())
    }

    async fn get_sql_audit(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<SqlAuditRecord, StorageError> {
        self.audits
            .lock()
            .unwrap()
            .iter()
            .find(|record| record.id == id && record.owner_user_id == owner_user_id)
            .cloned()
            .ok_or(StorageError::NotFound)
    }

    async fn approve_sql_audit(
        &self,
        owner_user_id: &str,
        id: &str,
        request: ApproveSqlAuditRequest,
    ) -> Result<SqlAuditRecord, StorageError> {
        let mut audits = self.audits.lock().unwrap();
        let Some(record) = audits
            .iter_mut()
            .find(|record| record.id == id && record.owner_user_id == owner_user_id)
        else {
            return Err(StorageError::NotFound);
        };
        if !matches!(record.status, SqlAuditStatus::PendingApproval) {
            return Err(StorageError::Conflict(
                "only pending approval audits can be approved".to_owned(),
            ));
        }
        record.status = SqlAuditStatus::Approved;
        record.approved_by_user_id = Some(owner_user_id.to_owned());
        record.approval_comment = request.comment;
        Ok(record.clone())
    }

    async fn reject_sql_audit(
        &self,
        owner_user_id: &str,
        id: &str,
        request: RejectSqlAuditRequest,
    ) -> Result<SqlAuditRecord, StorageError> {
        let mut audits = self.audits.lock().unwrap();
        let Some(record) = audits
            .iter_mut()
            .find(|record| record.id == id && record.owner_user_id == owner_user_id)
        else {
            return Err(StorageError::NotFound);
        };
        if !matches!(record.status, SqlAuditStatus::PendingApproval) {
            return Err(StorageError::Conflict(
                "only pending approval audits can be rejected".to_owned(),
            ));
        }
        record.status = SqlAuditStatus::Rejected;
        record.rejected_by_user_id = Some(owner_user_id.to_owned());
        record.rejection_comment = request.comment;
        Ok(record.clone())
    }

    async fn start_sql_audit_execution(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<SqlAuditRecord, StorageError> {
        let mut audits = self.audits.lock().unwrap();
        let Some(record) = audits
            .iter_mut()
            .find(|record| record.id == id && record.owner_user_id == owner_user_id)
        else {
            return Err(StorageError::NotFound);
        };
        if !matches!(record.status, SqlAuditStatus::Approved) {
            return Err(StorageError::Conflict(
                "only approved audits can be executed".to_owned(),
            ));
        }
        record.status = SqlAuditStatus::Executing;
        Ok(record.clone())
    }

    async fn complete_sql_audit_execution(
        &self,
        owner_user_id: &str,
        id: &str,
        result: SqlAuditExecutionResult,
    ) -> Result<SqlAuditRecord, StorageError> {
        let mut audits = self.audits.lock().unwrap();
        let Some(record) = audits
            .iter_mut()
            .find(|record| record.id == id && record.owner_user_id == owner_user_id)
        else {
            return Err(StorageError::NotFound);
        };
        if !matches!(record.status, SqlAuditStatus::Executing) {
            return Err(StorageError::Conflict(
                "only executing audits can be completed".to_owned(),
            ));
        }
        record.status = SqlAuditStatus::Executed;
        record.execution_result = Some(result);
        record.executed_at = Some(time::OffsetDateTime::UNIX_EPOCH);
        Ok(record.clone())
    }

    async fn fail_sql_audit_execution(
        &self,
        owner_user_id: &str,
        id: &str,
        error: String,
    ) -> Result<SqlAuditRecord, StorageError> {
        let mut audits = self.audits.lock().unwrap();
        let Some(record) = audits
            .iter_mut()
            .find(|record| record.id == id && record.owner_user_id == owner_user_id)
        else {
            return Err(StorageError::NotFound);
        };
        if !matches!(record.status, SqlAuditStatus::Executing) {
            return Err(StorageError::Conflict(
                "only executing audits can fail".to_owned(),
            ));
        }
        record.status = SqlAuditStatus::ExecutionFailed;
        record.execution_error = Some(error);
        Ok(record.clone())
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

#[derive(Default)]
struct FakeApprovedSqlExecutor {
    fail_with: Mutex<Option<String>>,
}

impl ApprovedSqlExecutor for FakeApprovedSqlExecutor {
    fn execute<'a>(
        &'a self,
        _config: PostgresToolConfig,
        sql: &'a str,
    ) -> ApprovedSqlExecutionFuture<'a> {
        Box::pin(async move {
            if let Some(message) = self.fail_with.lock().unwrap().clone() {
                return Err(anyhow::anyhow!(message));
            }

            let analysis = analyze_postgres_sql(PgSqlAnalysisRequest::new(sql));
            Ok(ApprovedWriteExecutionResult {
                statement_kind: analysis
                    .statements
                    .first()
                    .map(|statement| statement.kind.clone())
                    .unwrap_or(PgSqlStatementKind::Other),
                affected_rows: 1,
                elapsed_ms: 7,
                risk_floor: analysis.risk_floor(),
                analysis,
            })
        })
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
    test_app_with_agent_execution_and_executor(
        agent,
        sql_execution,
        Arc::new(FakeApprovedSqlExecutor::default()),
    )
}

fn test_app_with_agent_execution_and_executor(
    agent: Arc<dyn SqlAuditAgent>,
    sql_execution: PostgresToolExecutionMode,
    executor: Arc<dyn ApprovedSqlExecutor>,
) -> Router {
    let store = Arc::new(TestStore::default());
    let loader: Arc<dyn ManagedDatabaseConnectionLoader> = store.clone();
    let pool_manager = Arc::new(ManagedDatabasePoolManager::with_connector(
        loader,
        Arc::new(TestPoolConnector),
        ManagedDatabasePoolPolicy::default(),
    ));

    router(ApiState::with_pool_manager_and_executor(
        agent,
        store,
        pool_manager,
        false,
        sql_execution,
        executor,
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

async fn create_test_database(app: &Router) {
    let response = app
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

    assert_eq!(response.status(), StatusCode::CREATED);
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

#[tokio::test]
async fn sql_audit_persistence_requires_authentication() {
    let response = test_app()
        .oneshot(json_request(
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "select * from users"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sql_audit_persistence_creates_audited_select_record() {
    let app = test_app();
    create_test_database(&app).await;

    let response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "select * from users",
                "context": "read-only review"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = response_json(response).await;
    assert_eq!(payload["id"], "audit-1");
    assert_eq!(payload["status"], "audited");
    assert_eq!(payload["statement_kind"], "select");
    assert_eq!(payload["managed_database_id"], "db-1");
    assert_eq!(payload["sql"], "select * from users");
    assert_eq!(payload["report"]["summary"], "Mock SQL audit completed.");
    assert_eq!(payload["report"]["risk_score"], 50);

    let list_response = app
        .oneshot(auth_request(
            "/api/v1/sql-audits?managed_database_id=db-1&status=audited",
        ))
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let payload = response_json(list_response).await;
    assert_eq!(payload.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn sql_audit_approve_and_execute_runs_once_when_write_gated() {
    let app = test_app_with_agent_and_execution(
        Arc::new(CapturingSqlAuditAgent::default()),
        PostgresToolExecutionMode::WriteGated,
    );
    create_test_database(&app).await;

    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "update users set active = false where id = 1",
                "execution_purpose": "Deactivate test user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let payload = response_json(create_response).await;
    assert_eq!(payload["status"], "pending_approval");

    let approve_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/sql-audits/audit-1/approve",
            json!({
                "comment": "approved"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(approve_response.status(), StatusCode::OK);
    let payload = response_json(approve_response).await;
    assert_eq!(payload["status"], "approved");

    let execute_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sql-audits/audit-1/execute")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(execute_response.status(), StatusCode::OK);
    let payload = response_json(execute_response).await;
    assert_eq!(payload["status"], "executed");
    assert_eq!(payload["execution_result"]["affected_rows"], 1);

    let repeat_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sql-audits/audit-1/execute")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(repeat_response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn sql_audit_reject_blocks_execution() {
    let app = test_app_with_agent_and_execution(
        Arc::new(CapturingSqlAuditAgent::default()),
        PostgresToolExecutionMode::WriteGated,
    );
    create_test_database(&app).await;

    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "delete from users where id = 1",
                "execution_purpose": "Remove test user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let reject_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/sql-audits/audit-1/reject",
            json!({
                "comment": "too risky"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(reject_response.status(), StatusCode::OK);
    let payload = response_json(reject_response).await;
    assert_eq!(payload["status"], "rejected");

    let execute_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sql-audits/audit-1/execute")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(execute_response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn sql_audit_blocks_critical_sql() {
    let app = test_app();
    create_test_database(&app).await;

    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "drop table users",
                "execution_purpose": "Dangerous migration"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let payload = response_json(create_response).await;
    assert_eq!(payload["status"], "blocked");

    let approve_response = app
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/sql-audits/audit-1/approve",
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(approve_response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn sql_audit_execute_requires_write_gated_config() {
    let app = test_app();
    create_test_database(&app).await;

    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "update users set active = false where id = 1",
                "execution_purpose": "Deactivate test user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let execute_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sql-audits/audit-1/execute")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(execute_response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn sql_audit_execute_rejects_managed_database_drift() {
    let app = test_app_with_agent_and_execution(
        Arc::new(CapturingSqlAuditAgent::default()),
        PostgresToolExecutionMode::WriteGated,
    );
    create_test_database(&app).await;

    let create_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/managed-databases/db-1/sql-audits",
            json!({
                "sql": "update users set active = false where id = 1",
                "execution_purpose": "Deactivate test user"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let approve_response = app
        .clone()
        .oneshot(auth_json_request(
            "POST",
            "/api/v1/sql-audits/audit-1/approve",
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(approve_response.status(), StatusCode::OK);

    let update_response = app
        .clone()
        .oneshot(auth_json_request(
            "PATCH",
            "/api/v1/managed-databases/db-1",
            json!({
                "host": "other-host"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(update_response.status(), StatusCode::OK);

    let execute_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sql-audits/audit-1/execute")
                .header(AUTHORIZATION, format!("Bearer {VALID_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(execute_response.status(), StatusCode::CONFLICT);
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
