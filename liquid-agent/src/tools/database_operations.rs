use std::sync::Arc;

use anyhow::{Result, bail};
use async_trait::async_trait;
use liquid_core::{DatabaseBackupMetadataStore, DatabaseBackupStatus};
use liquid_llm::ToolDefinition;
use serde_json::{Value, json};

use crate::{tools::AgentTool, types::ToolOutput};

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

#[derive(Clone)]
pub(crate) struct PgStartDatabaseBackupTool {
    context: DatabaseOperationToolContext,
}

impl PgStartDatabaseBackupTool {
    pub(crate) fn new(context: DatabaseOperationToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgStartDatabaseBackupTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_start_database_backup",
            "Queue an asynchronous PostgreSQL managed database backup. Use pg_get_database_backup to monitor progress.",
            json!({
                "type": "object",
                "properties": {
                    "managed_database_id": {
                        "type": "string",
                        "description": "Managed database id to back up."
                    },
                    "purpose": {
                        "type": "string",
                        "description": "Optional human-readable reason for the backup."
                    }
                },
                "required": ["managed_database_id"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let managed_database_id = required_string_arg(
            &arguments,
            "managed_database_id",
            "pg_start_database_backup",
        )?;
        let purpose = optional_string_arg(&arguments, "purpose")?;
        let backup = self
            .context
            .metadata_store
            .create_database_backup(&self.context.owner_user_id, &managed_database_id, purpose)
            .await?;

        Ok(ToolOutput::json(json!({
            "backup": backup,
            "next_tool": "pg_get_database_backup",
        })))
    }
}

#[derive(Clone)]
pub(crate) struct PgGetDatabaseBackupTool {
    context: DatabaseOperationToolContext,
}

impl PgGetDatabaseBackupTool {
    pub(crate) fn new(context: DatabaseOperationToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgGetDatabaseBackupTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_get_database_backup",
            "Read one persisted PostgreSQL database backup record by backup id.",
            json!({
                "type": "object",
                "properties": {
                    "backup_id": {
                        "type": "string",
                        "description": "Database backup id."
                    }
                },
                "required": ["backup_id"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let backup_id = required_string_arg(&arguments, "backup_id", "pg_get_database_backup")?;
        let backup = self
            .context
            .metadata_store
            .get_database_backup(&self.context.owner_user_id, &backup_id)
            .await?;

        Ok(ToolOutput::json(json!({ "backup": backup })))
    }
}

#[derive(Clone)]
pub(crate) struct PgListDatabaseBackupsTool {
    context: DatabaseOperationToolContext,
}

impl PgListDatabaseBackupsTool {
    pub(crate) fn new(context: DatabaseOperationToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgListDatabaseBackupsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_list_database_backups",
            "List persisted PostgreSQL database backups for the current user, optionally filtered.",
            json!({
                "type": "object",
                "properties": {
                    "managed_database_id": {
                        "type": "string",
                        "description": "Optional managed database id filter."
                    },
                    "status": {
                        "type": "string",
                        "description": "Optional status: queued, running, succeeded, failed, or deleted."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum records to return; defaults to 20 and clamps at 100."
                    }
                },
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let managed_database_id = optional_string_arg(&arguments, "managed_database_id")?;
        let status = optional_status_arg(&arguments, "status")?;
        let limit = limit_arg(&arguments);
        let backups = self
            .context
            .metadata_store
            .list_database_backups(
                &self.context.owner_user_id,
                managed_database_id.as_deref(),
                status,
                limit,
            )
            .await?;

        Ok(ToolOutput::json(json!({
            "backups": backups,
            "count": backups.len(),
        })))
    }
}

#[derive(Clone)]
pub(crate) struct PgDeleteDatabaseBackupTool {
    context: DatabaseOperationToolContext,
}

impl PgDeleteDatabaseBackupTool {
    pub(crate) fn new(context: DatabaseOperationToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgDeleteDatabaseBackupTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_delete_database_backup",
            "Mark a persisted PostgreSQL database backup as deleted. Running backups cannot be deleted.",
            json!({
                "type": "object",
                "properties": {
                    "backup_id": {
                        "type": "string",
                        "description": "Database backup id to delete."
                    }
                },
                "required": ["backup_id"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let backup_id = required_string_arg(&arguments, "backup_id", "pg_delete_database_backup")?;
        let backup = self
            .context
            .metadata_store
            .delete_database_backup(&self.context.owner_user_id, &backup_id)
            .await?;

        Ok(ToolOutput::json(json!({ "backup": backup })))
    }
}

#[derive(Clone)]
pub(crate) struct PgStartDatabaseRestoreTool {
    context: DatabaseOperationToolContext,
}

impl PgStartDatabaseRestoreTool {
    pub(crate) fn new(context: DatabaseOperationToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgStartDatabaseRestoreTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_start_database_restore",
            "Queue an asynchronous destructive PostgreSQL restore from persisted backup metadata.",
            json!({
                "type": "object",
                "properties": {
                    "backup_id": {
                        "type": "string",
                        "description": "Succeeded backup id to restore from."
                    },
                    "target_managed_database_id": {
                        "type": "string",
                        "description": "Managed database id to restore into."
                    },
                    "purpose": {
                        "type": "string",
                        "description": "Human-readable reason for the destructive restore."
                    },
                    "confirm_destructive_restore": {
                        "type": "boolean",
                        "description": "Must be true to acknowledge that pg_restore may delete or replace target database objects."
                    }
                },
                "required": [
                    "backup_id",
                    "target_managed_database_id",
                    "purpose",
                    "confirm_destructive_restore"
                ],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let backup_id = required_string_arg(&arguments, "backup_id", "pg_start_database_restore")?;
        let target_managed_database_id = required_string_arg(
            &arguments,
            "target_managed_database_id",
            "pg_start_database_restore",
        )?;
        let purpose = required_string_arg(&arguments, "purpose", "pg_start_database_restore")?;
        if arguments
            .get("confirm_destructive_restore")
            .and_then(Value::as_bool)
            != Some(true)
        {
            bail!("pg_start_database_restore requires confirm_destructive_restore=true");
        }

        let restore = self
            .context
            .metadata_store
            .create_database_restore(
                &self.context.owner_user_id,
                &backup_id,
                &target_managed_database_id,
                purpose,
            )
            .await?;

        Ok(ToolOutput::json(json!({
            "restore": restore,
            "next_tool": "pg_get_database_restore",
        })))
    }
}

#[derive(Clone)]
pub(crate) struct PgGetDatabaseRestoreTool {
    context: DatabaseOperationToolContext,
}

impl PgGetDatabaseRestoreTool {
    pub(crate) fn new(context: DatabaseOperationToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgGetDatabaseRestoreTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_get_database_restore",
            "Read one persisted PostgreSQL database restore job by restore id.",
            json!({
                "type": "object",
                "properties": {
                    "restore_id": {
                        "type": "string",
                        "description": "Database restore job id."
                    }
                },
                "required": ["restore_id"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let restore_id = required_string_arg(&arguments, "restore_id", "pg_get_database_restore")?;
        let restore = self
            .context
            .metadata_store
            .get_database_restore(&self.context.owner_user_id, &restore_id)
            .await?;

        Ok(ToolOutput::json(json!({ "restore": restore })))
    }
}

#[derive(Clone)]
pub(crate) struct PgListDatabaseRestoresTool {
    context: DatabaseOperationToolContext,
}

impl PgListDatabaseRestoresTool {
    pub(crate) fn new(context: DatabaseOperationToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgListDatabaseRestoresTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_list_database_restores",
            "List persisted PostgreSQL database restore jobs for the current user, optionally filtered.",
            json!({
                "type": "object",
                "properties": {
                    "backup_id": {
                        "type": "string",
                        "description": "Optional backup id filter."
                    },
                    "target_managed_database_id": {
                        "type": "string",
                        "description": "Optional target managed database id filter."
                    },
                    "status": {
                        "type": "string",
                        "description": "Optional status: queued, running, succeeded, failed, or deleted."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum records to return; defaults to 20 and clamps at 100."
                    }
                },
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let backup_id = optional_string_arg(&arguments, "backup_id")?;
        let target_managed_database_id =
            optional_string_arg(&arguments, "target_managed_database_id")?;
        let status = optional_status_arg(&arguments, "status")?;
        let limit = limit_arg(&arguments);
        let restores = self
            .context
            .metadata_store
            .list_database_restores(
                &self.context.owner_user_id,
                backup_id.as_deref(),
                target_managed_database_id.as_deref(),
                status,
                limit,
            )
            .await?;

        Ok(ToolOutput::json(json!({
            "restores": restores,
            "count": restores.len(),
        })))
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
