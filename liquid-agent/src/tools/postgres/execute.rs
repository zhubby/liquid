use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use liquid_core::{SqlRollbackPlan, SqlRollbackStatus};
use liquid_llm::ToolDefinition;
use liquid_sql::{PgSqlAnalysis, PgSqlRiskSeverity, PgSqlStatementKind};
use pg_query::NodeEnum;
use serde_json::{Value, json};
use sqlx::Row;

use crate::{tools::AgentTool, types::ToolOutput};

use super::{
    args::{elapsed_ms, limit_arg, required_string_arg, validate_single_statement},
    config::{PostgresToolConfig, PostgresToolContext},
    rollback::{PostgresWriteExecutionMode, execute_write_sql_with_rollback},
};

#[derive(Debug, Clone)]
pub(crate) struct PgExecuteReadonlySqlTool {
    context: PostgresToolContext,
}

impl PgExecuteReadonlySqlTool {
    pub(crate) fn new(context: PostgresToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgExecuteReadonlySqlTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_execute_readonly_sql",
            "Execute one PostgreSQL read-only SELECT statement with strict limits and JSON output.",
            json!({
                "type": "object",
                "properties": {
                    "sql": {
                        "type": "string",
                        "description": "A single read-only PostgreSQL SELECT statement."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum rows to return; defaults to 100 and clamps at 1000."
                    }
                },
                "required": ["sql"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let sql = required_string_arg(&arguments, "sql", "pg_execute_readonly_sql")?;
        let limit = limit_arg(&arguments, &self.context, "pg_execute_readonly_sql")?;
        let (analysis, statement_kind, executable_sql) =
            validate_single_statement(&sql, "pg_execute_readonly_sql")?;

        if !matches!(statement_kind, PgSqlStatementKind::Select) {
            bail!(
                "pg_execute_readonly_sql only supports SELECT statements; got {:?}",
                statement_kind
            );
        }

        if analysis
            .findings
            .iter()
            .any(|finding| finding.rule_id == "select_for_locking")
        {
            bail!("pg_execute_readonly_sql rejects SELECT statements that request row locks");
        }

        let started_at = Instant::now();
        let fetch_limit = limit.saturating_add(1).min(self.context.max_limit + 1);
        let wrapped_sql = format!(
            "select to_jsonb(liquid_row) as row from ({}) liquid_row limit {}",
            executable_sql, fetch_limit
        );
        let mut transaction = readonly_transaction(&self.context).await?;
        let rows = sqlx::query(&wrapped_sql)
            .fetch_all(&mut *transaction)
            .await?;
        transaction.rollback().await?;
        let elapsed_ms = elapsed_ms(started_at);

        let mut row_values = rows
            .into_iter()
            .map(|row| row.get::<Value, _>("row"))
            .collect::<Vec<_>>();
        let truncated_by_limit = row_values.len() > limit;
        if truncated_by_limit {
            row_values.truncate(limit);
        }

        let columns = json_columns(&row_values);
        let payload = readonly_payload(
            columns,
            row_values,
            truncated_by_limit,
            elapsed_ms,
            self.context.max_output_bytes,
        );

        Ok(ToolOutput::json(payload))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PgExecuteWriteSqlTool {
    context: PostgresToolContext,
}

impl PgExecuteWriteSqlTool {
    pub(crate) fn new(context: PostgresToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgExecuteWriteSqlTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_execute_write_sql",
            "Execute one gated PostgreSQL write statement after deterministic risk inspection.",
            json!({
                "type": "object",
                "properties": {
                    "sql": {
                        "type": "string",
                        "description": "A single PostgreSQL write statement to execute."
                    },
                    "purpose": {
                        "type": "string",
                        "description": "The explicit user-approved purpose for this write."
                    }
                },
                "required": ["sql", "purpose"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let sql = required_string_arg(&arguments, "sql", "pg_execute_write_sql")?;
        let purpose = required_string_arg(&arguments, "purpose", "pg_execute_write_sql")?;

        let result =
            execute_approved_write_sql(&self.context, &sql, "pg_execute_write_sql").await?;
        tracing::info!(
            statement_kind = ?result.statement_kind,
            affected_rows = result.affected_rows,
            purpose_length = purpose.len(),
            "executed gated PostgreSQL write tool"
        );

        Ok(ToolOutput::json(json!({
            "statement_kind": result.statement_kind,
            "affected_rows": result.affected_rows,
            "elapsed_ms": result.elapsed_ms,
            "risk_floor": result.risk_floor,
            "findings": result.analysis.findings,
            "rollback": result.rollback,
        })))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApprovedWriteExecutionResult {
    pub statement_kind: PgSqlStatementKind,
    pub affected_rows: u64,
    pub elapsed_ms: u64,
    pub risk_floor: u8,
    pub analysis: PgSqlAnalysis,
    pub rollback: SqlRollbackPlan,
}

pub async fn execute_approved_write_sql_with_config(
    config: PostgresToolConfig,
    sql: &str,
) -> Result<ApprovedWriteExecutionResult> {
    let Some(pool) = config.pool.clone() else {
        bail!("approved write SQL execution requires a managed database pool");
    };
    let context = PostgresToolContext::new(pool, &config);

    execute_approved_write_sql(&context, sql, "approved_write_sql").await
}

pub(super) async fn execute_approved_write_sql(
    context: &PostgresToolContext,
    sql: &str,
    caller: &str,
) -> Result<ApprovedWriteExecutionResult> {
    let (analysis, statement_kind, executable_sql) = validate_single_statement(sql, caller)?;

    if matches!(statement_kind, PgSqlStatementKind::Select) {
        bail!("{caller} rejects SELECT; use read-only audit execution");
    }

    if matches!(
        statement_kind,
        PgSqlStatementKind::Transaction | PgSqlStatementKind::Control
    ) {
        bail!(
            "{caller} rejects transaction and control statements; got {:?}",
            statement_kind
        );
    }

    if analysis
        .findings
        .iter()
        .any(|finding| matches!(finding.severity, PgSqlRiskSeverity::Critical))
    {
        bail!("{caller} rejects statements with critical deterministic risk");
    }

    let risk_floor = analysis.risk_floor();
    let (affected_rows, elapsed_ms, rollback) = if statement_requires_autocommit(&executable_sql)? {
        let started_at = Instant::now();
        let result = execute_autocommit_write_sql(context, &executable_sql).await?;
        (
            result.rows_affected(),
            elapsed_ms(started_at),
            SqlRollbackPlan {
                status: SqlRollbackStatus::Unsupported,
                sql: None,
                reason: Some(
                    "rollback generation is unsupported for autocommit statements".to_owned(),
                ),
                generated_at: None,
            },
        )
    } else {
        let result = execute_write_sql_with_rollback(
            context,
            &executable_sql,
            PostgresWriteExecutionMode::Summary,
        )
        .await?;
        (result.affected_rows, result.elapsed_ms, result.rollback)
    };

    Ok(ApprovedWriteExecutionResult {
        statement_kind,
        affected_rows,
        elapsed_ms,
        risk_floor,
        analysis,
        rollback,
    })
}

pub(super) fn statement_requires_autocommit(sql: &str) -> Result<bool> {
    let parsed = pg_query::parse(sql)?;
    let [raw_stmt] = parsed.protobuf.stmts.as_slice() else {
        return Ok(false);
    };

    Ok(matches!(
        raw_stmt.stmt.as_deref().and_then(|stmt| stmt.node.as_ref()),
        Some(NodeEnum::CreatedbStmt(_))
    ))
}

async fn execute_autocommit_write_sql(
    context: &PostgresToolContext,
    executable_sql: &str,
) -> Result<sqlx::postgres::PgQueryResult> {
    let mut connection = context.pool.acquire().await?;
    set_session_tool_timeouts(&mut connection, context).await?;

    let execution_result = sqlx::query(executable_sql).execute(&mut *connection).await;
    let reset_result = reset_session_tool_timeouts(&mut connection).await;

    match (execution_result, reset_result) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), Ok(())) => Err(error.into()),
        (Ok(_), Err(reset_error)) => {
            connection.close_on_drop();
            Err(reset_error)
        }
        (Err(error), Err(reset_error)) => {
            connection.close_on_drop();
            Err(anyhow!(
                "{error}; additionally failed to reset session SQL timeouts: {reset_error}"
            ))
        }
    }
}

pub(super) async fn readonly_transaction(
    context: &PostgresToolContext,
) -> Result<sqlx::Transaction<'_, sqlx::Postgres>> {
    let mut transaction = context.pool.begin().await?;
    sqlx::query("set transaction read only")
        .execute(&mut *transaction)
        .await?;
    set_tool_timeouts(&mut transaction, context).await?;

    Ok(transaction)
}

pub(super) async fn set_tool_timeouts(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    context: &PostgresToolContext,
) -> Result<()> {
    sqlx::query("select set_config('statement_timeout', $1, true)")
        .bind(format!(
            "{}ms",
            context.metadata_options.statement_timeout_ms
        ))
        .execute(&mut **transaction)
        .await?;
    sqlx::query("select set_config('lock_timeout', $1, true)")
        .bind(format!("{}ms", context.metadata_options.lock_timeout_ms))
        .execute(&mut **transaction)
        .await?;

    Ok(())
}

async fn set_session_tool_timeouts(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    context: &PostgresToolContext,
) -> Result<()> {
    sqlx::query("select set_config('statement_timeout', $1, false)")
        .bind(format!(
            "{}ms",
            context.metadata_options.statement_timeout_ms
        ))
        .execute(&mut **connection)
        .await?;
    sqlx::query("select set_config('lock_timeout', $1, false)")
        .bind(format!("{}ms", context.metadata_options.lock_timeout_ms))
        .execute(&mut **connection)
        .await?;

    Ok(())
}

async fn reset_session_tool_timeouts(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
) -> Result<()> {
    sqlx::query("reset statement_timeout")
        .execute(&mut **connection)
        .await?;
    sqlx::query("reset lock_timeout")
        .execute(&mut **connection)
        .await?;

    Ok(())
}

pub(super) fn readonly_payload(
    columns: Vec<String>,
    mut rows: Vec<Value>,
    truncated_by_limit: bool,
    elapsed_ms: u64,
    max_output_bytes: usize,
) -> Value {
    let mut truncated_by_output = false;

    loop {
        let payload = json!({
            "columns": columns,
            "rows": rows,
            "row_count": rows.len(),
            "truncated": truncated_by_limit || truncated_by_output,
            "elapsed_ms": elapsed_ms,
        });

        if serde_json::to_vec(&payload)
            .map(|bytes| bytes.len() <= max_output_bytes)
            .unwrap_or(false)
            || rows.is_empty()
        {
            return payload;
        }

        rows.pop();
        truncated_by_output = true;
    }
}

fn json_columns(rows: &[Value]) -> Vec<String> {
    let mut columns = Vec::new();

    for row in rows {
        let Some(object) = row.as_object() else {
            continue;
        };

        for key in object.keys() {
            if !columns.contains(key) {
                columns.push(key.clone());
            }
        }
    }

    columns
}
