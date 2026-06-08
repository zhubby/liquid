use sqlx::PgPool;

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
    registry::ToolRegistry,
    sql_risk::SqlRiskInspectionTool,
};

pub fn sql_risk_tools(pool: Option<PgPool>, metadata_required: bool) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(SqlRiskInspectionTool::with_metadata(
        pool,
        metadata_required,
    ));
    registry
}

pub fn sql_audit_tools(config: PostgresToolConfig) -> ToolRegistry {
    let mut registry = sql_risk_tools(config.pool.clone(), config.metadata_required);

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

pub fn workbench_readonly_postgres_tools(config: PostgresToolConfig) -> ToolRegistry {
    let mut registry = ToolRegistry::new();

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

pub fn database_operation_tools(context: DatabaseOperationToolContext) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(PgStartDatabaseBackupTool::new(context.clone()));
    registry.register(PgGetDatabaseBackupTool::new(context.clone()));
    registry.register(PgListDatabaseBackupsTool::new(context.clone()));
    registry.register(PgDeleteDatabaseBackupTool::new(context.clone()));
    registry.register(PgStartDatabaseRestoreTool::new(context.clone()));
    registry.register(PgGetDatabaseRestoreTool::new(context.clone()));
    registry.register(PgListDatabaseRestoresTool::new(context));
    registry
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use liquid_core::{
        CompleteDatabaseBackup, DatabaseBackupMetadataStore, DatabaseBackupMetadataStoreError,
        DatabaseBackupRecord, DatabaseBackupStatus, DatabaseRestoreRecord,
    };

    use super::*;

    #[test]
    fn sql_risk_tool_set_registers_only_risk_inspection() {
        let registry = sql_risk_tools(None, false);

        assert!(has_tool(&registry, "inspect_sql_risk"));
        assert!(!has_tool(&registry, "pg_list_schemas"));
        assert!(!has_tool(&registry, "pg_start_database_backup"));
    }

    #[tokio::test]
    async fn sql_audit_tool_set_registers_postgres_tools_when_pool_exists() {
        let pool = lazy_test_pool();
        let readonly = sql_audit_tools(PostgresToolConfig::new(
            Some(pool.clone()),
            false,
            PostgresToolExecutionMode::Readonly,
        ));
        let write_gated = sql_audit_tools(PostgresToolConfig::new(
            Some(pool),
            false,
            PostgresToolExecutionMode::WriteGated,
        ));

        assert!(has_tool(&readonly, "inspect_sql_risk"));
        assert!(has_tool(&readonly, "pg_list_schemas"));
        assert!(has_tool(&readonly, "pg_execute_readonly_sql"));
        assert!(!has_tool(&readonly, "pg_execute_write_sql"));
        assert!(has_tool(&write_gated, "pg_execute_write_sql"));
        assert!(!has_tool(&write_gated, "pg_start_database_backup"));
    }

    #[tokio::test]
    async fn workbench_readonly_tool_set_excludes_sql_risk_and_write_tools() {
        let registry = workbench_readonly_postgres_tools(PostgresToolConfig::new(
            Some(lazy_test_pool()),
            false,
            PostgresToolExecutionMode::WriteGated,
        ));

        assert!(has_tool(&registry, "pg_list_schemas"));
        assert!(has_tool(&registry, "pg_execute_readonly_sql"));
        assert!(!has_tool(&registry, "inspect_sql_risk"));
        assert!(!has_tool(&registry, "pg_execute_write_sql"));
    }

    #[test]
    fn database_operation_tool_set_registers_backup_and_restore_tools() {
        let registry = database_operation_tools(DatabaseOperationToolContext::new(
            "user-1",
            Arc::new(NoopBackupMetadataStore),
        ));

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
        assert!(!has_tool(&registry, "inspect_sql_risk"));
    }

    fn has_tool(registry: &ToolRegistry, name: &str) -> bool {
        registry
            .definitions()
            .into_iter()
            .any(|definition| definition.name == name)
    }

    fn lazy_test_pool() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost:5432/liquid")
            .unwrap()
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
