use std::{sync::Arc, time::Instant};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use liquid_core::{
    AgentAction, AgentActionKind, AgentMessage, AgentResourceKind, AuditSummary, DatapanelCardKind,
    DatapanelChartConfig, DatapanelChartType, ManagedDatabase, SqlAuditRecord,
};
use liquid_llm::{LlmClient, LlmMessage, LlmProtocol, LlmRequest, ToolCall, ToolDefinition};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    tools::{AgentTool, ToolRegistry},
    types::ToolOutput,
};

const DEFAULT_MAX_WORKBENCH_TOOL_ROUNDS: usize = 6;

const WORKBENCH_SYSTEM_PROMPT: &str = r#"You are Liquid's AI workbench operator.
Your job is task-first ReAct orchestration: understand the user's desired outcome, use tools to obtain facts, and keep working until you can either give the final answer or ask the user to confirm a side-effect.
SQL audit is only one safety gate. It is not the product goal unless the user explicitly asks for audit, review, risk analysis, or approval.

Use the provided conversation, managed database, audit summary, SQL audits, and proposed actions as context.
Never invent database IDs, SQL audit IDs, action IDs, credentials, execution status, created resources, query rows, or direct tool results.
If the answer is already available from context, answer directly and return no actions.
If a tool is needed, use it. Do not replace safe read-only tool execution with a confirmation card.
The workbench_context.tool_capabilities object tells you what the server can actually do in this turn.

Operating modes:
- planning: decide whether to answer directly, use automatic read-only tools, or create a confirmation proposal.
- tool_observation_synthesis: the server has already executed or rejected a confirmed tool action and provides a structured tool_observation. In this mode, answer the user's original task from that observation. Use only the observation for factual claims about execution status, created resources, query rows, row counts, audit IDs, errors, and next steps. Do not invent database state, SQL results, IDs, or successful execution. Mention audit details only when the user asked for audit/risk feedback or when they materially affect the next step.

Tool selection rules:
- Read-only data retrieval, inspection, listing, reporting, or analytics: use automatic read-only PostgreSQL tools such as pg_list_schemas, pg_list_relations, pg_describe_relation, pg_explain_sql, and pg_execute_readonly_sql. This includes requests like "what databases are there", "list tables", "show sizes", "count rows", "trend", or "show me the result". After tool observations arrive, answer with the returned data.
- When the user asks to query or show table data, execute a narrow read-only SELECT and return rows. Do not answer only with schema or field descriptions unless the user explicitly asks for table structure.
- Saving, importing, pinning, or generating a persistent Datapanel card/chart/panel: call propose_datapanel_card_action with one safe SELECT statement. Do this only when the user asks to save/import/create a dashboard card or chart, not for ordinary read questions.
- SQL review, risk analysis, approval, rejection, or explicit audit requests: call propose_sql_operation without execution_purpose when the user wants review only.
- Mutating work such as create, alter, drop, insert, update, delete, migrate, grant, revoke, or any DDL/DML execution: only call propose_sql_operation with execution_purpose when tool_capabilities.write_sql_execution is true. The server will audit and, after user confirmation, execute through the write-gated path. If write_sql_execution is false, do not create a confirmation proposal for the write; explain that the server must be started with LIQUID_SQL_EXECUTION=write_gated before Liquid can perform the operation.
- Existing SQL audit lifecycle requests: call propose_sql_audit_decision only for SQL audit IDs that appear in the provided context.
- If no available tool can complete the user's task, say that plainly and propose the closest safe next step.

Do not present "I prepared an audit" as the main response for ordinary user tasks.
For confirmation proposals, write the message as a concise action-oriented confirmation of the intended outcome, for example "I prepared the database creation operation for confirmation."
For tool_observation_synthesis, write the final user-facing reply in the user's language and keep it concise. If the observation contains query/card rows, summarize the returned data directly and mention where the detailed result is available. If it contains successful DDL/DML execution, state the completed operation and key facts such as resource ID, statement kind, affected rows, and elapsed time when available. If it shows failure, explain what failed and what the user can do next.

When you are done and are not calling tools, return JSON only with this shape:
{
  "message": "assistant reply",
  "actions": []
}"#;

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

#[derive(Debug, Clone, PartialEq)]
pub struct WorkbenchActionSuggestion {
    pub kind: AgentActionKind,
    pub title: String,
    pub description: String,
    pub payload: Value,
    pub resource_kind: Option<AgentResourceKind>,
    pub resource_id: Option<String>,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkbenchToolStep {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub output: ToolOutput,
    pub succeeded: bool,
    pub elapsed_ms: u64,
    pub proposal: Option<WorkbenchActionSuggestion>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkbenchResponse {
    pub content: String,
    pub actions: Vec<WorkbenchActionSuggestion>,
    pub tool_steps: Vec<WorkbenchToolStep>,
    pub waiting_for_user: bool,
}

impl WorkbenchResponse {
    fn new(content: String, actions: Vec<WorkbenchActionSuggestion>) -> Self {
        let waiting_for_user = !actions.is_empty();

        Self {
            content,
            actions,
            tool_steps: Vec::new(),
            waiting_for_user,
        }
    }

    fn with_tool_steps(mut self, tool_steps: Vec<WorkbenchToolStep>) -> Self {
        self.tool_steps = tool_steps;
        self.waiting_for_user = !self.actions.is_empty();
        self
    }
}

#[derive(Clone)]
pub struct LlmWorkbenchAgent {
    llm: Arc<dyn LlmClient>,
    model: String,
    protocol: LlmProtocol,
    max_tool_rounds: usize,
}

impl LlmWorkbenchAgent {
    pub fn new(llm: Arc<dyn LlmClient>, model: impl Into<String>, protocol: LlmProtocol) -> Self {
        Self {
            llm,
            model: model.into(),
            protocol,
            max_tool_rounds: DEFAULT_MAX_WORKBENCH_TOOL_ROUNDS,
        }
    }

    pub fn with_max_tool_rounds(mut self, max_tool_rounds: usize) -> Self {
        self.max_tool_rounds = max_tool_rounds;
        self
    }

    pub async fn respond(&self, context: LlmWorkbenchContext) -> Result<WorkbenchResponse> {
        let request = LlmRequest::new(
            self.model.clone(),
            self.protocol,
            vec![
                LlmMessage::system(WORKBENCH_SYSTEM_PROMPT),
                LlmMessage::user(workbench_context_payload(&context)?),
            ],
        )
        .with_temperature(0.2)
        .with_max_output_tokens(1_200);
        let response = self.llm.complete(request).await?;

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

        self.run_tool_loop(context, messages, tools, 1_200).await
    }

    pub async fn synthesize_observation(
        &self,
        context: LlmWorkbenchContext,
        observation: Value,
    ) -> Result<WorkbenchResponse> {
        let request = LlmRequest::new(
            self.model.clone(),
            self.protocol,
            vec![
                LlmMessage::system(WORKBENCH_SYSTEM_PROMPT),
                LlmMessage::user(workbench_observation_payload(&context, observation)?),
            ],
        )
        .with_temperature(0.2)
        .with_max_output_tokens(1_000);
        let response = self.llm.complete(request).await?;

        parse_llm_workbench_response(&response.content, &context)
    }

    async fn run_tool_loop(
        &self,
        context: LlmWorkbenchContext,
        mut messages: Vec<LlmMessage>,
        tools: ToolRegistry,
        max_output_tokens: u32,
    ) -> Result<WorkbenchResponse> {
        let mut tool_steps = Vec::new();

        for _ in 0..self.max_tool_rounds {
            let response = self
                .llm
                .complete(self.llm_request(messages.clone(), &tools, max_output_tokens))
                .await?;

            if response.tool_calls.is_empty() {
                return Ok(parse_llm_workbench_response(&response.content, &context)?
                    .with_tool_steps(tool_steps));
            }

            messages.push(LlmMessage::assistant_with_response_items(
                response.content.clone(),
                response.tool_calls.clone(),
                response.output_items.clone(),
            ));

            let mut proposals = Vec::new();
            for call in &response.tool_calls {
                let step = execute_workbench_tool_for_model(&tools, call, &context).await?;
                messages.push(LlmMessage::tool_result(
                    call.id.clone(),
                    step.output.content.clone(),
                ));

                if let Some(proposal) = step.proposal.clone() {
                    proposals.push(proposal);
                }

                tool_steps.push(step);
            }

            if !proposals.is_empty() {
                let response = self
                    .llm
                    .complete(self.no_tool_llm_request(messages.clone(), max_output_tokens))
                    .await?;

                if !response.tool_calls.is_empty() {
                    bail!(
                        "LLM workbench response requested tools after creating a confirmation proposal"
                    );
                }

                let mut parsed = parse_llm_workbench_response(&response.content, &context)?;
                parsed.actions.splice(0..0, proposals);
                parsed.waiting_for_user = !parsed.actions.is_empty();
                parsed.tool_steps = tool_steps;
                return Ok(parsed);
            }
        }

        bail!(
            "LLM workbench exceeded maximum tool rounds ({})",
            self.max_tool_rounds
        )
    }

    fn llm_request(
        &self,
        messages: Vec<LlmMessage>,
        tools: &ToolRegistry,
        max_output_tokens: u32,
    ) -> LlmRequest {
        LlmRequest::new(self.model.clone(), self.protocol, messages)
            .with_tools(tools.definitions())
            .with_temperature(0.2)
            .with_max_output_tokens(max_output_tokens)
    }

    fn no_tool_llm_request(&self, messages: Vec<LlmMessage>, max_output_tokens: u32) -> LlmRequest {
        LlmRequest::new(self.model.clone(), self.protocol, messages)
            .with_temperature(0.2)
            .with_max_output_tokens(max_output_tokens)
    }
}

fn register_workbench_proposal_tools(tools: &mut ToolRegistry) {
    tools.register(ProposeSqlOperationTool);
    tools.register(ProposeDatapanelCardActionTool);
    tools.register(ProposeSqlAuditDecisionTool);
}

async fn execute_workbench_tool_for_model(
    tools: &ToolRegistry,
    call: &ToolCall,
    context: &LlmWorkbenchContext,
) -> Result<WorkbenchToolStep> {
    if !tools.contains(&call.name) {
        bail!("unsupported workbench tool: {}", call.name);
    }

    let started_at = Instant::now();
    let arguments = call.json_arguments()?;

    if is_workbench_proposal_tool(&call.name) {
        let output = tools.execute(call).await?;
        let proposal = proposal_tool_call_to_suggestion(call, context)?;

        return Ok(WorkbenchToolStep {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments,
            output,
            succeeded: true,
            elapsed_ms: elapsed_ms(started_at),
            proposal: Some(proposal),
        });
    }

    let result = tools.execute(call).await;
    match result {
        Ok(output) => Ok(WorkbenchToolStep {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments,
            output,
            succeeded: true,
            elapsed_ms: elapsed_ms(started_at),
            proposal: None,
        }),
        Err(error) => {
            let message = error.to_string();
            tracing::warn!(
                tool_name = %call.name,
                tool_call_id = %call.id,
                error = %message,
                "workbench agent tool call failed; returning error to model"
            );
            Ok(WorkbenchToolStep {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments,
                output: ToolOutput::json(json!({
                    "ok": false,
                    "tool": call.name,
                    "error": message,
                })),
                succeeded: false,
                elapsed_ms: elapsed_ms(started_at),
                proposal: None,
            })
        }
    }
}

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis() as u64
}

fn is_workbench_proposal_tool(name: &str) -> bool {
    matches!(
        name,
        "propose_sql_operation" | "propose_datapanel_card_action" | "propose_sql_audit_decision"
    )
}

#[derive(Debug, Default, Clone)]
struct ProposeSqlOperationTool;

#[async_trait]
impl AgentTool for ProposeSqlOperationTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "propose_sql_operation",
            "Create a user-confirmed SQL operation proposal. This does not execute SQL.",
            json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Short action title focused on the user goal."
                    },
                    "description": {
                        "type": "string",
                        "description": "One sentence explaining what will happen after confirmation."
                    },
                    "sql": {
                        "type": "string",
                        "description": "One SQL statement to audit and possibly execute after confirmation."
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional context for audit/review."
                    },
                    "schema": {
                        "type": "string",
                        "description": "Optional schema context."
                    },
                    "execution_purpose": {
                        "type": "string",
                        "description": "Required for mutating SQL; describes the user-approved business goal."
                    }
                },
                "required": ["title", "description", "sql"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let args: ProposeSqlOperationArgs = serde_json::from_value(arguments)?;

        Ok(ToolOutput::json(json!({
            "ok": true,
            "type": "action_proposal",
            "kind": "create_sql_audit",
            "title": args.title,
            "description": args.description,
        })))
    }
}

#[derive(Debug, Default, Clone)]
struct ProposeDatapanelCardActionTool;

#[async_trait]
impl AgentTool for ProposeDatapanelCardActionTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "propose_datapanel_card_action",
            "Create a user-confirmed Datapanel card proposal backed by one read-only SELECT statement. This does not save the card.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "display": {
                        "type": "string",
                        "enum": ["table", "chart"],
                        "description": "Use table unless the user asked for a chart."
                    },
                    "sql": {
                        "type": "string",
                        "description": "One read-only SELECT statement used to populate the Datapanel card."
                    },
                    "chart_type": {
                        "type": "string",
                        "enum": ["line", "bar", "area", "pie"]
                    },
                    "x_key": { "type": "string" },
                    "y_keys": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "limit": { "type": "integer" }
                },
                "required": ["title", "display", "sql"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let args: ProposeDatapanelCardActionArgs = serde_json::from_value(arguments)?;

        Ok(ToolOutput::json(json!({
            "ok": true,
            "type": "action_proposal",
            "kind": "create_datapanel_card",
            "title": args.title,
            "display": args.display,
        })))
    }
}

#[derive(Debug, Default, Clone)]
struct ProposeSqlAuditDecisionTool;

#[async_trait]
impl AgentTool for ProposeSqlAuditDecisionTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "propose_sql_audit_decision",
            "Create a user-confirmed SQL audit lifecycle proposal for a known audit id.",
            json!({
                "type": "object",
                "properties": {
                    "decision": {
                        "type": "string",
                        "enum": ["approve", "reject", "execute"]
                    },
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "sql_audit_id": {
                        "type": "string",
                        "description": "A SQL audit id that appears in the provided context."
                    }
                },
                "required": ["decision", "title", "description", "sql_audit_id"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let args: ProposeSqlAuditDecisionArgs = serde_json::from_value(arguments)?;

        Ok(ToolOutput::json(json!({
            "ok": true,
            "type": "action_proposal",
            "kind": format!("{}_sql_audit", args.decision),
            "title": args.title,
            "description": args.description,
            "sql_audit_id": args.sql_audit_id,
        })))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeSqlOperationArgs {
    title: String,
    description: String,
    sql: String,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    schema: Option<String>,
    #[serde(default)]
    execution_purpose: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeDatapanelCardActionArgs {
    title: String,
    #[serde(default)]
    description: Option<String>,
    display: DatapanelCardKind,
    sql: String,
    #[serde(default)]
    chart_type: Option<DatapanelChartType>,
    #[serde(default)]
    x_key: Option<String>,
    #[serde(default)]
    y_keys: Vec<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeSqlAuditDecisionArgs {
    decision: SqlAuditDecision,
    title: String,
    description: String,
    sql_audit_id: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SqlAuditDecision {
    Approve,
    Reject,
    Execute,
}

impl std::fmt::Display for SqlAuditDecision {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Approve => "approve",
            Self::Reject => "reject",
            Self::Execute => "execute",
        })
    }
}

fn proposal_tool_call_to_suggestion(
    call: &ToolCall,
    context: &LlmWorkbenchContext,
) -> Result<WorkbenchActionSuggestion> {
    match call.name.as_str() {
        "propose_sql_operation" => {
            let args: ProposeSqlOperationArgs = serde_json::from_str(&call.arguments)?;

            sql_operation_suggestion(
                context,
                args.title,
                args.description,
                args.sql,
                args.context,
                args.schema,
                args.execution_purpose,
            )
        }
        "propose_datapanel_card_action" => {
            let args: ProposeDatapanelCardActionArgs = serde_json::from_str(&call.arguments)?;

            datapanel_card_suggestion(
                context,
                args.title,
                args.description,
                args.display,
                args.sql,
                args.chart_type,
                args.x_key,
                args.y_keys,
                args.limit,
            )
        }
        "propose_sql_audit_decision" => {
            let args: ProposeSqlAuditDecisionArgs = serde_json::from_str(&call.arguments)?;
            let kind = match args.decision {
                SqlAuditDecision::Approve => AgentActionKind::ApproveSqlAudit,
                SqlAuditDecision::Reject => AgentActionKind::RejectSqlAudit,
                SqlAuditDecision::Execute => AgentActionKind::ExecuteSqlAudit,
            };

            sql_audit_llm_action(
                kind,
                args.title,
                args.description,
                args.sql_audit_id,
                context,
            )
        }
        _ => bail!("unsupported workbench proposal tool: {}", call.name),
    }
}

#[derive(Debug, Default, Clone)]
pub struct RuleBasedWorkbenchAgent;

impl RuleBasedWorkbenchAgent {
    pub fn respond(&self, context: WorkbenchContext) -> WorkbenchResponse {
        let message = context.message.trim();

        if let Some(sql_audit_id) = context.selected_sql_audit_id.as_deref() {
            if asks_for_execution(message) {
                return WorkbenchResponse::new(
                    "I prepared an execution action for the selected SQL audit. It still requires explicit confirmation and the existing write-gated execution checks.".to_owned(),
                    vec![sql_audit_action(
                        AgentActionKind::ExecuteSqlAudit,
                        "Execute approved SQL audit",
                        "Run the selected SQL audit against its managed database after approval and write-gated validation.",
                        sql_audit_id,
                    )],
                );
            }

            if asks_for_approval(message) {
                return WorkbenchResponse::new(
                    "I prepared an approval action for the selected SQL audit. Confirm it before the audit can be executed.".to_owned(),
                    vec![sql_audit_action(
                        AgentActionKind::ApproveSqlAudit,
                        "Approve SQL audit",
                        "Approve the selected SQL audit so it can be executed through the guarded execution path.",
                        sql_audit_id,
                    )],
                );
            }

            if asks_for_rejection(message) {
                return WorkbenchResponse::new(
                    "I prepared a rejection action for the selected SQL audit.".to_owned(),
                    vec![sql_audit_action(
                        AgentActionKind::RejectSqlAudit,
                        "Reject SQL audit",
                        "Reject the selected SQL audit and prevent it from being executed.",
                        sql_audit_id,
                    )],
                );
            }
        }

        if let (Some(sql), Some(database_id)) = (
            extract_sql_candidate(message),
            context.managed_database_id.as_deref(),
        ) {
            let mut request = json!({
                "sql": sql,
                "context": "Requested from the Liquid agent workbench."
            });

            if statement_needs_approval(&sql) {
                request["execution_purpose"] =
                    json!("User confirmed this SQL operation from the Liquid agent workbench.");
            }

            return WorkbenchResponse::new(
                "I found SQL in your message and prepared a confirmation action for the selected managed database. The server will audit it before execution.".to_owned(),
                vec![WorkbenchActionSuggestion {
                    kind: AgentActionKind::CreateSqlAudit,
                    title: "Confirm SQL operation".to_owned(),
                    description: "Audit the SQL and continue with execution when it passes the guarded checks.".to_owned(),
                    payload: json!({
                        "managed_database_id": database_id,
                        "request": request,
                    }),
                    resource_kind: Some(AgentResourceKind::SqlAudit),
                    resource_id: None,
                    requires_confirmation: true,
                }],
            );
        }

        let audit_score = context
            .audit_score
            .map(|score| format!(" Current audit score is {score}."))
            .unwrap_or_default();
        let database_hint = if context.managed_database_count == 0 {
            "No managed databases are connected yet."
        } else {
            "I can use your managed database list and SQL audit history as context."
        };

        WorkbenchResponse::new(
            format!(
                "{database_hint}{audit_score} Select a managed database and send SQL when you want me to prepare an audit action. For writes, I will only propose an action and keep execution behind explicit approval."
            ),
            Vec::new(),
        )
    }
}

pub fn parse_llm_workbench_response(
    content: &str,
    context: &LlmWorkbenchContext,
) -> Result<WorkbenchResponse> {
    let parsed = LlmWorkbenchResponse::parse(content)?;
    let message = required_trimmed("message", parsed.message)?;
    let mut actions = Vec::new();

    for action in parsed.actions {
        actions.push(action.into_suggestion(context)?);
    }

    Ok(WorkbenchResponse::new(message, actions))
}

fn workbench_context_payload(context: &LlmWorkbenchContext) -> Result<String> {
    serde_json::to_string_pretty(&json!({
        "mode": "planning",
        "workbench_context": workbench_context_value(context),
    }))
    .context("failed to serialize workbench context")
}

fn workbench_observation_payload(
    context: &LlmWorkbenchContext,
    observation: Value,
) -> Result<String> {
    serde_json::to_string_pretty(&json!({
        "mode": "tool_observation_synthesis",
        "workbench_context": workbench_context_value(context),
        "tool_observation": observation,
    }))
    .context("failed to serialize workbench observation context")
}

fn workbench_context_value(context: &LlmWorkbenchContext) -> Value {
    json!({
        "conversation_messages": context.messages.iter().map(message_context).collect::<Vec<_>>(),
        "managed_database": context.managed_database.as_ref().map(|database| json!({
            "id": database.id,
            "name": database.name,
            "engine": database.engine,
            "host": database.host,
            "port": database.port,
            "database": database.database,
            "username": database.username,
            "ssl_mode": database.ssl_mode,
            "has_password": database.has_password,
        })),
        "tool_capabilities": {
            "read_only_sql": context.managed_database.is_some(),
            "write_sql_execution": context.write_sql_execution_enabled,
            "writes_require_confirmation": true,
        },
        "selected_sql_audit_id": context.selected_sql_audit_id,
        "audit_summary": context.audit_summary,
        "recent_sql_audits": context.recent_sql_audits.iter().map(sql_audit_context).collect::<Vec<_>>(),
        "recent_actions": context.recent_actions.iter().map(action_context).collect::<Vec<_>>(),
    })
}

fn message_context(message: &AgentMessage) -> Value {
    json!({
        "id": message.id,
        "role": message.role,
        "content": message.content,
        "turn_id": message.turn_id,
        "created_at": message.created_at,
    })
}

fn sql_audit_context(record: &SqlAuditRecord) -> Value {
    json!({
        "id": record.id,
        "managed_database_id": record.managed_database_id,
        "status": record.status,
        "sql": record.sql,
        "context": record.context,
        "execution_purpose": record.execution_purpose,
        "risk_score": record.risk_score,
        "statement_kind": record.statement_kind,
    })
}

fn action_context(action: &AgentAction) -> Value {
    json!({
        "id": action.id,
        "kind": action.kind,
        "status": action.status,
        "title": action.title,
        "resource_kind": action.resource_kind,
        "resource_id": action.resource_id,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LlmWorkbenchResponse {
    message: String,
    #[serde(default)]
    actions: Vec<LlmWorkbenchAction>,
}

impl LlmWorkbenchResponse {
    fn parse(content: &str) -> Result<Self> {
        let trimmed = content.trim();

        if trimmed.is_empty() {
            bail!("LLM workbench response was empty");
        }

        if let Ok(response) = serde_json::from_str::<Self>(trimmed) {
            return Ok(response);
        }

        if let Some(json_content) = fenced_json(trimmed)
            && let Ok(response) = serde_json::from_str::<Self>(json_content)
        {
            return Ok(response);
        }

        if looks_like_structured_json(trimmed) {
            bail!("LLM workbench response was not valid JSON");
        }

        if let Some(json_content) = fenced_json(trimmed)
            && looks_like_structured_json(json_content)
        {
            bail!("LLM workbench response was not valid JSON");
        }

        tracing::debug!(
            response_length = trimmed.len(),
            "LLM workbench response was plain text; treating it as final assistant message"
        );
        Ok(Self {
            message: trimmed.to_owned(),
            actions: Vec::new(),
        })
    }
}

fn looks_like_structured_json(content: &str) -> bool {
    let trimmed = content.trim_start();

    trimmed.starts_with('{') || trimmed.starts_with('[') || trimmed.starts_with("```json")
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum LlmWorkbenchAction {
    CreateSqlAudit {
        title: String,
        description: String,
        sql: String,
        #[serde(default)]
        context: Option<String>,
        #[serde(default)]
        schema: Option<String>,
        #[serde(default)]
        execution_purpose: Option<String>,
    },
    CreateDatapanelCard {
        title: String,
        #[serde(default)]
        description: Option<String>,
        display: DatapanelCardKind,
        sql: String,
        #[serde(default)]
        chart_type: Option<DatapanelChartType>,
        #[serde(default)]
        x_key: Option<String>,
        #[serde(default)]
        y_keys: Vec<String>,
        #[serde(default)]
        limit: Option<usize>,
    },
    ApproveSqlAudit {
        title: String,
        description: String,
        sql_audit_id: String,
    },
    RejectSqlAudit {
        title: String,
        description: String,
        sql_audit_id: String,
    },
    ExecuteSqlAudit {
        title: String,
        description: String,
        sql_audit_id: String,
    },
}

impl LlmWorkbenchAction {
    fn into_suggestion(self, context: &LlmWorkbenchContext) -> Result<WorkbenchActionSuggestion> {
        match self {
            Self::CreateSqlAudit {
                title,
                description,
                sql,
                context: audit_context,
                schema,
                execution_purpose,
            } => sql_operation_suggestion(
                context,
                title,
                description,
                sql,
                audit_context,
                schema,
                execution_purpose,
            ),
            Self::CreateDatapanelCard {
                title,
                description,
                display,
                sql,
                chart_type,
                x_key,
                y_keys,
                limit,
            } => datapanel_card_suggestion(
                context,
                title,
                description,
                display,
                sql,
                chart_type,
                x_key,
                y_keys,
                limit,
            ),
            Self::ApproveSqlAudit {
                title,
                description,
                sql_audit_id,
            } => sql_audit_llm_action(
                AgentActionKind::ApproveSqlAudit,
                title,
                description,
                sql_audit_id,
                context,
            ),
            Self::RejectSqlAudit {
                title,
                description,
                sql_audit_id,
            } => sql_audit_llm_action(
                AgentActionKind::RejectSqlAudit,
                title,
                description,
                sql_audit_id,
                context,
            ),
            Self::ExecuteSqlAudit {
                title,
                description,
                sql_audit_id,
            } => sql_audit_llm_action(
                AgentActionKind::ExecuteSqlAudit,
                title,
                description,
                sql_audit_id,
                context,
            ),
        }
    }
}

fn sql_operation_suggestion(
    context: &LlmWorkbenchContext,
    title: String,
    description: String,
    sql: String,
    audit_context: Option<String>,
    schema: Option<String>,
    execution_purpose: Option<String>,
) -> Result<WorkbenchActionSuggestion> {
    let Some(database_id) = context
        .managed_database
        .as_ref()
        .map(|database| &database.id)
    else {
        bail!("create_sql_audit requires a selected managed database");
    };
    let sql = required_trimmed("sql", sql)?;
    let mut request = json!({ "sql": sql });

    if let Some(value) = optional_trimmed(audit_context) {
        request["context"] = json!(value);
    }

    if let Some(value) = optional_trimmed(schema) {
        request["schema"] = json!(value);
    }

    if let Some(value) = optional_trimmed(execution_purpose) {
        request["execution_purpose"] = json!(value);
    }

    Ok(WorkbenchActionSuggestion {
        kind: AgentActionKind::CreateSqlAudit,
        title: required_trimmed("title", title)?,
        description: required_trimmed("description", description)?,
        payload: json!({
            "managed_database_id": database_id,
            "request": request,
        }),
        resource_kind: Some(AgentResourceKind::SqlAudit),
        resource_id: None,
        requires_confirmation: true,
    })
}

fn datapanel_card_suggestion(
    context: &LlmWorkbenchContext,
    title: String,
    description: Option<String>,
    display: DatapanelCardKind,
    sql: String,
    chart_type: Option<DatapanelChartType>,
    x_key: Option<String>,
    y_keys: Vec<String>,
    limit: Option<usize>,
) -> Result<WorkbenchActionSuggestion> {
    let Some(database_id) = context
        .managed_database
        .as_ref()
        .map(|database| &database.id)
    else {
        bail!("create_datapanel_card requires a selected managed database");
    };
    let sql = required_trimmed("sql", sql)?;
    let title = required_trimmed("title", title)?;
    let description = optional_trimmed(description);
    let chart = match display {
        DatapanelCardKind::Table => None,
        DatapanelCardKind::Chart => Some(DatapanelChartConfig {
            chart_type: chart_type.ok_or_else(|| anyhow::anyhow!("chart_type is required"))?,
            x_key: required_trimmed(
                "x_key",
                x_key.ok_or_else(|| anyhow::anyhow!("x_key is required"))?,
            )?,
            y_keys: required_y_keys(y_keys)?,
        }),
    };

    Ok(WorkbenchActionSuggestion {
        kind: AgentActionKind::CreateDatapanelCard,
        title: title.clone(),
        description: description
            .clone()
            .unwrap_or_else(|| "Create a Datapanel card from a read-only query.".to_owned()),
        payload: json!({
            "managed_database_id": database_id,
            "title": title,
            "description": description,
            "kind": display,
            "sql": sql,
            "chart": chart,
            "limit": limit,
        }),
        resource_kind: Some(AgentResourceKind::DatapanelCard),
        resource_id: None,
        requires_confirmation: true,
    })
}

fn sql_audit_llm_action(
    kind: AgentActionKind,
    title: String,
    description: String,
    sql_audit_id: String,
    context: &LlmWorkbenchContext,
) -> Result<WorkbenchActionSuggestion> {
    let sql_audit_id = required_trimmed("sql_audit_id", sql_audit_id)?;

    if !known_sql_audit_id(context, &sql_audit_id) {
        bail!("sql_audit_id is not available in the current workbench context");
    }

    Ok(sql_audit_action(
        kind,
        &required_trimmed("title", title)?,
        &required_trimmed("description", description)?,
        &sql_audit_id,
    ))
}

fn known_sql_audit_id(context: &LlmWorkbenchContext, sql_audit_id: &str) -> bool {
    context.selected_sql_audit_id.as_deref() == Some(sql_audit_id)
        || context
            .recent_sql_audits
            .iter()
            .any(|record| record.id == sql_audit_id)
        || context.recent_actions.iter().any(|action| {
            action.resource_kind == Some(AgentResourceKind::SqlAudit)
                && action.resource_id.as_deref() == Some(sql_audit_id)
        })
}

fn required_trimmed(field: &str, value: String) -> Result<String> {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        bail!("{field} is required");
    }

    Ok(trimmed.to_owned())
}

fn optional_trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn required_y_keys(values: Vec<String>) -> Result<Vec<String>> {
    let keys = values
        .into_iter()
        .map(|value| required_trimmed("y_key", value))
        .collect::<Result<Vec<_>>>()?;

    if keys.is_empty() {
        bail!("y_keys is required");
    }

    Ok(keys)
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

fn sql_audit_action(
    kind: AgentActionKind,
    title: &str,
    description: &str,
    sql_audit_id: &str,
) -> WorkbenchActionSuggestion {
    WorkbenchActionSuggestion {
        kind,
        title: title.to_owned(),
        description: description.to_owned(),
        payload: json!({
            "sql_audit_id": sql_audit_id,
        }),
        resource_kind: Some(AgentResourceKind::SqlAudit),
        resource_id: Some(sql_audit_id.to_owned()),
        requires_confirmation: true,
    }
}

fn asks_for_execution(message: &str) -> bool {
    let value = message.to_ascii_lowercase();
    value.contains("execute")
        || value.contains("run it")
        || value.contains("apply")
        || value.contains("执行")
}

fn asks_for_approval(message: &str) -> bool {
    let value = message.to_ascii_lowercase();
    value.contains("approve") || value.contains("approval") || value.contains("批准")
}

fn asks_for_rejection(message: &str) -> bool {
    let value = message.to_ascii_lowercase();
    value.contains("reject") || value.contains("block") || value.contains("拒绝")
}

fn extract_sql_candidate(message: &str) -> Option<String> {
    if let Some(sql) = fenced_sql(message) {
        return Some(sql);
    }

    let trimmed = message.trim();
    let lower = trimmed.to_ascii_lowercase();

    if SQL_STARTERS
        .iter()
        .any(|starter| lower.starts_with(starter))
    {
        return Some(trimmed.to_owned());
    }

    None
}

fn fenced_sql(message: &str) -> Option<String> {
    let marker = "```";
    let start = message.find(marker)?;
    let rest = &message[start + marker.len()..];
    let rest = rest.strip_prefix("sql").unwrap_or(rest);
    let end = rest.find(marker)?;
    let sql = rest[..end].trim();

    if sql.is_empty() {
        None
    } else {
        Some(sql.to_owned())
    }
}

fn statement_needs_approval(sql: &str) -> bool {
    let lower = sql.trim_start().to_ascii_lowercase();

    !lower.starts_with("select")
}

const SQL_STARTERS: &[&str] = &[
    "select", "insert", "update", "delete", "merge", "create", "alter", "drop", "truncate",
    "grant", "revoke", "copy", "do ",
];

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
    fn rejects_unknown_bi_chart_type() {
        let error = parse_llm_workbench_response(
            r#"{
                "message": "I prepared a chart card.",
                "actions": [{
                    "kind": "create_datapanel_card",
                    "title": "Risk trend",
                    "display": "chart",
                    "sql": "select day, risk_count from risk_daily",
                    "chart_type": "scatter",
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
        assert_eq!(request.max_output_tokens, Some(1_000));
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
