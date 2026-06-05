use std::time::Instant;

use anyhow::{Result, bail};
use async_trait::async_trait;
use liquid_llm::ToolDefinition;
use serde_json::{Value, json};

use crate::{tools::AgentTool, types::ToolOutput};

use super::{
    args::{elapsed_ms, explain_tool_supported, required_string_arg, validate_single_statement},
    config::PostgresToolContext,
    execute::readonly_transaction,
};

#[derive(Debug, Clone)]
pub(crate) struct PgExplainSqlTool {
    context: PostgresToolContext,
}

impl PgExplainSqlTool {
    pub(crate) fn new(context: PostgresToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgExplainSqlTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_explain_sql",
            "Run PostgreSQL EXPLAIN (FORMAT JSON, VERBOSE, COSTS) for a single supported statement without ANALYZE.",
            json!({
                "type": "object",
                "properties": {
                    "sql": {
                        "type": "string",
                        "description": "A single PostgreSQL SELECT, INSERT, UPDATE, DELETE, or MERGE statement to explain."
                    }
                },
                "required": ["sql"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let sql = required_string_arg(&arguments, "sql", "pg_explain_sql")?;
        let (_, statement_kind, executable_sql) =
            validate_single_statement(&sql, "pg_explain_sql")?;

        if !explain_tool_supported(&statement_kind) {
            bail!(
                "pg_explain_sql supports SELECT, INSERT, UPDATE, DELETE, and MERGE statements; got {:?}",
                statement_kind
            );
        }

        let started_at = Instant::now();
        let raw_plan = explain_sql(&self.context, &executable_sql).await?;
        let elapsed_ms = elapsed_ms(started_at);
        let summary = explain_summary(&raw_plan);

        Ok(ToolOutput::json(json!({
            "statement_kind": statement_kind,
            "elapsed_ms": elapsed_ms,
            "summary": summary,
            "raw_plan": raw_plan,
            "warnings": Vec::<String>::new(),
        })))
    }
}

async fn explain_sql(context: &PostgresToolContext, sql: &str) -> Result<Value> {
    let mut transaction = readonly_transaction(context).await?;
    let explain_sql = format!("EXPLAIN (FORMAT JSON, VERBOSE, COSTS) {sql}");
    let value = sqlx::query_scalar::<_, Value>(&explain_sql)
        .fetch_one(&mut *transaction)
        .await?;
    transaction.rollback().await?;

    Ok(value)
}

fn explain_summary(raw_plan: &Value) -> Value {
    let Some(plan) = raw_plan.get(0).and_then(|value| value.get("Plan")) else {
        return json!({
            "total_cost": 0.0,
            "plan_rows": 0,
            "nodes": Vec::<Value>::new(),
        });
    };

    let mut nodes = Vec::new();
    collect_explain_nodes(plan, &mut nodes);

    json!({
        "total_cost": plan.get("Total Cost").and_then(Value::as_f64).unwrap_or(0.0),
        "plan_rows": plan.get("Plan Rows").and_then(Value::as_i64).unwrap_or(0),
        "nodes": nodes,
    })
}

fn collect_explain_nodes(plan: &Value, nodes: &mut Vec<Value>) {
    nodes.push(json!({
        "node_type": plan.get("Node Type").and_then(Value::as_str).unwrap_or("Unknown"),
        "relation_name": plan.get("Relation Name").and_then(Value::as_str),
        "total_cost": plan.get("Total Cost").and_then(Value::as_f64).unwrap_or(0.0),
        "plan_rows": plan.get("Plan Rows").and_then(Value::as_i64).unwrap_or(0),
    }));

    if let Some(children) = plan.get("Plans").and_then(Value::as_array) {
        for child in children {
            collect_explain_nodes(child, nodes);
        }
    }
}
