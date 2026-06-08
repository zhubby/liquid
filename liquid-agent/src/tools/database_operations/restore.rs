use anyhow::{Result, bail};
use async_trait::async_trait;
use liquid_llm::ToolDefinition;
use serde_json::{Value, json};

use crate::{tools::AgentTool, types::ToolOutput};

use super::{
    DatabaseOperationToolContext, limit_arg, optional_status_arg, optional_string_arg,
    required_string_arg,
};

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
