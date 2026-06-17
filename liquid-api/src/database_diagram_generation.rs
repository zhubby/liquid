use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    pin::Pin,
};

use anyhow::{Context, Result, bail};
use liquid_core::{
    DatabaseDiagramCardinality, DatabaseDiagramColumn, DatabaseDiagramDocument,
    DatabaseDiagramEnum, DatabaseDiagramEnumValue, DatabaseDiagramIndex, DatabaseDiagramPoint,
    DatabaseDiagramRelationship, DatabaseDiagramRelationshipEndpoint, DatabaseDiagramTable,
    ManagedDatabaseEngine,
};
use sqlx::{PgPool, Row};

const GRID_COLUMNS: usize = 3;
const GRID_X_OFFSET: i32 = 80;
const GRID_Y_OFFSET: i32 = 80;
const GRID_X_GAP: i32 = 360;
const GRID_Y_GAP: i32 = 260;
const MAX_GENERATED_DATABASE_DIAGRAM_TABLES: usize = 200;

pub type DatabaseDiagramGenerationFuture<'a> =
    Pin<Box<dyn Future<Output = anyhow::Result<DatabaseDiagramDocument>> + Send + 'a>>;

pub trait DatabaseDiagramGenerator: Send + Sync {
    fn generate<'a>(&'a self, pool: PgPool) -> DatabaseDiagramGenerationFuture<'a>;
}

#[derive(Debug, Default)]
pub struct PostgresDatabaseDiagramGenerator;

impl DatabaseDiagramGenerator for PostgresDatabaseDiagramGenerator {
    fn generate<'a>(&'a self, pool: PgPool) -> DatabaseDiagramGenerationFuture<'a> {
        Box::pin(async move {
            let snapshot = load_catalog_snapshot(&pool).await?;

            build_database_diagram_document(snapshot)
        })
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct CatalogSnapshot {
    tables: Vec<CatalogTable>,
    columns: Vec<CatalogColumn>,
    indexes: Vec<CatalogIndex>,
    foreign_keys: Vec<CatalogForeignKey>,
    enums: Vec<CatalogEnumValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogTable {
    oid: String,
    schema_name: String,
    table_name: String,
    comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogColumn {
    table_oid: String,
    name: String,
    ordinal: i32,
    data_type: String,
    nullable: bool,
    default_value: Option<String>,
    comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogIndex {
    table_oid: String,
    name: String,
    columns: Vec<String>,
    unique: bool,
    primary: bool,
    method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogForeignKey {
    name: String,
    source_table_oid: String,
    target_table_oid: String,
    source_column: String,
    target_column: String,
    ordinal: i32,
    on_update: Option<String>,
    on_delete: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogEnumValue {
    schema_name: String,
    enum_name: String,
    value_name: String,
    sort_order: i32,
}

async fn load_catalog_snapshot(pool: &PgPool) -> Result<CatalogSnapshot> {
    let tables = load_catalog_tables(pool).await?;

    if tables.len() > MAX_GENERATED_DATABASE_DIAGRAM_TABLES {
        bail!(
            "database diagram generation supports up to {MAX_GENERATED_DATABASE_DIAGRAM_TABLES} tables; selected database has {} user tables",
            tables.len()
        );
    }

    let columns = load_catalog_columns(pool).await?;
    let indexes = load_catalog_indexes(pool).await?;
    let foreign_keys = load_catalog_foreign_keys(pool).await?;
    let enums = load_catalog_enums(pool).await?;

    Ok(CatalogSnapshot {
        tables,
        columns,
        indexes,
        foreign_keys,
        enums,
    })
}

async fn load_catalog_tables(pool: &PgPool) -> Result<Vec<CatalogTable>> {
    let rows = sqlx::query(
        r#"
        select
            c.oid::text as table_oid,
            n.nspname as schema_name,
            c.relname as table_name,
            obj_description(c.oid, 'pg_class') as comment
        from pg_class c
        join pg_namespace n on n.oid = c.relnamespace
        where c.relkind in ('r', 'p')
          and n.nspname <> 'information_schema'
          and n.nspname not like 'pg_%'
        order by n.nspname, c.relname
        "#,
    )
    .fetch_all(pool)
    .await
    .context("failed to query PostgreSQL catalog tables")?;

    rows.into_iter()
        .map(|row| {
            Ok(CatalogTable {
                oid: row.try_get("table_oid")?,
                schema_name: row.try_get("schema_name")?,
                table_name: row.try_get("table_name")?,
                comment: row.try_get("comment")?,
            })
        })
        .collect()
}

async fn load_catalog_columns(pool: &PgPool) -> Result<Vec<CatalogColumn>> {
    let rows = sqlx::query(
        r#"
        select
            c.oid::text as table_oid,
            a.attname as column_name,
            a.attnum::int as ordinal,
            format_type(a.atttypid, a.atttypmod) as data_type,
            not a.attnotnull as nullable,
            pg_get_expr(ad.adbin, ad.adrelid) as default_value,
            col_description(c.oid, a.attnum) as comment
        from pg_attribute a
        join pg_class c on c.oid = a.attrelid
        join pg_namespace n on n.oid = c.relnamespace
        left join pg_attrdef ad on ad.adrelid = a.attrelid and ad.adnum = a.attnum
        where c.relkind in ('r', 'p')
          and a.attnum > 0
          and not a.attisdropped
          and n.nspname <> 'information_schema'
          and n.nspname not like 'pg_%'
        order by n.nspname, c.relname, a.attnum
        "#,
    )
    .fetch_all(pool)
    .await
    .context("failed to query PostgreSQL catalog columns")?;

    rows.into_iter()
        .map(|row| {
            Ok(CatalogColumn {
                table_oid: row.try_get("table_oid")?,
                name: row.try_get("column_name")?,
                ordinal: row.try_get("ordinal")?,
                data_type: row.try_get("data_type")?,
                nullable: row.try_get("nullable")?,
                default_value: row.try_get("default_value")?,
                comment: row.try_get("comment")?,
            })
        })
        .collect()
}

async fn load_catalog_indexes(pool: &PgPool) -> Result<Vec<CatalogIndex>> {
    let rows = sqlx::query(
        r#"
        select
            t.oid::text as table_oid,
            idx.relname as index_name,
            array_remove(array_agg(a.attname order by key.ordinality), null)::text[] as columns,
            ix.indisunique as is_unique,
            ix.indisprimary as is_primary,
            am.amname as method
        from pg_index ix
        join pg_class t on t.oid = ix.indrelid
        join pg_namespace n on n.oid = t.relnamespace
        join pg_class idx on idx.oid = ix.indexrelid
        join pg_am am on am.oid = idx.relam
        left join lateral unnest(ix.indkey) with ordinality as key(attnum, ordinality) on true
        left join pg_attribute a on a.attrelid = t.oid and a.attnum = key.attnum and key.attnum > 0
        where t.relkind in ('r', 'p')
          and n.nspname <> 'information_schema'
          and n.nspname not like 'pg_%'
        group by t.oid, n.nspname, t.relname, idx.relname, ix.indisunique, ix.indisprimary, am.amname
        order by n.nspname, t.relname, idx.relname
        "#,
    )
    .fetch_all(pool)
    .await
    .context("failed to query PostgreSQL catalog indexes")?;

    rows.into_iter()
        .map(|row| {
            let columns: Vec<String> = row.try_get("columns")?;

            Ok(CatalogIndex {
                table_oid: row.try_get("table_oid")?,
                name: row.try_get("index_name")?,
                columns,
                unique: row.try_get("is_unique")?,
                primary: row.try_get("is_primary")?,
                method: row.try_get("method")?,
            })
        })
        .filter(|index| {
            index
                .as_ref()
                .map_or(true, |index| !index.columns.is_empty())
        })
        .collect()
}

async fn load_catalog_foreign_keys(pool: &PgPool) -> Result<Vec<CatalogForeignKey>> {
    let rows = sqlx::query(
        r#"
        select
            con.conname as constraint_name,
            source.oid::text as source_table_oid,
            target.oid::text as target_table_oid,
            source_att.attname as source_column,
            target_att.attname as target_column,
            keys.ordinality::int as ordinal,
            con.confupdtype::text as on_update,
            con.confdeltype::text as on_delete
        from pg_constraint con
        join pg_class source on source.oid = con.conrelid
        join pg_namespace source_ns on source_ns.oid = source.relnamespace
        join pg_class target on target.oid = con.confrelid
        join pg_namespace target_ns on target_ns.oid = target.relnamespace
        join lateral unnest(con.conkey, con.confkey) with ordinality
            as keys(source_attnum, target_attnum, ordinality) on true
        join pg_attribute source_att
            on source_att.attrelid = source.oid and source_att.attnum = keys.source_attnum
        join pg_attribute target_att
            on target_att.attrelid = target.oid and target_att.attnum = keys.target_attnum
        where con.contype = 'f'
          and source.relkind in ('r', 'p')
          and target.relkind in ('r', 'p')
          and source_ns.nspname <> 'information_schema'
          and source_ns.nspname not like 'pg_%'
          and target_ns.nspname <> 'information_schema'
          and target_ns.nspname not like 'pg_%'
        order by source_ns.nspname, source.relname, con.conname, keys.ordinality
        "#,
    )
    .fetch_all(pool)
    .await
    .context("failed to query PostgreSQL catalog foreign keys")?;

    rows.into_iter()
        .map(|row| {
            Ok(CatalogForeignKey {
                name: row.try_get("constraint_name")?,
                source_table_oid: row.try_get("source_table_oid")?,
                target_table_oid: row.try_get("target_table_oid")?,
                source_column: row.try_get("source_column")?,
                target_column: row.try_get("target_column")?,
                ordinal: row.try_get("ordinal")?,
                on_update: action_code_label(row.try_get::<Option<String>, _>("on_update")?),
                on_delete: action_code_label(row.try_get::<Option<String>, _>("on_delete")?),
            })
        })
        .collect()
}

async fn load_catalog_enums(pool: &PgPool) -> Result<Vec<CatalogEnumValue>> {
    let rows = sqlx::query(
        r#"
        select
            n.nspname as schema_name,
            t.typname as enum_name,
            e.enumlabel as value_name,
            row_number() over (
                partition by n.nspname, t.typname
                order by e.enumsortorder
            )::int as sort_order
        from pg_type t
        join pg_namespace n on n.oid = t.typnamespace
        join pg_enum e on e.enumtypid = t.oid
        where n.nspname <> 'information_schema'
          and n.nspname not like 'pg_%'
        order by n.nspname, t.typname, e.enumsortorder
        "#,
    )
    .fetch_all(pool)
    .await
    .context("failed to query PostgreSQL catalog enum types")?;

    rows.into_iter()
        .map(|row| {
            Ok(CatalogEnumValue {
                schema_name: row.try_get("schema_name")?,
                enum_name: row.try_get("enum_name")?,
                value_name: row.try_get("value_name")?,
                sort_order: row.try_get("sort_order")?,
            })
        })
        .collect()
}

fn build_database_diagram_document(
    mut snapshot: CatalogSnapshot,
) -> Result<DatabaseDiagramDocument> {
    if snapshot.tables.len() > MAX_GENERATED_DATABASE_DIAGRAM_TABLES {
        bail!(
            "database diagram generation supports up to {MAX_GENERATED_DATABASE_DIAGRAM_TABLES} tables; selected database has {} user tables",
            snapshot.tables.len()
        );
    }

    sort_snapshot(&mut snapshot);

    let table_by_oid = snapshot
        .tables
        .iter()
        .map(|table| (table.oid.clone(), table.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut columns_by_table = BTreeMap::<String, Vec<CatalogColumn>>::new();
    for column in snapshot.columns {
        if table_by_oid.contains_key(&column.table_oid) {
            columns_by_table
                .entry(column.table_oid.clone())
                .or_default()
                .push(column);
        }
    }

    let mut indexes_by_table = BTreeMap::<String, Vec<CatalogIndex>>::new();
    let mut primary_columns = BTreeSet::<(String, String)>::new();
    let mut unique_columns = BTreeSet::<(String, String)>::new();
    for index in snapshot.indexes {
        if !table_by_oid.contains_key(&index.table_oid) || index.columns.is_empty() {
            continue;
        }

        if index.primary {
            for column in &index.columns {
                primary_columns.insert((index.table_oid.clone(), column.clone()));
            }
        }

        if index.unique && index.columns.len() == 1 {
            unique_columns.insert((index.table_oid.clone(), index.columns[0].clone()));
        }

        indexes_by_table
            .entry(index.table_oid.clone())
            .or_default()
            .push(index);
    }

    let tables = snapshot
        .tables
        .iter()
        .enumerate()
        .map(|(index, table)| {
            let columns = columns_by_table
                .remove(&table.oid)
                .unwrap_or_default()
                .into_iter()
                .map(|column| {
                    let column_key = (table.oid.clone(), column.name.clone());

                    DatabaseDiagramColumn {
                        id: column_id(table, &column.name),
                        name: column.name,
                        data_type: column.data_type,
                        nullable: column.nullable,
                        primary_key: primary_columns.contains(&column_key),
                        unique: unique_columns.contains(&column_key),
                        default_value: column.default_value,
                        comment: column.comment,
                    }
                })
                .collect();
            let indexes = indexes_by_table
                .remove(&table.oid)
                .unwrap_or_default()
                .into_iter()
                .map(|index| DatabaseDiagramIndex {
                    id: stable_id(
                        "index",
                        &[&table.schema_name, &table.table_name, &index.name],
                    ),
                    name: index.name,
                    columns: index.columns,
                    unique: index.unique,
                    method: index.method,
                })
                .collect();

            DatabaseDiagramTable {
                id: table_id(table),
                name: table.table_name.clone(),
                schema: Some(table.schema_name.clone()),
                position: table_position(index),
                color: None,
                comment: table.comment.clone(),
                columns,
                indexes,
            }
        })
        .collect::<Vec<_>>();

    let relationships = snapshot
        .foreign_keys
        .into_iter()
        .filter_map(|foreign_key| {
            let source_table = table_by_oid.get(&foreign_key.source_table_oid)?;
            let target_table = table_by_oid.get(&foreign_key.target_table_oid)?;
            let source_key = (
                foreign_key.source_table_oid.clone(),
                foreign_key.source_column.clone(),
            );
            let cardinality = if unique_columns.contains(&source_key) {
                DatabaseDiagramCardinality::OneToOne
            } else {
                DatabaseDiagramCardinality::ManyToOne
            };

            Some(DatabaseDiagramRelationship {
                id: stable_id(
                    "relationship",
                    &[
                        &source_table.schema_name,
                        &source_table.table_name,
                        &foreign_key.source_column,
                        &target_table.schema_name,
                        &target_table.table_name,
                        &foreign_key.target_column,
                        &foreign_key.name,
                    ],
                ),
                name: relationship_name(&foreign_key),
                source: relationship_endpoint(source_table, &foreign_key.source_column),
                target: relationship_endpoint(target_table, &foreign_key.target_column),
                cardinality,
                on_update: foreign_key.on_update,
                on_delete: foreign_key.on_delete,
            })
        })
        .collect();

    let enums = database_diagram_enums(snapshot.enums);

    Ok(DatabaseDiagramDocument {
        version: 1,
        database_engine: ManagedDatabaseEngine::Postgres,
        tables,
        relationships,
        notes: Vec::new(),
        areas: Vec::new(),
        enums,
    })
}

fn sort_snapshot(snapshot: &mut CatalogSnapshot) {
    snapshot.tables.sort_by(|left, right| {
        left.schema_name
            .cmp(&right.schema_name)
            .then_with(|| left.table_name.cmp(&right.table_name))
            .then_with(|| left.oid.cmp(&right.oid))
    });
    snapshot.columns.sort_by(|left, right| {
        left.table_oid
            .cmp(&right.table_oid)
            .then_with(|| left.ordinal.cmp(&right.ordinal))
            .then_with(|| left.name.cmp(&right.name))
    });
    snapshot.indexes.sort_by(|left, right| {
        left.table_oid
            .cmp(&right.table_oid)
            .then_with(|| left.name.cmp(&right.name))
    });
    snapshot.foreign_keys.sort_by(|left, right| {
        left.source_table_oid
            .cmp(&right.source_table_oid)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.ordinal.cmp(&right.ordinal))
    });
    snapshot.enums.sort_by(|left, right| {
        left.schema_name
            .cmp(&right.schema_name)
            .then_with(|| left.enum_name.cmp(&right.enum_name))
            .then_with(|| left.sort_order.cmp(&right.sort_order))
            .then_with(|| left.value_name.cmp(&right.value_name))
    });
}

fn database_diagram_enums(values: Vec<CatalogEnumValue>) -> Vec<DatabaseDiagramEnum> {
    let mut grouped = BTreeMap::<(String, String), Vec<String>>::new();
    for value in values {
        grouped
            .entry((value.schema_name, value.enum_name))
            .or_default()
            .push(value.value_name);
    }

    grouped
        .into_iter()
        .map(|((schema_name, enum_name), values)| DatabaseDiagramEnum {
            id: stable_id("enum", &[&schema_name, &enum_name]),
            name: qualified_name(&schema_name, &enum_name),
            values: values
                .into_iter()
                .map(|value_name| DatabaseDiagramEnumValue {
                    id: stable_id("enum_value", &[&schema_name, &enum_name, &value_name]),
                    name: value_name,
                    comment: None,
                })
                .collect(),
        })
        .collect()
}

fn relationship_name(foreign_key: &CatalogForeignKey) -> String {
    if foreign_key.ordinal <= 1 {
        return foreign_key.name.clone();
    }

    format!("{} ({})", foreign_key.name, foreign_key.source_column)
}

fn relationship_endpoint(
    table: &CatalogTable,
    column_name: &str,
) -> DatabaseDiagramRelationshipEndpoint {
    DatabaseDiagramRelationshipEndpoint {
        table_id: table_id(table),
        table_name: table.table_name.clone(),
        column_id: column_id(table, column_name),
        column_name: column_name.to_owned(),
    }
}

fn action_code_label(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        match value.as_str() {
            "a" => Some("no_action"),
            "r" => Some("restrict"),
            "c" => Some("cascade"),
            "n" => Some("set_null"),
            "d" => Some("set_default"),
            _ => None,
        }
        .map(str::to_owned)
    })
}

fn table_position(index: usize) -> DatabaseDiagramPoint {
    DatabaseDiagramPoint {
        x: GRID_X_OFFSET + (index % GRID_COLUMNS) as i32 * GRID_X_GAP,
        y: GRID_Y_OFFSET + (index / GRID_COLUMNS) as i32 * GRID_Y_GAP,
    }
}

fn table_id(table: &CatalogTable) -> String {
    stable_id("table", &[&table.schema_name, &table.table_name])
}

fn column_id(table: &CatalogTable, column_name: &str) -> String {
    stable_id(
        "column",
        &[&table.schema_name, &table.table_name, column_name],
    )
}

fn qualified_name(schema_name: &str, name: &str) -> String {
    if schema_name == "public" {
        name.to_owned()
    } else {
        format!("{schema_name}.{name}")
    }
}

fn stable_id(prefix: &str, parts: &[&str]) -> String {
    let mut id = String::from(prefix);

    for part in parts {
        id.push('_');
        id.push_str(&sanitize_id_part(part));
    }

    id.trim_end_matches('_').to_owned()
}

fn sanitize_id_part(value: &str) -> String {
    let mut output = String::new();
    let mut last_was_separator = false;

    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            output.push(character);
            last_was_separator = false;
        } else if !last_was_separator {
            output.push('_');
            last_was_separator = true;
        }
    }

    let output = output.trim_matches('_');
    if output.is_empty() {
        "item".to_owned()
    } else {
        output.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_document_includes_keys_indexes_relationships_and_enums() {
        let document = build_database_diagram_document(CatalogSnapshot {
            tables: vec![
                CatalogTable {
                    oid: "2".to_owned(),
                    schema_name: "public".to_owned(),
                    table_name: "orders".to_owned(),
                    comment: None,
                },
                CatalogTable {
                    oid: "1".to_owned(),
                    schema_name: "public".to_owned(),
                    table_name: "users".to_owned(),
                    comment: Some("Application users".to_owned()),
                },
            ],
            columns: vec![
                column("1", "id", 1, "uuid", false),
                column("1", "email", 2, "text", false),
                column("1", "status", 3, "user_status", false),
                column("2", "id", 1, "uuid", false),
                column("2", "user_id", 2, "uuid", false),
                column("2", "number", 3, "text", false),
            ],
            indexes: vec![
                index("1", "users_pkey", vec!["id"], true, true),
                index("1", "users_email_key", vec!["email"], true, false),
                index("2", "orders_pkey", vec!["id"], true, true),
                index("2", "orders_user_id_idx", vec!["user_id"], false, false),
            ],
            foreign_keys: vec![CatalogForeignKey {
                name: "orders_user_id_fkey".to_owned(),
                source_table_oid: "2".to_owned(),
                target_table_oid: "1".to_owned(),
                source_column: "user_id".to_owned(),
                target_column: "id".to_owned(),
                ordinal: 1,
                on_update: Some("cascade".to_owned()),
                on_delete: Some("restrict".to_owned()),
            }],
            enums: vec![
                enum_value("public", "user_status", "active"),
                enum_value("public", "user_status", "disabled"),
            ],
        })
        .unwrap();

        assert_eq!(
            document
                .tables
                .iter()
                .map(|table| table.name.as_str())
                .collect::<Vec<_>>(),
            vec!["orders", "users"]
        );
        let users = document
            .tables
            .iter()
            .find(|table| table.name == "users")
            .unwrap();
        let user_id = users
            .columns
            .iter()
            .find(|column| column.name == "id")
            .unwrap();
        let email = users
            .columns
            .iter()
            .find(|column| column.name == "email")
            .unwrap();

        assert_eq!(users.comment.as_deref(), Some("Application users"));
        assert!(user_id.primary_key);
        assert!(user_id.unique);
        assert!(email.unique);
        assert_eq!(users.indexes[0].name, "users_email_key");
        assert_eq!(document.relationships.len(), 1);
        assert_eq!(
            document.relationships[0].cardinality,
            DatabaseDiagramCardinality::ManyToOne
        );
        assert_eq!(
            document.relationships[0].on_update.as_deref(),
            Some("cascade")
        );
        assert_eq!(document.enums[0].name, "user_status");
        assert_eq!(
            document.enums[0]
                .values
                .iter()
                .map(|value| value.name.as_str())
                .collect::<Vec<_>>(),
            vec!["active", "disabled"]
        );
    }

    #[test]
    fn catalog_document_is_empty_when_catalog_has_no_user_tables() {
        let document = build_database_diagram_document(CatalogSnapshot::default()).unwrap();

        assert!(document.tables.is_empty());
        assert!(document.relationships.is_empty());
        assert!(document.enums.is_empty());
    }

    #[test]
    fn catalog_document_uses_deterministic_layout_and_ids() {
        let snapshot = CatalogSnapshot {
            tables: vec![
                table("3", "analytics", "Daily Revenue"),
                table("1", "public", "users"),
                table("2", "public", "orders"),
            ],
            columns: vec![
                column("1", "id", 1, "uuid", false),
                column("2", "id", 1, "uuid", false),
                column("3", "day", 1, "date", false),
            ],
            ..CatalogSnapshot::default()
        };

        let left = build_database_diagram_document(snapshot.clone()).unwrap();
        let right = build_database_diagram_document(snapshot).unwrap();

        assert_eq!(left, right);
        assert_eq!(left.tables[0].id, "table_analytics_daily_revenue");
        assert_eq!(
            left.tables[0].position,
            DatabaseDiagramPoint { x: 80, y: 80 }
        );
        assert_eq!(
            left.tables[2].position,
            DatabaseDiagramPoint { x: 800, y: 80 }
        );
    }

    #[test]
    fn composite_primary_key_columns_are_not_marked_individually_unique() {
        let document = build_database_diagram_document(CatalogSnapshot {
            tables: vec![table("1", "public", "membership")],
            columns: vec![
                column("1", "user_id", 1, "uuid", false),
                column("1", "team_id", 2, "uuid", false),
            ],
            indexes: vec![index(
                "1",
                "membership_pkey",
                vec!["user_id", "team_id"],
                true,
                true,
            )],
            ..CatalogSnapshot::default()
        })
        .unwrap();

        assert!(document.tables[0].columns[0].primary_key);
        assert!(!document.tables[0].columns[0].unique);
        assert!(document.tables[0].columns[1].primary_key);
        assert!(!document.tables[0].columns[1].unique);
    }

    #[test]
    fn catalog_document_rejects_table_count_over_limit() {
        let snapshot = CatalogSnapshot {
            tables: (0..=MAX_GENERATED_DATABASE_DIAGRAM_TABLES)
                .map(|index| table(&index.to_string(), "public", &format!("table_{index}")))
                .collect(),
            ..CatalogSnapshot::default()
        };

        let error = build_database_diagram_document(snapshot).unwrap_err();

        assert!(error.to_string().contains("supports up to 200 tables"));
    }

    fn table(oid: &str, schema_name: &str, table_name: &str) -> CatalogTable {
        CatalogTable {
            oid: oid.to_owned(),
            schema_name: schema_name.to_owned(),
            table_name: table_name.to_owned(),
            comment: None,
        }
    }

    fn column(
        table_oid: &str,
        name: &str,
        ordinal: i32,
        data_type: &str,
        nullable: bool,
    ) -> CatalogColumn {
        CatalogColumn {
            table_oid: table_oid.to_owned(),
            name: name.to_owned(),
            ordinal,
            data_type: data_type.to_owned(),
            nullable,
            default_value: None,
            comment: None,
        }
    }

    fn index(
        table_oid: &str,
        name: &str,
        columns: Vec<&str>,
        unique: bool,
        primary: bool,
    ) -> CatalogIndex {
        CatalogIndex {
            table_oid: table_oid.to_owned(),
            name: name.to_owned(),
            columns: columns.into_iter().map(str::to_owned).collect(),
            unique,
            primary,
            method: Some("btree".to_owned()),
        }
    }

    fn enum_value(schema_name: &str, enum_name: &str, value_name: &str) -> CatalogEnumValue {
        CatalogEnumValue {
            schema_name: schema_name.to_owned(),
            enum_name: enum_name.to_owned(),
            value_name: value_name.to_owned(),
            sort_order: 0,
        }
    }
}
