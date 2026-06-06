use std::{collections::BTreeMap, sync::Arc};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use liquid_llm::{ToolCall, ToolDefinition};
use serde_json::Value;
use sqlx::PgPool;

use crate::types::ToolOutput;

use super::{
    database_operations::{
        DatabaseOperationToolContext, PgDeleteDatabaseBackupTool, PgGetDatabaseBackupTool,
        PgGetDatabaseRestoreTool, PgListDatabaseBackupsTool, PgListDatabaseRestoresTool,
        PgStartDatabaseBackupTool, PgStartDatabaseRestoreTool,
    },
    postgres::{
        PgDescribeRelationTool, PgExecuteReadonlySqlTool, PgExecuteWriteSqlTool, PgExplainSqlTool,
        PgListRelationsTool, PgListSchemasTool, PostgresToolConfig, PostgresToolContext,
        PostgresToolExecutionMode,
    },
    sql_risk::SqlRiskInspectionTool,
};

#[async_trait]
pub trait AgentTool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, arguments: Value) -> Result<ToolOutput>;
}

#[derive(Clone, Default)]
pub struct ToolRegistry {
    pub(crate) tools: BTreeMap<String, Arc<dyn AgentTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_default_sql_tools() -> Self {
        let mut registry = Self::new();
        registry.register(SqlRiskInspectionTool::default());
        registry
    }

    pub fn with_sql_metadata_pool(pool: Option<PgPool>, metadata_required: bool) -> Self {
        let mut registry = Self::new();
        registry.register(SqlRiskInspectionTool::with_metadata(
            pool,
            metadata_required,
        ));
        registry
    }

    pub fn with_postgres_tools(config: PostgresToolConfig) -> Self {
        let mut registry = Self::new();
        registry.register(SqlRiskInspectionTool::with_metadata(
            config.pool.clone(),
            config.metadata_required,
        ));

        let Some(pool) = config.pool.clone() else {
            return registry;
        };

        let context = PostgresToolContext::new(pool, &config);
        registry.register(PgListSchemasTool::new(context.clone()));
        registry.register(PgListRelationsTool::new(context.clone()));
        registry.register(PgDescribeRelationTool::new(context.clone()));
        registry.register(PgExplainSqlTool::new(context.clone()));

        if matches!(
            config.execution,
            PostgresToolExecutionMode::Readonly | PostgresToolExecutionMode::WriteGated
        ) {
            registry.register(PgExecuteReadonlySqlTool::new(context.clone()));
        }

        if matches!(config.execution, PostgresToolExecutionMode::WriteGated) {
            registry.register(PgExecuteWriteSqlTool::new(context));
        }

        registry
    }

    pub fn with_workbench_readonly_postgres_tools(config: PostgresToolConfig) -> Self {
        let mut registry = Self::new();

        let Some(pool) = config.pool.clone() else {
            return registry;
        };

        let context = PostgresToolContext::new(pool, &config);
        registry.register(PgListSchemasTool::new(context.clone()));
        registry.register(PgListRelationsTool::new(context.clone()));
        registry.register(PgDescribeRelationTool::new(context.clone()));
        registry.register(PgExplainSqlTool::new(context.clone()));
        registry.register(PgExecuteReadonlySqlTool::new(context));
        registry
    }

    pub fn with_database_operation_tools(context: DatabaseOperationToolContext) -> Self {
        let mut registry = Self::new();
        registry.register(PgStartDatabaseBackupTool::new(context.clone()));
        registry.register(PgGetDatabaseBackupTool::new(context.clone()));
        registry.register(PgListDatabaseBackupsTool::new(context.clone()));
        registry.register(PgDeleteDatabaseBackupTool::new(context.clone()));
        registry.register(PgStartDatabaseRestoreTool::new(context.clone()));
        registry.register(PgGetDatabaseRestoreTool::new(context.clone()));
        registry.register(PgListDatabaseRestoresTool::new(context));
        registry
    }

    pub fn register<T>(&mut self, tool: T)
    where
        T: AgentTool + 'static,
    {
        let definition = tool.definition();
        self.tools.insert(definition.name, Arc::new(tool));
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|tool| tool.definition()).collect()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub async fn execute(&self, call: &ToolCall) -> Result<ToolOutput> {
        let tool = self
            .tools
            .get(&call.name)
            .ok_or_else(|| anyhow!("unknown agent tool: {}", call.name))?;
        let arguments = call.json_arguments()?;

        tool.execute(arguments)
            .await
            .with_context(|| format!("agent tool failed: {}", call.name))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use liquid_core::{
        CompleteDatabaseBackup, DatabaseBackupMetadataStore, DatabaseBackupMetadataStoreError,
        DatabaseBackupRecord, DatabaseBackupStatus, DatabaseRestoreRecord,
    };
    use liquid_llm::ToolCall;
    use serde_json::{Value, json};

    use super::*;

    #[derive(Default)]
    struct EchoTool;

    #[async_trait]
    impl AgentTool for EchoTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new(
                "echo_tool",
                "Echo a value.",
                json!({
                    "type": "object",
                    "properties": {
                        "value": { "type": "string" }
                    },
                    "required": ["value"],
                    "additionalProperties": false
                }),
            )
        }

        async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
            Ok(ToolOutput::json(json!({
                "value": arguments.get("value").and_then(Value::as_str).unwrap_or_default()
            })))
        }
    }

    #[tokio::test]
    async fn tool_registry_executes_registered_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(EchoTool);

        let output = registry
            .execute(&ToolCall::new("call_1", "echo_tool", r#"{"value":"ok"}"#))
            .await
            .unwrap();

        assert_eq!(output.content, r#"{"value":"ok"}"#);
    }

    #[tokio::test]
    async fn tool_registry_rejects_unknown_tool() {
        let registry = ToolRegistry::new();
        let error = registry
            .execute(&ToolCall::new("call_1", "missing_tool", "{}"))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("unknown agent tool"));
    }

    #[tokio::test]
    async fn database_operation_tool_registry_registers_backup_and_restore_tools() {
        let registry = ToolRegistry::with_database_operation_tools(
            DatabaseOperationToolContext::new("user-1", Arc::new(NoopBackupMetadataStore)),
        );

        for name in [
            "pg_start_database_backup",
            "pg_get_database_backup",
            "pg_list_database_backups",
            "pg_delete_database_backup",
            "pg_start_database_restore",
            "pg_get_database_restore",
            "pg_list_database_restores",
        ] {
            assert!(has_tool(&registry, name), "missing tool {name}");
        }
    }

    #[tokio::test]
    async fn postgres_tool_registry_does_not_register_database_operation_tools() {
        let registry = ToolRegistry::with_default_sql_tools();

        assert!(has_tool(&registry, "inspect_sql_risk"));
        assert!(!has_tool(&registry, "pg_start_database_backup"));
        assert!(!has_tool(&registry, "pg_start_database_restore"));
    }

    fn has_tool(registry: &ToolRegistry, name: &str) -> bool {
        registry
            .definitions()
            .into_iter()
            .any(|definition| definition.name == name)
    }

    struct NoopBackupMetadataStore;

    #[async_trait]
    impl DatabaseBackupMetadataStore for NoopBackupMetadataStore {
        async fn create_database_backup(
            &self,
            _owner_user_id: &str,
            _source_managed_database_id: &str,
            _purpose: Option<String>,
        ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn get_database_backup(
            &self,
            _owner_user_id: &str,
            _id: &str,
        ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn list_database_backups(
            &self,
            _owner_user_id: &str,
            _source_managed_database_id: Option<&str>,
            _status: Option<DatabaseBackupStatus>,
            _limit: i64,
        ) -> Result<Vec<DatabaseBackupRecord>, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn delete_database_backup(
            &self,
            _owner_user_id: &str,
            _id: &str,
        ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn create_database_restore(
            &self,
            _owner_user_id: &str,
            _backup_id: &str,
            _target_managed_database_id: &str,
            _purpose: String,
        ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn get_database_restore(
            &self,
            _owner_user_id: &str,
            _id: &str,
        ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn list_database_restores(
            &self,
            _owner_user_id: &str,
            _backup_id: Option<&str>,
            _target_managed_database_id: Option<&str>,
            _status: Option<DatabaseBackupStatus>,
            _limit: i64,
        ) -> Result<Vec<DatabaseRestoreRecord>, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn claim_next_database_backup(
            &self,
            _worker_id: &str,
        ) -> Result<Option<DatabaseBackupRecord>, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn update_database_backup_progress(
            &self,
            _id: &str,
            _phase: &str,
            _progress_percent: i32,
        ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn complete_database_backup(
            &self,
            _id: &str,
            _result: CompleteDatabaseBackup,
        ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn fail_database_backup(
            &self,
            _id: &str,
            _error: String,
        ) -> Result<DatabaseBackupRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn claim_next_database_restore(
            &self,
            _worker_id: &str,
        ) -> Result<Option<DatabaseRestoreRecord>, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn update_database_restore_progress(
            &self,
            _id: &str,
            _phase: &str,
            _progress_percent: i32,
        ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn complete_database_restore(
            &self,
            _id: &str,
        ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn fail_database_restore(
            &self,
            _id: &str,
            _error: String,
        ) -> Result<DatabaseRestoreRecord, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }

        async fn fail_stale_database_jobs(
            &self,
            _stale_after_seconds: i64,
        ) -> Result<u64, DatabaseBackupMetadataStoreError> {
            unreachable!()
        }
    }
}
