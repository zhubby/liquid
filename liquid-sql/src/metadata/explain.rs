use pg_query::NodeEnum;

use crate::types::{
    PgSqlAnalysis, PgSqlFinding, PgSqlMetadataOptions, PgSqlRiskSeverity, PgSqlStatementMetadata,
};

pub(crate) fn inspect_explain(
    statement_index: usize,
    node: &NodeEnum,
    metadata: &PgSqlStatementMetadata,
    options: &PgSqlMetadataOptions,
    analysis: &mut PgSqlAnalysis,
) {
    if !options.explain_enabled {
        return;
    }

    let Some(plan) = &metadata.plan else {
        analysis.findings.push(PgSqlFinding::new(
            "explain_unsupported",
            PgSqlRiskSeverity::Low,
            "EXPLAIN unavailable",
            "No PostgreSQL EXPLAIN plan was available for this statement.",
            Some(statement_index),
            None,
        ));
        return;
    };

    if plan.plan_rows >= options.high_estimated_rows_threshold {
        analysis.findings.push(PgSqlFinding::new(
            "high_estimated_rows",
            PgSqlRiskSeverity::High,
            "High estimated row count",
            "PostgreSQL estimates this statement will process more rows than the configured threshold.",
            Some(statement_index),
            Some(format!("plan_rows={}", plan.plan_rows)),
        ));

        if is_write_statement(node) {
            analysis.findings.push(PgSqlFinding::new(
                "high_estimated_write_rows",
                PgSqlRiskSeverity::High,
                "High estimated write row count",
                "PostgreSQL estimates this write statement will affect or process more rows than the configured threshold.",
                Some(statement_index),
                Some(format!("plan_rows={}", plan.plan_rows)),
            ));
        }
    }

    if plan.total_cost >= options.high_total_cost_threshold as f64 {
        analysis.findings.push(PgSqlFinding::new(
            "high_plan_cost",
            PgSqlRiskSeverity::Medium,
            "High estimated plan cost",
            "PostgreSQL estimates this statement has a high total execution cost.",
            Some(statement_index),
            Some(format!("total_cost={}", plan.total_cost)),
        ));
    }

    for node in &plan.nodes {
        match node.node_type.as_str() {
            "Seq Scan" if node.plan_rows >= options.high_estimated_rows_threshold => {
                analysis.findings.push(PgSqlFinding::new(
                    "large_seq_scan",
                    PgSqlRiskSeverity::High,
                    "Large sequential scan",
                    "The EXPLAIN plan contains a sequential scan with a high estimated row count.",
                    Some(statement_index),
                    Some(format!(
                        "relation={}, rows={}",
                        node.relation_name.as_deref().unwrap_or("<unknown>"),
                        node.plan_rows
                    )),
                ));
            }
            "Nested Loop" if node.total_cost >= options.high_total_cost_threshold as f64 => {
                analysis.findings.push(PgSqlFinding::new(
                    "high_cost_nested_loop",
                    PgSqlRiskSeverity::Medium,
                    "High-cost nested loop",
                    "The EXPLAIN plan contains a nested loop whose estimated cost exceeds the configured threshold.",
                    Some(statement_index),
                    Some(format!("total_cost={}", node.total_cost)),
                ));
            }
            "Sort" | "Hash" if node.plan_rows >= options.high_estimated_rows_threshold => {
                analysis.findings.push(PgSqlFinding::new(
                    "large_memory_plan_node",
                    PgSqlRiskSeverity::Medium,
                    "Large memory-sensitive plan node",
                    "The EXPLAIN plan contains a Sort or Hash node with a high estimated row count.",
                    Some(statement_index),
                    Some(format!("node_type={}, rows={}", node.node_type, node.plan_rows)),
                ));
            }
            _ => {}
        }
    }
}

fn is_write_statement(node: &NodeEnum) -> bool {
    match node {
        NodeEnum::InsertStmt(stmt) => stmt
            .select_stmt
            .as_deref()
            .and_then(|node| node.node.as_ref())
            .is_some_and(|node| matches!(node, NodeEnum::SelectStmt(select) if select.values_lists.is_empty())),
        NodeEnum::UpdateStmt(_) | NodeEnum::DeleteStmt(_) | NodeEnum::MergeStmt(_) => true,
        _ => false,
    }
}
