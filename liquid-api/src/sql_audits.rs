use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use liquid_agent::PostgresToolConfig;
use liquid_core::{
    ApproveSqlAuditRequest, CreateSqlAuditRequest, ManagedDatabase, ManagedDatabasePoolKey,
    RejectSqlAuditRequest, SqlAuditExecutionResult, SqlAuditRecord, SqlAuditStatus,
    SqlStatementKind,
};
use liquid_sql::{
    PgSqlAnalysis, PgSqlAnalysisRequest, PgSqlRiskSeverity, PgSqlStatementKind,
    analyze_postgres_sql,
};
use liquid_storage::CreateSqlAuditRecord;
use serde::Deserialize;

use crate::{auth::authenticated_user, error::ApiError, state::ApiState};

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/api/v1/managed-databases/{id}/sql-audits",
            post(create_sql_audit),
        )
        .route("/api/v1/sql-audits", get(list_sql_audits))
        .route("/api/v1/sql-audits/{id}", get(get_sql_audit))
        .route("/api/v1/sql-audits/{id}/approve", post(approve_sql_audit))
        .route("/api/v1/sql-audits/{id}/reject", post(reject_sql_audit))
        .route("/api/v1/sql-audits/{id}/execute", post(execute_sql_audit))
}

#[derive(Debug, Deserialize)]
struct ListSqlAuditsQuery {
    managed_database_id: Option<String>,
    status: Option<SqlAuditStatus>,
    limit: Option<i64>,
}

async fn create_sql_audit(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<CreateSqlAuditRequest>,
) -> Result<(StatusCode, Json<SqlAuditRecord>), ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let pool = state
        .managed_database_pools
        .get_pool(ManagedDatabasePoolKey::new(user.id.clone(), id.clone()))
        .await?;
    let analysis = analyze_postgres_sql(PgSqlAnalysisRequest::new(&request.sql));
    let statement_kind = audit_statement_kind(&analysis);
    let risk_score = analysis.risk_floor();
    let status = audit_status(&request, &analysis, statement_kind.as_ref());
    let deterministic_analysis = serde_json::to_value(&analysis).map_err(|error| {
        ApiError::internal(anyhow::anyhow!("failed to serialize SQL analysis: {error}"))
    })?;
    let tools = liquid_agent::ToolRegistry::with_postgres_tools(PostgresToolConfig::new(
        Some(pool),
        state.sql_metadata_required,
        state.sql_execution,
    ));
    let report = state
        .agent
        .audit_sql_with_tools(request.clone().into_audit_request(), tools)
        .await
        .map_err(ApiError::internal)?;
    let risk_score = risk_score.max(report.risk_score);
    let record = state
        .store
        .create_sql_audit(
            &user.id,
            &id,
            CreateSqlAuditRecord {
                request,
                report,
                deterministic_analysis,
                statement_kind: statement_kind.map(sql_statement_kind_from_pg),
                status,
                risk_score,
            },
        )
        .await?;

    Ok((StatusCode::CREATED, Json(record)))
}

async fn list_sql_audits(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<ListSqlAuditsQuery>,
) -> Result<Json<Vec<SqlAuditRecord>>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let records = state
        .store
        .list_sql_audits(
            &user.id,
            query.managed_database_id.as_deref(),
            query.status,
            query.limit.unwrap_or(50),
        )
        .await?;

    Ok(Json(records))
}

async fn get_sql_audit(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<SqlAuditRecord>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let record = state.store.get_sql_audit(&user.id, &id).await?;

    Ok(Json(record))
}

async fn approve_sql_audit(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<ApproveSqlAuditRequest>,
) -> Result<Json<SqlAuditRecord>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let record = state
        .store
        .approve_sql_audit(&user.id, &id, request)
        .await?;

    Ok(Json(record))
}

async fn reject_sql_audit(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<RejectSqlAuditRequest>,
) -> Result<Json<SqlAuditRecord>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let record = state.store.reject_sql_audit(&user.id, &id, request).await?;

    Ok(Json(record))
}

async fn execute_sql_audit(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<SqlAuditRecord>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;

    if !state.approved_write_execution_enabled {
        return Err(ApiError::forbidden(
            "approved SQL audit execution requires LIQUID_SQL_EXECUTION=write_gated",
        ));
    }

    let record = state.store.get_sql_audit(&user.id, &id).await?;
    ensure_database_snapshot_matches(&state, &user.id, &record).await?;
    let pool = state
        .managed_database_pools
        .get_pool(ManagedDatabasePoolKey::new(
            user.id.clone(),
            record.managed_database_id.clone(),
        ))
        .await?;
    let executing = state.store.start_sql_audit_execution(&user.id, &id).await?;
    let config = PostgresToolConfig::new(
        Some(pool),
        state.sql_metadata_required,
        liquid_agent::PostgresToolExecutionMode::WriteGated,
    );

    match state
        .approved_sql_executor
        .execute(config, &executing.sql)
        .await
    {
        Ok(result) => {
            let record = state
                .store
                .complete_sql_audit_execution(
                    &user.id,
                    &id,
                    SqlAuditExecutionResult {
                        statement_kind: sql_statement_kind_from_pg(result.statement_kind),
                        affected_rows: result.affected_rows,
                        elapsed_ms: result.elapsed_ms,
                        risk_floor: result.risk_floor,
                        findings: serde_json::to_value(result.analysis.findings).map_err(
                            |error| {
                                ApiError::internal(anyhow::anyhow!(
                                    "failed to serialize execution findings: {error}"
                                ))
                            },
                        )?,
                    },
                )
                .await?;

            Ok(Json(record))
        }
        Err(error) => {
            let message = error.to_string();
            let record = state
                .store
                .fail_sql_audit_execution(&user.id, &id, message.clone())
                .await?;

            if deterministic_execution_rejection(&message) {
                Err(ApiError::conflict(message))
            } else {
                Ok(Json(record))
            }
        }
    }
}

async fn ensure_database_snapshot_matches(
    state: &ApiState,
    owner_user_id: &str,
    record: &SqlAuditRecord,
) -> Result<(), ApiError> {
    let databases = state.store.list_managed_databases(owner_user_id).await?;
    let Some(database) = databases
        .iter()
        .find(|database| database.id == record.managed_database_id)
    else {
        return Err(ApiError::conflict(
            "managed database no longer exists; create a new SQL audit before executing",
        ));
    };

    if database_matches_record(database, record) {
        return Ok(());
    }

    Err(ApiError::conflict(
        "managed database connection changed; create a new SQL audit before executing",
    ))
}

fn database_matches_record(database: &ManagedDatabase, record: &SqlAuditRecord) -> bool {
    database.name == record.managed_database_name
        && database.engine.as_str() == record.managed_database_engine
        && database.host == record.managed_database_host
        && database.port == record.managed_database_port
        && database.database == record.managed_database_database
        && database.username == record.managed_database_username
        && database.ssl_mode.as_str() == record.managed_database_ssl_mode
}

fn audit_status(
    request: &CreateSqlAuditRequest,
    analysis: &PgSqlAnalysis,
    statement_kind: Option<&PgSqlStatementKind>,
) -> SqlAuditStatus {
    if has_critical_finding(analysis) {
        return SqlAuditStatus::Blocked;
    }

    match statement_kind {
        Some(PgSqlStatementKind::Select) => SqlAuditStatus::Audited,
        Some(PgSqlStatementKind::Transaction | PgSqlStatementKind::Control) => {
            SqlAuditStatus::Blocked
        }
        Some(_)
            if request
                .execution_purpose
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty()) =>
        {
            SqlAuditStatus::PendingApproval
        }
        Some(_) => SqlAuditStatus::Audited,
        None => SqlAuditStatus::Blocked,
    }
}

fn audit_statement_kind(analysis: &PgSqlAnalysis) -> Option<PgSqlStatementKind> {
    if analysis.statements.len() == 1 {
        return Some(analysis.statements[0].kind.clone());
    }

    None
}

fn has_critical_finding(analysis: &PgSqlAnalysis) -> bool {
    analysis
        .findings
        .iter()
        .any(|finding| matches!(finding.severity, PgSqlRiskSeverity::Critical))
}

fn sql_statement_kind_from_pg(kind: PgSqlStatementKind) -> SqlStatementKind {
    match kind {
        PgSqlStatementKind::Select => SqlStatementKind::Select,
        PgSqlStatementKind::Insert => SqlStatementKind::Insert,
        PgSqlStatementKind::Update => SqlStatementKind::Update,
        PgSqlStatementKind::Delete => SqlStatementKind::Delete,
        PgSqlStatementKind::Merge => SqlStatementKind::Merge,
        PgSqlStatementKind::Create => SqlStatementKind::Create,
        PgSqlStatementKind::Alter => SqlStatementKind::Alter,
        PgSqlStatementKind::Drop => SqlStatementKind::Drop,
        PgSqlStatementKind::Truncate => SqlStatementKind::Truncate,
        PgSqlStatementKind::Security => SqlStatementKind::Security,
        PgSqlStatementKind::Transaction => SqlStatementKind::Transaction,
        PgSqlStatementKind::Control => SqlStatementKind::Control,
        PgSqlStatementKind::Other => SqlStatementKind::Other,
    }
}

fn deterministic_execution_rejection(message: &str) -> bool {
    message.contains("requires valid PostgreSQL SQL")
        || message.contains("requires exactly one statement")
        || message.contains("rejects SELECT")
        || message.contains("rejects transaction and control")
        || message.contains("critical deterministic risk")
}
