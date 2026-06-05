use liquid_core::{
    ApproveSqlAuditRequest, CreateManagedDatabaseRequest, CreateSqlAuditRequest,
    ManagedDatabaseEngine, ManagedDatabaseSslMode, RegisterRequest, RejectSqlAuditRequest,
    RiskSeverity, SqlAuditExecutionResult, SqlAuditFinding, SqlAuditReport, SqlAuditStatus,
    SqlStatementKind,
};
use liquid_storage::{CreateSqlAuditRecord, LiquidStore, Storage, StorageOptions};
use serde_json::json;

#[tokio::test]
async fn sql_audit_store_persists_and_transitions_records() {
    let Some(storage) = test_storage().await else {
        return;
    };

    let auth = storage
        .register_user(RegisterRequest {
            email: unique_email("sql-audit"),
            display_name: "SQL Audit Test".to_owned(),
            password: "password123".to_owned(),
        })
        .await
        .unwrap();
    let database = storage
        .create_managed_database(
            &auth.user.id,
            CreateManagedDatabaseRequest {
                name: "Warehouse".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "warehouse".to_owned(),
                username: "readonly".to_owned(),
                password: "secret123".to_owned(),
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();
    let record = storage
        .create_sql_audit(
            &auth.user.id,
            &database.id,
            CreateSqlAuditRecord {
                request: CreateSqlAuditRequest {
                    sql: "update users set active = false where id = 1".to_owned(),
                    schema: None,
                    context: Some("test".to_owned()),
                    execution_purpose: Some("Deactivate test user".to_owned()),
                },
                report: test_report(),
                deterministic_analysis: json!({
                    "statements": [{"kind": "update"}],
                    "findings": []
                }),
                statement_kind: Some(SqlStatementKind::Update),
                status: SqlAuditStatus::PendingApproval,
                risk_score: 80,
            },
        )
        .await
        .unwrap();

    assert_eq!(record.status, SqlAuditStatus::PendingApproval);
    assert_eq!(record.managed_database_id, database.id);
    assert_eq!(record.managed_database_host, "localhost");

    let listed = storage
        .list_sql_audits(
            &auth.user.id,
            Some(&database.id),
            Some(SqlAuditStatus::PendingApproval),
            10,
        )
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);

    let approved = storage
        .approve_sql_audit(
            &auth.user.id,
            &record.id,
            ApproveSqlAuditRequest {
                comment: Some("approved".to_owned()),
            },
        )
        .await
        .unwrap();
    assert_eq!(approved.status, SqlAuditStatus::Approved);
    assert_eq!(approved.approval_comment.as_deref(), Some("approved"));

    let executing = storage
        .start_sql_audit_execution(&auth.user.id, &record.id)
        .await
        .unwrap();
    assert_eq!(executing.status, SqlAuditStatus::Executing);

    let executed = storage
        .complete_sql_audit_execution(
            &auth.user.id,
            &record.id,
            SqlAuditExecutionResult {
                statement_kind: SqlStatementKind::Update,
                affected_rows: 1,
                elapsed_ms: 10,
                risk_floor: 80,
                findings: json!([]),
            },
        )
        .await
        .unwrap();
    assert_eq!(executed.status, SqlAuditStatus::Executed);
    assert_eq!(executed.execution_result.unwrap().affected_rows, 1);

    let error = storage
        .start_sql_audit_execution(&auth.user.id, &record.id)
        .await
        .unwrap_err();
    assert!(matches!(error, liquid_storage::StorageError::Conflict(_)));
}

#[tokio::test]
async fn sql_audit_store_rejects_invalid_status_transitions() {
    let Some(storage) = test_storage().await else {
        return;
    };

    let auth = storage
        .register_user(RegisterRequest {
            email: unique_email("sql-audit-reject"),
            display_name: "SQL Audit Reject Test".to_owned(),
            password: "password123".to_owned(),
        })
        .await
        .unwrap();
    let database = storage
        .create_managed_database(
            &auth.user.id,
            CreateManagedDatabaseRequest {
                name: "Warehouse".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "warehouse".to_owned(),
                username: "readonly".to_owned(),
                password: "secret123".to_owned(),
                ssl_mode: ManagedDatabaseSslMode::Prefer,
            },
        )
        .await
        .unwrap();
    let record = storage
        .create_sql_audit(
            &auth.user.id,
            &database.id,
            CreateSqlAuditRecord {
                request: CreateSqlAuditRequest {
                    sql: "select * from users".to_owned(),
                    schema: None,
                    context: None,
                    execution_purpose: None,
                },
                report: test_report(),
                deterministic_analysis: json!({ "statements": [{"kind": "select"}], "findings": [] }),
                statement_kind: Some(SqlStatementKind::Select),
                status: SqlAuditStatus::Audited,
                risk_score: 50,
            },
        )
        .await
        .unwrap();

    let approve_error = storage
        .approve_sql_audit(
            &auth.user.id,
            &record.id,
            ApproveSqlAuditRequest { comment: None },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        approve_error,
        liquid_storage::StorageError::Conflict(_)
    ));

    let reject_error = storage
        .reject_sql_audit(
            &auth.user.id,
            &record.id,
            RejectSqlAuditRequest { comment: None },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        reject_error,
        liquid_storage::StorageError::Conflict(_)
    ));
}

async fn test_storage() -> Option<Storage> {
    let database_url = std::env::var("LIQUID_TEST_DATABASE_URL").ok()?;
    let storage = Storage::connect_with_options(StorageOptions::new(database_url))
        .await
        .ok()?;
    storage.migrate().await.ok()?;
    Some(storage)
}

fn test_report() -> SqlAuditReport {
    SqlAuditReport {
        summary: "Stored audit".to_owned(),
        risk_score: 50,
        findings: vec![SqlAuditFinding {
            title: "Finding".to_owned(),
            severity: RiskSeverity::Medium,
            explanation: "Explain".to_owned(),
            recommendation: "Review".to_owned(),
        }],
    }
}

fn unique_email(prefix: &str) -> String {
    format!(
        "{prefix}-{}@test.local",
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    )
}
