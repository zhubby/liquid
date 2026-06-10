mod actions;
mod prompt;
mod proposal_tools;
mod response;
mod rule_based;
mod tool_loop;

use std::{future::Future, sync::Arc};

use anyhow::Result;
use liquid_core::{AgentAction, AgentMessage, AuditSummary, ManagedDatabase, SqlAuditRecord};
use liquid_llm::{LlmClient, LlmMessage, LlmProtocol, LlmRequest};
use serde_json::Value;

use crate::{
    llm_invocation::{LlmInvocationMode, invoke_llm, invoke_llm_with_text_delta},
    tools::ToolRegistry,
};

use prompt::{WORKBENCH_SYSTEM_PROMPT, workbench_context_payload, workbench_observation_payload};
use proposal_tools::register_workbench_proposal_tools;
pub use response::{
    WorkbenchActionSuggestion, WorkbenchResponse, WorkbenchToolStep, parse_llm_workbench_response,
};
pub use rule_based::RuleBasedWorkbenchAgent;

#[cfg(test)]
use crate::{tools::AgentTool, types::ToolOutput};
#[cfg(test)]
use liquid_core::{AgentActionKind, AgentResourceKind};
#[cfg(test)]
use liquid_llm::ToolDefinition;
#[cfg(test)]
use serde_json::json;

const DEFAULT_MAX_WORKBENCH_TOOL_ROUNDS: usize = 10;

pub fn workbench_proposal_tool_names() -> Vec<String> {
    proposal_tools::workbench_proposal_tool_names()
}

#[derive(Debug, Clone)]
pub struct WorkbenchContext {
    pub message: String,
    pub managed_database_id: Option<String>,
    pub selected_sql_audit_id: Option<String>,
    pub managed_database_count: usize,
    pub audit_score: Option<u8>,
}

#[derive(Debug, Clone)]
pub struct LlmWorkbenchContext {
    pub messages: Vec<AgentMessage>,
    pub managed_database: Option<ManagedDatabase>,
    pub write_sql_execution_enabled: bool,
    pub selected_sql_audit_id: Option<String>,
    pub audit_summary: Option<AuditSummary>,
    pub recent_sql_audits: Vec<SqlAuditRecord>,
    pub recent_actions: Vec<AgentAction>,
}

#[derive(Clone)]
pub struct LlmWorkbenchAgent {
    llm: Arc<dyn LlmClient>,
    model: String,
    protocol: LlmProtocol,
    max_tool_rounds: usize,
    max_output_tokens: Option<u32>,
    invocation_mode: LlmInvocationMode,
}

impl LlmWorkbenchAgent {
    pub fn new(llm: Arc<dyn LlmClient>, model: impl Into<String>, protocol: LlmProtocol) -> Self {
        Self {
            llm,
            model: model.into(),
            protocol,
            max_tool_rounds: DEFAULT_MAX_WORKBENCH_TOOL_ROUNDS,
            max_output_tokens: None,
            invocation_mode: LlmInvocationMode::Complete,
        }
    }

    pub fn with_max_tool_rounds(mut self, max_tool_rounds: usize) -> Self {
        self.max_tool_rounds = max_tool_rounds;
        self
    }

    pub fn with_max_output_tokens(mut self, max_output_tokens: Option<u32>) -> Self {
        self.max_output_tokens = max_output_tokens;
        self
    }

    pub fn with_streaming_enabled(mut self, streaming_enabled: bool) -> Self {
        self.invocation_mode = LlmInvocationMode::from_streaming_enabled(streaming_enabled);
        self
    }

    pub async fn respond(&self, context: LlmWorkbenchContext) -> Result<WorkbenchResponse> {
        let request = optional_max_output_tokens(
            LlmRequest::new(
                self.model.clone(),
                self.protocol,
                vec![
                    LlmMessage::system(WORKBENCH_SYSTEM_PROMPT),
                    LlmMessage::user(workbench_context_payload(&context)?),
                ],
            )
            .with_temperature(0.2),
            self.max_output_tokens,
        );
        let response = invoke_llm(&self.llm, request, self.invocation_mode).await?;

        parse_llm_workbench_response(&response.content, &context)
    }

    pub async fn respond_with_tools(
        &self,
        context: LlmWorkbenchContext,
        mut tools: ToolRegistry,
    ) -> Result<WorkbenchResponse> {
        register_workbench_proposal_tools(&mut tools);
        let messages = vec![
            LlmMessage::system(WORKBENCH_SYSTEM_PROMPT),
            LlmMessage::user(workbench_context_payload(&context)?),
        ];

        tool_loop::run_tool_loop(self, context, messages, tools, self.max_output_tokens).await
    }

    pub async fn respond_with_tools_and_text_delta<F, Fut>(
        &self,
        context: LlmWorkbenchContext,
        tools: ToolRegistry,
        on_text_delta: F,
    ) -> Result<WorkbenchResponse>
    where
        F: FnMut(String) -> Fut + Send,
        Fut: Future<Output = ()> + Send,
    {
        tool_loop::run_tool_loop_with_text_delta(self, context, tools, on_text_delta).await
    }

    pub async fn synthesize_observation(
        &self,
        context: LlmWorkbenchContext,
        observation: Value,
    ) -> Result<WorkbenchResponse> {
        let request = optional_max_output_tokens(
            LlmRequest::new(
                self.model.clone(),
                self.protocol,
                vec![
                    LlmMessage::system(WORKBENCH_SYSTEM_PROMPT),
                    LlmMessage::user(workbench_observation_payload(&context, observation)?),
                ],
            )
            .with_temperature(0.2),
            self.max_output_tokens,
        );
        let response = invoke_llm(&self.llm, request, self.invocation_mode).await?;

        parse_llm_workbench_response(&response.content, &context)
    }

    pub async fn synthesize_observation_with_text_delta<F, Fut>(
        &self,
        context: LlmWorkbenchContext,
        observation: Value,
        on_text_delta: F,
    ) -> Result<WorkbenchResponse>
    where
        F: FnMut(String) -> Fut + Send,
        Fut: Future<Output = ()> + Send,
    {
        let request = optional_max_output_tokens(
            LlmRequest::new(
                self.model.clone(),
                self.protocol,
                vec![
                    LlmMessage::system(WORKBENCH_SYSTEM_PROMPT),
                    LlmMessage::user(workbench_observation_payload(&context, observation)?),
                ],
            )
            .with_temperature(0.2),
            self.max_output_tokens,
        );
        let response =
            invoke_llm_with_text_delta(&self.llm, request, self.invocation_mode, on_text_delta)
                .await?;

        parse_llm_workbench_response(&response.content, &context)
    }
}

fn optional_max_output_tokens(request: LlmRequest, max_output_tokens: Option<u32>) -> LlmRequest {
    match max_output_tokens {
        Some(max_output_tokens) => request.with_max_output_tokens(max_output_tokens),
        None => request,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use liquid_core::{
        AgentActionStatus, AgentMessageRole, ManagedDatabaseEngine, ManagedDatabaseSslMode,
        RiskSeverity, SqlAuditFinding, SqlAuditReport, SqlAuditStatus,
    };
    use std::collections::VecDeque;

    use liquid_llm::{
        LlmClient, LlmEvent, LlmRequest, LlmResponse, LlmStream, MessageRole, ToolCall,
    };

    #[test]
    fn workbench_proposal_tool_names_match_registered_tools() {
        let mut tools = ToolRegistry::new();
        register_workbench_proposal_tools(&mut tools);

        assert_eq!(workbench_proposal_tool_names(), tools.tool_names());
    }

    #[derive(Debug)]
    struct CapturingLlmClient {
        response: String,
        request: Mutex<Option<LlmRequest>>,
    }

    impl CapturingLlmClient {
        fn new(response: impl Into<String>) -> Self {
            Self {
                response: response.into(),
                request: Mutex::new(None),
            }
        }

        fn captured_request(&self) -> LlmRequest {
            self.request.lock().unwrap().clone().unwrap()
        }
    }

    #[async_trait]
    impl LlmClient for CapturingLlmClient {
        async fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
            *self.request.lock().unwrap() = Some(request);

            Ok(LlmResponse::text(self.response.clone()))
        }

        async fn stream(&self, _request: LlmRequest) -> Result<LlmStream> {
            Ok(Box::pin(futures_util::stream::empty::<Result<LlmEvent>>()))
        }
    }

    struct ScriptedWorkbenchLlmClient {
        responses: Mutex<VecDeque<LlmResponse>>,
        requests: Mutex<Vec<LlmRequest>>,
    }

    impl ScriptedWorkbenchLlmClient {
        fn new(responses: Vec<LlmResponse>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn requests(&self) -> Vec<LlmRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    struct StreamingWorkbenchLlmClient {
        events: Mutex<VecDeque<LlmEvent>>,
        requests: Mutex<Vec<LlmRequest>>,
        complete_calls: Mutex<usize>,
    }

    impl StreamingWorkbenchLlmClient {
        fn new(events: Vec<LlmEvent>) -> Self {
            Self {
                events: Mutex::new(events.into()),
                requests: Mutex::new(Vec::new()),
                complete_calls: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl LlmClient for StreamingWorkbenchLlmClient {
        async fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
            self.requests.lock().unwrap().push(request);
            *self.complete_calls.lock().unwrap() += 1;
            Ok(LlmResponse::text("fallback"))
        }

        async fn stream(&self, request: LlmRequest) -> Result<LlmStream> {
            self.requests.lock().unwrap().push(request);
            let events = self
                .events
                .lock()
                .unwrap()
                .drain(..)
                .map(Ok)
                .collect::<Vec<_>>();
            Ok(Box::pin(futures_util::stream::iter(events)))
        }
    }

    #[async_trait]
    impl LlmClient for ScriptedWorkbenchLlmClient {
        async fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
            self.requests.lock().unwrap().push(request);
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("no scripted response"))
        }

        async fn stream(&self, _request: LlmRequest) -> Result<LlmStream> {
            Ok(Box::pin(futures_util::stream::empty::<Result<LlmEvent>>()))
        }
    }

    struct StaticWorkbenchTool {
        name: &'static str,
        output: Value,
    }

    #[async_trait]
    impl AgentTool for StaticWorkbenchTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new(
                self.name,
                "Static test tool.",
                json!({
                    "type": "object",
                    "properties": {
                        "sql": { "type": "string" },
                        "limit": { "type": "integer" }
                    },
                    "additionalProperties": true
                }),
            )
        }

        async fn execute(&self, _arguments: Value) -> Result<ToolOutput> {
            Ok(ToolOutput::json(self.output.clone()))
        }
    }

    struct FailingWorkbenchTool {
        name: &'static str,
    }

    #[async_trait]
    impl AgentTool for FailingWorkbenchTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new(
                self.name,
                "Failing test tool.",
                json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": true
                }),
            )
        }

        async fn execute(&self, _arguments: Value) -> Result<ToolOutput> {
            anyhow::bail!("temporary tool failure")
        }
    }

    fn llm_context() -> LlmWorkbenchContext {
        LlmWorkbenchContext {
            messages: vec![AgentMessage {
                id: "message-1".to_owned(),
                conversation_id: "conversation-1".to_owned(),
                turn_id: Some("turn-1".to_owned()),
                role: AgentMessageRole::User,
                content: "select 1".to_owned(),
                metadata: None,
                created_at: time::OffsetDateTime::UNIX_EPOCH,
            }],
            managed_database: Some(ManagedDatabase {
                id: "db-1".to_owned(),
                name: "Warehouse".to_owned(),
                engine: ManagedDatabaseEngine::Postgres,
                host: "localhost".to_owned(),
                port: 5432,
                database: "app".to_owned(),
                username: "postgres".to_owned(),
                tags: vec![],
                ssl_mode: ManagedDatabaseSslMode::Disable,
                has_password: true,
            }),
            write_sql_execution_enabled: true,
            selected_sql_audit_id: Some("audit-1".to_owned()),
            audit_summary: None,
            recent_sql_audits: vec![SqlAuditRecord {
                id: "audit-1".to_owned(),
                owner_user_id: "user-1".to_owned(),
                managed_database_id: "db-1".to_owned(),
                managed_database_name: "Warehouse".to_owned(),
                managed_database_engine: "postgres".to_owned(),
                managed_database_host: "localhost".to_owned(),
                managed_database_port: 5432,
                managed_database_database: "app".to_owned(),
                managed_database_username: "postgres".to_owned(),
                managed_database_ssl_mode: "disable".to_owned(),
                sql: "select 1".to_owned(),
                schema: None,
                context: Some("review".to_owned()),
                execution_purpose: None,
                status: SqlAuditStatus::Audited,
                statement_kind: None,
                risk_score: 5,
                report: Some(SqlAuditReport {
                    summary: "ok".to_owned(),
                    risk_score: 5,
                    findings: vec![SqlAuditFinding {
                        title: "Low risk".to_owned(),
                        severity: RiskSeverity::Low,
                        explanation: "Read-only".to_owned(),
                        recommendation: "Proceed".to_owned(),
                    }],
                }),
                deterministic_analysis: Some(json!({})),
                approved_by_user_id: None,
                approved_at: None,
                approval_comment: None,
                rejected_by_user_id: None,
                rejected_at: None,
                rejection_comment: None,
                execution_result: None,
                execution_error: None,
                created_at: time::OffsetDateTime::UNIX_EPOCH,
                updated_at: time::OffsetDateTime::UNIX_EPOCH,
                executed_at: None,
            }],
            recent_actions: vec![AgentAction {
                id: "action-1".to_owned(),
                conversation_id: "conversation-1".to_owned(),
                turn_id: "turn-1".to_owned(),
                kind: AgentActionKind::CreateSqlAudit,
                status: AgentActionStatus::Proposed,
                title: "Create audit".to_owned(),
                description: "Create audit".to_owned(),
                payload: json!({}),
                resource_kind: Some(AgentResourceKind::SqlAudit),
                resource_id: Some("audit-1".to_owned()),
                requires_confirmation: true,
                created_at: time::OffsetDateTime::UNIX_EPOCH,
                updated_at: time::OffsetDateTime::UNIX_EPOCH,
            }],
        }
    }

    #[test]
    fn workbench_context_includes_compact_database_restore_metadata() {
        let mut context = llm_context();
        context.messages = vec![AgentMessage {
            id: "message-restore".to_owned(),
            conversation_id: "conversation-1".to_owned(),
            turn_id: Some("turn-restore".to_owned()),
            role: AgentMessageRole::Assistant,
            content: "Database restore failed for doro.".to_owned(),
            metadata: Some(json!({
                "kind": "database_operation_status",
                "database_restore": {
                    "id": "restore-1",
                    "owner_user_id": "user-1",
                    "backup_id": "backup-1",
                    "target": {
                        "id": "db-1",
                        "name": "doro",
                        "engine": "postgres",
                        "host": "localhost",
                        "port": 5432,
                        "database": "doro",
                        "username": "postgres",
                        "ssl_mode": "disable"
                    },
                    "format": "postgres_custom",
                    "status": "failed",
                    "phase": "failed",
                    "progress_percent": 60,
                    "restore_options": {},
                    "error": "pg_restore failed",
                    "created_at": "2026-06-10T08:31:57Z",
                    "updated_at": "2026-06-10T08:32:10Z"
                }
            })),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
        }];

        let payload = prompt::workbench_context_payload(&context).unwrap();
        let payload: Value = serde_json::from_str(&payload).unwrap();
        let operation =
            &payload["workbench_context"]["conversation_messages"][0]["database_operation"];

        assert_eq!(operation["kind"], "restore");
        assert_eq!(operation["id"], "restore-1");
        assert_eq!(operation["backup_id"], "backup-1");
        assert_eq!(operation["target_name"], "doro");
        assert_eq!(operation["error"], "pg_restore failed");
    }

    #[test]
    fn proposes_sql_audit_for_selected_database_and_sql() {
        let response = RuleBasedWorkbenchAgent.respond(WorkbenchContext {
            message: "select * from users".to_owned(),
            managed_database_id: Some("db-1".to_owned()),
            selected_sql_audit_id: None,
            managed_database_count: 1,
            audit_score: Some(92),
        });

        assert_eq!(response.actions.len(), 1);
        assert_eq!(response.actions[0].kind, AgentActionKind::CreateSqlAudit);
        assert_eq!(response.actions[0].payload["managed_database_id"], "db-1");
    }

    #[test]
    fn proposes_execute_for_selected_audit() {
        let response = RuleBasedWorkbenchAgent.respond(WorkbenchContext {
            message: "execute it".to_owned(),
            managed_database_id: None,
            selected_sql_audit_id: Some("audit-1".to_owned()),
            managed_database_count: 1,
            audit_score: None,
        });

        assert_eq!(response.actions.len(), 1);
        assert_eq!(response.actions[0].kind, AgentActionKind::ExecuteSqlAudit);
        assert_eq!(response.actions[0].payload["sql_audit_id"], "audit-1");
    }

    #[test]
    fn parses_llm_workbench_json_response() {
        let response = parse_llm_workbench_response(
            r#"{
                "message": "I prepared an audit.",
                "actions": [{
                    "kind": "create_sql_audit",
                    "title": "Create SQL audit",
                    "description": "Review the query",
                    "sql": "select 1",
                    "context": "from chat"
                }]
            }"#,
            &llm_context(),
        )
        .unwrap();

        assert_eq!(response.content, "I prepared an audit.");
        assert_eq!(response.actions.len(), 1);
        assert_eq!(response.actions[0].kind, AgentActionKind::CreateSqlAudit);
        assert_eq!(response.actions[0].payload["managed_database_id"], "db-1");
        assert_eq!(response.actions[0].payload["request"]["sql"], "select 1");
        assert_eq!(
            response.actions[0].payload["request"]["context"],
            "from chat"
        );
    }

    #[test]
    fn parses_llm_datapanel_card_action() {
        let response = parse_llm_workbench_response(
            r#"{
                "message": "I prepared a chart card.",
                "actions": [{
                    "kind": "create_datapanel_card",
                    "title": "Risk trend",
                    "description": "Risk count by day",
                    "display": "chart",
                    "sql": "select day, risk_count from risk_daily",
                    "chart_type": "line",
                    "x_key": "day",
                    "y_keys": ["risk_count"],
                    "limit": 50
                }]
            }"#,
            &llm_context(),
        )
        .unwrap();

        assert_eq!(response.actions.len(), 1);
        assert_eq!(
            response.actions[0].kind,
            AgentActionKind::CreateDatapanelCard
        );
        assert_eq!(
            response.actions[0].resource_kind,
            Some(AgentResourceKind::DatapanelCard)
        );
        assert_eq!(response.actions[0].payload["managed_database_id"], "db-1");
        assert_eq!(response.actions[0].payload["kind"], "chart");
        assert_eq!(response.actions[0].payload["chart"]["chart_type"], "line");
        assert_eq!(response.actions[0].payload["chart"]["x_key"], "day");
        assert_eq!(
            response.actions[0].payload["chart"]["y_keys"][0],
            "risk_count"
        );
        assert_eq!(response.actions[0].payload["limit"], 50);
    }

    #[test]
    fn rejects_bi_chart_without_y_keys() {
        let error = parse_llm_workbench_response(
            r#"{
                "message": "I prepared a chart card.",
                "actions": [{
                    "kind": "create_datapanel_card",
                    "title": "Risk trend",
                    "display": "chart",
                    "sql": "select day, risk_count from risk_daily",
                    "chart_type": "line",
                    "x_key": "day"
                }]
            }"#,
            &llm_context(),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "y_keys is required");
    }

    #[test]
    fn parses_llm_composed_datapanel_card_action() {
        let response = parse_llm_workbench_response(
            r#"{
                "message": "I prepared a composed chart card.",
                "actions": [{
                    "kind": "create_datapanel_card",
                    "title": "Revenue mix",
                    "display": "chart",
                    "sql": "select day, revenue, cost from revenue_daily",
                    "chart_type": "composed",
                    "x_key": "day",
                    "series": [
                        { "key": "revenue", "kind": "bar" },
                        { "key": "cost", "kind": "line" }
                    ]
                }]
            }"#,
            &llm_context(),
        )
        .unwrap();

        let chart = &response.actions[0].payload["chart"];
        assert_eq!(chart["chart_type"], "composed");
        assert_eq!(chart["x_key"], "day");
        assert_eq!(chart["series"][0]["key"], "revenue");
        assert_eq!(chart["series"][0]["kind"], "bar");
        assert_eq!(chart["series"][1]["key"], "cost");
        assert_eq!(chart["series"][1]["kind"], "line");
        assert_eq!(chart["y_keys"][0], "revenue");
        assert_eq!(chart["y_keys"][1], "cost");
    }

    #[test]
    fn parses_llm_hierarchy_datapanel_card_action() {
        let response = parse_llm_workbench_response(
            r#"{
                "message": "I prepared a treemap card.",
                "actions": [{
                    "kind": "create_datapanel_card",
                    "title": "Revenue hierarchy",
                    "display": "chart",
                    "sql": "select region, product, revenue from revenue_by_product",
                    "chart_type": "treemap",
                    "group_keys": ["region", "product"],
                    "value_key": "revenue"
                }]
            }"#,
            &llm_context(),
        )
        .unwrap();

        let chart = &response.actions[0].payload["chart"];
        assert_eq!(chart["chart_type"], "treemap");
        assert_eq!(chart["group_keys"][0], "region");
        assert_eq!(chart["group_keys"][1], "product");
        assert_eq!(chart["value_key"], "revenue");
        assert!(chart.get("x_key").is_none());
        assert!(chart.get("y_keys").is_none());
    }

    #[test]
    fn rejects_composed_chart_without_series() {
        let error = parse_llm_workbench_response(
            r#"{
                "message": "I prepared a chart card.",
                "actions": [{
                    "kind": "create_datapanel_card",
                    "title": "Revenue mix",
                    "display": "chart",
                    "sql": "select day, revenue from revenue_daily",
                    "chart_type": "composed",
                    "x_key": "day"
                }]
            }"#,
            &llm_context(),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "series is required");
    }

    #[test]
    fn rejects_hierarchy_chart_without_value_key() {
        let error = parse_llm_workbench_response(
            r#"{
                "message": "I prepared a chart card.",
                "actions": [{
                    "kind": "create_datapanel_card",
                    "title": "Revenue hierarchy",
                    "display": "chart",
                    "sql": "select region, product, revenue from revenue_by_product",
                    "chart_type": "sunburst",
                    "group_keys": ["region", "product"]
                }]
            }"#,
            &llm_context(),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "value_key is required");
    }

    #[test]
    fn rejects_unknown_bi_chart_type() {
        let error = parse_llm_workbench_response(
            r#"{
                "message": "I prepared a chart card.",
                "actions": [{
                    "kind": "create_datapanel_card",
                    "title": "Risk trend",
                    "display": "chart",
                    "sql": "select day, risk_count from risk_daily",
                    "chart_type": "bubble",
                    "x_key": "day",
                    "y_keys": ["risk_count"]
                }]
            }"#,
            &llm_context(),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("LLM workbench response was not valid JSON")
        );
    }

    #[test]
    fn parses_fenced_llm_workbench_json_response() {
        let response = parse_llm_workbench_response(
            r#"```json
            {
                "message": "I can approve that.",
                "actions": [{
                    "kind": "approve_sql_audit",
                    "title": "Approve SQL audit",
                    "description": "Approve audit-1",
                    "sql_audit_id": "audit-1"
                }]
            }
            ```"#,
            &llm_context(),
        )
        .unwrap();

        assert_eq!(response.content, "I can approve that.");
        assert_eq!(response.actions[0].kind, AgentActionKind::ApproveSqlAudit);
        assert_eq!(response.actions[0].payload["sql_audit_id"], "audit-1");
    }

    #[test]
    fn rejects_unsupported_llm_workbench_action() {
        let error = parse_llm_workbench_response(
            r#"{
                "message": "I will do that.",
                "actions": [{
                    "kind": "create_managed_database",
                    "title": "Create database",
                    "description": "Create it"
                }]
            }"#,
            &llm_context(),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("LLM workbench response was not valid JSON")
        );
    }

    #[test]
    fn rejects_raw_llm_workbench_action_payload() {
        let error = parse_llm_workbench_response(
            r#"{
                "message": "I prepared an audit.",
                "actions": [{
                    "kind": "create_sql_audit",
                    "title": "Create SQL audit",
                    "description": "Review the query",
                    "sql": "select 1",
                    "payload": {
                        "managed_database_id": "db-other"
                    }
                }]
            }"#,
            &llm_context(),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("LLM workbench response was not valid JSON")
        );
    }

    #[test]
    fn rejects_empty_llm_workbench_message() {
        let error = parse_llm_workbench_response(
            r#"{
                "message": " ",
                "actions": []
            }"#,
            &llm_context(),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "message is required");
    }

    #[test]
    fn rejects_unknown_sql_audit_id() {
        let error = parse_llm_workbench_response(
            r#"{
                "message": "I can execute that.",
                "actions": [{
                    "kind": "execute_sql_audit",
                    "title": "Execute SQL audit",
                    "description": "Execute unknown audit",
                    "sql_audit_id": "audit-missing"
                }]
            }"#,
            &llm_context(),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "sql_audit_id is not available in the current workbench context"
        );
    }

    #[test]
    fn parses_plain_text_llm_workbench_response_as_final_message() {
        let response =
            parse_llm_workbench_response("可以，我会先查看当前数据库列表。", &llm_context())
                .unwrap();

        assert_eq!(response.content, "可以，我会先查看当前数据库列表。");
        assert!(response.actions.is_empty());
        assert!(!response.waiting_for_user);
    }

    #[test]
    fn rejects_malformed_json_llm_workbench_response() {
        let error = parse_llm_workbench_response(r#"{"message":"missing end""#, &llm_context())
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "LLM workbench response was not valid JSON"
        );
    }

    #[tokio::test]
    async fn synthesizes_observation_from_real_tool_result() {
        let client = Arc::new(CapturingLlmClient::new(
            r#"{
                "message": "test1 数据库已创建成功。",
                "actions": []
            }"#,
        ));
        let agent = LlmWorkbenchAgent::new(
            client.clone(),
            "chat-model",
            liquid_llm::LlmProtocol::ChatCompletions,
        );

        let response = agent
            .synthesize_observation(
                llm_context(),
                json!({
                    "type": "tool_observation",
                    "success": true,
                    "resource": {
                        "kind": "sql_audit",
                        "id": "audit-1"
                    },
                    "result": {
                        "record": {
                            "status": "executed",
                            "execution_result": {
                                "statement_kind": "create",
                                "affected_rows": 1
                            }
                        }
                    }
                }),
            )
            .await
            .unwrap();

        assert_eq!(response.content, "test1 数据库已创建成功。");
        assert!(response.actions.is_empty());

        let request = client.captured_request();
        assert_eq!(request.model, "chat-model");
        assert_eq!(request.messages.len(), 2);
        assert!(
            request.messages[0]
                .content
                .contains("Liquid's AI workbench operator")
        );
        assert!(request.messages[0].content.contains("Operating modes"));
        assert!(request.messages[0].content.contains("tool_observation"));
        assert!(request.messages[0].content.contains("Do not invent"));
        assert!(
            request.messages[1]
                .content
                .contains("\"mode\": \"tool_observation_synthesis\"")
        );
        assert!(request.messages[1].content.contains("\"tool_observation\""));
        assert!(request.messages[1].content.contains("\"success\": true"));
        assert_eq!(request.max_output_tokens, None);
    }

    #[tokio::test]
    async fn workbench_max_output_tokens_is_configurable() {
        let client = Arc::new(CapturingLlmClient::new(
            r#"{
                "message": "I will use read-only tools for database listing tasks.",
                "actions": []
            }"#,
        ));
        let agent = LlmWorkbenchAgent::new(
            client.clone(),
            "chat-model",
            liquid_llm::LlmProtocol::ChatCompletions,
        )
        .with_max_output_tokens(Some(4096));

        let response = agent.respond(llm_context()).await.unwrap();

        assert_eq!(
            response.content,
            "I will use read-only tools for database listing tasks."
        );
        assert_eq!(client.captured_request().max_output_tokens, Some(4096));
    }

    #[tokio::test]
    async fn synthesizes_failed_tool_observation() {
        let client = Arc::new(CapturingLlmClient::new(
            r#"{
                "message": "执行失败：权限不足，无法创建数据库。",
                "actions": []
            }"#,
        ));
        let agent = LlmWorkbenchAgent::new(
            client,
            "chat-model",
            liquid_llm::LlmProtocol::ChatCompletions,
        );

        let response = agent
            .synthesize_observation(
                llm_context(),
                json!({
                    "type": "tool_observation",
                    "success": false,
                    "error": "permission denied to create database"
                }),
            )
            .await
            .unwrap();

        assert_eq!(response.content, "执行失败：权限不足，无法创建数据库。");
        assert!(response.actions.is_empty());
    }

    #[tokio::test]
    async fn workbench_tool_loop_runs_readonly_tool_and_answers_final_json() {
        let client = Arc::new(ScriptedWorkbenchLlmClient::new(vec![
            LlmResponse::text("").with_tool_calls(vec![ToolCall::new(
                "call_1",
                "pg_execute_readonly_sql",
                r#"{"sql":"select datname from pg_database where datistemplate = false order by datname","limit":100}"#,
            )]),
            LlmResponse::text(
                r#"{
                    "message": "当前有两个数据库：postgres、liquid。",
                    "actions": []
                }"#,
            ),
        ]));
        let mut tools = ToolRegistry::new();
        tools.register(StaticWorkbenchTool {
            name: "pg_execute_readonly_sql",
            output: json!({
                "columns": ["datname"],
                "rows": [
                    { "datname": "postgres" },
                    { "datname": "liquid" }
                ],
                "row_count": 2,
                "truncated": false
            }),
        });
        let agent = LlmWorkbenchAgent::new(
            client.clone(),
            "chat-model",
            liquid_llm::LlmProtocol::ChatCompletions,
        );

        let response = agent
            .respond_with_tools(llm_context(), tools)
            .await
            .unwrap();

        assert_eq!(response.content, "当前有两个数据库：postgres、liquid。");
        assert!(response.actions.is_empty());
        assert!(!response.waiting_for_user);
        assert_eq!(response.tool_steps.len(), 1);
        assert_eq!(response.tool_steps[0].name, "pg_execute_readonly_sql");
        assert!(response.tool_steps[0].succeeded);

        let requests = client.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].max_output_tokens, None);
        assert!(
            requests[0]
                .tools
                .iter()
                .any(|tool| tool.name == "pg_execute_readonly_sql")
        );
        let tool_message = requests[1]
            .messages
            .iter()
            .find(|message| message.role == MessageRole::Tool)
            .unwrap();
        assert_eq!(tool_message.tool_call_id.as_deref(), Some("call_1"));
        assert!(tool_message.content.contains(r#""row_count":2"#));
    }

    #[tokio::test]
    async fn workbench_tool_loop_returns_proposal_and_waits_for_user() {
        let client = Arc::new(ScriptedWorkbenchLlmClient::new(vec![
            LlmResponse::text("").with_tool_calls(vec![ToolCall::new(
                "call_1",
                "propose_sql_operation",
                r#"{
                    "title": "Create database test1",
                    "description": "Create the requested test1 database after confirmation.",
                    "sql": "create database test1",
                    "execution_purpose": "Create the user requested test1 database."
                }"#,
            )]),
            LlmResponse::text(
                r#"{
                    "message": "我已准备好创建 test1 数据库，确认后会执行。",
                    "actions": []
                }"#,
            ),
        ]));
        let agent = LlmWorkbenchAgent::new(
            client.clone(),
            "chat-model",
            liquid_llm::LlmProtocol::ChatCompletions,
        );

        let response = agent
            .respond_with_tools(llm_context(), ToolRegistry::new())
            .await
            .unwrap();

        assert_eq!(response.actions.len(), 1);
        assert_eq!(response.actions[0].kind, AgentActionKind::CreateSqlAudit);
        assert_eq!(
            response.actions[0].payload["request"]["sql"],
            "create database test1"
        );
        assert_eq!(
            response.actions[0].payload["request"]["execution_purpose"],
            "Create the user requested test1 database."
        );
        assert!(response.waiting_for_user);
        assert_eq!(response.tool_steps.len(), 1);
        assert!(response.tool_steps[0].proposal.is_some());

        let requests = client.requests();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].tools.is_empty());
    }

    #[tokio::test]
    async fn workbench_tool_loop_returns_tool_errors_to_model_for_correction() {
        let client = Arc::new(ScriptedWorkbenchLlmClient::new(vec![
            LlmResponse::text("").with_tool_calls(vec![ToolCall::new(
                "call_1",
                "pg_execute_readonly_sql",
                r#"{"sql":"select * from missing_table"}"#,
            )]),
            LlmResponse::text(
                r#"{
                    "message": "查询失败：表不存在。请确认表名后重试。",
                    "actions": []
                }"#,
            ),
        ]));
        let mut tools = ToolRegistry::new();
        tools.register(FailingWorkbenchTool {
            name: "pg_execute_readonly_sql",
        });
        let agent = LlmWorkbenchAgent::new(
            client.clone(),
            "chat-model",
            liquid_llm::LlmProtocol::ChatCompletions,
        );

        let response = agent
            .respond_with_tools(llm_context(), tools)
            .await
            .unwrap();

        assert_eq!(response.content, "查询失败：表不存在。请确认表名后重试。");
        assert_eq!(response.tool_steps.len(), 1);
        assert!(!response.tool_steps[0].succeeded);

        let requests = client.requests();
        let tool_message = requests[1]
            .messages
            .iter()
            .find(|message| message.role == MessageRole::Tool)
            .unwrap();
        assert!(tool_message.content.contains(r#""ok":false"#));
        assert!(tool_message.content.contains("agent tool failed"));
    }

    #[tokio::test]
    async fn workbench_tool_loop_replays_responses_output_items() {
        let function_call_item = json!({
            "id": "fc_1",
            "type": "function_call",
            "call_id": "call_1",
            "name": "pg_execute_readonly_sql",
            "arguments": "{\"sql\":\"select 1 as ok\"}"
        });
        let client = Arc::new(ScriptedWorkbenchLlmClient::new(vec![
            LlmResponse {
                id: Some("resp_1".to_owned()),
                content: String::new(),
                tool_calls: vec![ToolCall::new(
                    "call_1",
                    "pg_execute_readonly_sql",
                    r#"{"sql":"select 1 as ok"}"#,
                )],
                output_items: vec![function_call_item.clone()],
                raw: json!({}),
            },
            LlmResponse::text(
                r#"{
                    "message": "ok 为 1。",
                    "actions": []
                }"#,
            ),
        ]));
        let mut tools = ToolRegistry::new();
        tools.register(StaticWorkbenchTool {
            name: "pg_execute_readonly_sql",
            output: json!({
                "columns": ["ok"],
                "rows": [{ "ok": 1 }],
                "row_count": 1,
                "truncated": false
            }),
        });
        let agent = LlmWorkbenchAgent::new(
            client.clone(),
            "chat-model",
            liquid_llm::LlmProtocol::Responses,
        );

        let response = agent
            .respond_with_tools(llm_context(), tools)
            .await
            .unwrap();

        assert_eq!(response.content, "ok 为 1。");
        let requests = client.requests();
        let assistant_message = requests[1]
            .messages
            .iter()
            .find(|message| !message.response_items.is_empty())
            .unwrap();
        assert_eq!(assistant_message.response_items, vec![function_call_item]);
    }

    #[tokio::test]
    async fn workbench_tool_loop_stops_after_max_rounds() {
        let client = Arc::new(ScriptedWorkbenchLlmClient::new(vec![
            LlmResponse::text("").with_tool_calls(vec![ToolCall::new(
                "call_1",
                "pg_execute_readonly_sql",
                r#"{"sql":"select 1"}"#,
            )]),
        ]));
        let mut tools = ToolRegistry::new();
        tools.register(StaticWorkbenchTool {
            name: "pg_execute_readonly_sql",
            output: json!({ "row_count": 1, "rows": [{ "ok": 1 }] }),
        });
        let agent = LlmWorkbenchAgent::new(
            client,
            "chat-model",
            liquid_llm::LlmProtocol::ChatCompletions,
        )
        .with_max_tool_rounds(1);

        let error = agent
            .respond_with_tools(llm_context(), tools)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("maximum tool rounds"));
    }

    #[tokio::test]
    async fn workbench_streams_final_response_text_deltas() {
        let client = Arc::new(StreamingWorkbenchLlmClient::new(vec![
            LlmEvent::TextDelta("查询".to_owned()),
            LlmEvent::TextDelta("完成。".to_owned()),
            LlmEvent::Done,
        ]));
        let agent = LlmWorkbenchAgent::new(
            client.clone(),
            "chat-model",
            liquid_llm::LlmProtocol::ChatCompletions,
        )
        .with_streaming_enabled(true);
        let deltas = Arc::new(Mutex::new(Vec::new()));
        let captured = deltas.clone();

        let response = agent
            .respond_with_tools_and_text_delta(llm_context(), ToolRegistry::new(), move |delta| {
                let captured = captured.clone();
                async move {
                    captured.lock().unwrap().push(delta);
                }
            })
            .await
            .unwrap();

        assert_eq!(response.content, "查询完成。");
        assert_eq!(
            *deltas.lock().unwrap(),
            vec!["查询".to_owned(), "完成。".to_owned()]
        );
        assert_eq!(*client.complete_calls.lock().unwrap(), 0);
        assert_eq!(client.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn response_prompt_is_task_first_and_treats_audit_as_a_tool() {
        let client = Arc::new(CapturingLlmClient::new(
            r#"{
                "message": "I will use read-only tools for database listing tasks.",
                "actions": []
            }"#,
        ));
        let agent = LlmWorkbenchAgent::new(
            client.clone(),
            "chat-model",
            liquid_llm::LlmProtocol::ChatCompletions,
        );

        let response = agent.respond(llm_context()).await.unwrap();

        assert_eq!(
            response.content,
            "I will use read-only tools for database listing tasks."
        );
        assert!(response.actions.is_empty());

        let request = client.captured_request();
        let system_prompt = &request.messages[0].content;
        assert_eq!(request.messages.len(), 2);
        assert!(system_prompt.contains("task-first ReAct orchestration"));
        assert!(system_prompt.contains("SQL audit is only one safety gate"));
        assert!(system_prompt.contains("Read-only data retrieval"));
        assert!(system_prompt.contains("use automatic read-only PostgreSQL tools"));
        assert!(system_prompt.contains("tool_capabilities.write_sql_execution"));
        assert!(system_prompt.contains("LIQUID_SQL_EXECUTION=write_gated"));
        assert!(system_prompt.contains("propose_datapanel_card_action"));
        assert!(system_prompt.contains("not for ordinary read questions"));
        assert!(
            request.messages[1]
                .content
                .contains("\"mode\": \"planning\"")
        );
        assert!(
            request.messages[1]
                .content
                .contains("\"write_sql_execution\": true")
        );
    }
}
