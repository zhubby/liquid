use std::collections::BTreeSet;

use pg_query::{
    NodeEnum,
    protobuf::{
        AConst, AlterTableCmd, AlterTableStmt, AlterTableType, ColumnRef, InsertStmt, JoinExpr,
        Node, SelectStmt,
    },
};

use crate::ast::{node_children, select_child_nodes};
use crate::types::{
    PgSqlAnalysis, PgSqlColumnMetadata, PgSqlFinding, PgSqlIndexMetadata, PgSqlMetadataOptions,
    PgSqlRelationMetadata, PgSqlRiskSeverity, PgSqlStatementMetadata,
};

pub(crate) fn inspect_schema(
    statement_index: usize,
    node: &NodeEnum,
    metadata: &PgSqlStatementMetadata,
    options: &PgSqlMetadataOptions,
    analysis: &mut PgSqlAnalysis,
) {
    inspect_large_tables(statement_index, metadata, options, analysis);
    inspect_invalid_indexes(statement_index, metadata, analysis);
    inspect_privileges(statement_index, metadata, analysis);
    inspect_rls(statement_index, metadata, analysis);
    inspect_constraints(statement_index, metadata, analysis);
    inspect_missing_indexes(statement_index, node, metadata, options, analysis);
    inspect_foreign_key_indexes(statement_index, metadata, options, analysis);

    match node {
        NodeEnum::AlterTableStmt(stmt) => {
            inspect_alter_table_metadata(statement_index, stmt, metadata, options, analysis);
        }
        NodeEnum::InsertStmt(stmt) => {
            inspect_insert_nullable(statement_index, stmt, metadata, analysis);
        }
        NodeEnum::IndexStmt(stmt) => {
            let target_oid = stmt
                .relation
                .as_ref()
                .and_then(|relation| {
                    metadata.relations.iter().find(|candidate| {
                        relation.relname == candidate.name
                            && (relation.schemaname.is_empty()
                                || relation.schemaname == candidate.schema)
                    })
                })
                .map(|relation| relation.oid);

            if let Some(target_oid) = target_oid {
                let new_columns = stmt
                    .index_params
                    .iter()
                    .filter_map(index_param_name)
                    .collect::<Vec<_>>();
                inspect_duplicate_index(
                    statement_index,
                    target_oid,
                    &new_columns,
                    metadata,
                    analysis,
                );
            }
        }
        _ => {}
    }
}

fn inspect_large_tables(
    statement_index: usize,
    metadata: &PgSqlStatementMetadata,
    options: &PgSqlMetadataOptions,
    analysis: &mut PgSqlAnalysis,
) {
    for relation in &metadata.relations {
        if relation.total_size_bytes >= options.large_table_threshold_bytes {
            analysis.findings.push(PgSqlFinding::new(
                "large_table_operation",
                PgSqlRiskSeverity::Medium,
                "Large table involved",
                "The statement references a table whose total relation size exceeds the configured threshold.",
                Some(statement_index),
                Some(format!(
                    "{}.{} total_size_bytes={}",
                    relation.schema, relation.name, relation.total_size_bytes
                )),
            ));
        }
    }
}

fn inspect_invalid_indexes(
    statement_index: usize,
    metadata: &PgSqlStatementMetadata,
    analysis: &mut PgSqlAnalysis,
) {
    for index in &metadata.indexes {
        if !index.is_valid || !index.is_ready {
            analysis.findings.push(PgSqlFinding::new(
                "index_not_ready",
                PgSqlRiskSeverity::Medium,
                "Index is invalid or not ready",
                "A referenced relation has an index that PostgreSQL does not consider valid or ready.",
                Some(statement_index),
                Some(format!(
                    "{}.{} valid={} ready={}",
                    index.schema, index.name, index.is_valid, index.is_ready
                )),
            ));
        }
    }
}

fn inspect_duplicate_index(
    statement_index: usize,
    relation_oid: i64,
    new_columns: &[String],
    metadata: &PgSqlStatementMetadata,
    analysis: &mut PgSqlAnalysis,
) {
    if new_columns.is_empty() {
        return;
    }

    for index in metadata
        .indexes
        .iter()
        .filter(|index| index.relation_oid == relation_oid)
    {
        if same_columns(&index.columns, new_columns) {
            analysis.findings.push(PgSqlFinding::new(
                "duplicate_index",
                PgSqlRiskSeverity::Medium,
                "Duplicate index",
                "The CREATE INDEX statement appears to duplicate an existing index column set.",
                Some(statement_index),
                Some(format!("existing_index={}.{}", index.schema, index.name)),
            ));
        }
    }
}

fn inspect_missing_indexes(
    statement_index: usize,
    node: &NodeEnum,
    metadata: &PgSqlStatementMetadata,
    options: &PgSqlMetadataOptions,
    analysis: &mut PgSqlAnalysis,
) {
    let relation_scope = RelationScope::new(node, &metadata.relations, &metadata.columns);
    if relation_scope.relations.is_empty() {
        return;
    }

    let mut predicate_columns = Vec::new();
    let mut join_columns = Vec::new();
    collect_index_candidates(node, &mut predicate_columns, &mut join_columns);

    for column in predicate_columns {
        inspect_missing_index_for_column(
            statement_index,
            "missing_predicate_index",
            "Missing predicate index",
            "A large referenced table has a deterministic predicate column with no ready valid index whose first key column matches it.",
            &column,
            &relation_scope,
            metadata,
            options,
            analysis,
        );
    }

    for column in join_columns {
        inspect_missing_index_for_column(
            statement_index,
            "missing_join_index",
            "Missing join index",
            "A large referenced table has a deterministic join key column with no ready valid index whose first key column matches it.",
            &column,
            &relation_scope,
            metadata,
            options,
            analysis,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn inspect_missing_index_for_column(
    statement_index: usize,
    rule_id: &'static str,
    title: &'static str,
    detail: &'static str,
    column: &ColumnUsage,
    relation_scope: &RelationScope<'_>,
    metadata: &PgSqlStatementMetadata,
    options: &PgSqlMetadataOptions,
    analysis: &mut PgSqlAnalysis,
) {
    let Some(relation) = relation_scope.resolve(column) else {
        return;
    };

    if relation.total_size_bytes < options.large_table_threshold_bytes {
        return;
    }

    if has_index_on_first_column(&metadata.indexes, relation.oid, &column.column_name) {
        return;
    }

    let evidence = format!(
        "{}.{} column={} total_size_bytes={}",
        relation.schema, relation.name, column.column_name, relation.total_size_bytes
    );

    if analysis.findings.iter().any(|finding| {
        finding.rule_id == rule_id
            && finding.statement_index == Some(statement_index)
            && finding.evidence.as_deref() == Some(evidence.as_str())
    }) {
        return;
    }

    analysis.findings.push(PgSqlFinding::new(
        rule_id,
        PgSqlRiskSeverity::Medium,
        title,
        detail,
        Some(statement_index),
        Some(evidence),
    ));
}

fn has_index_on_first_column(
    indexes: &[PgSqlIndexMetadata],
    relation_oid: i64,
    column_name: &str,
) -> bool {
    indexes
        .iter()
        .filter(|index| index.relation_oid == relation_oid && index.is_valid && index.is_ready)
        .any(|index| {
            index
                .columns
                .first()
                .is_some_and(|indexed| indexed.eq_ignore_ascii_case(column_name))
        })
}

fn has_ready_valid_index_prefix(
    indexes: &[PgSqlIndexMetadata],
    relation_oid: i64,
    columns: &[String],
) -> bool {
    indexes
        .iter()
        .filter(|index| index.relation_oid == relation_oid && index.is_valid && index.is_ready)
        .any(|index| {
            index.columns.len() >= columns.len()
                && index
                    .columns
                    .iter()
                    .zip(columns)
                    .all(|(indexed, required)| indexed.eq_ignore_ascii_case(required))
        })
}

fn inspect_privileges(
    statement_index: usize,
    metadata: &PgSqlStatementMetadata,
    analysis: &mut PgSqlAnalysis,
) {
    for privilege in &metadata.privileges {
        if !privilege.allowed {
            analysis.findings.push(PgSqlFinding::new(
                "missing_privilege",
                PgSqlRiskSeverity::High,
                "Missing PostgreSQL privilege",
                "The current database role does not have a required privilege for this statement.",
                Some(statement_index),
                Some(format!(
                    "relation_oid={}, action={}",
                    privilege.relation_oid, privilege.action
                )),
            ));
        }
    }
}

fn inspect_rls(
    statement_index: usize,
    metadata: &PgSqlStatementMetadata,
    analysis: &mut PgSqlAnalysis,
) {
    for rls in &metadata.rls {
        if rls.enabled && rls.applicable_policy_count == 0 && !rls.current_role_bypasses_rls {
            analysis.findings.push(PgSqlFinding::new(
                "rls_without_applicable_policy",
                PgSqlRiskSeverity::High,
                "RLS has no applicable policy",
                "Row level security is enabled, but no applicable policy was found for the current role/action.",
                Some(statement_index),
                Some(format!("relation_oid={}", rls.relation_oid)),
            ));
        }

        if rls.current_role_bypasses_rls && rls.enabled {
            analysis.findings.push(PgSqlFinding::new(
                "rls_bypassed",
                PgSqlRiskSeverity::Medium,
                "Current role bypasses RLS",
                "Row level security is enabled, but the current role can bypass RLS.",
                Some(statement_index),
                Some(format!(
                    "relation_oid={}, forced={}",
                    rls.relation_oid, rls.forced
                )),
            ));
        }
    }
}

fn inspect_constraints(
    statement_index: usize,
    metadata: &PgSqlStatementMetadata,
    analysis: &mut PgSqlAnalysis,
) {
    for constraint in &metadata.constraints {
        if !constraint.is_validated {
            analysis.findings.push(PgSqlFinding::new(
                "constraint_not_validated",
                PgSqlRiskSeverity::Medium,
                "Constraint is not validated",
                "A referenced relation has a constraint that PostgreSQL has not fully validated.",
                Some(statement_index),
                Some(format!(
                    "relation_oid={}, constraint={}, kind={}",
                    constraint.relation_oid, constraint.name, constraint.kind
                )),
            ));
        }
    }
}

fn relation_oid_for_range(
    relname: &str,
    schemaname: &str,
    metadata: &PgSqlStatementMetadata,
) -> Option<i64> {
    metadata
        .relations
        .iter()
        .find(|relation| {
            relation.name.eq_ignore_ascii_case(relname)
                && (schemaname.is_empty() || relation.schema.eq_ignore_ascii_case(schemaname))
        })
        .map(|relation| relation.oid)
}

fn alter_table_command(node: &Node) -> Option<&AlterTableCmd> {
    match node.node.as_ref()? {
        NodeEnum::AlterTableCmd(command) => Some(command.as_ref()),
        _ => None,
    }
}

fn schema_validation_subtype(subtype: AlterTableType) -> bool {
    matches!(
        subtype,
        AlterTableType::AtSetNotNull
            | AlterTableType::AtAddConstraint
            | AlterTableType::AtValidateConstraint
    )
}

fn protective_constraint_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "f" => Some("foreign_key"),
        "u" => Some("unique"),
        "p" => Some("primary_key"),
        "c" => Some("check"),
        _ => None,
    }
}

fn inspect_alter_table_metadata(
    statement_index: usize,
    stmt: &AlterTableStmt,
    metadata: &PgSqlStatementMetadata,
    options: &PgSqlMetadataOptions,
    analysis: &mut PgSqlAnalysis,
) {
    let target_oid = stmt.relation.as_ref().and_then(|target| {
        relation_oid_for_range(
            target.relname.as_str(),
            target.schemaname.as_str(),
            metadata,
        )
    });

    for command in stmt.cmds.iter().filter_map(alter_table_command) {
        let Ok(subtype) = AlterTableType::try_from(command.subtype) else {
            continue;
        };

        if matches!(subtype, AlterTableType::AtDropConstraint) {
            inspect_drop_protective_constraint(
                statement_index,
                command,
                target_oid,
                metadata,
                analysis,
            );
        }

        if schema_validation_subtype(subtype) {
            inspect_large_table_schema_validation(
                statement_index,
                subtype,
                command,
                target_oid,
                metadata,
                options,
                analysis,
            );
        }
    }
}

fn inspect_drop_protective_constraint(
    statement_index: usize,
    command: &AlterTableCmd,
    target_oid: Option<i64>,
    metadata: &PgSqlStatementMetadata,
    analysis: &mut PgSqlAnalysis,
) {
    let Some(constraint) = metadata.constraints.iter().find(|constraint| {
        constraint.name.eq_ignore_ascii_case(&command.name)
            && target_oid.is_none_or(|oid| constraint.relation_oid == oid)
            && protective_constraint_kind(&constraint.kind).is_some()
    }) else {
        return;
    };

    let kind = protective_constraint_kind(&constraint.kind).unwrap_or("constraint");
    analysis.findings.push(PgSqlFinding::new(
        "drop_protective_constraint",
        PgSqlRiskSeverity::Critical,
        "ALTER TABLE drops protective constraint",
        "The ALTER TABLE statement drops a PostgreSQL constraint that protects referential integrity, uniqueness, primary key, or check invariants.",
        Some(statement_index),
        Some(format!(
            "relation_oid={}, constraint={}, kind={kind}",
            constraint.relation_oid, constraint.name
        )),
    ));
}

#[allow(clippy::too_many_arguments)]
fn inspect_large_table_schema_validation(
    statement_index: usize,
    subtype: AlterTableType,
    command: &AlterTableCmd,
    target_oid: Option<i64>,
    metadata: &PgSqlStatementMetadata,
    options: &PgSqlMetadataOptions,
    analysis: &mut PgSqlAnalysis,
) {
    for relation in metadata.relations.iter().filter(|relation| {
        target_oid.is_none_or(|oid| relation.oid == oid)
            && relation.total_size_bytes >= options.large_table_threshold_bytes
    }) {
        analysis.findings.push(PgSqlFinding::new(
            "large_table_schema_validation",
            PgSqlRiskSeverity::High,
            "Large table schema validation",
            "The ALTER TABLE command can scan or validate data on a large relation.",
            Some(statement_index),
            Some(format!(
                "{}.{} subtype={} name={} total_size_bytes={}",
                relation.schema,
                relation.name,
                subtype.as_str_name(),
                command.name,
                relation.total_size_bytes
            )),
        ));
    }
}

fn inspect_foreign_key_indexes(
    statement_index: usize,
    metadata: &PgSqlStatementMetadata,
    options: &PgSqlMetadataOptions,
    analysis: &mut PgSqlAnalysis,
) {
    for constraint in metadata
        .constraints
        .iter()
        .filter(|constraint| constraint.kind == "f" && !constraint.columns.is_empty())
    {
        let Some(relation) = metadata
            .relations
            .iter()
            .find(|relation| relation.oid == constraint.relation_oid)
        else {
            continue;
        };

        if relation.total_size_bytes < options.large_table_threshold_bytes {
            continue;
        }

        if has_ready_valid_index_prefix(&metadata.indexes, relation.oid, &constraint.columns) {
            continue;
        }

        analysis.findings.push(PgSqlFinding::new(
            "foreign_key_without_index",
            PgSqlRiskSeverity::Medium,
            "Foreign key without covering index",
            "A large referencing table has a foreign key whose columns are not covered by a ready valid index prefix.",
            Some(statement_index),
            Some(format!(
                "{}.{} constraint={} columns={}",
                relation.schema,
                relation.name,
                constraint.name,
                constraint.columns.join(",")
            )),
        ));
    }
}

fn inspect_insert_nullable(
    statement_index: usize,
    stmt: &InsertStmt,
    metadata: &PgSqlStatementMetadata,
    analysis: &mut PgSqlAnalysis,
) {
    let Some(relation_oid) = stmt.relation.as_ref().and_then(|target| {
        metadata
            .relations
            .iter()
            .find(|relation| {
                relation.name == target.relname
                    && (target.schemaname.is_empty() || target.schemaname == relation.schema)
            })
            .map(|relation| relation.oid)
    }) else {
        return;
    };

    let required_columns = metadata
        .columns
        .iter()
        .filter(|column| {
            column.relation_oid == relation_oid
                && !column.is_nullable
                && !column.has_default
                && !column.is_identity
                && !column.is_generated
        })
        .collect::<Vec<_>>();

    if required_columns.is_empty() {
        return;
    }

    let explicit_columns = stmt
        .cols
        .iter()
        .filter_map(column_ref_name)
        .collect::<Vec<_>>();
    let explicit_column_set = explicit_columns.iter().cloned().collect::<BTreeSet<_>>();
    for required in &required_columns {
        if !explicit_columns.is_empty() && !explicit_column_set.contains(&required.name) {
            analysis.findings.push(PgSqlFinding::new(
                "insert_missing_required_column",
                PgSqlRiskSeverity::High,
                "INSERT omits required column",
                "The INSERT statement omits a NOT NULL column that has no default, identity, or generated value.",
                Some(statement_index),
                Some(format!("column={}", required.name)),
            ));
        }
    }

    if let Some(NodeEnum::SelectStmt(select)) = stmt
        .select_stmt
        .as_deref()
        .and_then(|node| node.node.as_ref())
    {
        for row in &select.values_lists {
            let Some(NodeEnum::List(values)) = row.node.as_ref() else {
                continue;
            };

            for (position, value) in values.items.iter().enumerate() {
                if !is_null_const(value) {
                    continue;
                }

                let Some(column_name) = explicit_columns
                    .get(position)
                    .or_else(|| required_columns.get(position).map(|column| &column.name))
                else {
                    continue;
                };

                if required_columns
                    .iter()
                    .any(|column| &column.name == column_name)
                {
                    analysis.findings.push(PgSqlFinding::new(
                        "insert_null_into_not_null",
                        PgSqlRiskSeverity::High,
                        "INSERT writes NULL into NOT NULL column",
                        "The INSERT statement provides NULL for a column that PostgreSQL marks NOT NULL.",
                        Some(statement_index),
                        Some(format!("column={column_name}")),
                    ));
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ColumnUsage {
    qualifier: Option<String>,
    column_name: String,
}

#[derive(Debug)]
struct RelationScope<'a> {
    relations: &'a [PgSqlRelationMetadata],
    columns: &'a [PgSqlColumnMetadata],
    aliases: Vec<RelationAlias>,
}

impl<'a> RelationScope<'a> {
    fn new(
        node: &NodeEnum,
        relations: &'a [PgSqlRelationMetadata],
        columns: &'a [PgSqlColumnMetadata],
    ) -> Self {
        let mut aliases = Vec::new();
        collect_relation_aliases(node, &mut aliases);
        Self {
            relations,
            columns,
            aliases,
        }
    }

    fn resolve(&self, usage: &ColumnUsage) -> Option<&'a PgSqlRelationMetadata> {
        if let Some(qualifier) = usage.qualifier.as_deref() {
            return self.resolve_qualified(qualifier, &usage.column_name);
        }

        let mut matches = self
            .relations
            .iter()
            .filter(|relation| self.relation_has_column(relation.oid, &usage.column_name));
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }

    fn relation_has_column(&self, relation_oid: i64, column_name: &str) -> bool {
        self.columns.iter().any(|column| {
            column.relation_oid == relation_oid && column.name.eq_ignore_ascii_case(column_name)
        })
    }

    fn resolve_qualified(
        &self,
        qualifier: &str,
        column_name: &str,
    ) -> Option<&'a PgSqlRelationMetadata> {
        if let Some(relation) = self.relations.iter().find(|relation| {
            relation.name.eq_ignore_ascii_case(qualifier)
                && self.relation_has_column(relation.oid, column_name)
        }) {
            return Some(relation);
        }

        let mut matches = self.aliases.iter().filter(|alias| {
            alias.alias.eq_ignore_ascii_case(qualifier)
                && self.relations.iter().any(|relation| {
                    relation.name.eq_ignore_ascii_case(&alias.relation_name)
                        && alias
                            .schema
                            .as_deref()
                            .is_none_or(|schema| relation.schema.eq_ignore_ascii_case(schema))
                        && self.relation_has_column(relation.oid, column_name)
                })
        });
        let alias = matches.next()?;
        if matches.next().is_some() {
            return None;
        }

        self.relations.iter().find(|relation| {
            relation.name.eq_ignore_ascii_case(&alias.relation_name)
                && alias
                    .schema
                    .as_deref()
                    .is_none_or(|schema| relation.schema.eq_ignore_ascii_case(schema))
                && self.relation_has_column(relation.oid, column_name)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RelationAlias {
    alias: String,
    schema: Option<String>,
    relation_name: String,
}

fn collect_index_candidates(
    node: &NodeEnum,
    predicate_columns: &mut Vec<ColumnUsage>,
    join_columns: &mut Vec<ColumnUsage>,
) {
    match node {
        NodeEnum::SelectStmt(stmt) => {
            collect_select_candidates(stmt, predicate_columns, join_columns)
        }
        NodeEnum::UpdateStmt(stmt) => {
            if let Some(where_clause) = stmt.where_clause.as_deref() {
                collect_predicate_columns(where_clause, predicate_columns);
            }
            for node in &stmt.from_clause {
                collect_join_columns(node, join_columns);
            }
            for child in node_children(node) {
                collect_index_candidates_from_node(child, predicate_columns, join_columns);
            }
        }
        NodeEnum::DeleteStmt(stmt) => {
            if let Some(where_clause) = stmt.where_clause.as_deref() {
                collect_predicate_columns(where_clause, predicate_columns);
            }
            for node in &stmt.using_clause {
                collect_join_columns(node, join_columns);
            }
            for child in node_children(node) {
                collect_index_candidates_from_node(child, predicate_columns, join_columns);
            }
        }
        NodeEnum::MergeStmt(stmt) => {
            if let Some(join_condition) = stmt.join_condition.as_deref() {
                collect_columns(join_condition, join_columns);
            }
            for child in node_children(node) {
                collect_index_candidates_from_node(child, predicate_columns, join_columns);
            }
        }
        _ => {
            for child in node_children(node) {
                collect_index_candidates_from_node(child, predicate_columns, join_columns);
            }
        }
    }
}

fn collect_relation_aliases(node: &NodeEnum, aliases: &mut Vec<RelationAlias>) {
    match node {
        NodeEnum::RangeVar(range) => {
            if let Some(alias) = &range.alias {
                if !alias.aliasname.is_empty() {
                    push_unique_alias(
                        aliases,
                        RelationAlias {
                            alias: alias.aliasname.clone(),
                            schema: (!range.schemaname.is_empty())
                                .then(|| range.schemaname.clone()),
                            relation_name: range.relname.clone(),
                        },
                    );
                }
            }
        }
        _ => {
            for child in node_children(node) {
                if let Some(child_node) = child.node.as_ref() {
                    collect_relation_aliases(child_node, aliases);
                }
            }
        }
    }
}

fn push_unique_alias(aliases: &mut Vec<RelationAlias>, alias: RelationAlias) {
    if !aliases.iter().any(|existing| {
        existing.alias.eq_ignore_ascii_case(&alias.alias)
            && existing
                .schema
                .as_deref()
                .unwrap_or_default()
                .eq_ignore_ascii_case(alias.schema.as_deref().unwrap_or_default())
            && existing
                .relation_name
                .eq_ignore_ascii_case(&alias.relation_name)
    }) {
        aliases.push(alias);
    }
}

fn collect_index_candidates_from_node(
    node: &Node,
    predicate_columns: &mut Vec<ColumnUsage>,
    join_columns: &mut Vec<ColumnUsage>,
) {
    if let Some(node_enum) = node.node.as_ref() {
        collect_index_candidates(node_enum, predicate_columns, join_columns);
    }
}

fn collect_select_candidates(
    stmt: &SelectStmt,
    predicate_columns: &mut Vec<ColumnUsage>,
    join_columns: &mut Vec<ColumnUsage>,
) {
    if let Some(where_clause) = stmt.where_clause.as_deref() {
        collect_predicate_columns(where_clause, predicate_columns);
    }

    for from_node in &stmt.from_clause {
        collect_join_columns(from_node, join_columns);
    }

    for child in select_child_nodes(stmt) {
        collect_index_candidates_from_node(child, predicate_columns, join_columns);
    }
}

fn collect_predicate_columns(node: &Node, predicate_columns: &mut Vec<ColumnUsage>) {
    collect_columns(node, predicate_columns);
}

fn collect_join_columns(node: &Node, join_columns: &mut Vec<ColumnUsage>) {
    let Some(node_enum) = node.node.as_ref() else {
        return;
    };

    if let NodeEnum::JoinExpr(join) = node_enum {
        collect_join_expr_columns(join, join_columns);
    }

    for child in node_children(node_enum) {
        collect_join_columns(child, join_columns);
    }
}

fn collect_join_expr_columns(join: &JoinExpr, join_columns: &mut Vec<ColumnUsage>) {
    for node in &join.using_clause {
        if let Some(column_name) = bare_string_name(node) {
            push_unique_column(
                join_columns,
                ColumnUsage {
                    qualifier: None,
                    column_name,
                },
            );
        }
    }

    if let Some(quals) = join.quals.as_deref() {
        collect_columns(quals, join_columns);
    }
}

fn collect_columns(node: &Node, columns: &mut Vec<ColumnUsage>) {
    let Some(node_enum) = node.node.as_ref() else {
        return;
    };

    match node_enum {
        NodeEnum::ColumnRef(column) => {
            if let Some(usage) = column_usage(column) {
                push_unique_column(columns, usage);
            }
        }
        NodeEnum::AExpr(expr) => {
            let expr = expr.as_ref();
            if let Some(left) = expr.lexpr.as_deref() {
                collect_columns(left, columns);
            }
            if let Some(right) = expr.rexpr.as_deref() {
                collect_columns(right, columns);
            }
        }
        NodeEnum::BoolExpr(expr) => {
            for arg in &expr.args {
                collect_columns(arg, columns);
            }
        }
        NodeEnum::NullTest(test) => {
            if let Some(arg) = test.arg.as_deref().or(test.xpr.as_deref()) {
                collect_columns(arg, columns);
            }
        }
        _ => {}
    }
}

fn push_unique_column(columns: &mut Vec<ColumnUsage>, usage: ColumnUsage) {
    if !columns.iter().any(|existing| {
        existing
            .column_name
            .eq_ignore_ascii_case(&usage.column_name)
            && existing
                .qualifier
                .as_deref()
                .unwrap_or_default()
                .eq_ignore_ascii_case(usage.qualifier.as_deref().unwrap_or_default())
    }) {
        columns.push(usage);
    }
}

fn column_usage(column: &ColumnRef) -> Option<ColumnUsage> {
    let names = column
        .fields
        .iter()
        .filter_map(bare_string_name)
        .collect::<Vec<_>>();

    match names.as_slice() {
        [column_name] => Some(ColumnUsage {
            qualifier: None,
            column_name: column_name.clone(),
        }),
        [qualifier, column_name] | [_, qualifier, column_name] => Some(ColumnUsage {
            qualifier: Some(qualifier.clone()),
            column_name: column_name.clone(),
        }),
        _ => None,
    }
}

fn bare_string_name(node: &Node) -> Option<String> {
    match node.node.as_ref() {
        Some(NodeEnum::String(value)) => Some(value.sval.clone()),
        _ => None,
    }
}

fn column_ref_name(node: &Node) -> Option<String> {
    match node.node.as_ref()? {
        NodeEnum::ResTarget(target) if !target.name.is_empty() => Some(target.name.clone()),
        NodeEnum::ColumnRef(column) => column_ref_last_name(column),
        _ => None,
    }
}

fn index_param_name(node: &Node) -> Option<String> {
    match node.node.as_ref()? {
        NodeEnum::IndexElem(index) if !index.name.is_empty() => Some(index.name.clone()),
        _ => None,
    }
}

fn column_ref_last_name(column: &ColumnRef) -> Option<String> {
    column
        .fields
        .iter()
        .rev()
        .find_map(|field| match field.node.as_ref() {
            Some(NodeEnum::String(value)) => Some(value.sval.clone()),
            _ => None,
        })
}

fn is_null_const(node: &Node) -> bool {
    matches!(
        node.node.as_ref(),
        Some(NodeEnum::AConst(AConst { val: None, .. }))
    )
}

fn same_columns(existing: &[String], proposed: &[String]) -> bool {
    existing.len() == proposed.len()
        && existing
            .iter()
            .zip(proposed)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
}
