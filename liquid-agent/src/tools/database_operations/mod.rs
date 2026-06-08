mod backup;
mod restore;

use std::sync::Arc;

use anyhow::{Result, bail};
use liquid_core::{DatabaseBackupMetadataStore, DatabaseBackupStatus};
use serde_json::Value;

pub(crate) use backup::{
    PgDeleteDatabaseBackupTool, PgGetDatabaseBackupTool, PgListDatabaseBackupsTool,
    PgStartDatabaseBackupTool,
};
pub(crate) use restore::{
    PgGetDatabaseRestoreTool, PgListDatabaseRestoresTool, PgStartDatabaseRestoreTool,
};

#[derive(Clone)]
pub struct DatabaseOperationToolContext {
    owner_user_id: String,
    metadata_store: Arc<dyn DatabaseBackupMetadataStore>,
}

impl DatabaseOperationToolContext {
    pub fn new(
        owner_user_id: impl Into<String>,
        metadata_store: Arc<dyn DatabaseBackupMetadataStore>,
    ) -> Self {
        Self {
            owner_user_id: owner_user_id.into(),
            metadata_store,
        }
    }
}

fn required_string_arg(arguments: &Value, name: &str, tool: &str) -> Result<String> {
    let Some(value) = arguments.get(name).and_then(Value::as_str) else {
        bail!("{tool} requires string argument: {name}");
    };
    let value = value.trim();
    if value.is_empty() {
        bail!("{tool} requires non-empty string argument: {name}");
    }

    Ok(value.to_owned())
}

fn optional_string_arg(arguments: &Value, name: &str) -> Result<Option<String>> {
    arguments
        .get(name)
        .map(|value| {
            let Some(value) = value.as_str() else {
                bail!("{name} must be a string");
            };
            let value = value.trim();
            if value.is_empty() {
                bail!("{name} must not be empty");
            }

            Ok(value.to_owned())
        })
        .transpose()
}

fn optional_status_arg(arguments: &Value, name: &str) -> Result<Option<DatabaseBackupStatus>> {
    arguments
        .get(name)
        .map(|value| {
            let Some(value) = value.as_str() else {
                bail!("{name} must be a string");
            };
            match value.trim() {
                "queued" => Ok(DatabaseBackupStatus::Queued),
                "running" => Ok(DatabaseBackupStatus::Running),
                "succeeded" => Ok(DatabaseBackupStatus::Succeeded),
                "failed" => Ok(DatabaseBackupStatus::Failed),
                "deleted" => Ok(DatabaseBackupStatus::Deleted),
                other => bail!(
                    "{name} must be one of queued, running, succeeded, failed, or deleted; got {other}"
                ),
            }
        })
        .transpose()
}

fn limit_arg(arguments: &Value) -> i64 {
    arguments
        .get("limit")
        .and_then(Value::as_i64)
        .unwrap_or(20)
        .clamp(1, 100)
}
