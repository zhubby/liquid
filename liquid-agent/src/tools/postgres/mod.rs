pub(super) mod args;
pub(super) mod catalog;
pub(super) mod config;
pub(super) mod describe;
pub(super) mod execute;
pub(super) mod explain;

pub use config::{PostgresToolConfig, PostgresToolExecutionMode};

pub(super) use catalog::{PgListRelationsTool, PgListSchemasTool};
pub(super) use config::PostgresToolContext;
pub(super) use describe::PgDescribeRelationTool;
pub use execute::{ApprovedWriteExecutionResult, execute_approved_write_sql_with_config};
pub(super) use execute::{PgExecuteReadonlySqlTool, PgExecuteWriteSqlTool};
pub(super) use explain::PgExplainSqlTool;

#[cfg(test)]
mod tests {
    use liquid_llm::ToolCall;
    use serde_json::{Value, json};
    use sqlx::{PgPool, postgres::PgPoolOptions};

    use crate::tools::{AgentTool, ToolRegistry};

    use super::{args::limit_arg, config::MAX_TOOL_LIMIT, execute::readonly_payload, *};

    #[tokio::test]
    async fn postgres_tool_registry_registers_write_only_when_gated() {
        let pool = lazy_test_pool();
        let readonly = ToolRegistry::with_postgres_tools(PostgresToolConfig::new(
            Some(pool.clone()),
            false,
            PostgresToolExecutionMode::Readonly,
        ));
        let write_gated = ToolRegistry::with_postgres_tools(PostgresToolConfig::new(
            Some(pool),
            false,
            PostgresToolExecutionMode::WriteGated,
        ));

        assert!(has_tool(&readonly, "inspect_sql_risk"));
        assert!(has_tool(&readonly, "pg_list_schemas"));
        assert!(has_tool(&readonly, "pg_execute_readonly_sql"));
        assert!(!has_tool(&readonly, "pg_execute_write_sql"));
        assert!(has_tool(&write_gated, "pg_execute_write_sql"));
    }

    #[tokio::test]
    async fn readonly_sql_tool_rejects_non_select_before_database_access() {
        let tool = PgExecuteReadonlySqlTool::new(test_context());
        let error = tool
            .execute(json!({
                "sql": "delete from users"
            }))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("only supports SELECT"));
    }

    #[tokio::test]
    async fn readonly_sql_tool_rejects_multiple_statements_before_database_access() {
        let tool = PgExecuteReadonlySqlTool::new(test_context());
        let error = tool
            .execute(json!({
                "sql": "select 1; select 2"
            }))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("exactly one statement"));
    }

    #[tokio::test]
    async fn approved_write_executor_rejects_select_before_database_access() {
        let error = execute::execute_approved_write_sql(
            &test_context(),
            "select id from users",
            "approved_write_sql",
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("rejects SELECT"));
    }

    #[tokio::test]
    async fn approved_write_executor_rejects_multiple_statements_before_database_access() {
        let error = execute::execute_approved_write_sql(
            &test_context(),
            "update users set active = false where id = 1; select 1",
            "approved_write_sql",
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("exactly one statement"));
    }

    #[tokio::test]
    async fn approved_write_executor_rejects_transaction_before_database_access() {
        let error =
            execute::execute_approved_write_sql(&test_context(), "begin", "approved_write_sql")
                .await
                .unwrap_err();

        assert!(error.to_string().contains("transaction and control"));
    }

    #[tokio::test]
    async fn approved_write_executor_rejects_critical_sql_before_database_access() {
        let error = execute::execute_approved_write_sql(
            &test_context(),
            "drop table users",
            "approved_write_sql",
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("critical deterministic risk"));
    }

    #[test]
    fn approved_write_executor_detects_create_database_as_autocommit_statement() {
        assert!(execute::statement_requires_autocommit("create database liquid_sandbox").unwrap());
        assert!(
            !execute::statement_requires_autocommit("create table liquid_sandbox (id integer)")
                .unwrap()
        );
    }

    #[tokio::test]
    async fn limit_argument_clamps_to_context_max() {
        let context = test_context();
        let limit = limit_arg(
            &json!({
                "limit": 50_000
            }),
            &context,
            "test_tool",
        )
        .unwrap();

        assert_eq!(limit, MAX_TOOL_LIMIT);
    }

    #[test]
    fn readonly_payload_truncates_to_output_budget() {
        let payload = readonly_payload(
            vec!["payload".to_owned()],
            vec![json!({
                "payload": "x".repeat(512)
            })],
            false,
            1,
            128,
        );

        assert_eq!(payload["truncated"], true);
        assert_eq!(payload["row_count"], 0);
    }

    #[tokio::test]
    async fn postgres_tools_collect_catalog_explain_and_readonly_results() {
        let Some(pool) = integration_pool().await else {
            return;
        };

        sqlx::query(
            r#"
            create temporary table liquid_agent_tool_users (
                id integer primary key,
                email text not null
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("insert into liquid_agent_tool_users (id, email) values (1, 'a@test.local')")
            .execute(&pool)
            .await
            .unwrap();

        let registry = ToolRegistry::with_postgres_tools(PostgresToolConfig::new(
            Some(pool),
            false,
            PostgresToolExecutionMode::Readonly,
        ));

        let schemas = registry
            .execute(&ToolCall::new(
                "call_1",
                "pg_list_schemas",
                r#"{"include_system":true,"limit":5}"#,
            ))
            .await
            .unwrap();
        let schemas: Value = serde_json::from_str(&schemas.content).unwrap();
        assert!(schemas["schemas"].is_array());

        let relations = registry
            .execute(&ToolCall::new(
                "call_2",
                "pg_list_relations",
                r#"{"include_system":true,"search":"liquid_agent_tool_users","limit":10}"#,
            ))
            .await
            .unwrap();
        let relations: Value = serde_json::from_str(&relations.content).unwrap();
        assert!(
            relations["relations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|relation| relation["name"] == "liquid_agent_tool_users")
        );

        let description = registry
            .execute(&ToolCall::new(
                "call_3",
                "pg_describe_relation",
                r#"{"name":"liquid_agent_tool_users"}"#,
            ))
            .await
            .unwrap();
        let description: Value = serde_json::from_str(&description.content).unwrap();
        assert!(
            description["columns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|column| column["name"] == "email" && column["data_type"] == "text")
        );

        let explain = registry
            .execute(&ToolCall::new(
                "call_4",
                "pg_explain_sql",
                r#"{"sql":"select id from liquid_agent_tool_users"}"#,
            ))
            .await
            .unwrap();
        let explain: Value = serde_json::from_str(&explain.content).unwrap();
        assert_eq!(explain["statement_kind"], "select");
        assert!(explain["summary"]["nodes"].is_array());

        let readonly = registry
            .execute(&ToolCall::new(
                "call_5",
                "pg_execute_readonly_sql",
                r#"{"sql":"select id, email from liquid_agent_tool_users order by id","limit":1}"#,
            ))
            .await
            .unwrap();
        let readonly: Value = serde_json::from_str(&readonly.content).unwrap();
        assert_eq!(readonly["row_count"], 1);
        assert_eq!(readonly["rows"][0]["email"], "a@test.local");
        assert!(!has_tool(&registry, "pg_execute_write_sql"));
    }

    fn has_tool(registry: &ToolRegistry, name: &str) -> bool {
        registry
            .definitions()
            .iter()
            .any(|definition| definition.name == name)
    }

    fn test_context() -> PostgresToolContext {
        PostgresToolContext::new(
            lazy_test_pool(),
            &PostgresToolConfig::new(None, false, PostgresToolExecutionMode::Readonly),
        )
    }

    fn lazy_test_pool() -> PgPool {
        PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://postgres:postgres@localhost:1/liquid")
            .unwrap()
    }

    async fn integration_pool() -> Option<PgPool> {
        let database_url = std::env::var("LIQUID_TEST_DATABASE_URL").ok()?;
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .ok()
    }
}
