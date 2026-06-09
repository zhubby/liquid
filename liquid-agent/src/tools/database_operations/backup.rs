use anyhow::Result;
use async_trait::async_trait;
use liquid_core::{
    CreateDatabaseBackupScheduleRequest, EnqueueDatabaseBackup, UpdateDatabaseBackupScheduleRequest,
};
use liquid_llm::ToolDefinition;
use serde_json::{Value, json};
use time::OffsetDateTime;

use crate::{database_operations::validate_backup_schedule, tools::AgentTool, types::ToolOutput};

use super::{
    DatabaseOperationToolContext, limit_arg, optional_i32_arg, optional_schedule_status_arg,
    optional_status_arg, optional_string_arg, required_string_arg,
};

const MINIMUM_BACKUP_CRON_INTERVAL_SECONDS: i64 = 15 * 60;

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
            .enqueue_database_backup(
                &self.context.owner_user_id,
                EnqueueDatabaseBackup::immediate(
                    managed_database_id,
                    purpose,
                    self.context.conversation_id.clone(),
                    self.context.turn_id.clone(),
                ),
            )
            .await?;

        Ok(ToolOutput::json(json!({
            "backup": backup,
            "scheduled": true,
            "next_tool": "pg_get_database_backup",
        })))
    }
}

#[derive(Clone)]
pub(crate) struct PgCreateDatabaseBackupScheduleTool {
    context: DatabaseOperationToolContext,
}

impl PgCreateDatabaseBackupScheduleTool {
    pub(crate) fn new(context: DatabaseOperationToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgCreateDatabaseBackupScheduleTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_create_database_backup_schedule",
            "Create a cron schedule that queues asynchronous PostgreSQL managed database backups.",
            json!({
                "type": "object",
                "properties": {
                    "managed_database_id": { "type": "string" },
                    "cron_expression": {
                        "type": "string",
                        "description": "Cron expression; minimum interval is 15 minutes."
                    },
                    "timezone": {
                        "type": "string",
                        "description": "IANA timezone such as UTC, Asia/Shanghai, or America/Los_Angeles. Defaults to UTC."
                    },
                    "purpose": { "type": "string" },
                    "keep_last": { "type": "integer" },
                    "retention_days": { "type": "integer" }
                },
                "required": ["managed_database_id", "cron_expression"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let managed_database_id = required_string_arg(
            &arguments,
            "managed_database_id",
            "pg_create_database_backup_schedule",
        )?;
        let cron_expression = required_string_arg(
            &arguments,
            "cron_expression",
            "pg_create_database_backup_schedule",
        )?;
        let timezone =
            optional_string_arg(&arguments, "timezone")?.unwrap_or_else(|| "UTC".to_owned());
        let purpose = optional_string_arg(&arguments, "purpose")?;
        let keep_last = optional_i32_arg(&arguments, "keep_last")?;
        let retention_days = optional_i32_arg(&arguments, "retention_days")?;
        let next_run_at = validate_backup_schedule(
            &cron_expression,
            &timezone,
            OffsetDateTime::now_utc(),
            MINIMUM_BACKUP_CRON_INTERVAL_SECONDS,
        )?;
        let schedule = self
            .context
            .metadata_store
            .create_database_backup_schedule(
                &self.context.owner_user_id,
                CreateDatabaseBackupScheduleRequest {
                    managed_database_id,
                    cron_expression,
                    timezone: Some(timezone),
                    purpose,
                    keep_last,
                    retention_days,
                },
                self.context.conversation_id.clone(),
                self.context.turn_id.clone(),
                next_run_at,
            )
            .await?;

        Ok(ToolOutput::json(json!({
            "schedule": schedule,
            "scheduled": true,
        })))
    }
}

#[derive(Clone)]
pub(crate) struct PgListDatabaseBackupSchedulesTool {
    context: DatabaseOperationToolContext,
}

impl PgListDatabaseBackupSchedulesTool {
    pub(crate) fn new(context: DatabaseOperationToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgListDatabaseBackupSchedulesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_list_database_backup_schedules",
            "List persisted PostgreSQL database backup schedules for the current user.",
            json!({
                "type": "object",
                "properties": {
                    "managed_database_id": { "type": "string" },
                    "status": {
                        "type": "string",
                        "description": "Optional status: active, paused, or deleted."
                    },
                    "limit": { "type": "integer" }
                },
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let managed_database_id = optional_string_arg(&arguments, "managed_database_id")?;
        let status = optional_schedule_status_arg(&arguments, "status")?;
        let schedules = self
            .context
            .metadata_store
            .list_database_backup_schedules(
                &self.context.owner_user_id,
                managed_database_id.as_deref(),
                status,
                limit_arg(&arguments),
            )
            .await?;

        Ok(ToolOutput::json(json!({
            "schedules": schedules,
            "count": schedules.len(),
        })))
    }
}

#[derive(Clone)]
pub(crate) struct PgUpdateDatabaseBackupScheduleTool {
    context: DatabaseOperationToolContext,
}

impl PgUpdateDatabaseBackupScheduleTool {
    pub(crate) fn new(context: DatabaseOperationToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgUpdateDatabaseBackupScheduleTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_update_database_backup_schedule",
            "Update a PostgreSQL database backup schedule.",
            json!({
                "type": "object",
                "properties": {
                    "schedule_id": { "type": "string" },
                    "cron_expression": { "type": "string" },
                    "timezone": { "type": "string" },
                    "status": {
                        "type": "string",
                        "description": "active, paused, or deleted."
                    },
                    "purpose": { "type": "string" },
                    "keep_last": { "type": "integer" },
                    "retention_days": { "type": "integer" }
                },
                "required": ["schedule_id"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let schedule_id = required_string_arg(
            &arguments,
            "schedule_id",
            "pg_update_database_backup_schedule",
        )?;
        let current = self
            .context
            .metadata_store
            .get_database_backup_schedule(&self.context.owner_user_id, &schedule_id)
            .await?;
        let cron_expression = optional_string_arg(&arguments, "cron_expression")?;
        let timezone = optional_string_arg(&arguments, "timezone")?;
        let status = optional_schedule_status_arg(&arguments, "status")?;
        let keep_last = optional_i32_arg(&arguments, "keep_last")?;
        let retention_days = optional_i32_arg(&arguments, "retention_days")?;
        let next_run_at = if cron_expression.is_some()
            || timezone.is_some()
            || status == Some(liquid_core::DatabaseBackupScheduleStatus::Active)
        {
            Some(validate_backup_schedule(
                cron_expression
                    .as_deref()
                    .unwrap_or(&current.cron_expression),
                timezone.as_deref().unwrap_or(&current.timezone),
                OffsetDateTime::now_utc(),
                MINIMUM_BACKUP_CRON_INTERVAL_SECONDS,
            )?)
        } else {
            None
        };
        let schedule = self
            .context
            .metadata_store
            .update_database_backup_schedule(
                &self.context.owner_user_id,
                &schedule_id,
                UpdateDatabaseBackupScheduleRequest {
                    cron_expression,
                    timezone,
                    status,
                    purpose: optional_string_arg(&arguments, "purpose")?,
                    keep_last,
                    retention_days,
                },
                next_run_at,
            )
            .await?;

        Ok(ToolOutput::json(json!({ "schedule": schedule })))
    }
}

#[derive(Clone)]
pub(crate) struct PgDeleteDatabaseBackupScheduleTool {
    context: DatabaseOperationToolContext,
}

impl PgDeleteDatabaseBackupScheduleTool {
    pub(crate) fn new(context: DatabaseOperationToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgDeleteDatabaseBackupScheduleTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_delete_database_backup_schedule",
            "Delete a persisted PostgreSQL database backup schedule.",
            json!({
                "type": "object",
                "properties": {
                    "schedule_id": { "type": "string" }
                },
                "required": ["schedule_id"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let schedule_id = required_string_arg(
            &arguments,
            "schedule_id",
            "pg_delete_database_backup_schedule",
        )?;
        let schedule = self
            .context
            .metadata_store
            .delete_database_backup_schedule(&self.context.owner_user_id, &schedule_id)
            .await?;

        Ok(ToolOutput::json(json!({ "schedule": schedule })))
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
