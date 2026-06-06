mod control;
mod ddl;
mod dml;
mod query;
mod security;

use pg_query::NodeEnum;

use crate::{
    ast::node_children,
    types::{PgSqlAnalysis, PgSqlRuleOptions},
};

pub(crate) fn inspect_statement(
    index: usize,
    node: &NodeEnum,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    match node {
        NodeEnum::SelectStmt(stmt) => query::inspect_select(index, stmt, options, analysis),
        NodeEnum::InsertStmt(stmt) => dml::inspect_insert(index, stmt, options, analysis),
        NodeEnum::UpdateStmt(stmt) => dml::inspect_update(index, stmt, options, analysis),
        NodeEnum::DeleteStmt(stmt) => dml::inspect_delete(index, stmt, options, analysis),
        NodeEnum::MergeStmt(stmt) => dml::inspect_merge(index, stmt, options, analysis),
        NodeEnum::DropStmt(stmt) => ddl::inspect_drop(index, stmt, options, analysis),
        NodeEnum::TruncateStmt(stmt) => ddl::inspect_truncate(index, stmt, options, analysis),
        NodeEnum::AlterTableStmt(stmt) => ddl::inspect_alter_table(index, stmt, options, analysis),
        NodeEnum::CreateTableAsStmt(stmt) => {
            ddl::inspect_create_table_as(index, stmt, options, analysis)
        }
        NodeEnum::IndexStmt(stmt) => ddl::inspect_index(index, stmt, analysis),
        NodeEnum::RefreshMatViewStmt(stmt) => {
            ddl::inspect_refresh_materialized_view(index, stmt, analysis)
        }
        NodeEnum::CreateExtensionStmt(stmt) => ddl::inspect_create_extension(index, stmt, analysis),
        NodeEnum::CreateFunctionStmt(stmt) => ddl::inspect_create_function(index, stmt, analysis),
        NodeEnum::CreatedbStmt(stmt) => ddl::inspect_create_database(index, stmt, analysis),
        NodeEnum::GrantStmt(stmt) => security::inspect_grant(index, stmt, analysis),
        NodeEnum::GrantRoleStmt(stmt) => security::inspect_grant_role(index, stmt, analysis),
        NodeEnum::AlterRoleStmt(stmt) => security::inspect_alter_role(index, stmt, analysis),
        NodeEnum::AlterRoleSetStmt(stmt) => security::inspect_alter_role_set(index, stmt, analysis),
        NodeEnum::DropRoleStmt(stmt) => security::inspect_drop_role(index, stmt, analysis),
        NodeEnum::CopyStmt(stmt) => control::inspect_copy(index, stmt, analysis),
        NodeEnum::LockStmt(stmt) => control::inspect_lock(index, stmt, analysis),
        NodeEnum::DoStmt(_) => control::inspect_do_block(index, analysis),
        NodeEnum::TransactionStmt(_) if options.check_transaction_controls => {
            control::inspect_transaction(index, analysis);
        }
        _ => {}
    }
}

pub(crate) fn inspect_nested_statements(
    index: usize,
    node: &NodeEnum,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    for child in node_children(node) {
        inspect_nested_statement_node(index, child, options, analysis);
    }
}

fn inspect_nested_statement_node(
    index: usize,
    node: &pg_query::protobuf::Node,
    options: &PgSqlRuleOptions,
    analysis: &mut PgSqlAnalysis,
) {
    let Some(node_enum) = node.node.as_ref() else {
        return;
    };

    if !matches!(node_enum, NodeEnum::SelectStmt(_)) {
        inspect_statement(index, node_enum, options, analysis);
    }

    for child in node_children(node_enum) {
        inspect_nested_statement_node(index, child, options, analysis);
    }
}
