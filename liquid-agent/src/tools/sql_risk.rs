use anyhow::{Result, anyhow};
use async_trait::async_trait;
use liquid_llm::ToolDefinition;
use liquid_sql::{
    PgSqlAnalysisRequest, PgSqlFinding, PgSqlMetadataOptions, PgSqlMetadataReport,
    PgSqlMetadataStatus, PgSqlRiskSeverity, analyze_postgres_sql,
    analyze_postgres_sql_with_database,
};
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::types::ToolOutput;

use super::registry::AgentTool;

#[derive(Debug, Clone, Default)]
pub struct SqlRiskInspectionTool {
    metadata_pool: Option<PgPool>,
    metadata_required: bool,
}

impl SqlRiskInspectionTool {
    pub fn with_metadata(metadata_pool: Option<PgPool>, metadata_required: bool) -> Self {
        Self {
            metadata_pool,
            metadata_required,
        }
    }
}

#[async_trait]
impl AgentTool for SqlRiskInspectionTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "inspect_sql_risk",
            "Inspect PostgreSQL SQL text for deterministic parser and AST risk findings.",
            json!({
                "type": "object",
                "properties": {
                    "sql": {
                        "type": "string",
                        "description": "The PostgreSQL statement or script to inspect."
                    }
                },
                "required": ["sql"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let sql = arguments
            .get("sql")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|sql| !sql.is_empty())
            .ok_or_else(|| anyhow!("inspect_sql_risk requires a non-empty sql argument"))?;
        let mut analysis = if let Some(pool) = &self.metadata_pool {
            analyze_postgres_sql_with_database(
                PgSqlAnalysisRequest::new(sql),
                pool,
                PgSqlMetadataOptions::default(),
            )
            .await
        } else {
            let mut analysis = analyze_postgres_sql(PgSqlAnalysisRequest::new(sql));
            if self.metadata_required {
                analysis.metadata = Some(PgSqlMetadataReport::unavailable(
                    "PostgreSQL metadata is required but no metadata pool is configured.",
                ));
                analysis.findings.push(PgSqlFinding::new(
                    "metadata_unavailable",
                    PgSqlRiskSeverity::Low,
                    "Metadata unavailable",
                    "PostgreSQL metadata is required but no metadata pool is configured.",
                    None,
                    None,
                ));
            }
            analysis
        };
        let metadata_status = analysis
            .metadata
            .as_ref()
            .map(|metadata| metadata.status.clone())
            .unwrap_or(PgSqlMetadataStatus::NotRequested);
        let metadata = analysis.metadata.take();

        Ok(ToolOutput::json(json!({
            "dialect": "postgresql",
            "parse_ok": analysis.parse_ok(),
            "metadata_status": metadata_status,
            "statement_count": analysis.statements.len(),
            "statements": analysis.statements,
            "findings": analysis.findings,
            "metadata": metadata,
            "parse_error": analysis.parse_error,
            "risk_floor": analysis.risk_floor(),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn inspect_sql_risk_returns_postgresql_deterministic_findings() {
        let output = SqlRiskInspectionTool::default()
            .execute(json!({
                "sql": "delete from users"
            }))
            .await
            .unwrap();
        let payload: Value = serde_json::from_str(&output.content).unwrap();

        assert_eq!(payload["dialect"], "postgresql");
        assert_eq!(payload["parse_ok"], true);
        assert_eq!(payload["metadata_status"], "not_requested");
        assert_eq!(payload["statement_count"], 1);
        assert_eq!(payload["risk_floor"], 95);
        assert_eq!(payload["findings"][0]["rule_id"], "delete_without_where");
    }

    #[tokio::test]
    async fn inspect_sql_risk_reports_required_metadata_unavailable() {
        let output = SqlRiskInspectionTool::with_metadata(None, true)
            .execute(json!({
                "sql": "select id from users"
            }))
            .await
            .unwrap();
        let payload: Value = serde_json::from_str(&output.content).unwrap();

        assert_eq!(payload["metadata_status"], "unavailable");
        assert_eq!(payload["metadata"]["status"], "unavailable");
        assert!(
            payload["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|finding| { finding["rule_id"] == "metadata_unavailable" })
        );
    }
}
