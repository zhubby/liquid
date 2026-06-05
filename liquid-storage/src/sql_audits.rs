use liquid_core::{
    ApproveSqlAuditRequest, RejectSqlAuditRequest, SqlAuditExecutionResult, SqlAuditRecord,
    SqlAuditStatus, SqlStatementKind,
};
use serde_json::{Value, json};
use sqlx::Row;
use time::OffsetDateTime;

use crate::{
    error::{StorageError, map_database_error},
    managed_databases,
    store::Storage,
    traits::CreateSqlAuditRecord,
    validation::{optional_string, required_string},
};

const SQL_AUDIT_COLUMNS: &str = r#"
id::text,
owner_user_id::text,
managed_database_id::text,
managed_database_name,
managed_database_engine,
managed_database_host,
managed_database_port,
managed_database_database,
managed_database_username,
managed_database_ssl_mode,
sql,
schema,
context,
execution_purpose,
status,
statement_kind,
risk_score,
report,
deterministic_analysis,
approved_by_user_id::text,
approved_at,
approval_comment,
rejected_by_user_id::text,
rejected_at,
rejection_comment,
execution_result,
execution_error,
created_at,
updated_at,
executed_at
"#;

pub(crate) async fn create_sql_audit(
    storage: &Storage,
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
    let sql = required_string("sql", &request.sql)?;
    let schema = optional_string("schema", request.schema)?;
    let context = optional_string("context", request.context)?;
    let execution_purpose = optional_string("execution_purpose", request.execution_purpose)?;

    if matches!(status, SqlAuditStatus::PendingApproval) && execution_purpose.is_none() {
        return Err(StorageError::Validation(
            "execution_purpose is required for approvable SQL audits".to_owned(),
        ));
    }

    let snapshot = managed_databases::load_managed_database_snapshot(
        storage,
        owner_user_id,
        managed_database_id,
    )
    .await?;
    let report_json = serde_json::to_value(report).map_err(json_storage_error)?;

    let mut transaction = storage.pool.begin().await?;
    let query = format!(
        r#"
        insert into sql_audits (
            owner_user_id,
            managed_database_id,
            managed_database_name,
            managed_database_engine,
            managed_database_host,
            managed_database_port,
            managed_database_database,
            managed_database_username,
            managed_database_ssl_mode,
            sql,
            schema,
            context,
            execution_purpose,
            status,
            statement_kind,
            risk_score,
            report,
            deterministic_analysis
        )
        values (
            $1::uuid,
            $2::uuid,
            $3,
            $4,
            $5,
            $6,
            $7,
            $8,
            $9,
            $10,
            $11,
            $12,
            $13,
            $14,
            $15,
            $16,
            $17,
            $18
        )
        returning {SQL_AUDIT_COLUMNS}
        "#,
    );
    let row = sqlx::query_as::<_, SqlAuditRow>(&query)
        .bind(owner_user_id)
        .bind(&snapshot.id)
        .bind(&snapshot.name)
        .bind(snapshot.engine.as_str())
        .bind(&snapshot.host)
        .bind(snapshot.port)
        .bind(&snapshot.database)
        .bind(&snapshot.username)
        .bind(snapshot.ssl_mode.as_str())
        .bind(sql)
        .bind(schema)
        .bind(context)
        .bind(execution_purpose)
        .bind(status.as_str())
        .bind(statement_kind.map(SqlStatementKind::as_str))
        .bind(i32::from(risk_score))
        .bind(report_json)
        .bind(deterministic_analysis)
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_database_error)?;

    insert_event(
        &mut transaction,
        &row.id,
        owner_user_id,
        "created",
        Some(owner_user_id),
        None,
        None,
    )
    .await?;

    let audit_event = if matches!(status, SqlAuditStatus::Blocked) {
        "blocked"
    } else {
        "audited"
    };
    insert_event(
        &mut transaction,
        &row.id,
        owner_user_id,
        audit_event,
        Some(owner_user_id),
        None,
        Some(json!({
            "status": status.as_str(),
            "risk_score": risk_score,
        })),
    )
    .await?;

    transaction.commit().await?;
    row.try_into()
}

pub(crate) async fn list_sql_audits(
    storage: &Storage,
    owner_user_id: &str,
    managed_database_id: Option<&str>,
    status: Option<SqlAuditStatus>,
    limit: i64,
) -> Result<Vec<SqlAuditRecord>, StorageError> {
    let limit = limit.clamp(1, 100);
    let status = status.map(SqlAuditStatus::as_str);
    let query = format!(
        r#"
        select {SQL_AUDIT_COLUMNS}
        from sql_audits
        where owner_user_id = $1::uuid
          and ($2::uuid is null or managed_database_id = $2::uuid)
          and ($3::text is null or status = $3)
        order by created_at desc
        limit $4
        "#,
    );
    let rows = sqlx::query_as::<_, SqlAuditRow>(&query)
        .bind(owner_user_id)
        .bind(managed_database_id)
        .bind(status)
        .bind(limit)
        .fetch_all(&storage.pool)
        .await
        .map_err(map_database_error)?;

    rows.into_iter().map(SqlAuditRecord::try_from).collect()
}

pub(crate) async fn get_sql_audit(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<SqlAuditRecord, StorageError> {
    fetch_sql_audit(storage, owner_user_id, id).await
}

pub(crate) async fn approve_sql_audit(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
    request: ApproveSqlAuditRequest,
) -> Result<SqlAuditRecord, StorageError> {
    let comment = optional_string("comment", request.comment)?;
    let mut transaction = storage.pool.begin().await?;
    let query = format!(
        r#"
        update sql_audits
        set status = 'approved',
            approved_by_user_id = $3::uuid,
            approved_at = now(),
            approval_comment = $4,
            updated_at = now()
        where id = $1::uuid
          and owner_user_id = $2::uuid
          and status = 'pending_approval'
        returning {SQL_AUDIT_COLUMNS}
        "#,
    );
    let row = sqlx::query_as::<_, SqlAuditRow>(&query)
        .bind(id)
        .bind(owner_user_id)
        .bind(owner_user_id)
        .bind(comment.as_deref())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_database_error)?;

    let Some(row) = row else {
        drop(transaction);
        return transition_conflict(
            storage,
            owner_user_id,
            id,
            "only pending approval audits can be approved",
        )
        .await;
    };

    insert_event(
        &mut transaction,
        &row.id,
        owner_user_id,
        "approved",
        Some(owner_user_id),
        comment.as_deref(),
        None,
    )
    .await?;
    transaction.commit().await?;

    row.try_into()
}

pub(crate) async fn reject_sql_audit(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
    request: RejectSqlAuditRequest,
) -> Result<SqlAuditRecord, StorageError> {
    let comment = optional_string("comment", request.comment)?;
    let mut transaction = storage.pool.begin().await?;
    let query = format!(
        r#"
        update sql_audits
        set status = 'rejected',
            rejected_by_user_id = $3::uuid,
            rejected_at = now(),
            rejection_comment = $4,
            updated_at = now()
        where id = $1::uuid
          and owner_user_id = $2::uuid
          and status = 'pending_approval'
        returning {SQL_AUDIT_COLUMNS}
        "#,
    );
    let row = sqlx::query_as::<_, SqlAuditRow>(&query)
        .bind(id)
        .bind(owner_user_id)
        .bind(owner_user_id)
        .bind(comment.as_deref())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_database_error)?;

    let Some(row) = row else {
        drop(transaction);
        return transition_conflict(
            storage,
            owner_user_id,
            id,
            "only pending approval audits can be rejected",
        )
        .await;
    };

    insert_event(
        &mut transaction,
        &row.id,
        owner_user_id,
        "rejected",
        Some(owner_user_id),
        comment.as_deref(),
        None,
    )
    .await?;
    transaction.commit().await?;

    row.try_into()
}

pub(crate) async fn start_sql_audit_execution(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<SqlAuditRecord, StorageError> {
    let mut transaction = storage.pool.begin().await?;
    let query = format!(
        r#"
        update sql_audits
        set status = 'executing',
            execution_error = null,
            updated_at = now()
        where id = $1::uuid
          and owner_user_id = $2::uuid
          and status = 'approved'
        returning {SQL_AUDIT_COLUMNS}
        "#,
    );
    let row = sqlx::query_as::<_, SqlAuditRow>(&query)
        .bind(id)
        .bind(owner_user_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_database_error)?;

    let Some(row) = row else {
        drop(transaction);
        return transition_conflict(
            storage,
            owner_user_id,
            id,
            "only approved audits can be executed",
        )
        .await;
    };

    insert_event(
        &mut transaction,
        &row.id,
        owner_user_id,
        "execution_started",
        Some(owner_user_id),
        None,
        None,
    )
    .await?;
    transaction.commit().await?;

    row.try_into()
}

pub(crate) async fn complete_sql_audit_execution(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
    result: SqlAuditExecutionResult,
) -> Result<SqlAuditRecord, StorageError> {
    let result_json = serde_json::to_value(&result).map_err(json_storage_error)?;
    let mut transaction = storage.pool.begin().await?;
    let query = format!(
        r#"
        update sql_audits
        set status = 'executed',
            execution_result = $3,
            execution_error = null,
            executed_at = now(),
            updated_at = now()
        where id = $1::uuid
          and owner_user_id = $2::uuid
          and status = 'executing'
        returning {SQL_AUDIT_COLUMNS}
        "#,
    );
    let row = sqlx::query_as::<_, SqlAuditRow>(&query)
        .bind(id)
        .bind(owner_user_id)
        .bind(result_json.clone())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_database_error)?;

    let Some(row) = row else {
        drop(transaction);
        return transition_conflict(
            storage,
            owner_user_id,
            id,
            "only executing audits can be completed",
        )
        .await;
    };

    insert_event(
        &mut transaction,
        &row.id,
        owner_user_id,
        "executed",
        Some(owner_user_id),
        None,
        Some(result_json),
    )
    .await?;
    transaction.commit().await?;

    row.try_into()
}

pub(crate) async fn fail_sql_audit_execution(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
    error: String,
) -> Result<SqlAuditRecord, StorageError> {
    let error = required_string("execution_error", &error)?;
    let mut transaction = storage.pool.begin().await?;
    let query = format!(
        r#"
        update sql_audits
        set status = 'execution_failed',
            execution_error = $3,
            updated_at = now()
        where id = $1::uuid
          and owner_user_id = $2::uuid
          and status = 'executing'
        returning {SQL_AUDIT_COLUMNS}
        "#,
    );
    let row = sqlx::query_as::<_, SqlAuditRow>(&query)
        .bind(id)
        .bind(owner_user_id)
        .bind(&error)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_database_error)?;

    let Some(row) = row else {
        drop(transaction);
        return transition_conflict(storage, owner_user_id, id, "only executing audits can fail")
            .await;
    };

    insert_event(
        &mut transaction,
        &row.id,
        owner_user_id,
        "execution_failed",
        Some(owner_user_id),
        Some(&error),
        None,
    )
    .await?;
    transaction.commit().await?;

    row.try_into()
}

async fn transition_conflict<T>(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
    message: &str,
) -> Result<T, StorageError> {
    let _ = fetch_sql_audit(storage, owner_user_id, id).await?;
    Err(StorageError::Conflict(message.to_owned()))
}

async fn fetch_sql_audit(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<SqlAuditRecord, StorageError> {
    let query = format!(
        r#"
        select {SQL_AUDIT_COLUMNS}
        from sql_audits
        where id = $1::uuid
          and owner_user_id = $2::uuid
        "#,
    );
    let row = sqlx::query_as::<_, SqlAuditRow>(&query)
        .bind(id)
        .bind(owner_user_id)
        .fetch_optional(&storage.pool)
        .await
        .map_err(map_database_error)?;

    let Some(row) = row else {
        return Err(StorageError::NotFound);
    };

    row.try_into()
}

async fn insert_event(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    sql_audit_id: &str,
    owner_user_id: &str,
    event_type: &str,
    actor_user_id: Option<&str>,
    message: Option<&str>,
    payload: Option<Value>,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        insert into sql_audit_events (
            sql_audit_id,
            owner_user_id,
            event_type,
            actor_user_id,
            message,
            payload
        )
        values ($1::uuid, $2::uuid, $3, $4::uuid, $5, $6)
        "#,
    )
    .bind(sql_audit_id)
    .bind(owner_user_id)
    .bind(event_type)
    .bind(actor_user_id)
    .bind(message)
    .bind(payload)
    .execute(&mut **transaction)
    .await
    .map_err(map_database_error)?;

    Ok(())
}

#[derive(Debug)]
struct SqlAuditRow {
    id: String,
    owner_user_id: String,
    managed_database_id: String,
    managed_database_name: String,
    managed_database_engine: String,
    managed_database_host: String,
    managed_database_port: i32,
    managed_database_database: String,
    managed_database_username: String,
    managed_database_ssl_mode: String,
    sql: String,
    schema: Option<String>,
    context: Option<String>,
    execution_purpose: Option<String>,
    status: String,
    statement_kind: Option<String>,
    risk_score: i32,
    report: Option<Value>,
    deterministic_analysis: Option<Value>,
    approved_by_user_id: Option<String>,
    approved_at: Option<OffsetDateTime>,
    approval_comment: Option<String>,
    rejected_by_user_id: Option<String>,
    rejected_at: Option<OffsetDateTime>,
    rejection_comment: Option<String>,
    execution_result: Option<Value>,
    execution_error: Option<String>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    executed_at: Option<OffsetDateTime>,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for SqlAuditRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            owner_user_id: row.try_get("owner_user_id")?,
            managed_database_id: row.try_get("managed_database_id")?,
            managed_database_name: row.try_get("managed_database_name")?,
            managed_database_engine: row.try_get("managed_database_engine")?,
            managed_database_host: row.try_get("managed_database_host")?,
            managed_database_port: row.try_get("managed_database_port")?,
            managed_database_database: row.try_get("managed_database_database")?,
            managed_database_username: row.try_get("managed_database_username")?,
            managed_database_ssl_mode: row.try_get("managed_database_ssl_mode")?,
            sql: row.try_get("sql")?,
            schema: row.try_get("schema")?,
            context: row.try_get("context")?,
            execution_purpose: row.try_get("execution_purpose")?,
            status: row.try_get("status")?,
            statement_kind: row.try_get("statement_kind")?,
            risk_score: row.try_get("risk_score")?,
            report: row.try_get("report")?,
            deterministic_analysis: row.try_get("deterministic_analysis")?,
            approved_by_user_id: row.try_get("approved_by_user_id")?,
            approved_at: row.try_get("approved_at")?,
            approval_comment: row.try_get("approval_comment")?,
            rejected_by_user_id: row.try_get("rejected_by_user_id")?,
            rejected_at: row.try_get("rejected_at")?,
            rejection_comment: row.try_get("rejection_comment")?,
            execution_result: row.try_get("execution_result")?,
            execution_error: row.try_get("execution_error")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            executed_at: row.try_get("executed_at")?,
        })
    }
}

impl TryFrom<SqlAuditRow> for SqlAuditRecord {
    type Error = StorageError;

    fn try_from(row: SqlAuditRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            owner_user_id: row.owner_user_id,
            managed_database_id: row.managed_database_id,
            managed_database_name: row.managed_database_name,
            managed_database_engine: row.managed_database_engine,
            managed_database_host: row.managed_database_host,
            managed_database_port: row.managed_database_port,
            managed_database_database: row.managed_database_database,
            managed_database_username: row.managed_database_username,
            managed_database_ssl_mode: row.managed_database_ssl_mode,
            sql: row.sql,
            schema: row.schema,
            context: row.context,
            execution_purpose: row.execution_purpose,
            status: parse_status(&row.status)?,
            statement_kind: row
                .statement_kind
                .as_deref()
                .map(parse_statement_kind)
                .transpose()?,
            risk_score: u8::try_from(row.risk_score).map_err(|_| {
                StorageError::Validation("stored SQL audit risk score is invalid".to_owned())
            })?,
            report: row.report.map(from_json).transpose()?,
            deterministic_analysis: row.deterministic_analysis,
            approved_by_user_id: row.approved_by_user_id,
            approved_at: row.approved_at,
            approval_comment: row.approval_comment,
            rejected_by_user_id: row.rejected_by_user_id,
            rejected_at: row.rejected_at,
            rejection_comment: row.rejection_comment,
            execution_result: row.execution_result.map(from_json).transpose()?,
            execution_error: row.execution_error,
            created_at: row.created_at,
            updated_at: row.updated_at,
            executed_at: row.executed_at,
        })
    }
}

fn parse_status(value: &str) -> Result<SqlAuditStatus, StorageError> {
    match value {
        "audited" => Ok(SqlAuditStatus::Audited),
        "pending_approval" => Ok(SqlAuditStatus::PendingApproval),
        "approved" => Ok(SqlAuditStatus::Approved),
        "rejected" => Ok(SqlAuditStatus::Rejected),
        "blocked" => Ok(SqlAuditStatus::Blocked),
        "executing" => Ok(SqlAuditStatus::Executing),
        "executed" => Ok(SqlAuditStatus::Executed),
        "execution_failed" => Ok(SqlAuditStatus::ExecutionFailed),
        other => Err(StorageError::Validation(format!(
            "unsupported SQL audit status: {other}"
        ))),
    }
}

fn parse_statement_kind(value: &str) -> Result<SqlStatementKind, StorageError> {
    match value {
        "select" => Ok(SqlStatementKind::Select),
        "insert" => Ok(SqlStatementKind::Insert),
        "update" => Ok(SqlStatementKind::Update),
        "delete" => Ok(SqlStatementKind::Delete),
        "merge" => Ok(SqlStatementKind::Merge),
        "create" => Ok(SqlStatementKind::Create),
        "alter" => Ok(SqlStatementKind::Alter),
        "drop" => Ok(SqlStatementKind::Drop),
        "truncate" => Ok(SqlStatementKind::Truncate),
        "security" => Ok(SqlStatementKind::Security),
        "transaction" => Ok(SqlStatementKind::Transaction),
        "control" => Ok(SqlStatementKind::Control),
        "other" => Ok(SqlStatementKind::Other),
        other => Err(StorageError::Validation(format!(
            "unsupported SQL statement kind: {other}"
        ))),
    }
}

fn from_json<T>(value: Value) -> Result<T, StorageError>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(value).map_err(json_storage_error)
}

fn json_storage_error(error: serde_json::Error) -> StorageError {
    StorageError::Validation(format!("stored SQL audit JSON is invalid: {error}"))
}
