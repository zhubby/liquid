use pg_query::{
    NodeEnum,
    protobuf::{CmdType, DeleteStmt, InsertStmt, MergeStmt, MergeWhenClause, Node, UpdateStmt},
};

use crate::{
    ast::{delete_child_nodes, is_tautology, merge_child_nodes, update_child_nodes},
    types::{PgSqlAnalysis, PgSqlFinding, PgSqlRiskSeverity, PgSqlRuleOptions},
};

use super::query;

pub(crate) fn inspect_insert(
    index: usize,
    stmt: &InsertStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    let Some(select_node) = stmt
        .select_stmt
        .as_deref()
        .and_then(|node| node.node.as_ref())
    else {
        return;
    };
    let NodeEnum::SelectStmt(select) = select_node else {
        return;
    };

    query::inspect_select(index, select, options, analysis);

    if select.values_lists.is_empty() && !select.from_clause.is_empty() {
        analysis.findings.push(PgSqlFinding::new(
            "insert_from_select",
            PgSqlRiskSeverity::Medium,
            "INSERT from SELECT",
            "The INSERT statement derives rows from a SELECT query; review source cardinality and target constraints.",
            Some(index),
            Some(format!("from_items={}", select.from_clause.len())),
        ));
    }

    if options.max_insert_rows == 0 {
        return;
    }

    let row_count = select.values_lists.len();
    if row_count > options.max_insert_rows {
        analysis.findings.push(PgSqlFinding::new(
            "insert_values_row_limit",
            PgSqlRiskSeverity::Medium,
            "Large INSERT VALUES batch",
            format!(
                "The INSERT statement contains {row_count} VALUES rows, exceeding the configured limit of {}.",
                options.max_insert_rows
            ),
            Some(index),
            Some(format!("{row_count} rows")),
        ));
    }
}

pub(crate) fn inspect_update(
    index: usize,
    stmt: &UpdateStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    let children = update_child_nodes(stmt);
    query::inspect_nested_queries(index, &children, options, analysis);
    if options.check_joins {
        query::inspect_joins_in_nodes(index, &stmt.from_clause, analysis);
    }

    if !options.check_dml_scope {
        return;
    }

    inspect_dml_where(
        index,
        "update_without_where",
        "UPDATE without WHERE",
        "The UPDATE statement has no WHERE clause and can modify every row in the target relation.",
        stmt.where_clause.as_deref(),
        analysis,
    );
}

pub(crate) fn inspect_delete(
    index: usize,
    stmt: &DeleteStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    let children = delete_child_nodes(stmt);
    query::inspect_nested_queries(index, &children, options, analysis);
    if options.check_joins {
        query::inspect_joins_in_nodes(index, &stmt.using_clause, analysis);
    }

    if !options.check_dml_scope {
        return;
    }

    inspect_dml_where(
        index,
        "delete_without_where",
        "DELETE without WHERE",
        "The DELETE statement has no WHERE clause and can remove every row in the target relation.",
        stmt.where_clause.as_deref(),
        analysis,
    );
}

pub(crate) fn inspect_merge(
    index: usize,
    stmt: &MergeStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    let children = merge_child_nodes(stmt);
    query::inspect_nested_queries(index, &children, options, analysis);
    if options.check_joins {
        for node in &children {
            query::inspect_joins_in_node(index, node, analysis);
        }
    }

    if !options.check_dml_scope {
        return;
    }

    let write_actions = stmt
        .merge_when_clauses
        .iter()
        .filter_map(|node| match node.node.as_ref() {
            Some(NodeEnum::MergeWhenClause(clause)) if merge_clause_writes(clause) => Some(clause),
            _ => None,
        })
        .count();

    if write_actions > 0 {
        analysis.findings.push(PgSqlFinding::new(
            "merge_write_actions",
            PgSqlRiskSeverity::High,
            "MERGE write actions",
            "The MERGE statement can update, insert, or delete rows based on source matches.",
            Some(index),
            Some(format!("write_actions={write_actions}")),
        ));
    }
}

fn inspect_dml_where(
    index: usize,
    missing_rule: &'static str,
    missing_title: &'static str,
    missing_detail: &'static str,
    where_clause: Option<&Node>,
    analysis: &mut PgSqlAnalysis,
) {
    match where_clause {
        None => analysis.findings.push(PgSqlFinding::new(
            missing_rule,
            PgSqlRiskSeverity::Critical,
            missing_title,
            missing_detail,
            Some(index),
            None,
        )),
        Some(node) if is_tautology(node) => analysis.findings.push(PgSqlFinding::new(
            "tautological_where",
            PgSqlRiskSeverity::High,
            "Tautological WHERE clause",
            "The WHERE clause is a constant true expression and does not meaningfully scope the write.",
            Some(index),
            Some("WHERE true / 1 = 1".to_owned()),
        )),
        Some(_) => {}
    }
}

fn merge_clause_writes(clause: &MergeWhenClause) -> bool {
    let Ok(command_type) = CmdType::try_from(clause.command_type) else {
        return false;
    };

    matches!(
        command_type,
        CmdType::CmdUpdate | CmdType::CmdInsert | CmdType::CmdDelete
    )
}
