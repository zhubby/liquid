use std::{future::Future, pin::Pin, time::Instant};

use anyhow::{Context, Result, anyhow, bail};
use liquid_core::{DatapanelQueryResult, SqlStatementKind};
use liquid_sql::{PgSqlAnalysisRequest, PgSqlStatementKind, analyze_postgres_sql};
use pg_query::NodeEnum;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction};
use time::OffsetDateTime;

use crate::datapanels::materialize_datapanel_query_with_pool;

const DEFAULT_CHAT_SQL_QUERY_LIMIT: usize = 100;
const MAX_CHAT_SQL_QUERY_LIMIT: usize = 1_000;

pub type ChatSqlExecutionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ChatSqlExecutionOutcome>> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq)]
pub enum ChatSqlExecutionOutcome {
    Query {
        statement_kind: SqlStatementKind,
        result: DatapanelQueryResult,
        saveable: bool,
    },
    Summary {
        statement_kind: SqlStatementKind,
        affected_rows: Option<i64>,
        elapsed_ms: i64,
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
        });
    }

    if statement_has_returning(&validated.executable_sql)? {
        let result = materialize_returning_statement(pool, &validated.executable_sql).await?;

        return Ok(ChatSqlExecutionOutcome::Query {
            statement_kind: validated.statement_kind,
            result,
            saveable: false,
        });
    }

    execute_statement_summary(pool, &validated.executable_sql, validated.statement_kind).await
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

async fn materialize_returning_statement(
    pool: PgPool,
    executable_sql: &str,
) -> Result<DatapanelQueryResult> {
    let started_at = Instant::now();
    let fetch_limit = DEFAULT_CHAT_SQL_QUERY_LIMIT
        .saturating_add(1)
        .min(MAX_CHAT_SQL_QUERY_LIMIT + 1);
    let wrapped_sql = format!(
        "with liquid_mutation as ({}) select to_jsonb(liquid_row) as row from liquid_mutation liquid_row limit {}",
        executable_sql, fetch_limit
    );
    let mut transaction = pool
        .begin()
        .await
        .context("failed to start SQL execution transaction")?;

    set_transaction_timeouts(&mut transaction).await?;
    let rows = sqlx::query(&wrapped_sql)
        .fetch_all(&mut *transaction)
        .await
        .context("SQL execution failed")?;
    transaction
        .commit()
        .await
        .context("failed to commit SQL execution transaction")?;

    let mut row_values = rows
        .into_iter()
        .map(|row| row.get::<Value, _>("row"))
        .collect::<Vec<_>>();
    let truncated = row_values.len() > DEFAULT_CHAT_SQL_QUERY_LIMIT;

    if truncated {
        row_values.truncate(DEFAULT_CHAT_SQL_QUERY_LIMIT);
    }

    Ok(DatapanelQueryResult {
        columns: json_columns(&row_values),
        row_count: row_values.len() as i32,
        rows: row_values,
        truncated,
        elapsed_ms: elapsed_ms(started_at),
        refreshed_at: OffsetDateTime::now_utc(),
    })
}

async fn execute_statement_summary(
    pool: PgPool,
    executable_sql: &str,
    statement_kind: SqlStatementKind,
) -> Result<ChatSqlExecutionOutcome> {
    let started_at = Instant::now();
    let mut transaction = pool
        .begin()
        .await
        .context("failed to start SQL execution transaction")?;

    set_transaction_timeouts(&mut transaction).await?;
    let result = sqlx::query(executable_sql)
        .execute(&mut *transaction)
        .await
        .context("SQL execution failed")?;
    transaction
        .commit()
        .await
        .context("failed to commit SQL execution transaction")?;

    Ok(ChatSqlExecutionOutcome::Summary {
        statement_kind,
        affected_rows: Some(result.rows_affected().min(i64::MAX as u64) as i64),
        elapsed_ms: elapsed_ms(started_at),
    })
}

async fn set_transaction_timeouts(transaction: &mut Transaction<'_, Postgres>) -> Result<()> {
    sqlx::query("set local statement_timeout = '5s'")
        .execute(&mut **transaction)
        .await
        .context("failed to set SQL statement timeout")?;
    sqlx::query("set local lock_timeout = '5s'")
        .execute(&mut **transaction)
        .await
        .context("failed to set SQL lock timeout")?;

    Ok(())
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

fn elapsed_ms(started_at: Instant) -> i64 {
    started_at.elapsed().as_millis().min(i64::MAX as u128) as i64
}
