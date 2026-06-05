use liquid_core::{AgentActionKind, AgentResourceKind};
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct WorkbenchContext {
    pub message: String,
    pub managed_database_id: Option<String>,
    pub selected_sql_audit_id: Option<String>,
    pub managed_database_count: usize,
    pub audit_score: Option<u8>,
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
}
