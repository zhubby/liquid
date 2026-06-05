use pg_query::NodeEnum;

use crate::types::{
    PgSqlAnalysis, PgSqlFinding, PgSqlMetadataOptions, PgSqlRiskSeverity, PgSqlStatementMetadata,
};

pub(crate) fn inspect_runtime(
    statement_index: usize,
    node: &NodeEnum,
    metadata: &PgSqlStatementMetadata,
    _options: &PgSqlMetadataOptions,
    analysis: &mut PgSqlAnalysis,
) {
    inspect_locks(statement_index, metadata, analysis);

    if matches!(node, NodeEnum::TruncateStmt(_)) {
        let estimated_rows = metadata
            .relations
            .iter()
            .filter_map(|relation| relation.estimated_rows)
            .sum::<f64>();

        if estimated_rows > 0.0 {
            analysis.findings.push(PgSqlFinding::new(
                "truncate_estimated_rows",
                PgSqlRiskSeverity::Critical,
                "TRUNCATE estimated affected rows",
                "PostgreSQL catalog statistics estimate rows currently stored in relations targeted by TRUNCATE.",
                Some(statement_index),
                Some(format!("estimated_rows={estimated_rows:.0}")),
            ));
        }
    }
}

fn inspect_locks(
    statement_index: usize,
    metadata: &PgSqlStatementMetadata,
    analysis: &mut PgSqlAnalysis,
) {
    for lock in &metadata.locks {
        if lock.conflicting_granted_locks > 0 || lock.conflicting_waiting_locks > 0 {
            analysis.findings.push(PgSqlFinding::new(
                "lock_conflict",
                PgSqlRiskSeverity::High,
                "Current lock conflict",
                "Live PostgreSQL lock metadata shows conflicting locks for a relation this statement is expected to lock.",
                Some(statement_index),
                Some(format!(
                    "relation_oid={}, expected_mode={}, granted_conflicts={}, waiting_conflicts={}, longest_conflict_age_ms={}",
                    lock.relation_oid,
                    lock.expected_mode,
                    lock.conflicting_granted_locks,
                    lock.conflicting_waiting_locks,
                    lock.longest_conflict_age_ms.unwrap_or_default()
                )),
            ));
        }
    }
}
