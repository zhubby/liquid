use pg_query::{
    NodeEnum,
    protobuf::{JoinExpr, Node, SelectStmt},
};

use crate::{
    ast::{node_children, select_child_nodes, select_has_star, select_set_operands},
    types::{PgSqlAnalysis, PgSqlFinding, PgSqlRiskSeverity, PgSqlRuleOptions},
};

pub(crate) fn inspect_select(
    index: usize,
    stmt: &SelectStmt,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    if options.check_broad_projection && select_has_star(stmt) {
        analysis.findings.push(PgSqlFinding::new(
            "select_star",
            PgSqlRiskSeverity::Medium,
            "Broad column projection",
            "SELECT * returns every visible column and can increase data exposure and scan cost.",
            Some(index),
            Some("SELECT *".to_owned()),
        ));
    }

    if !stmt.locking_clause.is_empty() {
        analysis.findings.push(PgSqlFinding::new(
            "select_for_locking",
            PgSqlRiskSeverity::Medium,
            "SELECT row locking",
            "The SELECT statement includes row locking and can block concurrent writers or readers.",
            Some(index),
            Some(format!("locking_clauses={}", stmt.locking_clause.len())),
        ));
    }

    if options.check_joins {
        inspect_joins_in_nodes(index, &stmt.from_clause, analysis);
    }

    inspect_nested_queries(index, &select_child_nodes(stmt), options, analysis);

    for operand in select_set_operands(stmt) {
        inspect_select(index, operand, options, analysis);
    }
}

pub(crate) fn inspect_nested_queries(
    index: usize,
    nodes: &[&Node],
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    for node in nodes {
        inspect_nested_query(index, node, options, analysis);
    }
}

fn inspect_nested_query(
    index: usize,
    node: &Node,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    let Some(node_enum) = node.node.as_ref() else {
        return;
    };

    if let NodeEnum::SelectStmt(select) = node_enum {
        inspect_select(index, select, options, analysis);
        return;
    }

    for child in node_children(node_enum) {
        inspect_nested_query(index, child, options, analysis);
    }
}

pub(crate) fn inspect_joins_in_nodes(index: usize, nodes: &[Node], analysis: &mut PgSqlAnalysis) {
    for node in nodes {
        inspect_joins_in_node(index, node, analysis);
    }
}

pub(crate) fn inspect_joins_in_node(index: usize, node: &Node, analysis: &mut PgSqlAnalysis) {
    let Some(node_enum) = node.node.as_ref() else {
        return;
    };

    if let NodeEnum::JoinExpr(join) = node_enum {
        inspect_join(index, join, analysis);
    }

    for child in node_children(node_enum) {
        inspect_joins_in_node(index, child, analysis);
    }
}

fn inspect_join(index: usize, join: &JoinExpr, analysis: &mut PgSqlAnalysis) {
    if join.quals.is_none() && join.using_clause.is_empty() && !join.is_natural {
        analysis.findings.push(PgSqlFinding::new(
            "join_without_qualification",
            PgSqlRiskSeverity::Medium,
            "Join without qualification",
            "The join has no ON, USING, or NATURAL qualification in the PostgreSQL AST.",
            Some(index),
            Some(format!("join_type={}", join.jointype)),
        ));
    }
}
