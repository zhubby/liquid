use liquid_core::{
    CreateDatabaseDiagramRequest, DatabaseDiagram, DatabaseDiagramArea, DatabaseDiagramColumn,
    DatabaseDiagramDocument, DatabaseDiagramEnum, DatabaseDiagramIndex, DatabaseDiagramNote,
    DatabaseDiagramRelationship, DatabaseDiagramRelationshipEndpoint, DatabaseDiagramTable,
    UpdateDatabaseDiagramRequest,
};
use serde_json::Value;
use time::OffsetDateTime;

use crate::{
    error::{StorageError, map_database_error},
    store::Storage,
    validation::required_string,
};

const DATABASE_DIAGRAM_COLUMNS: &str = r#"
id::text,
title,
description,
document,
created_at,
updated_at
"#;

pub(crate) async fn list_database_diagrams(
    storage: &Storage,
    owner_user_id: &str,
) -> Result<Vec<DatabaseDiagram>, StorageError> {
    let rows = sqlx::query_as::<_, DatabaseDiagramRow>(&format!(
        r#"
        select {DATABASE_DIAGRAM_COLUMNS}
        from database_diagrams
        where owner_user_id = $1::uuid
        order by updated_at desc, created_at desc
        "#
    ))
    .bind(owner_user_id)
    .fetch_all(&storage.pool)
    .await
    .map_err(map_database_error)?;

    rows.into_iter().map(DatabaseDiagram::try_from).collect()
}

pub(crate) async fn create_database_diagram(
    storage: &Storage,
    owner_user_id: &str,
    request: CreateDatabaseDiagramRequest,
) -> Result<DatabaseDiagram, StorageError> {
    let title = required_string("title", &request.title)?;
    let description = blank_to_none(request.description);
    let document = request.document.unwrap_or_default();
    validate_document(&document)?;
    let document = checked_json("document", &document)?;
    let row = sqlx::query_as::<_, DatabaseDiagramRow>(&format!(
        r#"
        insert into database_diagrams (
            owner_user_id,
            title,
            description,
            document
        )
        values ($1::uuid, $2, $3, $4)
        returning {DATABASE_DIAGRAM_COLUMNS}
        "#
    ))
    .bind(owner_user_id)
    .bind(title)
    .bind(description)
    .bind(document)
    .fetch_one(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.try_into()
}

pub(crate) async fn get_database_diagram(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<DatabaseDiagram, StorageError> {
    fetch_diagram(storage, owner_user_id, id).await
}

pub(crate) async fn update_database_diagram(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
    request: UpdateDatabaseDiagramRequest,
) -> Result<DatabaseDiagram, StorageError> {
    let title = request
        .title
        .map(|value| required_string("title", &value))
        .transpose()?;
    let description_present = request.description.is_some();
    let description = blank_to_none(request.description);
    let document_present = request.document.is_some();
    if let Some(document) = request.document.as_ref() {
        validate_document(document)?;
    }
    let document = checked_optional_json("document", &request.document)?;
    let row = sqlx::query_as::<_, DatabaseDiagramRow>(&format!(
        r#"
        update database_diagrams
        set title = coalesce($3, title),
            description = case when $4 then $5 else description end,
            document = case when $6 then $7 else document end,
            updated_at = now()
        where owner_user_id = $1::uuid
          and id = $2::uuid
        returning {DATABASE_DIAGRAM_COLUMNS}
        "#
    ))
    .bind(owner_user_id)
    .bind(id)
    .bind(title)
    .bind(description_present)
    .bind(description)
    .bind(document_present)
    .bind(document)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

pub(crate) async fn delete_database_diagram(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<(), StorageError> {
    let result = sqlx::query(
        r#"
        delete from database_diagrams
        where owner_user_id = $1::uuid
          and id = $2::uuid
        "#,
    )
    .bind(owner_user_id)
    .bind(id)
    .execute(&storage.pool)
    .await
    .map_err(map_database_error)?;

    if result.rows_affected() == 0 {
        return Err(StorageError::NotFound);
    }

    Ok(())
}

async fn fetch_diagram(
    storage: &Storage,
    owner_user_id: &str,
    id: &str,
) -> Result<DatabaseDiagram, StorageError> {
    let row = sqlx::query_as::<_, DatabaseDiagramRow>(&format!(
        r#"
        select {DATABASE_DIAGRAM_COLUMNS}
        from database_diagrams
        where owner_user_id = $1::uuid
          and id = $2::uuid
        "#
    ))
    .bind(owner_user_id)
    .bind(id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

fn validate_document(document: &DatabaseDiagramDocument) -> Result<(), StorageError> {
    if document.version <= 0 {
        return Err(StorageError::Validation(
            "diagram document version must be positive".to_owned(),
        ));
    }

    for table in &document.tables {
        validate_table(table)?;
    }

    for relationship in &document.relationships {
        validate_relationship(relationship)?;
    }

    for note in &document.notes {
        validate_note(note)?;
    }

    for area in &document.areas {
        validate_area(area)?;
    }

    for enum_item in &document.enums {
        validate_enum(enum_item)?;
    }

    Ok(())
}

fn validate_table(table: &DatabaseDiagramTable) -> Result<(), StorageError> {
    required_string("table.id", &table.id)?;
    required_string("table.name", &table.name)?;

    if let Some(schema) = &table.schema {
        required_string("table.schema", schema)?;
    }

    if let Some(color) = &table.color {
        required_string("table.color", color)?;
    }

    if let Some(comment) = &table.comment {
        required_string("table.comment", comment)?;
    }

    for column in &table.columns {
        validate_column(column)?;
    }

    for index in &table.indexes {
        validate_index(index)?;
    }

    Ok(())
}

fn validate_column(column: &DatabaseDiagramColumn) -> Result<(), StorageError> {
    required_string("column.id", &column.id)?;
    required_string("column.name", &column.name)?;
    required_string("column.data_type", &column.data_type)?;

    if let Some(default_value) = &column.default_value {
        required_string("column.default_value", default_value)?;
    }

    if let Some(comment) = &column.comment {
        required_string("column.comment", comment)?;
    }

    Ok(())
}

fn validate_index(index: &DatabaseDiagramIndex) -> Result<(), StorageError> {
    required_string("index.id", &index.id)?;
    required_string("index.name", &index.name)?;

    if index.columns.is_empty() {
        return Err(StorageError::Validation(
            "index requires at least one column".to_owned(),
        ));
    }

    for column in &index.columns {
        required_string("index.column", column)?;
    }

    if let Some(method) = &index.method {
        required_string("index.method", method)?;
    }

    Ok(())
}

fn validate_relationship(relationship: &DatabaseDiagramRelationship) -> Result<(), StorageError> {
    required_string("relationship.id", &relationship.id)?;
    required_string("relationship.name", &relationship.name)?;
    validate_endpoint(&relationship.source)?;
    validate_endpoint(&relationship.target)?;

    if let Some(on_update) = &relationship.on_update {
        required_string("relationship.on_update", on_update)?;
    }

    if let Some(on_delete) = &relationship.on_delete {
        required_string("relationship.on_delete", on_delete)?;
    }

    Ok(())
}

fn validate_endpoint(endpoint: &DatabaseDiagramRelationshipEndpoint) -> Result<(), StorageError> {
    required_string("relationship.table_id", &endpoint.table_id)?;
    required_string("relationship.table_name", &endpoint.table_name)?;
    required_string("relationship.column_id", &endpoint.column_id)?;
    required_string("relationship.column_name", &endpoint.column_name)?;
    Ok(())
}

fn validate_note(note: &DatabaseDiagramNote) -> Result<(), StorageError> {
    required_string("note.id", &note.id)?;
    required_string("note.title", &note.title)?;
    Ok(())
}

fn validate_area(area: &DatabaseDiagramArea) -> Result<(), StorageError> {
    required_string("area.id", &area.id)?;
    required_string("area.title", &area.title)?;

    if area.size.width <= 0 || area.size.height <= 0 {
        return Err(StorageError::Validation(
            "area size must be positive".to_owned(),
        ));
    }

    if let Some(color) = &area.color {
        required_string("area.color", color)?;
    }

    Ok(())
}

fn validate_enum(enum_item: &DatabaseDiagramEnum) -> Result<(), StorageError> {
    required_string("enum.id", &enum_item.id)?;
    required_string("enum.name", &enum_item.name)?;

    for value in &enum_item.values {
        required_string("enum.value.id", &value.id)?;
        required_string("enum.value.name", &value.name)?;

        if let Some(comment) = &value.comment {
            required_string("enum.value.comment", comment)?;
        }
    }

    Ok(())
}

fn checked_json<T: serde::Serialize>(field: &str, value: &T) -> Result<Value, StorageError> {
    serde_json::to_value(value)
        .map_err(|error| StorageError::Validation(format!("{field} is invalid: {error}")))
}

fn checked_optional_json<T: serde::Serialize>(
    field: &str,
    value: &Option<T>,
) -> Result<Option<Value>, StorageError> {
    value
        .as_ref()
        .map(|value| checked_json(field, value))
        .transpose()
}

fn blank_to_none(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn json_error(error: serde_json::Error) -> StorageError {
    StorageError::Validation(error.to_string())
}

#[derive(sqlx::FromRow)]
struct DatabaseDiagramRow {
    id: String,
    title: String,
    description: Option<String>,
    document: Value,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl TryFrom<DatabaseDiagramRow> for DatabaseDiagram {
    type Error = StorageError;

    fn try_from(row: DatabaseDiagramRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            title: row.title,
            description: row.description,
            document: serde_json::from_value(row.document).map_err(json_error)?,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use liquid_core::{
        DatabaseDiagramColumn, DatabaseDiagramDocument, DatabaseDiagramPoint, DatabaseDiagramTable,
    };

    use super::*;

    #[test]
    fn validate_document_accepts_table_and_column_shape() {
        let document = DatabaseDiagramDocument {
            tables: vec![DatabaseDiagramTable {
                id: "table_1".to_owned(),
                name: "customers".to_owned(),
                schema: Some("public".to_owned()),
                position: DatabaseDiagramPoint { x: 0, y: 0 },
                color: None,
                comment: None,
                columns: vec![DatabaseDiagramColumn {
                    id: "column_1".to_owned(),
                    name: "id".to_owned(),
                    data_type: "uuid".to_owned(),
                    nullable: false,
                    primary_key: true,
                    unique: true,
                    default_value: None,
                    comment: None,
                }],
                indexes: Vec::new(),
            }],
            ..DatabaseDiagramDocument::default()
        };

        validate_document(&document).unwrap();
    }

    #[test]
    fn validate_document_rejects_blank_table_name() {
        let document = DatabaseDiagramDocument {
            tables: vec![DatabaseDiagramTable {
                id: "table_1".to_owned(),
                name: " ".to_owned(),
                schema: None,
                position: DatabaseDiagramPoint { x: 0, y: 0 },
                color: None,
                comment: None,
                columns: Vec::new(),
                indexes: Vec::new(),
            }],
            ..DatabaseDiagramDocument::default()
        };

        let error = validate_document(&document).unwrap_err();

        assert_eq!(error.to_string(), "table.name is required");
    }
}
