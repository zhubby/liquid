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
use liquid_agent::MockSqlAuditAgent;
use liquid_api::{ApiState, router};
use liquid_core::{
    AuthResponse, CreateManagedDatabaseRequest, LoginRequest, ManagedDatabase,
    ManagedDatabaseEngine, ManagedDatabaseSslMode, PublicUser, RegisterRequest,
    UpdateManagedDatabaseRequest,
};
use liquid_storage::{LiquidStore, StorageError};
use serde_json::{Value, json};
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
