use std::{collections::BTreeMap, pin::Pin, sync::Arc};

use anyhow::{Context, Result, anyhow, bail};
use async_stream::try_stream;
use async_trait::async_trait;
use futures_core::Stream;
use liquid_core::{AuditSummary, RiskSeverity};
use liquid_llm::{LlmClient, LlmMessage, LlmProtocol, LlmRequest, ToolCall, ToolDefinition};
use liquid_sql::{PgSqlAnalysisRequest, PgSqlFinding, PgSqlRiskSeverity, analyze_postgres_sql};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const DEFAULT_MAX_TOOL_ROUNDS: usize = 6;

const SQL_AUDIT_SYSTEM_PROMPT: &str = r#"You are Liquid's SQL audit agent.
Audit PostgreSQL for data safety, governance, operational risk, and performance risk.
Use inspect_sql_risk for deterministic PostgreSQL parser and AST rule findings.
Treat tool output as factual evidence: do not override parse errors, statement classifications, missing WHERE checks, destructive DDL classifications, or other deterministic rule results.
Return the final answer as JSON only with this shape:
{
  "summary": "short operational summary",
  "risk_score": 0,
  "findings": [
    {
      "title": "finding title",
      "severity": "low|medium|high|critical",
      "explanation": "why it matters",
      "recommendation": "specific mitigation"
    }
  ]
}"#;

pub type AgentStream = Pin<Box<dyn Stream<Item = Result<AgentEvent>> + Send>>;

#[async_trait]
pub trait SqlAuditAgent: Send + Sync {
    async fn audit_summary(&self) -> Result<AuditSummary>;
    async fn audit_sql(&self, request: SqlAuditRequest) -> Result<SqlAuditReport>;
    async fn audit_sql_stream(&self, request: SqlAuditRequest) -> Result<AgentStream>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SqlAuditRequest {
    pub sql: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

impl SqlAuditRequest {
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            schema: None,
            context: None,
        }
    }

    pub fn with_schema(mut self, schema: impl Into<String>) -> Self {
        self.schema = Some(schema.into());
        self
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SqlAuditReport {
    pub summary: String,
    pub risk_score: u8,
    #[serde(default)]
    pub findings: Vec<SqlAuditFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SqlAuditFinding {
    pub title: String,
    pub severity: RiskSeverity,
    pub explanation: String,
    pub recommendation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Started,
    ToolCallStarted {
        id: String,
        name: String,
    },
    ToolCallFinished {
        id: String,
        name: String,
        output: ToolOutput,
    },
    Completed {
        report: SqlAuditReport,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolOutput {
    pub content: String,
}

impl ToolOutput {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
        }
    }

    pub fn json(value: Value) -> Self {
        Self::new(value.to_string())
    }
}

#[async_trait]
pub trait AgentTool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, arguments: Value) -> Result<ToolOutput>;
}

#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn AgentTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_default_sql_tools() -> Self {
        let mut registry = Self::new();
        registry.register(SqlRiskInspectionTool);
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

#[derive(Debug, Clone, Copy, Default)]
pub struct SqlRiskInspectionTool;

#[async_trait]
impl AgentTool for SqlRiskInspectionTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "inspect_sql_risk",
            "Inspect PostgreSQL SQL text for deterministic parser and AST risk findings.",
            json!({
                "type": "object",
                "properties": {
                    "sql": {
                        "type": "string",
                        "description": "The PostgreSQL statement or script to inspect."
                    }
                },
                "required": ["sql"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let sql = arguments
            .get("sql")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|sql| !sql.is_empty())
            .ok_or_else(|| anyhow!("inspect_sql_risk requires a non-empty sql argument"))?;
        let analysis = analyze_postgres_sql(PgSqlAnalysisRequest::new(sql));

        Ok(ToolOutput::json(json!({
            "dialect": "postgresql",
            "parse_ok": analysis.parse_ok(),
            "statement_count": analysis.statements.len(),
            "statements": analysis.statements,
            "findings": analysis.findings,
            "parse_error": analysis.parse_error,
            "risk_floor": analysis.risk_floor(),
        })))
    }
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

#[derive(Debug, Default)]
pub struct MockSqlAuditAgent;

#[async_trait]
impl SqlAuditAgent for MockSqlAuditAgent {
    async fn audit_summary(&self) -> Result<AuditSummary> {
        Ok(AuditSummary::sample())
    }

    async fn audit_sql(&self, request: SqlAuditRequest) -> Result<SqlAuditReport> {
        let analysis = analyze_postgres_sql(PgSqlAnalysisRequest::new(request.sql));
        let risk_score = analysis.risk_floor().max(10);
        let findings = analysis.findings.iter().map(mock_finding_from_pg).collect();

        Ok(SqlAuditReport {
            summary: "Mock SQL audit completed.".to_owned(),
            risk_score,
            findings,
        })
    }

    async fn audit_sql_stream(&self, request: SqlAuditRequest) -> Result<AgentStream> {
        let report = self.audit_sql(request).await?;

        Ok(Box::pin(try_stream! {
            yield AgentEvent::Started;
            yield AgentEvent::Completed { report };
        }))
    }
}

fn audit_messages(request: &SqlAuditRequest) -> Result<Vec<LlmMessage>> {
    let sql = request.sql.trim();

    if sql.is_empty() {
        bail!("SQL audit request must include SQL");
    }

    let mut user = format!("Audit this SQL:\n\n```sql\n{sql}\n```");

    if let Some(schema) = request
        .schema
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        user.push_str("\n\nSchema context:\n\n");
        user.push_str(schema);
    }

    if let Some(context) = request
        .context
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        user.push_str("\n\nBusiness context:\n\n");
        user.push_str(context);
    }

    Ok(vec![
        LlmMessage::system(SQL_AUDIT_SYSTEM_PROMPT),
        LlmMessage::user(user),
    ])
}

fn parse_audit_report(content: &str) -> Result<SqlAuditReport> {
    let trimmed = content.trim();

    if let Ok(report) = serde_json::from_str(trimmed) {
        return Ok(report);
    }

    let fenced_report = fenced_json(trimmed)
        .and_then(|json_content| serde_json::from_str::<SqlAuditReport>(json_content).ok());

    if let Some(report) = fenced_report {
        return Ok(report);
    }

    bail!("LLM audit report was not valid JSON")
}

fn fenced_json(content: &str) -> Option<&str> {
    let start = content.find("```")?;
    let after_fence = &content[start + 3..];
    let json_start = after_fence.strip_prefix("json").unwrap_or(after_fence);
    let json_start = json_start
        .strip_prefix('\n')
        .or_else(|| json_start.strip_prefix("\r\n"))
        .unwrap_or(json_start);
    let end = json_start.find("```")?;

    Some(json_start[..end].trim())
}

fn mock_finding_from_pg(finding: &PgSqlFinding) -> SqlAuditFinding {
    SqlAuditFinding {
        title: finding.title.clone(),
        severity: risk_severity_from_pg(&finding.severity),
        explanation: finding.detail.clone(),
        recommendation: recommendation_for_rule(&finding.rule_id).to_owned(),
    }
}

fn risk_severity_from_pg(severity: &PgSqlRiskSeverity) -> RiskSeverity {
    match severity {
        PgSqlRiskSeverity::Low => RiskSeverity::Low,
        PgSqlRiskSeverity::Medium => RiskSeverity::Medium,
        PgSqlRiskSeverity::High => RiskSeverity::High,
        PgSqlRiskSeverity::Critical => RiskSeverity::Critical,
    }
}

fn recommendation_for_rule(rule_id: &str) -> &'static str {
    match rule_id {
        "parse_error" => "Fix the PostgreSQL syntax before risk review.",
        "delete_without_where" | "update_without_where" | "tautological_where" => {
            "Add a selective predicate or split the write into a reviewed migration."
        }
        "destructive_drop"
        | "destructive_truncate"
        | "dangerous_alter_table"
        | "drop_cascade"
        | "alter_table_drop_object"
        | "alter_table_rewrite_or_validate"
        | "alter_table_disables_safety" => {
            "Require explicit approval, maintenance timing, and rollback planning."
        }
        "create_index_without_concurrently" | "refresh_matview_without_concurrently" => {
            "Prefer PostgreSQL concurrent forms or schedule a maintenance window."
        }
        "select_star" => "Select only the columns required by the workflow.",
        "join_without_qualification" => "Add an explicit ON or USING condition.",
        "insert_values_row_limit" | "insert_from_select" | "copy_from" => {
            "Batch the write or use a controlled bulk-load path."
        }
        "merge_write_actions" => "Review source cardinality and each MERGE action predicate.",
        "copy_program" => "Avoid server-side program execution unless it is explicitly approved.",
        "create_extension" | "create_function" | "do_block" => {
            "Review executable database code and required privileges before execution."
        }
        "grant_privileges" | "revoke_privileges" | "grant_role" | "revoke_role" | "alter_role"
        | "alter_role_set" | "drop_role" => {
            "Require privilege-owner review and confirm operational access impact."
        }
        "select_for_locking" | "explicit_lock" => {
            "Review lock scope, transaction duration, and concurrent workload impact."
        }
        _ => "Review this deterministic PostgreSQL risk finding before execution.",
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use futures_util::stream;
    use liquid_llm::{LlmEvent, LlmResponse};

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
    async fn inspect_sql_risk_returns_postgresql_deterministic_findings() {
        let output = SqlRiskInspectionTool
            .execute(json!({
                "sql": "delete from users"
            }))
            .await
            .unwrap();
        let payload: Value = serde_json::from_str(&output.content).unwrap();

        assert_eq!(payload["dialect"], "postgresql");
        assert_eq!(payload["parse_ok"], true);
        assert_eq!(payload["statement_count"], 1);
        assert_eq!(payload["risk_floor"], 95);
        assert_eq!(payload["findings"][0]["rule_id"], "delete_without_where");
    }

    #[tokio::test]
    async fn mock_agent_uses_deterministic_sql_findings() {
        let report = MockSqlAuditAgent
            .audit_sql(SqlAuditRequest::new("select * from users"))
            .await
            .unwrap();

        assert_eq!(report.risk_score, 50);
        assert_eq!(report.findings[0].title, "Broad column projection");
        assert_eq!(report.findings[0].severity, RiskSeverity::Medium);
    }

    #[test]
    fn parses_fenced_json_report() {
        let report = parse_audit_report(
            r#"```json
            {
                "summary": "ok",
                "risk_score": 12,
                "findings": []
            }
            ```"#,
        )
        .unwrap();

        assert_eq!(report.risk_score, 12);
    }
}
