use pg_query::protobuf::{AlterRoleSetStmt, AlterRoleStmt, DropRoleStmt, GrantRoleStmt, GrantStmt};

use crate::types::{PgSqlAnalysis, PgSqlFinding, PgSqlRiskSeverity};

pub(crate) fn inspect_grant(index: usize, stmt: &GrantStmt, analysis: &mut PgSqlAnalysis) {
    let (rule_id, title, action) = if stmt.is_grant {
        ("grant_privileges", "GRANT privileges", "grants")
    } else {
        ("revoke_privileges", "REVOKE privileges", "revokes")
    };

    analysis.findings.push(PgSqlFinding::new(
        rule_id,
        PgSqlRiskSeverity::High,
        title,
        "Privilege changes affect who can read, write, or administer database objects.",
        Some(index),
        Some(format!(
            "action={}, objtype={}, privileges={}, grantees={}, grant_option={}",
            action,
            stmt.objtype,
            stmt.privileges.len(),
            stmt.grantees.len(),
            stmt.grant_option
        )),
    ));
}

pub(crate) fn inspect_grant_role(index: usize, stmt: &GrantRoleStmt, analysis: &mut PgSqlAnalysis) {
    let (rule_id, title, action) = if stmt.is_grant {
        ("grant_role", "GRANT role", "grants")
    } else {
        ("revoke_role", "REVOKE role", "revokes")
    };

    analysis.findings.push(PgSqlFinding::new(
        rule_id,
        PgSqlRiskSeverity::High,
        title,
        "Role membership changes can alter effective privileges for users and services.",
        Some(index),
        Some(format!(
            "action={}, granted_roles={}, grantee_roles={}, options={}",
            action,
            stmt.granted_roles.len(),
            stmt.grantee_roles.len(),
            stmt.opt.len()
        )),
    ));
}

pub(crate) fn inspect_alter_role(index: usize, stmt: &AlterRoleStmt, analysis: &mut PgSqlAnalysis) {
    analysis.findings.push(PgSqlFinding::new(
        "alter_role",
        PgSqlRiskSeverity::High,
        "ALTER ROLE",
        "ALTER ROLE changes account attributes or privileges and should be reviewed explicitly.",
        Some(index),
        Some(format!(
            "options={}, action={}",
            stmt.options.len(),
            stmt.action
        )),
    ));
}

pub(crate) fn inspect_alter_role_set(
    index: usize,
    stmt: &AlterRoleSetStmt,
    analysis: &mut PgSqlAnalysis,
) {
    analysis.findings.push(PgSqlFinding::new(
        "alter_role_set",
        PgSqlRiskSeverity::Medium,
        "ALTER ROLE SET",
        "ALTER ROLE SET changes default runtime configuration for a role or database.",
        Some(index),
        Some(format!(
            "database={}, has_setstmt={}",
            stmt.database,
            stmt.setstmt.is_some()
        )),
    ));
}

pub(crate) fn inspect_drop_role(index: usize, stmt: &DropRoleStmt, analysis: &mut PgSqlAnalysis) {
    analysis.findings.push(PgSqlFinding::new(
        "drop_role",
        PgSqlRiskSeverity::Critical,
        "DROP ROLE",
        "Dropping roles can break ownership, permissions, and dependent operational access.",
        Some(index),
        Some(format!(
            "roles={}, missing_ok={}",
            stmt.roles.len(),
            stmt.missing_ok
        )),
    ));
}
