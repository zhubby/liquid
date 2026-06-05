use pg_query::protobuf::{CopyStmt, LockStmt};

use crate::types::{PgSqlAnalysis, PgSqlFinding, PgSqlRiskSeverity};

pub(crate) fn inspect_copy(index: usize, stmt: &CopyStmt, analysis: &mut PgSqlAnalysis) {
    if stmt.is_from {
        analysis.findings.push(PgSqlFinding::new(
            "copy_from",
            PgSqlRiskSeverity::Medium,
            "COPY FROM data load",
            "COPY FROM can bulk-load many rows and should be reviewed for source trust and target scope.",
            Some(index),
            copy_evidence(stmt),
        ));
    }

    if stmt.is_program {
        analysis.findings.push(PgSqlFinding::new(
            "copy_program",
            PgSqlRiskSeverity::Critical,
            "COPY PROGRAM execution",
            "COPY PROGRAM executes a server-side command and requires privileged review.",
            Some(index),
            copy_evidence(stmt),
        ));
    }
}

pub(crate) fn inspect_lock(index: usize, stmt: &LockStmt, analysis: &mut PgSqlAnalysis) {
    analysis.findings.push(PgSqlFinding::new(
        "explicit_lock",
        PgSqlRiskSeverity::Medium,
        "Explicit table lock",
        "LOCK TABLE can block concurrent work depending on lock mode and transaction duration.",
        Some(index),
        Some(format!(
            "relations={}, mode={}",
            stmt.relations.len(),
            stmt.mode
        )),
    ));
}

pub(crate) fn inspect_transaction(index: usize, analysis: &mut PgSqlAnalysis) {
    analysis.findings.push(PgSqlFinding::new(
        "transaction_control",
        PgSqlRiskSeverity::Low,
        "Transaction control statement",
        "The SQL changes transaction state; review ordering and rollback expectations.",
        Some(index),
        Some("transaction statement".to_owned()),
    ));
}

pub(crate) fn inspect_do_block(index: usize, analysis: &mut PgSqlAnalysis) {
    analysis.findings.push(PgSqlFinding::new(
        "do_block",
        PgSqlRiskSeverity::Medium,
        "DO block",
        "A DO block executes procedural database code that can include side effects not visible as top-level SQL statements.",
        Some(index),
        None,
    ));
}

fn copy_evidence(stmt: &CopyStmt) -> Option<String> {
    if stmt.filename.is_empty() {
        None
    } else {
        Some(format!("filename={}", stmt.filename))
    }
}
