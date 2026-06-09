use std::{future::Future, pin::Pin};

use anyhow::{Context, Result, anyhow, bail};
use liquid_agent::{
    PostgresToolConfig, PostgresToolExecutionMode, PostgresWriteExecutionMode,
    execute_write_sql_with_rollback_with_config,
};
use liquid_core::{DatapanelQueryResult, SqlRollbackPlan, SqlStatementKind};
use liquid_sql::{PgSqlAnalysisRequest, PgSqlStatementKind, analyze_postgres_sql};
use pg_query::NodeEnum;
use serde_json::Value;
use sqlx::PgPool;
use time::OffsetDateTime;

use crate::datapanels::materialize_datapanel_query_with_pool;

const DEFAULT_CHAT_SQL_QUERY_LIMIT: usize = 100;

pub type ChatSqlExecutionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ChatSqlExecutionOutcome>> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq)]
pub enum ChatSqlExecutionOutcome {
    Query {
        statement_kind: SqlStatementKind,
        result: DatapanelQueryResult,
        saveable: bool,
        rollback: Option<SqlRollbackPlan>,
    },
    Summary {
        statement_kind: SqlStatementKind,
        affected_rows: Option<i64>,
        elapsed_ms: i64,
        rollback: Option<SqlRollbackPlan>,
    },
}

pub trait ChatSqlExecutor: Send + Sync {
    fn execute<'a>(&'a self, pool: PgPool, sql: &'a str) -> ChatSqlExecutionFuture<'a>;
}

#[derive(Debug, Default)]
pub struct DefaultChatSqlExecutor;

impl ChatSqlExecutor for DefaultChatSqlExecutor {
    fn execute<'a>(&'a self, pool: PgPool, sql: &'a str) -> ChatSqlExecutionFuture<'a> {
        Box::pin(async move { execute_chat_sql(pool, sql).await })
    }
}

async fn execute_chat_sql(pool: PgPool, sql: &str) -> Result<ChatSqlExecutionOutcome> {
    let validated = validate_chat_sql(sql)?;

    if matches!(validated.pg_statement_kind, PgSqlStatementKind::Select) {
        let result = materialize_datapanel_query_with_pool(
            pool,
            &validated.executable_sql,
            DEFAULT_CHAT_SQL_QUERY_LIMIT,
        )
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

        return Ok(ChatSqlExecutionOutcome::Query {
            statement_kind: validated.statement_kind,
            result,
            saveable: true,
            rollback: None,
        });
    }

    if statement_has_returning(&validated.executable_sql)? {
        let execution = execute_chat_write_sql(
            pool,
            &validated.executable_sql,
            PostgresWriteExecutionMode::ReturningRows {
                limit: DEFAULT_CHAT_SQL_QUERY_LIMIT,
            },
        )
        .await?;
        let rows = execution.returned_rows.unwrap_or_default();
        let result = DatapanelQueryResult {
            columns: json_columns(&rows),
            row_count: rows.len().min(i32::MAX as usize) as i32,
            rows,
            truncated: execution.returned_rows_truncated,
            elapsed_ms: execution.elapsed_ms.min(i64::MAX as u64) as i64,
            refreshed_at: OffsetDateTime::now_utc(),
        };

        return Ok(ChatSqlExecutionOutcome::Query {
            statement_kind: validated.statement_kind,
            result,
            saveable: false,
            rollback: Some(execution.rollback),
        });
    }

    let execution = execute_chat_write_sql(
        pool,
        &validated.executable_sql,
        PostgresWriteExecutionMode::Summary,
    )
    .await?;

    Ok(ChatSqlExecutionOutcome::Summary {
        statement_kind: validated.statement_kind,
        affected_rows: Some(execution.affected_rows.min(i64::MAX as u64) as i64),
        elapsed_ms: execution.elapsed_ms.min(i64::MAX as u64) as i64,
        rollback: Some(execution.rollback),
    })
}

struct ValidatedChatSql {
    executable_sql: String,
    pg_statement_kind: PgSqlStatementKind,
    statement_kind: SqlStatementKind,
}

fn validate_chat_sql(sql: &str) -> Result<ValidatedChatSql> {
    let trimmed = sql.trim();

    if trimmed.is_empty() {
        bail!("SQL is required");
    }

    let analysis = analyze_postgres_sql(PgSqlAnalysisRequest::new(trimmed));

    if let Some(error) = analysis.parse_error {
        bail!("SQL parse failed: {}", error.message);
    }

    if analysis.statements.len() != 1 {
        bail!("SQL mode requires exactly one PostgreSQL statement");
    }

    let pg_statement_kind = analysis.statements[0].kind.clone();

    if matches!(
        pg_statement_kind,
        PgSqlStatementKind::Transaction | PgSqlStatementKind::Control
    ) {
        bail!("SQL mode rejects transaction and control statements");
    }

    let statement_kind = sql_statement_kind_from_pg(&pg_statement_kind);

    Ok(ValidatedChatSql {
        executable_sql: strip_trailing_semicolon(trimmed),
        pg_statement_kind,
        statement_kind,
    })
}

async fn execute_chat_write_sql(
    pool: PgPool,
    executable_sql: &str,
    mode: PostgresWriteExecutionMode,
) -> Result<liquid_agent::PostgresWriteExecutionResult> {
    execute_write_sql_with_rollback_with_config(
        PostgresToolConfig::new(Some(pool), false, PostgresToolExecutionMode::WriteGated),
        executable_sql,
        mode,
    )
    .await
    .context("SQL execution failed")
}

fn statement_has_returning(sql: &str) -> Result<bool> {
    let parsed = pg_query::parse(sql).context("SQL parse failed")?;
    let [raw_statement] = parsed.protobuf.stmts.as_slice() else {
        return Ok(false);
    };
    let Some(node) = raw_statement
        .stmt
        .as_deref()
        .and_then(|stmt| stmt.node.as_ref())
    else {
        return Ok(false);
    };

    Ok(match node {
        NodeEnum::InsertStmt(statement) => !statement.returning_list.is_empty(),
        NodeEnum::UpdateStmt(statement) => !statement.returning_list.is_empty(),
        NodeEnum::DeleteStmt(statement) => !statement.returning_list.is_empty(),
        NodeEnum::MergeStmt(statement) => !statement.returning_list.is_empty(),
        _ => false,
    })
}

fn strip_trailing_semicolon(sql: &str) -> String {
    sql.trim_end()
        .strip_suffix(';')
        .unwrap_or(sql)
        .trim()
        .to_owned()
}

fn json_columns(rows: &[Value]) -> Vec<String> {
    let mut columns = Vec::new();

    for row in rows {
        let Some(object) = row.as_object() else {
            continue;
        };

        for key in object.keys() {
            if !columns.iter().any(|column| column == key) {
                columns.push(key.clone());
            }
        }
    }

    columns
}

fn sql_statement_kind_from_pg(kind: &PgSqlStatementKind) -> SqlStatementKind {
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
