use std::time::Instant;

use anyhow::{Result, bail};
use async_trait::async_trait;
use liquid_llm::ToolDefinition;
use liquid_sql::{PgSqlRiskSeverity, PgSqlStatementKind};
use serde_json::{Value, json};
use sqlx::Row;

use crate::{tools::AgentTool, types::ToolOutput};

use super::{
    args::{elapsed_ms, limit_arg, required_string_arg, validate_single_statement},
    config::PostgresToolContext,
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
        let (analysis, statement_kind, executable_sql) =
            validate_single_statement(&sql, "pg_execute_write_sql")?;

        if matches!(statement_kind, PgSqlStatementKind::Select) {
            bail!("pg_execute_write_sql rejects SELECT; use pg_execute_readonly_sql");
        }

        if matches!(
            statement_kind,
            PgSqlStatementKind::Transaction | PgSqlStatementKind::Control
        ) {
            bail!(
                "pg_execute_write_sql rejects transaction and control statements; got {:?}",
                statement_kind
            );
        }

        if analysis
            .findings
            .iter()
            .any(|finding| matches!(finding.severity, PgSqlRiskSeverity::Critical))
        {
            bail!("pg_execute_write_sql rejects statements with critical deterministic risk");
        }

        let started_at = Instant::now();
        let mut transaction = self.context.pool.begin().await?;
        set_tool_timeouts(&mut transaction, &self.context).await?;
        let result = sqlx::query(&executable_sql)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        let elapsed_ms = elapsed_ms(started_at);

        tracing::info!(
            statement_kind = ?statement_kind,
            affected_rows = result.rows_affected(),
            purpose_length = purpose.len(),
            "executed gated PostgreSQL write tool"
        );

        Ok(ToolOutput::json(json!({
            "statement_kind": statement_kind,
            "affected_rows": result.rows_affected(),
            "elapsed_ms": elapsed_ms,
            "risk_floor": analysis.risk_floor(),
            "findings": analysis.findings,
        })))
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

async fn set_tool_timeouts(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    context: &PostgresToolContext,
) -> Result<()> {
    sqlx::query("set local statement_timeout = $1")
        .bind(format!(
            "{}ms",
            context.metadata_options.statement_timeout_ms
        ))
        .execute(&mut **transaction)
        .await?;
    sqlx::query("set local lock_timeout = $1")
        .bind(format!("{}ms", context.metadata_options.lock_timeout_ms))
        .execute(&mut **transaction)
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
