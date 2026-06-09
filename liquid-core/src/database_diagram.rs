use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::ManagedDatabaseEngine;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseDiagram {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    pub document: DatabaseDiagramDocument,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseDiagramDocument {
    #[serde(default = "default_document_version")]
    pub version: i32,
    #[serde(default = "default_database_engine")]
    pub database_engine: ManagedDatabaseEngine,
    #[serde(default)]
    pub tables: Vec<DatabaseDiagramTable>,
    #[serde(default)]
    pub relationships: Vec<DatabaseDiagramRelationship>,
    #[serde(default)]
    pub notes: Vec<DatabaseDiagramNote>,
    #[serde(default)]
    pub areas: Vec<DatabaseDiagramArea>,
    #[serde(default)]
    pub enums: Vec<DatabaseDiagramEnum>,
}

impl Default for DatabaseDiagramDocument {
    fn default() -> Self {
        Self {
            version: default_document_version(),
            database_engine: default_database_engine(),
            tables: Vec::new(),
            relationships: Vec::new(),
            notes: Vec::new(),
            areas: Vec::new(),
            enums: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseDiagramTable {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub schema: Option<String>,
    pub position: DatabaseDiagramPoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment: Option<String>,
    #[serde(default)]
    pub columns: Vec<DatabaseDiagramColumn>,
    #[serde(default)]
    pub indexes: Vec<DatabaseDiagramIndex>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseDiagramPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseDiagramSize {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseDiagramColumn {
    pub id: String,
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub default_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseDiagramIndex {
    pub id: String,
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseDiagramRelationship {
    pub id: String,
    pub name: String,
    pub source: DatabaseDiagramRelationshipEndpoint,
    pub target: DatabaseDiagramRelationshipEndpoint,
    pub cardinality: DatabaseDiagramCardinality,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub on_update: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub on_delete: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseDiagramRelationshipEndpoint {
    pub table_id: String,
    pub table_name: String,
    pub column_id: String,
    pub column_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DatabaseDiagramCardinality {
    OneToOne,
    OneToMany,
    ManyToOne,
    ManyToMany,
}

impl DatabaseDiagramCardinality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OneToOne => "one_to_one",
            Self::OneToMany => "one_to_many",
            Self::ManyToOne => "many_to_one",
            Self::ManyToMany => "many_to_many",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseDiagramNote {
    pub id: String,
    pub title: String,
    pub body: String,
    pub position: DatabaseDiagramPoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseDiagramArea {
    pub id: String,
    pub title: String,
    pub position: DatabaseDiagramPoint,
    pub size: DatabaseDiagramSize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseDiagramEnum {
    pub id: String,
    pub name: String,
    pub values: Vec<DatabaseDiagramEnumValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseDiagramEnumValue {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateDatabaseDiagramRequest {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub document: Option<DatabaseDiagramDocument>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateDatabaseDiagramRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub document: Option<DatabaseDiagramDocument>,
}

fn default_document_version() -> i32 {
    1
}

fn default_database_engine() -> ManagedDatabaseEngine {
    ManagedDatabaseEngine::Postgres
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagram_document_deserializes_minimal_llm_shape() {
        let document = serde_json::from_value::<DatabaseDiagramDocument>(serde_json::json!({
            "tables": [{
                "id": "table_customers",
                "name": "customers",
                "position": { "x": 100, "y": 120 },
                "columns": [{
                    "id": "column_customer_id",
                    "name": "id",
                    "data_type": "uuid",
                    "nullable": false,
                    "primary_key": true,
                    "unique": true
                }]
            }],
            "relationships": []
        }))
        .unwrap();

        assert_eq!(document.version, 1);
        assert_eq!(document.database_engine, ManagedDatabaseEngine::Postgres);
        assert_eq!(document.tables[0].name, "customers");
    }
}
