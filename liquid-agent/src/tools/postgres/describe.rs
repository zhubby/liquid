use anyhow::{Result, anyhow};
use async_trait::async_trait;
use liquid_llm::ToolDefinition;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};

use crate::{tools::AgentTool, types::ToolOutput};

use super::{
    args::{optional_string_arg, relation_name, required_string_arg},
    catalog::{PgRelationSummary, relation_summary_from_row},
    config::PostgresToolContext,
};

#[derive(Debug, Clone)]
pub(crate) struct PgDescribeRelationTool {
    context: PostgresToolContext,
}

impl PgDescribeRelationTool {
    pub(crate) fn new(context: PostgresToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgDescribeRelationTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_describe_relation",
            "Describe one PostgreSQL relation with columns, indexes, constraints, privileges, row-level security, and size statistics.",
            json!({
                "type": "object",
                "properties": {
                    "schema": {
                        "type": "string",
                        "description": "Optional schema name. If omitted, current_schemas(true) resolution is used."
                    },
                    "name": {
                        "type": "string",
                        "description": "Relation name to describe."
                    }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let schema = optional_string_arg(&arguments, "schema")?;
        let name = required_string_arg(&arguments, "name", "pg_describe_relation")?;
        let relation = fetch_relation_summary(&self.context.pool, schema.as_deref(), &name)
            .await?
            .ok_or_else(|| {
                anyhow!(
                    "pg_describe_relation could not resolve relation {}",
                    relation_name(schema.as_deref(), &name)
                )
            })?;
        let relation_oid = relation.oid;
        let columns = fetch_describe_columns(&self.context.pool, relation_oid).await?;
        let indexes = fetch_describe_indexes(&self.context.pool, relation_oid).await?;
        let constraints = fetch_describe_constraints(&self.context.pool, relation_oid).await?;
        let privileges = fetch_describe_privileges(&self.context.pool, relation_oid).await?;
        let rls = fetch_describe_rls(&self.context.pool, relation_oid).await?;

        Ok(ToolOutput::json(json!({
            "relation": relation,
            "columns": columns,
            "indexes": indexes,
            "constraints": constraints,
            "privileges": privileges,
            "rls": rls,
        })))
    }
}

#[derive(Debug, Clone, Serialize)]
struct PgDescribeColumn {
    name: String,
    data_type: String,
    is_nullable: bool,
    has_default: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_expression: Option<String>,
    is_identity: bool,
    is_generated: bool,
}

#[derive(Debug, Clone, Serialize)]
struct PgDescribeIndex {
    oid: i64,
    schema: String,
    name: String,
    columns: Vec<String>,
    is_unique: bool,
    is_primary: bool,
    is_valid: bool,
    is_ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    predicate: Option<String>,
    definition: String,
}

#[derive(Debug, Clone, Serialize)]
struct PgDescribeConstraint {
    name: String,
    kind: String,
    columns: Vec<String>,
    is_validated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    definition: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PgDescribePrivilege {
    action: String,
    allowed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct PgDescribeRls {
    enabled: bool,
    forced: bool,
    current_role_bypasses_rls: bool,
    policy_count: i64,
}

async fn fetch_relation_summary(
    pool: &PgPool,
    schema: Option<&str>,
    name: &str,
) -> Result<Option<PgRelationSummary>> {
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
    .bind(name)
    .bind(schema)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(relation_summary_from_row))
}

async fn fetch_describe_columns(pool: &PgPool, relation_oid: i64) -> Result<Vec<PgDescribeColumn>> {
    let rows = sqlx::query(
        r#"
        select
          a.attname,
          format_type(a.atttypid, a.atttypmod) as data_type,
          not a.attnotnull as is_nullable,
          a.atthasdef as has_default,
          pg_get_expr(ad.adbin, ad.adrelid) as default_expression,
          a.attidentity <> '' as is_identity,
          a.attgenerated <> '' as is_generated
        from pg_attribute a
        left join pg_attrdef ad on ad.adrelid = a.attrelid and ad.adnum = a.attnum
        where a.attrelid = $1::bigint::oid
          and a.attnum > 0
          and not a.attisdropped
        order by a.attnum
        "#,
    )
    .bind(relation_oid)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PgDescribeColumn {
            name: row.get("attname"),
            data_type: row.get("data_type"),
            is_nullable: row.get("is_nullable"),
            has_default: row.get("has_default"),
            default_expression: row.get("default_expression"),
            is_identity: row.get("is_identity"),
            is_generated: row.get("is_generated"),
        })
        .collect())
}

async fn fetch_describe_indexes(pool: &PgPool, relation_oid: i64) -> Result<Vec<PgDescribeIndex>> {
    let rows = sqlx::query(
        r#"
        select
          ix.indexrelid::bigint as index_oid,
          ni.nspname as schema_name,
          ci.relname as index_name,
          coalesce(array_remove(array_agg(a.attname order by key_ord.ordinality), null), array[]::text[]) as columns,
          ix.indisunique,
          ix.indisprimary,
          ix.indisvalid,
          ix.indisready,
          pg_get_expr(ix.indpred, ix.indrelid) as predicate,
          pg_get_indexdef(ix.indexrelid) as definition
        from pg_index ix
        join pg_class ci on ci.oid = ix.indexrelid
        join pg_namespace ni on ni.oid = ci.relnamespace
        left join unnest(ix.indkey) with ordinality as key_ord(attnum, ordinality) on true
        left join pg_attribute a on a.attrelid = ix.indrelid and a.attnum = key_ord.attnum
        where ix.indrelid = $1::bigint::oid
        group by ix.indexrelid, ni.nspname, ci.relname, ix.indisunique, ix.indisprimary,
                 ix.indisvalid, ix.indisready, ix.indpred, ix.indrelid
        order by ci.relname
        "#,
    )
    .bind(relation_oid)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PgDescribeIndex {
            oid: row.get("index_oid"),
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

async fn fetch_describe_constraints(
    pool: &PgPool,
    relation_oid: i64,
) -> Result<Vec<PgDescribeConstraint>> {
    let rows = sqlx::query(
        r#"
        select
          conname,
          contype::text as kind,
          coalesce(array_remove(array_agg(a.attname order by ordinality), null), array[]::text[]) as columns,
          convalidated,
          pg_get_constraintdef(pg_constraint.oid, true) as definition
        from pg_constraint
        left join unnest(conkey) with ordinality as key(attnum, ordinality) on true
        left join pg_attribute a on a.attrelid = conrelid and a.attnum = key.attnum
        where conrelid = $1::bigint::oid
        group by pg_constraint.oid
        order by conname
        "#,
    )
    .bind(relation_oid)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PgDescribeConstraint {
            name: row.get("conname"),
            kind: row.get("kind"),
            columns: row.get::<Vec<String>, _>("columns"),
            is_validated: row.get("convalidated"),
            definition: row.get("definition"),
        })
        .collect())
}

async fn fetch_describe_privileges(
    pool: &PgPool,
    relation_oid: i64,
) -> Result<Vec<PgDescribePrivilege>> {
    let actions = [
        "SELECT",
        "INSERT",
        "UPDATE",
        "DELETE",
        "TRUNCATE",
        "REFERENCES",
        "TRIGGER",
    ];
    let mut privileges = Vec::new();

    for action in actions {
        let allowed =
            sqlx::query_scalar::<_, bool>("select has_table_privilege($1::bigint::oid, $2)")
                .bind(relation_oid)
                .bind(action)
                .fetch_one(pool)
                .await?;

        privileges.push(PgDescribePrivilege {
            action: action.to_owned(),
            allowed,
        });
    }

    Ok(privileges)
}

async fn fetch_describe_rls(pool: &PgPool, relation_oid: i64) -> Result<Option<PgDescribeRls>> {
    let row = sqlx::query(
        r#"
        select
          c.relrowsecurity as enabled,
          c.relforcerowsecurity as forced,
          r.rolbypassrls as current_role_bypasses_rls,
          (
            select count(*)::bigint from pg_policy p where p.polrelid = c.oid
          ) as policy_count
        from pg_class c
        cross join pg_roles r
        where c.oid = $1::bigint::oid
          and r.rolname = current_user
        "#,
    )
    .bind(relation_oid)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| PgDescribeRls {
        enabled: row.get("enabled"),
        forced: row.get("forced"),
        current_role_bypasses_rls: row.get("current_role_bypasses_rls"),
        policy_count: row.get("policy_count"),
    }))
}
