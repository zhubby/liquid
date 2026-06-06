use std::sync::Arc;

use anyhow::{Context, Result, bail};
use liquid_core::{
    AgentAction, AgentActionKind, AgentMessage, AgentResourceKind, AuditSummary, ManagedDatabase,
    SqlAuditRecord,
};
use liquid_llm::{LlmClient, LlmMessage, LlmProtocol, LlmRequest};
use serde::Deserialize;
use serde_json::{Value, json};

const WORKBENCH_SYSTEM_PROMPT: &str = r#"You are Liquid's AI workbench agent for SQL governance.
Answer the user's current message using the provided conversation, managed database, audit summary, SQL audits, and proposed actions.
You may propose actions only when the user intent is clear.
Allowed actions:
- create_sql_audit: propose a SQL audit for SQL supplied by the user.
- approve_sql_audit: propose approving one of the provided SQL audit IDs.
- reject_sql_audit: propose rejecting one of the provided SQL audit IDs.
- execute_sql_audit: propose executing one of the provided SQL audit IDs.
Never invent database IDs, SQL audit IDs, action IDs, credentials, or direct execution results.
Return JSON only with this shape:
{
  "message": "assistant reply",
  "actions": [
    {
      "kind": "create_sql_audit",
      "title": "Create SQL audit",
      "description": "Review this SQL",
      "sql": "select 1",
      "context": "optional",
      "schema": "optional",
      "execution_purpose": "optional"
    },
    {
      "kind": "approve_sql_audit|reject_sql_audit|execute_sql_audit",
      "title": "Approve SQL audit",
      "description": "Approve the selected audit",
      "sql_audit_id": "known audit id"
    }
  ]
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
pub struct WorkbenchResponse {
    pub content: String,
    pub actions: Vec<WorkbenchActionSuggestion>,
}

#[derive(Clone)]
pub struct LlmWorkbenchAgent {
    llm: Arc<dyn LlmClient>,
    model: String,
    protocol: LlmProtocol,
}

impl LlmWorkbenchAgent {
    pub fn new(llm: Arc<dyn LlmClient>, model: impl Into<String>, protocol: LlmProtocol) -> Self {
        Self {
            llm,
            model: model.into(),
            protocol,
        }
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
}

#[derive(Debug, Default, Clone)]
pub struct RuleBasedWorkbenchAgent;

impl RuleBasedWorkbenchAgent {
    pub fn respond(&self, context: WorkbenchContext) -> WorkbenchResponse {
        let message = context.message.trim();

        if let Some(sql_audit_id) = context.selected_sql_audit_id.as_deref() {
            if asks_for_execution(message) {
                return WorkbenchResponse {
                    content: "I prepared an execution action for the selected SQL audit. It still requires explicit confirmation and the existing write-gated execution checks.".to_owned(),
                    actions: vec![sql_audit_action(
                        AgentActionKind::ExecuteSqlAudit,
                        "Execute approved SQL audit",
                        "Run the selected SQL audit against its managed database after approval and write-gated validation.",
                        sql_audit_id,
                    )],
                };
            }

            if asks_for_approval(message) {
                return WorkbenchResponse {
                    content: "I prepared an approval action for the selected SQL audit. Confirm it before the audit can be executed.".to_owned(),
                    actions: vec![sql_audit_action(
                        AgentActionKind::ApproveSqlAudit,
                        "Approve SQL audit",
                        "Approve the selected SQL audit so it can be executed through the guarded execution path.",
                        sql_audit_id,
                    )],
                };
            }

            if asks_for_rejection(message) {
                return WorkbenchResponse {
                    content: "I prepared a rejection action for the selected SQL audit.".to_owned(),
                    actions: vec![sql_audit_action(
                        AgentActionKind::RejectSqlAudit,
                        "Reject SQL audit",
                        "Reject the selected SQL audit and prevent it from being executed.",
                        sql_audit_id,
                    )],
                };
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
                    json!("User requested this SQL review from the Liquid agent workbench.");
            }

            return WorkbenchResponse {
                content: "I found SQL in your message and prepared a SQL audit action for the selected managed database. Confirm the action to create the audit record.".to_owned(),
                actions: vec![WorkbenchActionSuggestion {
                    kind: AgentActionKind::CreateSqlAudit,
                    title: "Create SQL audit".to_owned(),
                    description: "Create a persisted SQL audit using the selected managed database and the existing audit agent.".to_owned(),
                    payload: json!({
                        "managed_database_id": database_id,
                        "request": request,
                    }),
                    resource_kind: Some(AgentResourceKind::SqlAudit),
                    resource_id: None,
                    requires_confirmation: true,
                }],
            };
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

        WorkbenchResponse {
            content: format!(
                "{database_hint}{audit_score} Select a managed database and send SQL when you want me to prepare an audit action. For writes, I will only propose an action and keep execution behind explicit approval."
            ),
            actions: Vec::new(),
        }
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

    Ok(WorkbenchResponse {
        content: message,
        actions,
    })
}

fn workbench_context_payload(context: &LlmWorkbenchContext) -> Result<String> {
    serde_json::to_string_pretty(&json!({
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
        "selected_sql_audit_id": context.selected_sql_audit_id,
        "audit_summary": context.audit_summary,
        "recent_sql_audits": context.recent_sql_audits.iter().map(sql_audit_context).collect::<Vec<_>>(),
        "recent_actions": context.recent_actions.iter().map(action_context).collect::<Vec<_>>(),
    }))
    .context("failed to serialize workbench context")
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

        bail!("LLM workbench response was not valid JSON")
    }
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
            } => {
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
    use liquid_core::{
        AgentActionStatus, AgentMessageRole, ManagedDatabaseEngine, ManagedDatabaseSslMode,
        RiskSeverity, SqlAuditFinding, SqlAuditReport, SqlAuditStatus,
    };

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
                ssl_mode: ManagedDatabaseSslMode::Disable,
                has_password: true,
            }),
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
    fn rejects_invalid_llm_workbench_json() {
        let error = parse_llm_workbench_response("not json", &llm_context()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "LLM workbench response was not valid JSON"
        );
    }
}
