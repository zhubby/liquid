use std::collections::HashSet;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use liquid_core::{SqlRollbackPlan, SqlRollbackStatus};
use pg_query::{
    NodeEnum,
    protobuf::{self, RangeVar},
};
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction};
use time::OffsetDateTime;

use super::{PostgresToolConfig, PostgresToolContext, execute::set_tool_timeouts};
use crate::tools::postgres::args::elapsed_ms;

pub(super) const ROLLBACK_SNAPSHOT_LIMIT: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostgresWriteExecutionMode {
    Summary,
    ReturningRows { limit: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PostgresWriteExecutionResult {
    pub affected_rows: u64,
    pub elapsed_ms: u64,
    pub rollback: SqlRollbackPlan,
    pub returned_rows: Option<Vec<Value>>,
    pub returned_rows_truncated: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum RollbackSupport {
    Insert(InsertRollbackTarget),
    Update(UpdateRollbackTarget),
    Delete(DeleteRollbackTarget),
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq)]
struct InsertRollbackTarget {
    relation: RangeVar,
    has_returning: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct UpdateRollbackTarget {
    relation: RangeVar,
    where_clause: Option<String>,
    updated_columns: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct DeleteRollbackTarget {
    relation: RangeVar,
    where_clause: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RelationMetadata {
    schema_name: String,
    table_name: String,
    columns: Vec<ColumnMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ColumnMetadata {
    name: String,
    data_type: String,
    is_primary_key: bool,
    is_identity: bool,
    is_generated: bool,
}

impl RelationMetadata {
    fn qualified_name(&self) -> String {
        format!(
            "{}.{}",
            quote_ident(&self.schema_name),
            quote_ident(&self.table_name)
        )
    }

    fn primary_key_columns(&self) -> Vec<&ColumnMetadata> {
        self.columns
            .iter()
            .filter(|column| column.is_primary_key)
            .collect()
    }
}

pub async fn execute_write_sql_with_rollback_with_config(
    config: PostgresToolConfig,
    executable_sql: &str,
    mode: PostgresWriteExecutionMode,
) -> Result<PostgresWriteExecutionResult> {
    let Some(pool) = config.pool.clone() else {
        bail!("rollback-aware write SQL execution requires a managed database pool");
    };
    let context = PostgresToolContext::new(pool, &config);

    execute_write_sql_with_rollback(&context, executable_sql, mode).await
}

pub(super) async fn execute_write_sql_with_rollback(
    context: &PostgresToolContext,
    executable_sql: &str,
    mode: PostgresWriteExecutionMode,
) -> Result<PostgresWriteExecutionResult> {
    let started_at = Instant::now();
    let rollback_support = classify_rollback_support(executable_sql)
        .unwrap_or_else(|error| RollbackSupport::Unsupported(error.to_string()));
    let result = match rollback_support {
        RollbackSupport::Insert(target) => {
            match execute_insert_with_rollback(context, executable_sql, mode, target).await {
                Ok(result) => result,
                Err(error) => {
                    execute_original_transaction(
                        context,
                        executable_sql,
                        mode,
                        failed_plan(error.to_string()),
                    )
                    .await?
                }
            }
        }
        RollbackSupport::Update(target) => {
            execute_update_with_rollback(context, executable_sql, mode, target).await?
        }
        RollbackSupport::Delete(target) => {
            execute_delete_with_rollback(context, executable_sql, mode, target).await?
        }
        RollbackSupport::Unsupported(reason) => {
            execute_original_transaction(context, executable_sql, mode, unsupported_plan(reason))
                .await?
        }
    };

    Ok(PostgresWriteExecutionResult {
        elapsed_ms: elapsed_ms(started_at),
        ..result
    })
}

async fn execute_original_transaction(
    context: &PostgresToolContext,
    executable_sql: &str,
    mode: PostgresWriteExecutionMode,
    rollback: SqlRollbackPlan,
) -> Result<PostgresWriteExecutionResult> {
    let mut transaction = context.pool.begin().await?;
    set_tool_timeouts(&mut transaction, context).await?;
    let result = execute_original_with_plan(&mut transaction, executable_sql, mode, rollback)
        .await
        .with_context(|| "SQL execution failed")?;
    transaction
        .commit()
        .await
        .context("failed to commit SQL execution transaction")?;

    Ok(result)
}

async fn execute_original_with_plan_and_commit(
    mut transaction: Transaction<'_, Postgres>,
    executable_sql: &str,
    mode: PostgresWriteExecutionMode,
    rollback: SqlRollbackPlan,
) -> Result<PostgresWriteExecutionResult> {
    let result = execute_original_with_plan(&mut transaction, executable_sql, mode, rollback)
        .await
        .with_context(|| "SQL execution failed")?;
    transaction
        .commit()
        .await
        .context("failed to commit SQL execution transaction")?;

    Ok(result)
}

async fn execute_insert_with_rollback(
    context: &PostgresToolContext,
    executable_sql: &str,
    mode: PostgresWriteExecutionMode,
    target: InsertRollbackTarget,
) -> Result<PostgresWriteExecutionResult> {
    let mut transaction = context.pool.begin().await?;
    set_tool_timeouts(&mut transaction, context).await?;
    let metadata = load_relation_metadata(&mut transaction, &target.relation).await?;
    if let Err(error) = ensure_primary_key(&metadata) {
        return execute_original_with_plan_and_commit(
            transaction,
            executable_sql,
            mode,
            unsupported_plan(error.to_string()),
        )
        .await;
    }

    if target.has_returning {
        let plan =
            unsupported_plan("INSERT statements with RETURNING are not rollback-supported in v1");
        return execute_original_with_plan_and_commit(transaction, executable_sql, mode, plan)
            .await;
    }

    let row_ref = target_row_reference(&target.relation);
    let wrapped_sql = format!(
        r#"
with liquid_mutation as (
    {executable_sql}
    returning to_jsonb({row_ref}) as row
),
liquid_limited as (
    select row from liquid_mutation limit {snapshot_limit}
)
select
    (select count(*) from liquid_mutation)::bigint as affected_rows,
    coalesce(jsonb_agg(row), '[]'::jsonb) as rows
from liquid_limited
"#,
        snapshot_limit = ROLLBACK_SNAPSHOT_LIMIT + 1,
    );
    let row = sqlx::query(&wrapped_sql)
        .fetch_one(&mut *transaction)
        .await
        .context("failed to execute INSERT with rollback capture")?;
    let affected_rows = row.get::<i64, _>("affected_rows").max(0) as u64;
    let snapshot_rows = json_array_rows(row.get::<Value, _>("rows"));
    let rollback = if snapshot_rows.len() > ROLLBACK_SNAPSHOT_LIMIT {
        unsupported_plan(format!(
            "rollback generation supports at most {ROLLBACK_SNAPSHOT_LIMIT} affected rows"
        ))
    } else {
        generated_plan(insert_rollback_sql(&metadata, &snapshot_rows)?)
    };

    let result = PostgresWriteExecutionResult {
        affected_rows,
        elapsed_ms: 0,
        rollback,
        returned_rows: None,
        returned_rows_truncated: false,
    };
    transaction
        .commit()
        .await
        .context("failed to commit SQL execution transaction")?;

    Ok(result)
}

async fn execute_update_with_rollback(
    context: &PostgresToolContext,
    executable_sql: &str,
    mode: PostgresWriteExecutionMode,
    target: UpdateRollbackTarget,
) -> Result<PostgresWriteExecutionResult> {
    let mut transaction = context.pool.begin().await?;
    set_tool_timeouts(&mut transaction, context).await?;
    let metadata = match load_relation_metadata(&mut transaction, &target.relation).await {
        Ok(metadata) => metadata,
        Err(error) => {
            drop(transaction);
            return execute_original_transaction(
                context,
                executable_sql,
                mode,
                failed_plan(error.to_string()),
            )
            .await;
        }
    };
    if let Err(error) = ensure_primary_key(&metadata) {
        return execute_original_with_plan_and_commit(
            transaction,
            executable_sql,
            mode,
            unsupported_plan(error.to_string()),
        )
        .await;
    }
    if updates_primary_key(&metadata, &target.updated_columns) {
        let plan = unsupported_plan(
            "UPDATE statements that modify primary key columns are not supported in v1",
        );
        return execute_original_with_plan_and_commit(transaction, executable_sql, mode, plan)
            .await;
    }

    let snapshot_rows = match select_old_rows(
        &mut transaction,
        &metadata,
        &target.relation,
        target.where_clause.as_deref(),
    )
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            drop(transaction);
            return execute_original_transaction(
                context,
                executable_sql,
                mode,
                failed_plan(error.to_string()),
            )
            .await;
        }
    };
    if snapshot_rows.len() > ROLLBACK_SNAPSHOT_LIMIT {
        let plan = unsupported_plan(format!(
            "rollback generation supports at most {ROLLBACK_SNAPSHOT_LIMIT} affected rows"
        ));
        return execute_original_with_plan_and_commit(transaction, executable_sql, mode, plan)
            .await;
    }

    let rollback = match update_rollback_sql(&metadata, &snapshot_rows) {
        Ok(sql) => generated_plan(sql),
        Err(error) => failed_plan(error.to_string()),
    };
    let execution = execute_original(&mut transaction, executable_sql, mode).await?;
    transaction
        .commit()
        .await
        .context("failed to commit SQL execution transaction")?;

    Ok(PostgresWriteExecutionResult {
        rollback,
        ..execution
    })
}

async fn execute_delete_with_rollback(
    context: &PostgresToolContext,
    executable_sql: &str,
    mode: PostgresWriteExecutionMode,
    target: DeleteRollbackTarget,
) -> Result<PostgresWriteExecutionResult> {
    let mut transaction = context.pool.begin().await?;
    set_tool_timeouts(&mut transaction, context).await?;
    let metadata = match load_relation_metadata(&mut transaction, &target.relation).await {
        Ok(metadata) => metadata,
        Err(error) => {
            drop(transaction);
            return execute_original_transaction(
                context,
                executable_sql,
                mode,
                failed_plan(error.to_string()),
            )
            .await;
        }
    };
    if let Err(error) = ensure_primary_key(&metadata) {
        return execute_original_with_plan_and_commit(
            transaction,
            executable_sql,
            mode,
            unsupported_plan(error.to_string()),
        )
        .await;
    }

    let snapshot_rows = match select_old_rows(
        &mut transaction,
        &metadata,
        &target.relation,
        target.where_clause.as_deref(),
    )
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            drop(transaction);
            return execute_original_transaction(
                context,
                executable_sql,
                mode,
                failed_plan(error.to_string()),
            )
            .await;
        }
    };
    if snapshot_rows.len() > ROLLBACK_SNAPSHOT_LIMIT {
        let plan = unsupported_plan(format!(
            "rollback generation supports at most {ROLLBACK_SNAPSHOT_LIMIT} affected rows"
        ));
        return execute_original_with_plan_and_commit(transaction, executable_sql, mode, plan)
            .await;
    }

    let rollback = match delete_rollback_sql(&metadata, &snapshot_rows) {
        Ok(sql) => generated_plan(sql),
        Err(error) => failed_plan(error.to_string()),
    };
    let execution = execute_original(&mut transaction, executable_sql, mode).await?;
    transaction
        .commit()
        .await
        .context("failed to commit SQL execution transaction")?;

    Ok(PostgresWriteExecutionResult {
        rollback,
        ..execution
    })
}

async fn execute_original_with_plan(
    transaction: &mut Transaction<'_, Postgres>,
    executable_sql: &str,
    mode: PostgresWriteExecutionMode,
    rollback: SqlRollbackPlan,
) -> Result<PostgresWriteExecutionResult> {
    let execution = execute_original(transaction, executable_sql, mode).await?;

    Ok(PostgresWriteExecutionResult {
        rollback,
        ..execution
    })
}

async fn execute_original(
    transaction: &mut Transaction<'_, Postgres>,
    executable_sql: &str,
    mode: PostgresWriteExecutionMode,
) -> Result<PostgresWriteExecutionResult> {
    match mode {
        PostgresWriteExecutionMode::Summary => {
            let result = sqlx::query(executable_sql)
                .execute(&mut **transaction)
                .await?;

            Ok(PostgresWriteExecutionResult {
                affected_rows: result.rows_affected(),
                elapsed_ms: 0,
                rollback: unsupported_plan("rollback generation was not attempted"),
                returned_rows: None,
                returned_rows_truncated: false,
            })
        }
        PostgresWriteExecutionMode::ReturningRows { limit } => {
            let fetch_limit = limit.saturating_add(1).min(ROLLBACK_SNAPSHOT_LIMIT + 1);
            let wrapped_sql = format!(
                "with liquid_mutation as ({executable_sql}) select to_jsonb(liquid_row) as row from liquid_mutation liquid_row limit {fetch_limit}"
            );
            let rows = sqlx::query(&wrapped_sql)
                .fetch_all(&mut **transaction)
                .await?;
            let mut returned_rows = rows
                .into_iter()
                .map(|row| row.get::<Value, _>("row"))
                .collect::<Vec<_>>();
            let returned_rows_truncated = returned_rows.len() > limit;

            if returned_rows_truncated {
                returned_rows.truncate(limit);
            }

            Ok(PostgresWriteExecutionResult {
                affected_rows: returned_rows.len() as u64,
                elapsed_ms: 0,
                rollback: unsupported_plan("rollback generation was not attempted"),
                returned_rows: Some(returned_rows),
                returned_rows_truncated,
            })
        }
    }
}

async fn select_old_rows(
    transaction: &mut Transaction<'_, Postgres>,
    metadata: &RelationMetadata,
    relation: &RangeVar,
    where_clause: Option<&str>,
) -> Result<Vec<Value>> {
    let alias_clause = relation_alias(relation)
        .map(|alias| format!(" as {}", quote_ident(&alias)))
        .unwrap_or_default();
    let table_ref = relation_alias(relation)
        .map(|alias| quote_ident(&alias))
        .unwrap_or_else(|| quote_ident(&metadata.table_name));
    let where_sql = where_clause
        .map(|where_clause| format!(" where {where_clause}"))
        .unwrap_or_default();
    let snapshot_sql = format!(
        "select to_jsonb({table_ref}) as row from {qualified_name}{alias_clause}{where_sql} for update limit {limit}",
        qualified_name = metadata.qualified_name(),
        limit = ROLLBACK_SNAPSHOT_LIMIT + 1,
    );
    let rows = sqlx::query(&snapshot_sql)
        .fetch_all(&mut **transaction)
        .await
        .context("failed to capture rollback row snapshot")?;

    Ok(rows
        .into_iter()
        .map(|row| row.get::<Value, _>("row"))
        .collect())
}

async fn load_relation_metadata(
    transaction: &mut Transaction<'_, Postgres>,
    relation: &RangeVar,
) -> Result<RelationMetadata> {
    if !relation.catalogname.is_empty() {
        bail!("cross-database rollback generation is not supported");
    }

    let relation_lookup = relation_lookup_name(relation);
    let rows = sqlx::query(
        r#"
select
    n.nspname as schema_name,
    c.relname as table_name,
    a.attname as column_name,
    pg_catalog.format_type(a.atttypid, a.atttypmod) as data_type,
    a.attidentity <> '' as is_identity,
    a.attgenerated <> '' as is_generated,
    exists (
        select 1
        from pg_index i
        where i.indrelid = a.attrelid
          and i.indisprimary
          and a.attnum = any(i.indkey)
    ) as is_primary_key
from pg_class c
join pg_namespace n on n.oid = c.relnamespace
join pg_attribute a on a.attrelid = c.oid
where c.oid = to_regclass($1)
  and c.relkind in ('r', 'p')
  and a.attnum > 0
  and not a.attisdropped
order by a.attnum
"#,
    )
    .bind(&relation_lookup)
    .fetch_all(&mut **transaction)
    .await
    .with_context(|| format!("failed to load metadata for {relation_lookup}"))?;

    let Some(first) = rows.first() else {
        bail!("target relation {relation_lookup} was not found");
    };

    Ok(RelationMetadata {
        schema_name: first.get("schema_name"),
        table_name: first.get("table_name"),
        columns: rows
            .into_iter()
            .map(|row| ColumnMetadata {
                name: row.get("column_name"),
                data_type: row.get("data_type"),
                is_identity: row.get("is_identity"),
                is_generated: row.get("is_generated"),
                is_primary_key: row.get("is_primary_key"),
            })
            .collect(),
    })
}

fn classify_rollback_support(sql: &str) -> Result<RollbackSupport> {
    let parsed = pg_query::parse(sql).context("SQL parse failed")?;
    let [raw_statement] = parsed.protobuf.stmts.as_slice() else {
        return Ok(RollbackSupport::Unsupported(
            "rollback generation requires exactly one statement".to_owned(),
        ));
    };
    let Some(node) = raw_statement
        .stmt
        .as_deref()
        .and_then(|stmt| stmt.node.as_ref())
    else {
        return Ok(RollbackSupport::Unsupported(
            "rollback generation could not inspect the statement".to_owned(),
        ));
    };

    Ok(match node {
        NodeEnum::InsertStmt(statement) => {
            if has_cte(statement.with_clause.as_ref()) {
                RollbackSupport::Unsupported(
                    "INSERT statements with CTEs are not supported in v1".to_owned(),
                )
            } else if on_conflict_updates(statement.on_conflict_clause.as_deref()) {
                RollbackSupport::Unsupported(
                    "INSERT ... ON CONFLICT DO UPDATE is not supported in v1".to_owned(),
                )
            } else {
                let Some(relation) = statement.relation.clone() else {
                    return Ok(RollbackSupport::Unsupported(
                        "INSERT rollback requires a target table".to_owned(),
                    ));
                };
                RollbackSupport::Insert(InsertRollbackTarget {
                    relation,
                    has_returning: !statement.returning_list.is_empty(),
                })
            }
        }
        NodeEnum::UpdateStmt(statement) => {
            if has_cte(statement.with_clause.as_ref()) {
                RollbackSupport::Unsupported(
                    "UPDATE statements with CTEs are not supported in v1".to_owned(),
                )
            } else if !statement.from_clause.is_empty() {
                RollbackSupport::Unsupported(
                    "UPDATE statements with FROM are not supported in v1".to_owned(),
                )
            } else {
                let Some(relation) = statement.relation.clone() else {
                    return Ok(RollbackSupport::Unsupported(
                        "UPDATE rollback requires a target table".to_owned(),
                    ));
                };
                let updated_columns = updated_column_names(&statement.target_list)?;
                RollbackSupport::Update(UpdateRollbackTarget {
                    relation,
                    where_clause: extract_where_clause(sql, statement.where_clause.is_some())?,
                    updated_columns,
                })
            }
        }
        NodeEnum::DeleteStmt(statement) => {
            if has_cte(statement.with_clause.as_ref()) {
                RollbackSupport::Unsupported(
                    "DELETE statements with CTEs are not supported in v1".to_owned(),
                )
            } else if !statement.using_clause.is_empty() {
                RollbackSupport::Unsupported(
                    "DELETE statements with USING are not supported in v1".to_owned(),
                )
            } else {
                let Some(relation) = statement.relation.clone() else {
                    return Ok(RollbackSupport::Unsupported(
                        "DELETE rollback requires a target table".to_owned(),
                    ));
                };
                RollbackSupport::Delete(DeleteRollbackTarget {
                    relation,
                    where_clause: extract_where_clause(sql, statement.where_clause.is_some())?,
                })
            }
        }
        _ => RollbackSupport::Unsupported(
            "rollback generation only supports single-table INSERT, UPDATE, and DELETE in v1"
                .to_owned(),
        ),
    })
}

fn updates_primary_key(metadata: &RelationMetadata, updated_columns: &HashSet<String>) -> bool {
    metadata
        .primary_key_columns()
        .iter()
        .any(|column| updated_columns.contains(&column.name))
}

fn updated_column_names(target_list: &[protobuf::Node]) -> Result<HashSet<String>> {
    let mut columns = HashSet::new();

    for target in target_list {
        let Some(NodeEnum::ResTarget(target)) = target.node.as_ref() else {
            bail!("UPDATE rollback does not support complex target lists");
        };
        if target.name.is_empty() {
            bail!("UPDATE rollback does not support complex target lists");
        }
        columns.insert(target.name.clone());
    }

    Ok(columns)
}

fn ensure_primary_key(metadata: &RelationMetadata) -> Result<()> {
    if metadata.primary_key_columns().is_empty() {
        bail!("target table does not have a primary key");
    }

    Ok(())
}

fn insert_rollback_sql(metadata: &RelationMetadata, snapshot_rows: &[Value]) -> Result<String> {
    let pk_columns = metadata.primary_key_columns();
    let typed_columns = typed_recordset_columns(&pk_columns);
    let join_predicate = pk_join_predicate(&pk_columns, "target", "rollback_rows");
    let snapshot = project_snapshot_rows(snapshot_rows, &pk_columns);
    let json = dollar_quoted_json(&snapshot)?;

    Ok(format!(
        r#"with rollback_rows as (
    select * from jsonb_to_recordset({json}::jsonb) as rollback_rows({typed_columns})
)
delete from {table_name} as target
using rollback_rows
where {join_predicate};"#,
        table_name = metadata.qualified_name(),
    ))
}

fn update_rollback_sql(metadata: &RelationMetadata, snapshot_rows: &[Value]) -> Result<String> {
    let pk_columns = metadata.primary_key_columns();
    let set_columns = metadata
        .columns
        .iter()
        .filter(|column| !column.is_primary_key && !column.is_generated)
        .collect::<Vec<_>>();

    if set_columns.is_empty() {
        bail!("target table has no rollback-updatable columns");
    }

    let mut typed_columns = pk_columns.clone();
    typed_columns.extend(set_columns.iter().copied());
    let snapshot = project_snapshot_rows(snapshot_rows, &typed_columns);
    let json = dollar_quoted_json(&snapshot)?;
    let typed_columns = typed_recordset_columns(&typed_columns);
    let assignments = set_columns
        .iter()
        .map(|column| {
            format!(
                "{} = rollback_rows.{}",
                quote_ident(&column.name),
                quote_ident(&column.name)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n    ");
    let join_predicate = pk_join_predicate(&pk_columns, "target", "rollback_rows");

    Ok(format!(
        r#"with rollback_rows as (
    select * from jsonb_to_recordset({json}::jsonb) as rollback_rows({typed_columns})
)
update {table_name} as target
set {assignments}
from rollback_rows
where {join_predicate};"#,
        table_name = metadata.qualified_name(),
    ))
}

fn delete_rollback_sql(metadata: &RelationMetadata, snapshot_rows: &[Value]) -> Result<String> {
    let insert_columns = metadata
        .columns
        .iter()
        .filter(|column| !column.is_generated)
        .collect::<Vec<_>>();
    let typed_columns = typed_recordset_columns(&insert_columns);
    let snapshot = project_snapshot_rows(snapshot_rows, &insert_columns);
    let json = dollar_quoted_json(&snapshot)?;
    let insert_column_names = insert_columns
        .iter()
        .map(|column| quote_ident(&column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let select_columns = insert_columns
        .iter()
        .map(|column| format!("rollback_rows.{}", quote_ident(&column.name)))
        .collect::<Vec<_>>()
        .join(", ");
    let identity_override = if insert_columns.iter().any(|column| column.is_identity) {
        " overriding system value"
    } else {
        ""
    };

    Ok(format!(
        r#"with rollback_rows as (
    select * from jsonb_to_recordset({json}::jsonb) as rollback_rows({typed_columns})
)
insert into {table_name} ({insert_column_names}){identity_override}
select {select_columns}
from rollback_rows;"#,
        table_name = metadata.qualified_name(),
    ))
}

fn typed_recordset_columns(columns: &[&ColumnMetadata]) -> String {
    columns
        .iter()
        .map(|column| format!("{} {}", quote_ident(&column.name), column.data_type))
        .collect::<Vec<_>>()
        .join(", ")
}

fn pk_join_predicate(
    pk_columns: &[&ColumnMetadata],
    target_alias: &str,
    rows_alias: &str,
) -> String {
    pk_columns
        .iter()
        .map(|column| {
            format!(
                "{target_alias}.{} is not distinct from {rows_alias}.{}",
                quote_ident(&column.name),
                quote_ident(&column.name)
            )
        })
        .collect::<Vec<_>>()
        .join(" and ")
}

fn project_snapshot_rows(snapshot_rows: &[Value], columns: &[&ColumnMetadata]) -> Value {
    Value::Array(
        snapshot_rows
            .iter()
            .filter_map(|row| row.as_object())
            .map(|row| {
                Value::Object(
                    columns
                        .iter()
                        .map(|column| {
                            (
                                column.name.clone(),
                                row.get(&column.name).cloned().unwrap_or(Value::Null),
                            )
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

fn extract_where_clause(sql: &str, has_where_clause: bool) -> Result<Option<String>> {
    if !has_where_clause {
        return Ok(None);
    }

    let scan = pg_query::scan(sql).context("failed to scan SQL for WHERE clause")?;
    let mut depth = 0_i32;
    let mut where_start = None;
    let mut where_end = None;

    for token in scan.tokens {
        let token_kind = protobuf::Token::try_from(token.token).ok();

        match token_kind {
            Some(protobuf::Token::Ascii40) => depth += 1,
            Some(protobuf::Token::Ascii41) => depth = depth.saturating_sub(1),
            Some(protobuf::Token::Where) if depth == 0 && where_start.is_none() => {
                where_start = Some(token.end as usize);
            }
            Some(protobuf::Token::Returning | protobuf::Token::Ascii59)
                if depth == 0 && where_start.is_some() =>
            {
                where_end = Some(token.start as usize);
                break;
            }
            _ => {}
        }
    }

    let Some(start) = where_start else {
        bail!("failed to locate top-level WHERE clause");
    };
    let end = where_end.unwrap_or(sql.len());
    let where_clause = sql
        .get(start..end)
        .map(str::trim)
        .filter(|where_clause| !where_clause.is_empty())
        .ok_or_else(|| anyhow!("failed to extract WHERE clause"))?;

    Ok(Some(where_clause.to_owned()))
}

fn has_cte(with_clause: Option<&protobuf::WithClause>) -> bool {
    with_clause
        .map(|with_clause| !with_clause.ctes.is_empty())
        .unwrap_or(false)
}

fn on_conflict_updates(on_conflict: Option<&protobuf::OnConflictClause>) -> bool {
    on_conflict
        .and_then(|on_conflict| protobuf::OnConflictAction::try_from(on_conflict.action).ok())
        == Some(protobuf::OnConflictAction::OnconflictUpdate)
}

fn relation_lookup_name(relation: &RangeVar) -> String {
    if relation.schemaname.is_empty() {
        quote_ident(&relation.relname)
    } else {
        format!(
            "{}.{}",
            quote_ident(&relation.schemaname),
            quote_ident(&relation.relname)
        )
    }
}

fn relation_alias(relation: &RangeVar) -> Option<String> {
    relation
        .alias
        .as_ref()
        .map(|alias| alias.aliasname.clone())
        .filter(|alias| !alias.is_empty())
}

fn target_row_reference(relation: &RangeVar) -> String {
    relation_alias(relation)
        .map(|alias| quote_ident(&alias))
        .unwrap_or_else(|| quote_ident(&relation.relname))
}

fn quote_ident(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn dollar_quoted_json(value: &Value) -> Result<String> {
    let json = serde_json::to_string(value)?;
    for index in 0..100 {
        let tag = if index == 0 {
            "$liquid_rollback$".to_owned()
        } else {
            format!("$liquid_rollback_{index}$")
        };
        if !json.contains(&tag) {
            return Ok(format!("{tag}{json}{tag}"));
        }
    }

    Err(anyhow!(
        "failed to find a safe dollar-quote tag for rollback JSON"
    ))
}

fn json_array_rows(value: Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

fn generated_plan(sql: String) -> SqlRollbackPlan {
    SqlRollbackPlan {
        status: SqlRollbackStatus::Generated,
        sql: Some(sql),
        reason: None,
        generated_at: Some(OffsetDateTime::now_utc()),
    }
}

fn unsupported_plan(reason: impl Into<String>) -> SqlRollbackPlan {
    SqlRollbackPlan {
        status: SqlRollbackStatus::Unsupported,
        sql: None,
        reason: Some(reason.into()),
        generated_at: None,
    }
}

fn failed_plan(reason: impl Into<String>) -> SqlRollbackPlan {
    SqlRollbackPlan {
        status: SqlRollbackStatus::Failed,
        sql: None,
        reason: Some(reason.into()),
        generated_at: None,
    }
}

#[cfg(test)]
mod tests {
    use liquid_core::SqlRollbackStatus;
    use sqlx::{PgPool, postgres::PgPoolOptions};

    use crate::tools::postgres::{
        PostgresToolConfig, PostgresToolExecutionMode,
        rollback::{PostgresWriteExecutionMode, execute_write_sql_with_rollback_with_config},
    };

    use serde_json::json;

    use super::*;

    #[test]
    fn classify_update_with_from_as_unsupported() {
        let support = classify_rollback_support(
            "update users set name = src.name from src where users.id = src.id",
        )
        .unwrap();

        assert!(matches!(support, RollbackSupport::Unsupported(reason) if reason.contains("FROM")));
    }

    #[test]
    fn classify_update_extracts_where_clause() {
        let support =
            classify_rollback_support("update users set active = false where id = 1").unwrap();

        let RollbackSupport::Update(target) = support else {
            panic!("expected supported UPDATE");
        };
        assert_eq!(target.where_clause.as_deref(), Some("id = 1"));
    }

    #[test]
    fn classify_delete_with_using_as_unsupported() {
        let support = classify_rollback_support(
            "delete from users using archived where users.id = archived.id",
        )
        .unwrap();

        assert!(
            matches!(support, RollbackSupport::Unsupported(reason) if reason.contains("USING"))
        );
    }

    #[test]
    fn insert_rollback_deletes_by_primary_key_json_recordset() {
        let metadata = RelationMetadata {
            schema_name: "public".to_owned(),
            table_name: "users".to_owned(),
            columns: vec![
                ColumnMetadata {
                    name: "id".to_owned(),
                    data_type: "integer".to_owned(),
                    is_primary_key: true,
                    is_identity: false,
                    is_generated: false,
                },
                ColumnMetadata {
                    name: "email".to_owned(),
                    data_type: "text".to_owned(),
                    is_primary_key: false,
                    is_identity: false,
                    is_generated: false,
                },
            ],
        };

        let sql =
            insert_rollback_sql(&metadata, &[json!({"id": 1, "email": "a@test.local"})]).unwrap();

        assert!(sql.contains("delete from \"public\".\"users\" as target"));
        assert!(sql.contains("jsonb_to_recordset"));
        assert!(sql.contains("\"id\" integer"));
        assert!(!sql.contains("\"email\" text"));
    }

    #[test]
    fn delete_rollback_skips_generated_columns_and_overrides_identity() {
        let metadata = RelationMetadata {
            schema_name: "public".to_owned(),
            table_name: "events".to_owned(),
            columns: vec![
                ColumnMetadata {
                    name: "id".to_owned(),
                    data_type: "bigint".to_owned(),
                    is_primary_key: true,
                    is_identity: true,
                    is_generated: false,
                },
                ColumnMetadata {
                    name: "name".to_owned(),
                    data_type: "text".to_owned(),
                    is_primary_key: false,
                    is_identity: false,
                    is_generated: false,
                },
                ColumnMetadata {
                    name: "name_search".to_owned(),
                    data_type: "tsvector".to_owned(),
                    is_primary_key: false,
                    is_identity: false,
                    is_generated: true,
                },
            ],
        };

        let sql = delete_rollback_sql(
            &metadata,
            &[json!({"id": 1, "name": "deploy", "name_search": "'deploy'"})],
        )
        .unwrap();

        assert!(sql.contains("overriding system value"));
        assert!(sql.contains("\"id\", \"name\""));
        assert!(!sql.contains("name_search"));
    }

    #[tokio::test]
    async fn generated_rollback_sql_restores_insert_update_and_delete() {
        let Some(pool) = integration_pool().await else {
            return;
        };
        let table_name = format!(
            "liquid_agent_rollback_{}",
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        );
        let table = quote_ident(&table_name);

        sqlx::query(&format!(
            r#"
create table {table} (
    id integer generated always as identity primary key,
    name text not null,
    active boolean not null default true,
    name_upper text generated always as (upper(name)) stored
)
"#
        ))
        .execute(&pool)
        .await
        .unwrap();

        let config = PostgresToolConfig::new(
            Some(pool.clone()),
            false,
            PostgresToolExecutionMode::WriteGated,
        );
        let insert = execute_write_sql_with_rollback_with_config(
            config.clone(),
            &format!("insert into {table} (name, active) values ('alpha', true), ('beta', false)"),
            PostgresWriteExecutionMode::Summary,
        )
        .await
        .unwrap();
        assert_eq!(insert.rollback.status, SqlRollbackStatus::Generated);
        sqlx::query(insert.rollback.sql.as_deref().unwrap())
            .execute(&pool)
            .await
            .unwrap();
        let count = sqlx::query_scalar::<_, i64>(&format!("select count(*) from {table}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);

        sqlx::query(&format!(
            "insert into {table} (name, active) values ('alpha', true), ('beta', false)"
        ))
        .execute(&pool)
        .await
        .unwrap();
        let update = execute_write_sql_with_rollback_with_config(
            config.clone(),
            &format!("update {table} set active = false where name = 'alpha'"),
            PostgresWriteExecutionMode::Summary,
        )
        .await
        .unwrap();
        assert_eq!(update.rollback.status, SqlRollbackStatus::Generated);
        sqlx::query(update.rollback.sql.as_deref().unwrap())
            .execute(&pool)
            .await
            .unwrap();
        let active = sqlx::query_scalar::<_, bool>(&format!(
            "select active from {table} where name = 'alpha'"
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(active);

        let delete = execute_write_sql_with_rollback_with_config(
            config,
            &format!("delete from {table} where name = 'alpha'"),
            PostgresWriteExecutionMode::Summary,
        )
        .await
        .unwrap();
        assert_eq!(delete.rollback.status, SqlRollbackStatus::Generated);
        sqlx::query(delete.rollback.sql.as_deref().unwrap())
            .execute(&pool)
            .await
            .unwrap();
        let restored = sqlx::query_scalar::<_, i64>(&format!(
            "select count(*) from {table} where name = 'alpha'"
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(restored, 1);

        sqlx::query(&format!("drop table {table}"))
            .execute(&pool)
            .await
            .unwrap();
    }

    async fn integration_pool() -> Option<PgPool> {
        let database_url = std::env::var("LIQUID_TEST_DATABASE_URL").ok()?;
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .ok()
    }
}
