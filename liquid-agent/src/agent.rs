use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use async_stream::try_stream;
use async_trait::async_trait;
use liquid_core::AuditSummary;
use liquid_llm::{LlmClient, LlmMessage, LlmProtocol, LlmRequest};

use crate::{
    prompt::{audit_messages, parse_audit_report},
    tools::ToolRegistry,
    types::{AgentEvent, AgentStream, SqlAuditReport, SqlAuditRequest},
};

const DEFAULT_MAX_TOOL_ROUNDS: usize = 6;

#[async_trait]
pub trait SqlAuditAgent: Send + Sync {
    async fn audit_summary(&self) -> Result<AuditSummary>;
    async fn audit_sql(&self, request: SqlAuditRequest) -> Result<SqlAuditReport>;
    async fn audit_sql_stream(&self, request: SqlAuditRequest) -> Result<AgentStream>;
}

#[derive(Clone)]
pub struct ToolCallingSqlAuditAgent {
    llm: Arc<dyn LlmClient>,
    model: String,
    protocol: LlmProtocol,
    tools: ToolRegistry,
    max_tool_rounds: usize,
}

impl ToolCallingSqlAuditAgent {
    pub fn new(llm: Arc<dyn LlmClient>, model: impl Into<String>, protocol: LlmProtocol) -> Self {
        Self {
            llm,
            model: model.into(),
            protocol,
            tools: ToolRegistry::with_default_sql_tools(),
            max_tool_rounds: DEFAULT_MAX_TOOL_ROUNDS,
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

    async fn run_audit(&self, request: SqlAuditRequest) -> Result<SqlAuditReport> {
        let mut messages = audit_messages(&request)?;

        for _ in 0..self.max_tool_rounds {
            let response = self
                .llm
                .complete(self.llm_request(messages.clone()))
                .await?;

            if response.tool_calls.is_empty() {
                return parse_audit_report(&response.content);
            }

            messages.push(LlmMessage::assistant_with_tool_calls(
                response.content,
                response.tool_calls.clone(),
            ));

            for call in &response.tool_calls {
                let output = self.tools.execute(call).await?;
                messages.push(LlmMessage::tool_result(call.id.clone(), output.content));
            }
        }

        bail!(
            "SQL audit agent exceeded maximum tool rounds ({})",
            self.max_tool_rounds
        )
    }

    fn llm_request(&self, messages: Vec<LlmMessage>) -> LlmRequest {
        LlmRequest::new(self.model.clone(), self.protocol, messages)
            .with_tools(self.tools.definitions())
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

    async fn audit_sql_stream(&self, request: SqlAuditRequest) -> Result<AgentStream> {
        let agent = self.clone();

        Ok(Box::pin(try_stream! {
            yield AgentEvent::Started;
            let mut messages = audit_messages(&request)?;

            for _ in 0..agent.max_tool_rounds {
                let response = agent.llm.complete(agent.llm_request(messages.clone())).await?;

                if response.tool_calls.is_empty() {
                    let report = parse_audit_report(&response.content)?;
                    yield AgentEvent::Completed { report };
                    return;
                }

                messages.push(LlmMessage::assistant_with_tool_calls(
                    response.content,
                    response.tool_calls.clone(),
                ));

                for call in &response.tool_calls {
                    yield AgentEvent::ToolCallStarted {
                        id: call.id.clone(),
                        name: call.name.clone(),
                    };
                    let output = agent.tools.execute(call).await?;
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
    use liquid_llm::{LlmEvent, LlmResponse, ToolCall, ToolDefinition};
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
    }

    impl ScriptedLlm {
        fn new(responses: Vec<LlmResponse>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }
    }

    #[async_trait]
    impl LlmClient for ScriptedLlm {
        async fn complete(&self, _request: LlmRequest) -> Result<LlmResponse> {
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
