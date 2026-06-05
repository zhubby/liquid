use anyhow::Result;
use async_stream::try_stream;
use async_trait::async_trait;
use liquid_core::{AuditSummary, RiskSeverity, SqlAuditFinding, SqlAuditReport, SqlAuditRequest};
use liquid_sql::{PgSqlAnalysisRequest, PgSqlFinding, PgSqlRiskSeverity, analyze_postgres_sql};

use crate::{
    agent::SqlAuditAgent,
    types::{AgentEvent, AgentStream},
};

#[derive(Debug, Default)]
pub struct MockSqlAuditAgent;

#[async_trait]
impl SqlAuditAgent for MockSqlAuditAgent {
    async fn audit_summary(&self) -> Result<AuditSummary> {
        Ok(AuditSummary::sample())
    }

    async fn audit_sql(&self, request: SqlAuditRequest) -> Result<SqlAuditReport> {
        let analysis = analyze_postgres_sql(PgSqlAnalysisRequest::new(request.sql));
        let risk_score = analysis.risk_floor().max(10);
        let findings = analysis.findings.iter().map(mock_finding_from_pg).collect();

        Ok(SqlAuditReport {
            summary: "Mock SQL audit completed.".to_owned(),
            risk_score,
            findings,
        })
    }

    async fn audit_sql_stream(&self, request: SqlAuditRequest) -> Result<AgentStream> {
        let report = self.audit_sql(request).await?;

        Ok(Box::pin(try_stream! {
            yield AgentEvent::Started;
            yield AgentEvent::Completed { report };
        }))
    }
}

fn mock_finding_from_pg(finding: &PgSqlFinding) -> SqlAuditFinding {
    SqlAuditFinding {
        title: finding.title.clone(),
        severity: risk_severity_from_pg(&finding.severity),
        explanation: finding.detail.clone(),
        recommendation: recommendation_for_rule(&finding.rule_id).to_owned(),
    }
}

fn risk_severity_from_pg(severity: &PgSqlRiskSeverity) -> RiskSeverity {
    match severity {
        PgSqlRiskSeverity::Low => RiskSeverity::Low,
        PgSqlRiskSeverity::Medium => RiskSeverity::Medium,
        PgSqlRiskSeverity::High => RiskSeverity::High,
        PgSqlRiskSeverity::Critical => RiskSeverity::Critical,
    }
}

fn recommendation_for_rule(rule_id: &str) -> &'static str {
    match rule_id {
        "parse_error" => "Fix the PostgreSQL syntax before risk review.",
        "delete_without_where" | "update_without_where" | "tautological_where" => {
            "Add a selective predicate or split the write into a reviewed migration."
        }
        "destructive_drop"
        | "destructive_truncate"
        | "dangerous_alter_table"
        | "drop_cascade"
        | "alter_table_drop_object"
        | "alter_table_rewrite_or_validate"
        | "alter_table_disables_safety" => {
            "Require explicit approval, maintenance timing, and rollback planning."
        }
        "create_index_without_concurrently" | "refresh_matview_without_concurrently" => {
            "Prefer PostgreSQL concurrent forms or schedule a maintenance window."
        }
        "select_star" => "Select only the columns required by the workflow.",
        "join_without_qualification" => "Add an explicit ON or USING condition.",
        "insert_values_row_limit" | "insert_from_select" | "copy_from" => {
            "Batch the write or use a controlled bulk-load path."
        }
        "insert_on_conflict_update" => {
            "Review conflict cardinality, update predicates, and idempotency before execution."
        }
        "high_estimated_write_rows" => {
            "Reduce affected rows, batch the write, or schedule it with explicit operational approval."
        }
        "drop_protective_constraint" => {
            "Require data integrity review and a rollback plan before dropping the constraint."
        }
        "large_table_schema_validation" => {
            "Run validation during a maintenance window or use a staged PostgreSQL-safe migration pattern."
        }
        "foreign_key_without_index" => {
            "Add a ready valid covering index on the referencing foreign-key columns before relying on this constraint at scale."
        }
        "merge_write_actions" => "Review source cardinality and each MERGE action predicate.",
        "copy_program" => "Avoid server-side program execution unless it is explicitly approved.",
        "create_extension" | "create_function" | "do_block" => {
            "Review executable database code and required privileges before execution."
        }
        "grant_privileges" | "revoke_privileges" | "grant_role" | "revoke_role" | "alter_role"
        | "alter_role_set" | "drop_role" => {
            "Require privilege-owner review and confirm operational access impact."
        }
        "select_for_locking" | "explicit_lock" => {
            "Review lock scope, transaction duration, and concurrent workload impact."
        }
        _ => "Review this deterministic PostgreSQL risk finding before execution.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_agent_uses_deterministic_sql_findings() {
        let report = MockSqlAuditAgent
            .audit_sql(SqlAuditRequest::new("select * from users"))
            .await
            .unwrap();

        assert_eq!(report.risk_score, 50);
        assert_eq!(report.findings[0].title, "Broad column projection");
        assert_eq!(report.findings[0].severity, RiskSeverity::Medium);
    }

    #[tokio::test]
    async fn mock_agent_maps_hardened_rule_recommendations() {
        let report = MockSqlAuditAgent
            .audit_sql(SqlAuditRequest::new(
                "insert into users(id, email) values (1, 'a@b.test') on conflict (id) do update set email = excluded.email",
            ))
            .await
            .unwrap();

        assert_eq!(report.findings[0].title, "INSERT ON CONFLICT DO UPDATE");
        assert_eq!(report.findings[0].severity, RiskSeverity::High);
        assert!(report.findings[0].recommendation.contains("idempotency"));
    }
}
