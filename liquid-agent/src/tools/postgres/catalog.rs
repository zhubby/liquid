use anyhow::Result;
use async_trait::async_trait;
use liquid_llm::ToolDefinition;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::Row;

use crate::{tools::AgentTool, types::ToolOutput};

use super::{
    args::{limit_arg, optional_bool_arg, optional_string_arg, relation_kind_codes},
    config::PostgresToolContext,
};

#[derive(Debug, Clone)]
pub(crate) struct PgListSchemasTool {
    context: PostgresToolContext,
}

impl PgListSchemasTool {
    pub(crate) fn new(context: PostgresToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgListSchemasTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_list_schemas",
            "List PostgreSQL schemas visible to the configured database role.",
            json!({
                "type": "object",
                "properties": {
                    "include_system": {
                        "type": "boolean",
                        "description": "Include pg_catalog, information_schema, and pg_* schemas."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum schemas to return; defaults to 100 and clamps at 1000."
                    }
                },
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let include_system = optional_bool_arg(&arguments, "include_system")?.unwrap_or(false);
        let limit = limit_arg(&arguments, &self.context, "pg_list_schemas")?;
        let rows = sqlx::query(
            r#"
            select
              n.nspname as name,
              pg_get_userbyid(n.nspowner) as owner,
              (n.nspname = 'information_schema' or n.nspname like 'pg_%') as is_system
            from pg_namespace n
            where $1::bool
               or not (n.nspname = 'information_schema' or n.nspname like 'pg_%')
            order by is_system, n.nspname
            limit $2
            "#,
        )
        .bind(include_system)
        .bind(limit as i64)
        .fetch_all(&self.context.pool)
        .await?;

        let schemas = rows
            .into_iter()
            .map(|row| PgSchemaSummary {
                name: row.get("name"),
                owner: row.get("owner"),
                is_system: row.get("is_system"),
            })
            .collect::<Vec<_>>();

        Ok(ToolOutput::json(json!({
            "schemas": schemas,
            "count": schemas.len(),
            "truncated": schemas.len() == limit,
        })))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PgListRelationsTool {
    context: PostgresToolContext,
}

impl PgListRelationsTool {
    pub(crate) fn new(context: PostgresToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgListRelationsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_list_relations",
            "List PostgreSQL tables, views, materialized views, partitioned tables, and foreign tables.",
            json!({
                "type": "object",
                "properties": {
                    "schema": {
                        "type": "string",
                        "description": "Optional schema name filter."
                    },
                    "search": {
                        "type": "string",
                        "description": "Optional case-insensitive relation name substring."
                    },
                    "kinds": {
                        "type": "array",
                        "description": "Optional relation kinds: table, partitioned_table, view, materialized_view, foreign_table, or PostgreSQL relkind codes r/p/v/m/f.",
                        "items": { "type": "string" }
                    },
                    "include_system": {
                        "type": "boolean",
                        "description": "Include pg_* and information_schema relations."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum relations to return; defaults to 100 and clamps at 1000."
                    }
                },
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let schema = optional_string_arg(&arguments, "schema")?;
        let search = optional_string_arg(&arguments, "search")?;
        let include_system = optional_bool_arg(&arguments, "include_system")?.unwrap_or(false);
        let kind_codes = relation_kind_codes(&arguments)?;
        let filter_kinds = !kind_codes.is_empty();
        let limit = limit_arg(&arguments, &self.context, "pg_list_relations")?;
        let rows = sqlx::query(
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
            where c.relkind in ('r', 'p', 'v', 'm', 'f')
              and ($1::bool or not (n.nspname = 'information_schema' or n.nspname like 'pg_%'))
              and ($2::text is null or n.nspname = $2)
              and ($3::text is null or c.relname ilike '%' || $3 || '%')
              and (not $4::bool or c.relkind::text = any($5::text[]))
            order by n.nspname, c.relname
            limit $6
            "#,
        )
        .bind(include_system)
        .bind(schema.as_deref())
        .bind(search.as_deref())
        .bind(filter_kinds)
        .bind(kind_codes)
        .bind(limit as i64)
        .fetch_all(&self.context.pool)
        .await?;

        let relations = rows
            .into_iter()
            .map(relation_summary_from_row)
            .collect::<Vec<_>>();

        Ok(ToolOutput::json(json!({
            "relations": relations,
            "count": relations.len(),
            "truncated": relations.len() == limit,
        })))
    }
}

#[derive(Debug, Clone, Serialize)]
struct PgSchemaSummary {
    name: String,
    owner: String,
    is_system: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct PgRelationSummary {
    pub(super) oid: i64,
    schema: String,
    name: String,
    kind: String,
    owner: String,
    total_size_bytes: i64,
    relation_size_bytes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    estimated_rows: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    live_rows: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dead_rows: Option<i64>,
    is_partitioned: bool,
    partition_count: i64,
}

pub(super) fn relation_summary_from_row(row: sqlx::postgres::PgRow) -> PgRelationSummary {
    PgRelationSummary {
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
    }
}
