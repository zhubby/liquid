use anyhow::{Result, bail};
use async_trait::async_trait;
use liquid_core::AgentActionKind;
use liquid_llm::{ToolCall, ToolDefinition};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    tools::{AgentTool, ToolRegistry},
    types::ToolOutput,
};

use super::{
    LlmWorkbenchContext,
    actions::{
        DatapanelCardSuggestionInput, datapanel_card_suggestion, sql_audit_llm_action,
        sql_operation_suggestion,
    },
    response::WorkbenchActionSuggestion,
};

const WORKBENCH_PROPOSAL_TOOL_NAMES: &[&str] = &[
    "propose_sql_operation",
    "propose_datapanel_card_action",
    "propose_sql_audit_decision",
];

pub(super) fn register_workbench_proposal_tools(tools: &mut ToolRegistry) {
    tools.register(ProposeSqlOperationTool);
    tools.register(ProposeDatapanelCardActionTool);
    tools.register(ProposeSqlAuditDecisionTool);
}

pub(super) fn workbench_proposal_tool_names() -> Vec<String> {
    let mut names = WORKBENCH_PROPOSAL_TOOL_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

pub(super) fn is_workbench_proposal_tool(name: &str) -> bool {
    WORKBENCH_PROPOSAL_TOOL_NAMES.contains(&name)
}

pub(super) fn proposal_tool_call_to_suggestion(
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
            let args: DatapanelCardSuggestionInput = serde_json::from_str(&call.arguments)?;

            datapanel_card_suggestion(context, args)
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
                        "enum": [
                            "line",
                            "bar",
                            "area",
                            "pie",
                            "scatter",
                            "radar",
                            "radial_bar",
                            "composed",
                            "treemap",
                            "funnel",
                            "sunburst"
                        ]
                    },
                    "x_key": {
                        "type": "string",
                        "description": "Category or x-axis column. Required for line, bar, area, pie, scatter, radar, radial_bar, composed, and funnel charts."
                    },
                    "y_keys": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Metric columns. Required for line, bar, area, pie, scatter, radar, radial_bar, and funnel charts."
                    },
                    "z_key": {
                        "type": "string",
                        "description": "Optional point-size column for scatter charts."
                    },
                    "series": {
                        "type": "array",
                        "description": "Required for composed charts. Each series maps one query result column to line, bar, or area.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "key": { "type": "string" },
                                "kind": {
                                    "type": "string",
                                    "enum": ["line", "bar", "area"]
                                }
                            },
                            "required": ["key", "kind"],
                            "additionalProperties": false
                        }
                    },
                    "group_keys": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Required for treemap and sunburst charts. Ordered columns that form the hierarchy path."
                    },
                    "value_key": {
                        "type": "string",
                        "description": "Required for treemap and sunburst charts. Numeric metric column used for area or arc size."
                    },
                    "limit": { "type": "integer" }
                },
                "required": ["title", "display", "sql"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, arguments: Value) -> Result<ToolOutput> {
        let args: DatapanelCardSuggestionInput = serde_json::from_value(arguments)?;

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
