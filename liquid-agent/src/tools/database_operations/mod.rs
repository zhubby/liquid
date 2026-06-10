mod backup;
mod diagnostic;
mod restore;

use std::sync::Arc;

use anyhow::{Result, bail};
use liquid_core::{
    DatabaseBackupMetadataStore, DatabaseBackupScheduleStatus, DatabaseBackupStatus,
    DatabaseOperationDiagnosticFilters, DatabaseOperationDiagnosticRecord, DatabaseOperationKind,
};
use serde_json::Value;

pub(crate) use backup::{
    PgCreateDatabaseBackupScheduleTool, PgDeleteDatabaseBackupScheduleTool,
    PgDeleteDatabaseBackupTool, PgGetDatabaseBackupTool, PgListDatabaseBackupSchedulesTool,
    PgListDatabaseBackupsTool, PgStartDatabaseBackupTool, PgUpdateDatabaseBackupScheduleTool,
};
pub(crate) use diagnostic::PgGetDatabaseOperationDiagnosticsTool;
pub(crate) use restore::{
    PgGetDatabaseRestoreTool, PgListDatabaseRestoresTool, PgStartDatabaseRestoreTool,
};

#[derive(Clone)]
pub struct DatabaseOperationToolContext {
    owner_user_id: String,
    metadata_store: Arc<dyn DatabaseBackupMetadataStore>,
    conversation_id: Option<String>,
    turn_id: Option<String>,
}

impl DatabaseOperationToolContext {
    pub fn new(
        owner_user_id: impl Into<String>,
        metadata_store: Arc<dyn DatabaseBackupMetadataStore>,
    ) -> Self {
        Self {
            owner_user_id: owner_user_id.into(),
            metadata_store,
            conversation_id: None,
            turn_id: None,
        }
    }

    pub fn with_chat_context(
        mut self,
        conversation_id: Option<String>,
        turn_id: Option<String>,
    ) -> Self {
        self.conversation_id = conversation_id;
        self.turn_id = turn_id;
        self
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

fn optional_schedule_status_arg(
    arguments: &Value,
    name: &str,
) -> Result<Option<DatabaseBackupScheduleStatus>> {
    arguments
        .get(name)
        .map(|value| {
            let Some(value) = value.as_str() else {
                bail!("{name} must be a string");
            };
            match value.trim() {
                "active" => Ok(DatabaseBackupScheduleStatus::Active),
                "paused" => Ok(DatabaseBackupScheduleStatus::Paused),
                "deleted" => Ok(DatabaseBackupScheduleStatus::Deleted),
                other => bail!("{name} must be one of active, paused, or deleted; got {other}"),
            }
        })
        .transpose()
}

fn optional_i32_arg(arguments: &Value, name: &str) -> Result<Option<i32>> {
    arguments
        .get(name)
        .map(|value| {
            let Some(value) = value.as_i64() else {
                bail!("{name} must be an integer");
            };
            i32::try_from(value).map_err(|_| anyhow::anyhow!("{name} is out of range"))
        })
        .transpose()
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

fn operation_kind_arg(arguments: &Value, name: &str, tool: &str) -> Result<DatabaseOperationKind> {
    let value = required_string_arg(arguments, name, tool)?;
    match value.as_str() {
        "backup" => Ok(DatabaseOperationKind::Backup),
        "restore" => Ok(DatabaseOperationKind::Restore),
        other => bail!("{name} must be one of backup or restore; got {other}"),
    }
}

fn limit_arg(arguments: &Value) -> i64 {
    arguments
        .get("limit")
        .and_then(Value::as_i64)
        .unwrap_or(20)
        .clamp(1, 100)
}

async fn recent_operation_diagnostics(
    context: &DatabaseOperationToolContext,
    operation_kind: DatabaseOperationKind,
    operation_id: &str,
    limit: i64,
) -> Result<Vec<DatabaseOperationDiagnosticRecord>> {
    Ok(context
        .metadata_store
        .list_database_operation_diagnostics(
            &context.owner_user_id,
            DatabaseOperationDiagnosticFilters {
                operation_kind,
                operation_id,
                limit,
            },
        )
        .await?)
}
