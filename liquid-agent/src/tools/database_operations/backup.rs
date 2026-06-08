use anyhow::Result;
use async_trait::async_trait;
use liquid_llm::ToolDefinition;
use serde_json::{Value, json};

use crate::{tools::AgentTool, types::ToolOutput};

use super::{
    DatabaseOperationToolContext, limit_arg, optional_status_arg, optional_string_arg,
    required_string_arg,
};

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
