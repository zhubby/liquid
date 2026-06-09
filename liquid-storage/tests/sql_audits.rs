use liquid_core::{
    ApproveSqlAuditRequest, CreateManagedDatabaseRequest, CreateSqlAuditRequest,
    ManagedDatabaseEngine, ManagedDatabaseSslMode, RegisterRequest, RejectSqlAuditRequest,
    RiskSeverity, SqlAuditExecutionResult, SqlAuditExecutionStatus, SqlAuditFinding,
    SqlAuditLifecycleStatus, SqlAuditReport, SqlAuditStatus, SqlStatementKind,
};
use liquid_storage::{
    CreateSqlAuditRecord, LiquidStore, SqlAuditListFilters, Storage, StorageOptions,
};
use serde_json::json;
use time::{Duration, OffsetDateTime};

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
                tags: None,
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
            SqlAuditListFilters {
                managed_database_id: Some(&database.id),
                status: Some(SqlAuditStatus::PendingApproval),
                audit_status: None,
                execution_status: None,
                created_from: None,
                created_to: None,
                page: 1,
                page_size: 10,
            },
        )
        .await
        .unwrap();
    assert_eq!(listed.records.len(), 1);
    assert_eq!(listed.total_count, 1);

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
                rollback: None,
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
async fn sql_audit_store_filters_and_paginates_records() {
    let Some(storage) = test_storage().await else {
        return;
    };

    let auth = storage
        .register_user(RegisterRequest {
            email: unique_email("sql-audit-list"),
            display_name: "SQL Audit List Test".to_owned(),
            password: "password123".to_owned(),
        })
        .await
        .unwrap();
    let first_database = storage
        .create_managed_database(
            &auth.user.id,
            CreateManagedDatabaseRequest {
                name: "Primary".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "primary".to_owned(),
                username: "readonly".to_owned(),
                password: "secret123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();
    let second_database = storage
        .create_managed_database(
            &auth.user.id,
            CreateManagedDatabaseRequest {
                name: "Archive".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "archive".to_owned(),
                username: "readonly".to_owned(),
                password: "secret123".to_owned(),
                tags: None,
                ssl_mode: ManagedDatabaseSslMode::Disable,
            },
        )
        .await
        .unwrap();
    let range_start = OffsetDateTime::now_utc() - Duration::seconds(1);

    storage
        .create_sql_audit(
            &auth.user.id,
            &first_database.id,
            CreateSqlAuditRecord {
                request: CreateSqlAuditRequest {
                    sql: "select * from users".to_owned(),
                    schema: None,
                    context: None,
                    execution_purpose: None,
                },
                report: test_report(),
                deterministic_analysis: json!({ "statements": [{"kind": "select"}] }),
                statement_kind: Some(SqlStatementKind::Select),
                status: SqlAuditStatus::Audited,
                risk_score: 20,
            },
        )
        .await
        .unwrap();
    let update_record = storage
        .create_sql_audit(
            &auth.user.id,
            &first_database.id,
            CreateSqlAuditRecord {
                request: CreateSqlAuditRequest {
                    sql: "update users set active = false where id = 1".to_owned(),
                    schema: None,
                    context: None,
                    execution_purpose: Some("Deactivate test user".to_owned()),
                },
                report: test_report(),
                deterministic_analysis: json!({ "statements": [{"kind": "update"}] }),
                statement_kind: Some(SqlStatementKind::Update),
                status: SqlAuditStatus::PendingApproval,
                risk_score: 80,
            },
        )
        .await
        .unwrap();
    storage
        .approve_sql_audit(
            &auth.user.id,
            &update_record.id,
            ApproveSqlAuditRequest { comment: None },
        )
        .await
        .unwrap();
    storage
        .start_sql_audit_execution(&auth.user.id, &update_record.id)
        .await
        .unwrap();
    storage
        .complete_sql_audit_execution(
            &auth.user.id,
            &update_record.id,
            SqlAuditExecutionResult {
                statement_kind: SqlStatementKind::Update,
                affected_rows: 1,
                elapsed_ms: 12,
                risk_floor: 20,
                findings: json!({}),
                rollback: None,
            },
        )
        .await
        .unwrap();
    storage
        .create_sql_audit(
            &auth.user.id,
            &second_database.id,
            CreateSqlAuditRecord {
                request: CreateSqlAuditRequest {
                    sql: "drop table users".to_owned(),
                    schema: None,
                    context: None,
                    execution_purpose: None,
                },
                report: test_report(),
                deterministic_analysis: json!({ "statements": [{"kind": "drop"}] }),
                statement_kind: Some(SqlStatementKind::Drop),
                status: SqlAuditStatus::Blocked,
                risk_score: 100,
            },
        )
        .await
        .unwrap();
    let range_end = OffsetDateTime::now_utc() + Duration::seconds(1);

    let first_page = storage
        .list_sql_audits(
            &auth.user.id,
            SqlAuditListFilters {
                managed_database_id: None,
                status: None,
                audit_status: None,
                execution_status: None,
                created_from: Some(range_start),
                created_to: Some(range_end),
                page: 1,
                page_size: 2,
            },
        )
        .await
        .unwrap();
    assert_eq!(first_page.records.len(), 2);
    assert_eq!(first_page.total_count, 3);

    let second_page = storage
        .list_sql_audits(
            &auth.user.id,
            SqlAuditListFilters {
                page: 2,
                ..audit_filters()
            },
        )
        .await
        .unwrap();
    assert_eq!(second_page.records.len(), 1);
    assert_eq!(second_page.total_count, 3);

    let first_database_records = storage
        .list_sql_audits(
            &auth.user.id,
            SqlAuditListFilters {
                managed_database_id: Some(&first_database.id),
                ..audit_filters()
            },
        )
        .await
        .unwrap();
    assert_eq!(first_database_records.total_count, 2);

    let blocked_records = storage
        .list_sql_audits(
            &auth.user.id,
            SqlAuditListFilters {
                audit_status: Some(SqlAuditLifecycleStatus::Blocked),
                ..audit_filters()
            },
        )
        .await
        .unwrap();
    assert_eq!(blocked_records.total_count, 1);
    assert_eq!(blocked_records.records[0].status, SqlAuditStatus::Blocked);

    let not_executed_records = storage
        .list_sql_audits(
            &auth.user.id,
            SqlAuditListFilters {
                execution_status: Some(SqlAuditExecutionStatus::NotExecuted),
                ..audit_filters()
            },
        )
        .await
        .unwrap();
    assert_eq!(not_executed_records.total_count, 2);

    let executed_records = storage
        .list_sql_audits(
            &auth.user.id,
            SqlAuditListFilters {
                execution_status: Some(SqlAuditExecutionStatus::Executed),
                ..audit_filters()
            },
        )
        .await
        .unwrap();
    assert_eq!(executed_records.total_count, 1);
    assert_eq!(executed_records.records[0].status, SqlAuditStatus::Executed);
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
                tags: None,
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

fn audit_filters<'a>() -> SqlAuditListFilters<'a> {
    SqlAuditListFilters {
        managed_database_id: None,
        status: None,
        audit_status: None,
        execution_status: None,
        created_from: None,
        created_to: None,
        page: 1,
        page_size: 100,
    }
}

fn unique_email(prefix: &str) -> String {
    format!(
        "{prefix}-{}@test.local",
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    )
}
