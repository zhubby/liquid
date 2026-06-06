use pg_query::{
    NodeEnum,
    protobuf::{
        AlterTableCmd, AlterTableStmt, AlterTableType, CreateExtensionStmt, CreateFunctionStmt,
        CreateTableAsStmt, CreatedbStmt, DropBehavior, DropStmt, IndexStmt, RefreshMatViewStmt,
        TruncateStmt,
    },
};

use crate::types::{PgSqlAnalysis, PgSqlFinding, PgSqlRiskSeverity, PgSqlRuleOptions};

use super::query;

pub(crate) fn inspect_drop(
    index: usize,
    stmt: &DropStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    if !options.check_destructive_ddl {
        return;
    }

    analysis.findings.push(PgSqlFinding::new(
        "destructive_drop",
        PgSqlRiskSeverity::Critical,
        "Destructive DROP statement",
        "DROP removes database objects and should require explicit review and rollback planning.",
        Some(index),
        Some(format!("remove_type={}", stmt.remove_type)),
    ));

    if matches!(
        DropBehavior::try_from(stmt.behavior),
        Ok(DropBehavior::DropCascade)
    ) {
        analysis.findings.push(PgSqlFinding::new(
            "drop_cascade",
            PgSqlRiskSeverity::Critical,
            "DROP CASCADE",
            "DROP CASCADE can remove dependent database objects beyond the explicitly named target.",
            Some(index),
            Some(format!("objects={}", stmt.objects.len())),
        ));
    }
}

pub(crate) fn inspect_truncate(
    index: usize,
    stmt: &TruncateStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    if !options.check_destructive_ddl {
        return;
    }

    analysis.findings.push(PgSqlFinding::new(
        "destructive_truncate",
        PgSqlRiskSeverity::Critical,
        "Destructive TRUNCATE statement",
        "TRUNCATE removes table contents without row-by-row delete semantics.",
        Some(index),
        Some(format!("relations={}", stmt.relations.len())),
    ));
}

pub(crate) fn inspect_alter_table(
    index: usize,
    stmt: &AlterTableStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    if !options.check_destructive_ddl {
        return;
    }

    analysis.findings.push(PgSqlFinding::new(
        "dangerous_alter_table",
        PgSqlRiskSeverity::High,
        "Potentially disruptive ALTER TABLE",
        "ALTER TABLE can rewrite data, acquire strong locks, or change application-visible schema.",
        Some(index),
        Some(alter_table_evidence(stmt)),
    ));

    for command in stmt
        .cmds
        .iter()
        .filter_map(|node| match node.node.as_ref() {
            Some(NodeEnum::AlterTableCmd(command)) => Some(command.as_ref()),
            _ => None,
        })
    {
        inspect_alter_table_command(index, command, analysis);
    }
}

pub(crate) fn inspect_create_table_as(
    index: usize,
    stmt: &CreateTableAsStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    if let Some(NodeEnum::SelectStmt(select)) =
        stmt.query.as_deref().and_then(|node| node.node.as_ref())
    {
        query::inspect_select(index, select, options, analysis);
    }

    analysis.findings.push(PgSqlFinding::new(
        "create_table_as_select",
        PgSqlRiskSeverity::Medium,
        "CREATE TABLE AS query",
        "CREATE TABLE AS materializes query results and can create large tables from broad source data.",
        Some(index),
        Some(format!(
            "is_select_into={}, if_not_exists={}",
            stmt.is_select_into, stmt.if_not_exists
        )),
    ));
}

pub(crate) fn inspect_index(index: usize, stmt: &IndexStmt, analysis: &mut PgSqlAnalysis) {
    if stmt.concurrent {
        return;
    }

    analysis.findings.push(PgSqlFinding::new(
        "create_index_without_concurrently",
        PgSqlRiskSeverity::High,
        "CREATE INDEX without CONCURRENTLY",
        "CREATE INDEX without CONCURRENTLY can block writes on the target table while the index is built.",
        Some(index),
        Some(format!(
            "unique={}, primary={}, access_method={}",
            stmt.unique, stmt.primary, stmt.access_method
        )),
    ));
}

pub(crate) fn inspect_refresh_materialized_view(
    index: usize,
    stmt: &RefreshMatViewStmt,
    analysis: &mut PgSqlAnalysis,
) {
    if stmt.concurrent {
        return;
    }

    analysis.findings.push(PgSqlFinding::new(
        "refresh_matview_without_concurrently",
        PgSqlRiskSeverity::Medium,
        "REFRESH MATERIALIZED VIEW without CONCURRENTLY",
        "Refreshing a materialized view without CONCURRENTLY can block readers until refresh completes.",
        Some(index),
        Some(format!("skip_data={}", stmt.skip_data)),
    ));
}

pub(crate) fn inspect_create_extension(
    index: usize,
    stmt: &CreateExtensionStmt,
    analysis: &mut PgSqlAnalysis,
) {
    analysis.findings.push(PgSqlFinding::new(
        "create_extension",
        PgSqlRiskSeverity::High,
        "CREATE EXTENSION",
        "CREATE EXTENSION installs database-level code and objects and should be reviewed for trust and privileges.",
        Some(index),
        Some(format!(
            "extension={}, if_not_exists={}",
            stmt.extname, stmt.if_not_exists
        )),
    ));
}

pub(crate) fn inspect_create_function(
    index: usize,
    stmt: &CreateFunctionStmt,
    analysis: &mut PgSqlAnalysis,
) {
    analysis.findings.push(PgSqlFinding::new(
        "create_function",
        PgSqlRiskSeverity::Medium,
        "CREATE FUNCTION or PROCEDURE",
        "Creating executable database code can affect privileges, side effects, and runtime behavior.",
        Some(index),
        Some(format!(
            "is_procedure={}, replace={}, options={}",
            stmt.is_procedure,
            stmt.replace,
            stmt.options.len()
        )),
    ));
}

pub(crate) fn inspect_create_database(
    index: usize,
    stmt: &CreatedbStmt,
    analysis: &mut PgSqlAnalysis,
) {
    analysis.findings.push(PgSqlFinding::new(
        "create_database",
        PgSqlRiskSeverity::Medium,
        "CREATE DATABASE",
        "CREATE DATABASE creates a new database outside the current database and should be reviewed for ownership, privileges, and resource impact.",
        Some(index),
        Some(format!("database={}, options={}", stmt.dbname, stmt.options.len())),
    ));
}

fn inspect_alter_table_command(
    index: usize,
    command: &AlterTableCmd,
    analysis: &mut PgSqlAnalysis,
) {
    let Ok(subtype) = AlterTableType::try_from(command.subtype) else {
        return;
    };

    let Some((rule_id, severity, title, detail)) = alter_table_command_finding(subtype) else {
        return;
    };

    analysis.findings.push(PgSqlFinding::new(
        rule_id,
        severity,
        title,
        detail,
        Some(index),
        Some(format!(
            "subtype={}, name={}, recurse={}",
            subtype.as_str_name(),
            command.name,
            command.recurse
        )),
    ));
}

fn alter_table_command_finding(
    subtype: AlterTableType,
) -> Option<(&'static str, PgSqlRiskSeverity, &'static str, &'static str)> {
    match subtype {
        AlterTableType::AtDropColumn | AlterTableType::AtDropConstraint => Some((
            "alter_table_drop_object",
            PgSqlRiskSeverity::Critical,
            "ALTER TABLE drops schema objects",
            "The ALTER TABLE command drops a column or constraint and can permanently remove schema state.",
        )),
        AlterTableType::AtAlterColumnType
        | AlterTableType::AtSetNotNull
        | AlterTableType::AtAddColumn
        | AlterTableType::AtAddConstraint
        | AlterTableType::AtValidateConstraint => Some((
            "alter_table_rewrite_or_validate",
            PgSqlRiskSeverity::High,
            "ALTER TABLE may rewrite or validate table data",
            "The ALTER TABLE command can scan, validate, or rewrite table data and may take strong locks.",
        )),
        AlterTableType::AtDisableTrig
        | AlterTableType::AtDisableTrigAll
        | AlterTableType::AtDisableTrigUser
        | AlterTableType::AtDisableRule
        | AlterTableType::AtDisableRowSecurity => Some((
            "alter_table_disables_safety",
            PgSqlRiskSeverity::Critical,
            "ALTER TABLE disables safety controls",
            "The ALTER TABLE command disables triggers, rules, or row security behavior.",
        )),
        _ => None,
    }
}

fn alter_table_evidence(stmt: &AlterTableStmt) -> String {
    let subtypes = stmt
        .cmds
        .iter()
        .filter_map(|node| match node.node.as_ref() {
            Some(NodeEnum::AlterTableCmd(command)) => {
                AlterTableType::try_from(command.subtype).ok()
            }
            _ => None,
        })
        .map(|subtype| subtype.as_str_name())
        .collect::<Vec<_>>();

    if subtypes.is_empty() {
        format!("commands={}", stmt.cmds.len())
    } else {
        format!(
            "commands={}, subtypes={}",
            stmt.cmds.len(),
            subtypes.join(",")
        )
    }
}
