use anyhow::Result;
use async_trait::async_trait;
use liquid_llm::ToolDefinition;
use serde_json::{Value, json};

use crate::{tools::AgentTool, types::ToolOutput};

use super::{
    DatabaseOperationToolContext, limit_arg, operation_kind_arg, recent_operation_diagnostics,
    required_string_arg,
};

#[derive(Clone)]
pub(crate) struct PgGetDatabaseOperationDiagnosticsTool {
    context: DatabaseOperationToolContext,
}

impl PgGetDatabaseOperationDiagnosticsTool {
    pub(crate) fn new(context: DatabaseOperationToolContext) -> Self {
        Self { context }
    }
}

#[async_trait]
impl AgentTool for PgGetDatabaseOperationDiagnosticsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "pg_get_database_operation_diagnostics",
            "Read persisted failure diagnostics for a PostgreSQL database backup or restore operation.",
            json!({
                "type": "object",
                "properties": {
                    "operation_kind": {
                        "type": "string",
                        "description": "Operation kind: backup or restore."
                    },
                    "operation_id": {
                        "type": "string",
                        "description": "Database backup or restore job id."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum diagnostic records to return; defaults to 20 and clamps at 100."
                    }
                },
                "required": ["operation_kind", "operation_id"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let operation_kind = operation_kind_arg(
            &arguments,
            "operation_kind",
            "pg_get_database_operation_diagnostics",
        )?;
        let operation_id = required_string_arg(
            &arguments,
            "operation_id",
            "pg_get_database_operation_diagnostics",
        )?;
        let limit = limit_arg(&arguments);
        let diagnostics =
            recent_operation_diagnostics(&self.context, operation_kind, &operation_id, limit)
                .await?;

        Ok(ToolOutput::json(json!({
            "operation_kind": operation_kind,
            "operation_id": operation_id,
            "diagnostics": diagnostics,
            "count": diagnostics.len(),
        })))
    }
}
