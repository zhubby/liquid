use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use async_stream::try_stream;
use async_trait::async_trait;
use liquid_core::{AuditSummary, SqlAuditReport, SqlAuditRequest};
use liquid_llm::{LlmClient, LlmMessage, LlmProtocol, LlmRequest};

use crate::{
    llm_invocation::{LlmInvocationMode, invoke_llm},
    prompt::{audit_messages, parse_audit_report},
    tools::{ToolRegistry, execution::execute_tool_for_model, sets::sql_risk_tools},
    types::{AgentEvent, AgentStream},
};

const DEFAULT_MAX_TOOL_ROUNDS: usize = 6;

#[async_trait]
pub trait SqlAuditAgent: Send + Sync {
    async fn audit_summary(&self) -> Result<AuditSummary>;
    async fn audit_sql(&self, request: SqlAuditRequest) -> Result<SqlAuditReport>;
    async fn audit_sql_with_tools(
        &self,
        request: SqlAuditRequest,
        tools: ToolRegistry,
    ) -> Result<SqlAuditReport> {
        let _ = tools;
        self.audit_sql(request).await
    }
    async fn audit_sql_stream(&self, request: SqlAuditRequest) -> Result<AgentStream>;
}

#[derive(Clone)]
pub struct ToolCallingSqlAuditAgent {
    llm: Arc<dyn LlmClient>,
    model: String,
    protocol: LlmProtocol,
    tools: ToolRegistry,
    max_tool_rounds: usize,
    invocation_mode: LlmInvocationMode,
}

impl ToolCallingSqlAuditAgent {
    pub fn new(llm: Arc<dyn LlmClient>, model: impl Into<String>, protocol: LlmProtocol) -> Self {
        Self {
            llm,
            model: model.into(),
            protocol,
            tools: sql_risk_tools(None, false),
            max_tool_rounds: DEFAULT_MAX_TOOL_ROUNDS,
            invocation_mode: LlmInvocationMode::Complete,
        }
    }

    pub fn with_tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_max_tool_rounds(mut self, max_tool_rounds: usize) -> Self {
        self.max_tool_rounds = max_tool_rounds;
        self
    }

    pub fn with_streaming_enabled(mut self, streaming_enabled: bool) -> Self {
        self.invocation_mode = LlmInvocationMode::from_streaming_enabled(streaming_enabled);
        self
    }

    async fn run_audit_with_tools(
        &self,
        request: SqlAuditRequest,
        tools: ToolRegistry,
    ) -> Result<SqlAuditReport> {
        let mut messages = audit_messages(&request)?;

        for _ in 0..self.max_tool_rounds {
            let response = invoke_llm(
                &self.llm,
                self.llm_request(messages.clone(), &tools),
                self.invocation_mode,
            )
            .await?;

            if response.tool_calls.is_empty() {
                return parse_audit_report(&response.content);
            }

            messages.push(LlmMessage::assistant_with_response_items(
                response.content.clone(),
                response.tool_calls.clone(),
                response.output_items.clone(),
            ));

            for call in &response.tool_calls {
                let output = execute_tool_for_model(&tools, call, "sql_audit_agent").await;
                messages.push(LlmMessage::tool_result(call.id.clone(), output.content));
            }
        }

        bail!(
            "SQL audit agent exceeded maximum tool rounds ({})",
            self.max_tool_rounds
        )
    }

    async fn run_audit(&self, request: SqlAuditRequest) -> Result<SqlAuditReport> {
        self.run_audit_with_tools(request, self.tools.clone()).await
    }

    fn llm_request(&self, messages: Vec<LlmMessage>, tools: &ToolRegistry) -> LlmRequest {
        LlmRequest::new(self.model.clone(), self.protocol, messages)
            .with_tools(tools.definitions())
            .with_temperature(0.1)
            .with_max_output_tokens(1_200)
    }
}

#[async_trait]
impl SqlAuditAgent for ToolCallingSqlAuditAgent {
    async fn audit_summary(&self) -> Result<AuditSummary> {
        Ok(AuditSummary::sample())
    }

    async fn audit_sql(&self, request: SqlAuditRequest) -> Result<SqlAuditReport> {
        self.run_audit(request).await
    }

    async fn audit_sql_with_tools(
        &self,
        request: SqlAuditRequest,
        tools: ToolRegistry,
    ) -> Result<SqlAuditReport> {
        self.run_audit_with_tools(request, tools).await
    }

    async fn audit_sql_stream(&self, request: SqlAuditRequest) -> Result<AgentStream> {
        let agent = self.clone();

        Ok(Box::pin(try_stream! {
            yield AgentEvent::Started;
            let mut messages = audit_messages(&request)?;

            for _ in 0..agent.max_tool_rounds {
                let response = invoke_llm(
                    &agent.llm,
                    agent.llm_request(messages.clone(), &agent.tools),
                    agent.invocation_mode,
                ).await?;

                if response.tool_calls.is_empty() {
                    let report = parse_audit_report(&response.content)?;
                    yield AgentEvent::Completed { report };
                    return;
                }

                messages.push(LlmMessage::assistant_with_response_items(
                    response.content.clone(),
                    response.tool_calls.clone(),
                    response.output_items.clone(),
                ));

                for call in &response.tool_calls {
                    yield AgentEvent::ToolCallStarted {
                        id: call.id.clone(),
                        name: call.name.clone(),
                    };
                    let output = execute_tool_for_model(&agent.tools, call, "sql_audit_agent").await;
                    yield AgentEvent::ToolCallFinished {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        output: output.clone(),
                    };
                    messages.push(LlmMessage::tool_result(call.id.clone(), output.content));
                }
            }

            Err(anyhow!(
                "SQL audit agent exceeded maximum tool rounds ({})",
                agent.max_tool_rounds
            ))?;
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use futures_util::stream;
    use liquid_core::RiskSeverity;
    use liquid_llm::{LlmEvent, LlmResponse, MessageRole, ToolCall, ToolDefinition};
    use serde_json::{Value, json};

    use crate::{tools::AgentTool, types::ToolOutput};

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

    #[derive(Default)]
    struct FailingTool;

    #[async_trait]
    impl AgentTool for FailingTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new(
                "failing_tool",
                "Always fail.",
                json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
            )
        }

        async fn execute(&self, _arguments: Value) -> Result<ToolOutput> {
            Err(anyhow!("tool is unavailable"))
        }
    }

    struct StaticJsonTool {
        name: &'static str,
    }

    #[async_trait]
    impl AgentTool for StaticJsonTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new(
                self.name,
                "Return a static JSON object.",
                json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": true
                }),
            )
        }

        async fn execute(&self, _arguments: Value) -> Result<ToolOutput> {
            Ok(ToolOutput::json(json!({
                "ok": true,
                "tool": self.name,
            })))
        }
    }

    struct ScriptedLlm {
        responses: Mutex<VecDeque<LlmResponse>>,
        requests: Mutex<Vec<LlmRequest>>,
    }

    impl ScriptedLlm {
        fn new(responses: Vec<LlmResponse>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn requests(&self) -> Vec<LlmRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LlmClient for ScriptedLlm {
        async fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
            self.requests.lock().unwrap().push(request);
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow!("no scripted response"))
        }

        async fn stream(&self, _request: LlmRequest) -> Result<liquid_llm::LlmStream> {
            Ok(Box::pin(stream::empty::<Result<LlmEvent>>()))
        }
    }

    #[tokio::test]
    async fn audit_agent_runs_tool_loop_and_parses_final_report() {
        let llm: Arc<dyn LlmClient> = Arc::new(ScriptedLlm::new(vec![
            LlmResponse::text("").with_tool_calls(vec![ToolCall::new(
                "call_1",
                "echo_tool",
                r#"{"value":"ok"}"#,
            )]),
            LlmResponse::text(
                r#"{
                    "summary": "The query is acceptable with one note.",
                    "risk_score": 25,
                    "findings": [{
                        "title": "Review projection",
                        "severity": "low",
                        "explanation": "The query is simple.",
                        "recommendation": "Keep the projection narrow."
                    }]
                }"#,
            ),
        ]));
        let mut tools = ToolRegistry::new();
        tools.register(EchoTool);
        let agent = ToolCallingSqlAuditAgent::new(llm, "gpt-test", LlmProtocol::ChatCompletions)
            .with_tools(tools);

        let report = agent
            .audit_sql(SqlAuditRequest::new("select id from users"))
            .await
            .unwrap();

        assert_eq!(report.risk_score, 25);
        assert_eq!(report.findings[0].severity, RiskSeverity::Low);
    }

    #[tokio::test]
    async fn audit_agent_returns_tool_errors_to_model() {
        let llm = Arc::new(ScriptedLlm::new(vec![
            LlmResponse::text("").with_tool_calls(vec![ToolCall::new(
                "call_1",
                "failing_tool",
                r#"{}"#,
            )]),
            LlmResponse::text(
                r#"{
                    "summary": "The query can still be reviewed from deterministic analysis.",
                    "risk_score": 50,
                    "findings": []
                }"#,
            ),
        ]));
        let mut tools = ToolRegistry::new();
        tools.register(FailingTool);
        let agent =
            ToolCallingSqlAuditAgent::new(llm.clone(), "gpt-test", LlmProtocol::ChatCompletions)
                .with_tools(tools);

        let report = agent
            .audit_sql(SqlAuditRequest::new("create database liquid_sandbox"))
            .await
            .unwrap();

        let requests = llm.requests();
        let tool_message = requests[1]
            .messages
            .iter()
            .find(|message| message.role == MessageRole::Tool)
            .unwrap();
        assert_eq!(report.risk_score, 50);
        assert!(tool_message.content.contains(r#""ok":false"#));
        assert!(tool_message.content.contains(r#""tool":"failing_tool""#));
        assert!(tool_message.content.contains(r#""error":"#));
    }

    #[tokio::test]
    async fn audit_agent_replays_responses_output_items_before_tool_results() {
        let function_call_item = json!({
            "id": "fc_1",
            "type": "function_call",
            "call_id": "call_1",
            "name": "echo_tool",
            "arguments": "{\"value\":\"ok\"}"
        });
        let llm = Arc::new(ScriptedLlm::new(vec![
            LlmResponse {
                id: Some("resp_1".to_owned()),
                content: String::new(),
                tool_calls: vec![ToolCall::new("call_1", "echo_tool", r#"{"value":"ok"}"#)],
                output_items: vec![function_call_item.clone()],
                raw: json!({}),
            },
            LlmResponse::text(
                r#"{
                    "summary": "The query is acceptable.",
                    "risk_score": 10,
                    "findings": []
                }"#,
            ),
        ]));
        let mut tools = ToolRegistry::new();
        tools.register(EchoTool);
        let agent = ToolCallingSqlAuditAgent::new(llm.clone(), "gpt-test", LlmProtocol::Responses)
            .with_tools(tools);

        let report = agent
            .audit_sql(SqlAuditRequest::new("select id from users"))
            .await
            .unwrap();

        let requests = llm.requests();
        assert_eq!(report.risk_score, 10);
        assert_eq!(requests.len(), 2);
        let assistant_message = requests[1]
            .messages
            .iter()
            .find(|message| !message.response_items.is_empty())
            .unwrap();
        let tool_message = requests[1]
            .messages
            .iter()
            .find(|message| message.role == MessageRole::Tool)
            .unwrap();
        assert_eq!(assistant_message.response_items, vec![function_call_item]);
        assert_eq!(tool_message.tool_call_id.as_deref(), Some("call_1"));
    }

    #[tokio::test]
    async fn audit_agent_stops_after_max_tool_rounds() {
        let llm: Arc<dyn LlmClient> = Arc::new(ScriptedLlm::new(vec![
            LlmResponse::text("").with_tool_calls(vec![ToolCall::new(
                "call_1",
                "echo_tool",
                r#"{"value":"ok"}"#,
            )]),
        ]));
        let mut tools = ToolRegistry::new();
        tools.register(EchoTool);
        let agent = ToolCallingSqlAuditAgent::new(llm, "gpt-test", LlmProtocol::ChatCompletions)
            .with_tools(tools)
            .with_max_tool_rounds(1);

        let error = agent
            .audit_sql(SqlAuditRequest::new("select id from users"))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("maximum tool rounds"));
    }

    #[tokio::test]
    async fn audit_agent_runs_postgres_tool_sequence() {
        let llm: Arc<dyn LlmClient> = Arc::new(ScriptedLlm::new(vec![
            LlmResponse::text("").with_tool_calls(vec![ToolCall::new(
                "call_1",
                "pg_list_relations",
                r#"{"search":"users"}"#,
            )]),
            LlmResponse::text("").with_tool_calls(vec![ToolCall::new(
                "call_2",
                "pg_execute_readonly_sql",
                r#"{"sql":"select 1 as ok","limit":1}"#,
            )]),
            LlmResponse::text(
                r#"{
                    "summary": "The query is acceptable.",
                    "risk_score": 10,
                    "findings": []
                }"#,
            ),
        ]));
        let mut tools = ToolRegistry::new();
        tools.register(StaticJsonTool {
            name: "pg_list_relations",
        });
        tools.register(StaticJsonTool {
            name: "pg_execute_readonly_sql",
        });
        let agent = ToolCallingSqlAuditAgent::new(llm, "gpt-test", LlmProtocol::ChatCompletions)
            .with_tools(tools);

        let report = agent
            .audit_sql(SqlAuditRequest::new("select id from users"))
            .await
            .unwrap();

        assert_eq!(report.risk_score, 10);
    }
}
