use async_trait::async_trait;
use pg_query::{
    NodeEnum,
    protobuf::{ObjectType, RangeVar},
};
use serde_json::Value;
use sqlx::{PgPool, Row};

use crate::{
    metadata::{PgSqlMetadataError, PgSqlMetadataProvider, PgSqlStatementMetadataRequest},
    types::{
        PgSqlColumnMetadata, PgSqlConstraintMetadata, PgSqlIndexMetadata, PgSqlLockMetadata,
        PgSqlMetadataOptions, PgSqlPlanMetadata, PgSqlPlanNodeMetadata, PgSqlPrivilegeMetadata,
        PgSqlRelationMetadata, PgSqlRelationRef, PgSqlRlsMetadata, PgSqlStatementMetadata,
    },
};

pub async fn analyze_postgres_sql_with_database(
    request: crate::types::PgSqlAnalysisRequest,
    pool: &PgPool,
    options: PgSqlMetadataOptions,
) -> crate::types::PgSqlAnalysis {
    crate::metadata::analyze_postgres_sql_with_metadata(
        request,
        &PgSqlDatabaseMetadataProvider::new(pool.clone()),
        options,
    )
    .await
}

#[derive(Debug, Clone)]
pub struct PgSqlDatabaseMetadataProvider {
    pool: PgPool,
}

impl PgSqlDatabaseMetadataProvider {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PgSqlMetadataProvider for PgSqlDatabaseMetadataProvider {
    async fn metadata_for_statement(
        &self,
        request: PgSqlStatementMetadataRequest<'_>,
    ) -> Result<PgSqlStatementMetadata, PgSqlMetadataError> {
        let mut metadata = PgSqlStatementMetadata::new(request.statement_index);
        let relation_refs = relation_refs(request.node);
        let relations = resolve_relations(&self.pool, &relation_refs)
            .await
            .map_err(to_metadata_error)?;

        metadata.relations = relations;

        if metadata.relations.is_empty() {
            metadata
                .warnings
                .push("No catalog-backed relations were resolved for this statement.".to_owned());
        }

        let relation_oids = metadata
            .relations
            .iter()
            .map(|relation| relation.oid)
            .collect::<Vec<_>>();

        metadata.indexes = fetch_indexes(&self.pool, &relation_oids)
            .await
            .map_err(to_metadata_error)?;
        metadata.constraints = fetch_constraints(&self.pool, &relation_oids)
            .await
            .map_err(to_metadata_error)?;
        metadata.columns = fetch_columns(&self.pool, &relation_oids)
            .await
            .map_err(to_metadata_error)?;
        metadata.privileges = fetch_privileges(&self.pool, request.node, &metadata.relations)
            .await
            .map_err(to_metadata_error)?;
        metadata.rls = fetch_rls(&self.pool, request.node, &relation_oids)
            .await
            .map_err(to_metadata_error)?;

        if request.options.runtime_enabled {
            metadata.locks = fetch_locks(&self.pool, request.node, &relation_oids)
                .await
                .map_err(to_metadata_error)?;
        }

        if request.options.explain_enabled && explain_supported(request.node) {
            metadata.plan = fetch_explain(
                &self.pool,
                request.statement_index,
                request.sql,
                request.options,
            )
            .await
            .map_err(to_metadata_error)?;
        } else if request.options.explain_enabled {
            metadata
                .warnings
                .push("EXPLAIN is unsupported for this statement kind.".to_owned());
        }

        Ok(metadata)
    }
}

async fn resolve_relations(
    pool: &PgPool,
    refs: &[PgSqlRelationRef],
) -> Result<Vec<PgSqlRelationMetadata>, sqlx::Error> {
    let mut relations = Vec::new();

    for relation_ref in refs {
        let row = sqlx::query(
            r#"
            select
              c.oid::bigint,
              n.nspname,
              c.relname,
              c.relkind::text,
              pg_get_userbyid(c.relowner) as owner,
              pg_total_relation_size(c.oid)::bigint as total_size_bytes,
              pg_relation_size(c.oid)::bigint as relation_size_bytes,
              c.reltuples::float8 as estimated_rows,
              s.n_live_tup::bigint,
              s.n_dead_tup::bigint,
              (c.relkind = 'p') as is_partitioned,
              (
                select count(*)::bigint
                from pg_inherits i
                where i.inhparent = c.oid
              ) as partition_count
            from pg_class c
            join pg_namespace n on n.oid = c.relnamespace
            left join pg_stat_user_tables s on s.relid = c.oid
            where c.relname = $1
              and c.relkind in ('r', 'p', 'v', 'm', 'f')
              and (
                $2::text is not null
                and n.nspname = $2
                or $2::text is null
                and n.nspname = any(current_schemas(true))
              )
            order by array_position(current_schemas(true), n.nspname) nulls last
            limit 1
            "#,
        )
        .bind(&relation_ref.name)
        .bind(relation_ref.schema.as_deref())
        .fetch_optional(pool)
        .await?;

        let Some(row) = row else {
            continue;
        };

        relations.push(PgSqlRelationMetadata {
            oid: row.get("oid"),
            schema: row.get("nspname"),
            name: row.get("relname"),
            kind: row.get("relkind"),
            owner: row.get("owner"),
            total_size_bytes: row.get("total_size_bytes"),
            relation_size_bytes: row.get("relation_size_bytes"),
            estimated_rows: row.get("estimated_rows"),
            live_rows: row.get("n_live_tup"),
            dead_rows: row.get("n_dead_tup"),
            is_partitioned: row.get("is_partitioned"),
            partition_count: row.get("partition_count"),
        });
    }

    Ok(relations)
}

async fn fetch_indexes(
    pool: &PgPool,
    relation_oids: &[i64],
) -> Result<Vec<PgSqlIndexMetadata>, sqlx::Error> {
    if relation_oids.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query(
        r#"
        select
          t.oid::bigint as relation_oid,
          i.indexrelid::bigint as index_oid,
          ni.nspname as schema_name,
          ci.relname as index_name,
          coalesce(array_remove(array_agg(a.attname order by key_ord.ordinality), null), array[]::text[]) as columns,
          ix.indisunique,
          ix.indisprimary,
          ix.indisvalid,
          ix.indisready,
          pg_get_expr(ix.indpred, ix.indrelid) as predicate,
          pg_get_indexdef(i.indexrelid) as definition
        from pg_class t
        join pg_index ix on ix.indrelid = t.oid
        join pg_class ci on ci.oid = ix.indexrelid
        join pg_namespace ni on ni.oid = ci.relnamespace
        join pg_class i on i.oid = ix.indexrelid
        left join unnest(ix.indkey) with ordinality as key_ord(attnum, ordinality) on true
        left join pg_attribute a on a.attrelid = t.oid and a.attnum = key_ord.attnum
        where t.oid::bigint = any($1::bigint[])
        group by t.oid, i.indexrelid, ni.nspname, ci.relname, ix.indisunique, ix.indisprimary,
                 ix.indisvalid, ix.indisready, ix.indpred, ix.indrelid
        "#,
    )
    .bind(relation_oids)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PgSqlIndexMetadata {
            relation_oid: row.get("relation_oid"),
            index_oid: row.get("index_oid"),
            schema: row.get("schema_name"),
            name: row.get("index_name"),
            columns: row.get::<Vec<String>, _>("columns"),
            is_unique: row.get("indisunique"),
            is_primary: row.get("indisprimary"),
            is_valid: row.get("indisvalid"),
            is_ready: row.get("indisready"),
            predicate: row.get("predicate"),
            definition: row.get("definition"),
        })
        .collect())
}

async fn fetch_constraints(
    pool: &PgPool,
    relation_oids: &[i64],
) -> Result<Vec<PgSqlConstraintMetadata>, sqlx::Error> {
    if relation_oids.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query(
        r#"
        select
          conrelid::bigint as relation_oid,
          conname,
          contype::text as kind,
          coalesce(array_remove(array_agg(a.attname order by ordinality), null), array[]::text[]) as columns,
          convalidated,
          pg_get_constraintdef(pg_constraint.oid, true) as definition
        from pg_constraint
        left join unnest(conkey) with ordinality as key(attnum, ordinality) on true
        left join pg_attribute a on a.attrelid = conrelid and a.attnum = key.attnum
        where conrelid::bigint = any($1::bigint[])
        group by pg_constraint.oid
        "#,
    )
    .bind(relation_oids)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PgSqlConstraintMetadata {
            relation_oid: row.get("relation_oid"),
            name: row.get("conname"),
            kind: row.get("kind"),
            columns: row.get::<Vec<String>, _>("columns"),
            is_validated: row.get("convalidated"),
            definition: row.get("definition"),
        })
        .collect())
}

async fn fetch_columns(
    pool: &PgPool,
    relation_oids: &[i64],
) -> Result<Vec<PgSqlColumnMetadata>, sqlx::Error> {
    if relation_oids.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query(
        r#"
        select
          attrelid::bigint as relation_oid,
          attname,
          not attnotnull as is_nullable,
          atthasdef as has_default,
          attidentity <> '' as is_identity,
          attgenerated <> '' as is_generated
        from pg_attribute
        where attrelid::bigint = any($1::bigint[])
          and attnum > 0
          and not attisdropped
        "#,
    )
    .bind(relation_oids)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PgSqlColumnMetadata {
            relation_oid: row.get("relation_oid"),
            name: row.get("attname"),
            is_nullable: row.get("is_nullable"),
            has_default: row.get("has_default"),
            is_identity: row.get("is_identity"),
            is_generated: row.get("is_generated"),
        })
        .collect())
}

async fn fetch_privileges(
    pool: &PgPool,
    node: &NodeEnum,
    relations: &[PgSqlRelationMetadata],
) -> Result<Vec<PgSqlPrivilegeMetadata>, sqlx::Error> {
    let mut privileges = Vec::new();

    for relation in relations {
        let actions = required_privileges(node, relation);
        for action in &actions {
            let allowed = if *action == "OWNER" {
                sqlx::query_scalar::<_, bool>(
                    "select coalesce((select pg_has_role(c.relowner, 'USAGE') from pg_class c where c.oid = $1::bigint::oid), false)",
                )
                .bind(relation.oid)
                .fetch_one(pool)
                .await?
            } else {
                sqlx::query_scalar::<_, bool>("select has_table_privilege($1::bigint::oid, $2)")
                    .bind(relation.oid)
                    .bind(action)
                    .fetch_one(pool)
                    .await?
            };

            privileges.push(PgSqlPrivilegeMetadata {
                relation_oid: relation.oid,
                action: action.to_string(),
                allowed,
            });
        }
    }

    Ok(privileges)
}

async fn fetch_rls(
    pool: &PgPool,
    node: &NodeEnum,
    relation_oids: &[i64],
) -> Result<Vec<PgSqlRlsMetadata>, sqlx::Error> {
    if relation_oids.is_empty() {
        return Ok(Vec::new());
    }

    let command = rls_command(node);
    let rows = sqlx::query(
        r#"
        select
          c.oid::bigint as relation_oid,
          c.relrowsecurity as enabled,
          c.relforcerowsecurity as forced,
          r.rolbypassrls as current_role_bypasses_rls,
          (
            select count(*)::bigint from pg_policy p where p.polrelid = c.oid
          ) as policy_count,
          (
            select count(*)::bigint
            from pg_policy p
            where p.polrelid = c.oid
              and ($2::text is null or p.polcmd = '*' or p.polcmd = $2)
          ) as applicable_policy_count
        from pg_class c
        cross join pg_roles r
        where c.oid::bigint = any($1::bigint[])
          and r.rolname = current_user
        "#,
    )
    .bind(relation_oids)
    .bind(command)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PgSqlRlsMetadata {
            relation_oid: row.get("relation_oid"),
            enabled: row.get("enabled"),
            forced: row.get("forced"),
            current_role_bypasses_rls: row.get("current_role_bypasses_rls"),
            policy_count: row.get("policy_count"),
            applicable_policy_count: row.get("applicable_policy_count"),
        })
        .collect())
}

async fn fetch_locks(
    pool: &PgPool,
    node: &NodeEnum,
    relation_oids: &[i64],
) -> Result<Vec<PgSqlLockMetadata>, sqlx::Error> {
    if relation_oids.is_empty() {
        return Ok(Vec::new());
    }

    let expected_mode = expected_lock_mode(node);
    let conflicting_modes = conflicting_lock_modes(expected_mode);
    let rows = sqlx::query(
        r#"
        select
          relation::bigint as relation_oid,
          count(*) filter (where granted)::bigint as conflicting_granted_locks,
          count(*) filter (where not granted)::bigint as conflicting_waiting_locks,
          extract(epoch from max(now() - a.query_start))::bigint * 1000 as longest_conflict_age_ms
        from pg_locks l
        left join pg_stat_activity a on a.pid = l.pid
        where relation::bigint = any($1::bigint[])
          and pid <> pg_backend_pid()
          and locktype = 'relation'
          and mode = any($2::text[])
        group by relation
        "#,
    )
    .bind(relation_oids)
    .bind(conflicting_modes)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PgSqlLockMetadata {
            relation_oid: row.get("relation_oid"),
            expected_mode: expected_mode.to_owned(),
            conflicting_granted_locks: row.get("conflicting_granted_locks"),
            conflicting_waiting_locks: row.get("conflicting_waiting_locks"),
            longest_conflict_age_ms: row.get("longest_conflict_age_ms"),
        })
        .collect())
}

async fn fetch_explain(
    pool: &PgPool,
    statement_index: usize,
    sql: &str,
    options: &PgSqlMetadataOptions,
) -> Result<Option<PgSqlPlanMetadata>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query("select set_config('statement_timeout', $1, true)")
        .bind(format!("{}ms", options.statement_timeout_ms))
        .execute(&mut *transaction)
        .await?;
    sqlx::query("select set_config('lock_timeout', $1, true)")
        .bind(format!("{}ms", options.lock_timeout_ms))
        .execute(&mut *transaction)
        .await?;

    let explain_sql = format!("EXPLAIN (FORMAT JSON, VERBOSE, COSTS) {sql}");
    let value = sqlx::query_scalar::<_, Value>(&explain_sql)
        .fetch_one(&mut *transaction)
        .await?;
    transaction.rollback().await?;

    Ok(parse_plan(value, statement_index))
}

fn parse_plan(value: Value, statement_index: usize) -> Option<PgSqlPlanMetadata> {
    let plan = value.get(0)?.get("Plan")?;
    let mut nodes = Vec::new();
    collect_plan_nodes(plan, &mut nodes);

    Some(PgSqlPlanMetadata {
        statement_index,
        total_cost: plan
            .get("Total Cost")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        plan_rows: plan.get("Plan Rows").and_then(Value::as_i64).unwrap_or(0),
        nodes,
    })
}

fn collect_plan_nodes(plan: &Value, nodes: &mut Vec<PgSqlPlanNodeMetadata>) {
    nodes.push(PgSqlPlanNodeMetadata {
        node_type: plan
            .get("Node Type")
            .and_then(Value::as_str)
            .unwrap_or("Unknown")
            .to_owned(),
        relation_name: plan
            .get("Relation Name")
            .and_then(Value::as_str)
            .map(str::to_owned),
        total_cost: plan
            .get("Total Cost")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        plan_rows: plan.get("Plan Rows").and_then(Value::as_i64).unwrap_or(0),
    });

    if let Some(children) = plan.get("Plans").and_then(Value::as_array) {
        for child in children {
            collect_plan_nodes(child, nodes);
        }
    }
}

fn relation_refs(node: &NodeEnum) -> Vec<PgSqlRelationRef> {
    let mut refs = Vec::new();
    let mut cte_names = Vec::new();
    collect_cte_names(node, &mut cte_names);
    collect_relation_refs(node, &mut refs, &cte_names);
    dedupe_refs(refs)
}

fn collect_relation_refs(node: &NodeEnum, refs: &mut Vec<PgSqlRelationRef>, cte_names: &[String]) {
    match node {
        NodeEnum::RangeVar(range) => {
            if !is_cte_ref(range, cte_names) {
                refs.push(range_var_ref(range));
            }
        }
        NodeEnum::InsertStmt(stmt) => {
            if let Some(relation) = &stmt.relation {
                refs.push(range_var_ref(relation));
            }
            if let Some(select_stmt) = stmt.select_stmt.as_deref() {
                collect_relation_refs_from_node(select_stmt, refs, cte_names);
            }
            if let Some(with_clause) = &stmt.with_clause {
                for cte in &with_clause.ctes {
                    collect_relation_refs_from_node(cte, refs, cte_names);
                }
            }
        }
        NodeEnum::UpdateStmt(stmt) => {
            if let Some(relation) = &stmt.relation {
                refs.push(range_var_ref(relation));
            }
            collect_child_refs(node, refs, cte_names);
        }
        NodeEnum::DeleteStmt(stmt) => {
            if let Some(relation) = &stmt.relation {
                refs.push(range_var_ref(relation));
            }
            collect_child_refs(node, refs, cte_names);
        }
        NodeEnum::MergeStmt(stmt) => {
            if let Some(relation) = &stmt.relation {
                refs.push(range_var_ref(relation));
            }
            collect_child_refs(node, refs, cte_names);
        }
        NodeEnum::AlterTableStmt(stmt) => {
            if let Some(relation) = &stmt.relation {
                refs.push(range_var_ref(relation));
            }
        }
        NodeEnum::TruncateStmt(stmt) => {
            for relation in &stmt.relations {
                if let Some(NodeEnum::RangeVar(range)) = relation.node.as_ref() {
                    refs.push(range_var_ref(range));
                }
            }
        }
        NodeEnum::IndexStmt(stmt) => {
            if let Some(relation) = &stmt.relation {
                refs.push(range_var_ref(relation));
            }
        }
        _ => collect_child_refs(node, refs, cte_names),
    }
}

fn collect_child_refs(node: &NodeEnum, refs: &mut Vec<PgSqlRelationRef>, cte_names: &[String]) {
    for child in crate::ast::node_children(node) {
        collect_relation_refs_from_node(child, refs, cte_names);
    }
}

fn collect_relation_refs_from_node(
    node: &pg_query::protobuf::Node,
    refs: &mut Vec<PgSqlRelationRef>,
    cte_names: &[String],
) {
    if let Some(child_node) = node.node.as_ref() {
        collect_relation_refs(child_node, refs, cte_names);
    }
}

fn collect_cte_names(node: &NodeEnum, names: &mut Vec<String>) {
    match node {
        NodeEnum::CommonTableExpr(cte) if !cte.ctename.is_empty() => {
            push_unique_name(names, cte.ctename.clone());
            if let Some(query) = cte.ctequery.as_deref().and_then(|node| node.node.as_ref()) {
                collect_cte_names(query, names);
            }
        }
        _ => {
            for child in crate::ast::node_children(node) {
                if let Some(child_node) = child.node.as_ref() {
                    collect_cte_names(child_node, names);
                }
            }
        }
    }
}

fn push_unique_name(names: &mut Vec<String>, name: String) {
    if !names
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&name))
    {
        names.push(name);
    }
}

fn is_cte_ref(range: &RangeVar, cte_names: &[String]) -> bool {
    range.schemaname.is_empty()
        && cte_names
            .iter()
            .any(|cte_name| cte_name.eq_ignore_ascii_case(&range.relname))
}

fn range_var_ref(range: &RangeVar) -> PgSqlRelationRef {
    PgSqlRelationRef {
        schema: (!range.schemaname.is_empty()).then(|| range.schemaname.clone()),
        name: range.relname.clone(),
    }
}

fn dedupe_refs(refs: Vec<PgSqlRelationRef>) -> Vec<PgSqlRelationRef> {
    let mut unique = Vec::new();
    for item in refs {
        if !unique.iter().any(|existing: &PgSqlRelationRef| {
            existing.name == item.name && existing.schema == item.schema
        }) {
            unique.push(item);
        }
    }
    unique
}

#[cfg(test)]
pub(crate) fn relation_refs_for_test(sql: &str) -> Vec<PgSqlRelationRef> {
    let parsed = pg_query::parse(sql).expect("valid PostgreSQL");
    let Some(node) = parsed
        .protobuf
        .stmts
        .first()
        .and_then(|raw_stmt| raw_stmt.stmt.as_deref())
        .and_then(|stmt| stmt.node.as_ref())
    else {
        return Vec::new();
    };

    relation_refs(node)
}

fn required_privileges(node: &NodeEnum, relation: &PgSqlRelationMetadata) -> Vec<&'static str> {
    match node {
        NodeEnum::SelectStmt(_) => vec!["SELECT"],
        NodeEnum::InsertStmt(_) => {
            if is_statement_target(node, relation) {
                vec!["INSERT"]
            } else {
                vec!["SELECT"]
            }
        }
        NodeEnum::UpdateStmt(_) => {
            if is_statement_target(node, relation) {
                vec!["UPDATE"]
            } else {
                vec!["SELECT"]
            }
        }
        NodeEnum::DeleteStmt(_) => {
            if is_statement_target(node, relation) {
                vec!["DELETE"]
            } else {
                vec!["SELECT"]
            }
        }
        NodeEnum::MergeStmt(_) => vec!["SELECT", "INSERT", "UPDATE", "DELETE"],
        NodeEnum::TruncateStmt(_) => vec!["TRUNCATE"],
        NodeEnum::DropStmt(stmt) if drop_stmt_requires_relation_owner(stmt) => vec!["OWNER"],
        NodeEnum::AlterTableStmt(_) | NodeEnum::IndexStmt(_) => vec!["OWNER"],
        _ => Vec::new(),
    }
}

fn is_statement_target(node: &NodeEnum, relation: &PgSqlRelationMetadata) -> bool {
    statement_target_relation(node).is_some_and(|target| relation_matches(target, relation))
}

fn statement_target_relation(node: &NodeEnum) -> Option<&RangeVar> {
    match node {
        NodeEnum::InsertStmt(stmt) => stmt.relation.as_ref(),
        NodeEnum::UpdateStmt(stmt) => stmt.relation.as_ref(),
        NodeEnum::DeleteStmt(stmt) => stmt.relation.as_ref(),
        NodeEnum::MergeStmt(stmt) => stmt.relation.as_ref(),
        _ => None,
    }
}

fn relation_matches(range: &RangeVar, relation: &PgSqlRelationMetadata) -> bool {
    range.relname.eq_ignore_ascii_case(&relation.name)
        && (range.schemaname.is_empty() || range.schemaname.eq_ignore_ascii_case(&relation.schema))
}

fn drop_stmt_requires_relation_owner(stmt: &pg_query::protobuf::DropStmt) -> bool {
    matches!(
        ObjectType::try_from(stmt.remove_type),
        Ok(ObjectType::ObjectTable)
            | Ok(ObjectType::ObjectIndex)
            | Ok(ObjectType::ObjectView)
            | Ok(ObjectType::ObjectMatview)
            | Ok(ObjectType::ObjectForeignTable)
            | Ok(ObjectType::ObjectSequence)
    )
}

fn rls_command(node: &NodeEnum) -> Option<&'static str> {
    match node {
        NodeEnum::SelectStmt(_) => Some("r"),
        NodeEnum::InsertStmt(_) => Some("a"),
        NodeEnum::UpdateStmt(_) => Some("w"),
        NodeEnum::DeleteStmt(_) => Some("d"),
        _ => None,
    }
}

fn expected_lock_mode(node: &NodeEnum) -> &'static str {
    match node {
        NodeEnum::SelectStmt(_) => "AccessShareLock",
        NodeEnum::InsertStmt(_) | NodeEnum::UpdateStmt(_) | NodeEnum::DeleteStmt(_) => {
            "RowExclusiveLock"
        }
        NodeEnum::IndexStmt(_) => "ShareLock",
        NodeEnum::TruncateStmt(_) | NodeEnum::DropStmt(_) | NodeEnum::AlterTableStmt(_) => {
            "AccessExclusiveLock"
        }
        _ => "AccessShareLock",
    }
}

fn conflicting_lock_modes(expected_mode: &str) -> Vec<&'static str> {
    match expected_mode {
        "AccessShareLock" => vec!["AccessExclusiveLock"],
        "RowExclusiveLock" => vec![
            "ShareLock",
            "ShareRowExclusiveLock",
            "ExclusiveLock",
            "AccessExclusiveLock",
        ],
        "ShareLock" => vec![
            "RowExclusiveLock",
            "ShareUpdateExclusiveLock",
            "ShareRowExclusiveLock",
            "ExclusiveLock",
            "AccessExclusiveLock",
        ],
        "AccessExclusiveLock" => vec![
            "AccessShareLock",
            "RowShareLock",
            "RowExclusiveLock",
            "ShareUpdateExclusiveLock",
            "ShareLock",
            "ShareRowExclusiveLock",
            "ExclusiveLock",
            "AccessExclusiveLock",
        ],
        _ => vec!["AccessExclusiveLock"],
    }
}

fn explain_supported(node: &NodeEnum) -> bool {
    matches!(
        node,
        NodeEnum::SelectStmt(_)
            | NodeEnum::InsertStmt(_)
            | NodeEnum::UpdateStmt(_)
            | NodeEnum::DeleteStmt(_)
            | NodeEnum::MergeStmt(_)
    )
}

fn to_metadata_error(error: sqlx::Error) -> PgSqlMetadataError {
    PgSqlMetadataError::new(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relation_refs_include_insert_select_sources_without_duplicate_target() {
        let refs = relation_refs_for_test("insert into archive_users select * from users");

        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].name, "archive_users");
        assert_eq!(refs[1].name, "users");
    }

    #[test]
    fn required_privileges_distinguish_insert_target_from_select_source() {
        let parsed =
            pg_query::parse("insert into archive_users select * from users").expect("valid SQL");
        let node = parsed.protobuf.stmts[0]
            .stmt
            .as_deref()
            .and_then(|stmt| stmt.node.as_ref())
            .expect("statement node");
        let target = relation(1, "archive_users");
        let source = relation(2, "users");

        assert_eq!(required_privileges(node, &target), vec!["INSERT"]);
        assert_eq!(required_privileges(node, &source), vec!["SELECT"]);
    }

    #[test]
    fn lock_conflict_matrix_filters_non_conflicting_modes() {
        assert_eq!(
            conflicting_lock_modes("AccessShareLock"),
            vec!["AccessExclusiveLock"]
        );
        assert!(!conflicting_lock_modes("RowExclusiveLock").contains(&"AccessShareLock"));
        assert!(conflicting_lock_modes("AccessExclusiveLock").contains(&"AccessShareLock"));
    }

    fn relation(oid: i64, name: &str) -> PgSqlRelationMetadata {
        PgSqlRelationMetadata {
            oid,
            schema: "public".to_owned(),
            name: name.to_owned(),
            kind: "r".to_owned(),
            owner: "postgres".to_owned(),
            total_size_bytes: 0,
            relation_size_bytes: 0,
            estimated_rows: None,
            live_rows: None,
            dead_rows: None,
            is_partitioned: false,
            partition_count: 0,
        }
    }
}
