use anyhow::{Result, bail};
use liquid_core::{
    AgentActionKind, AgentResourceKind, DatapanelCardKind, DatapanelChartSeries, DatapanelChartType,
};
use serde::Deserialize;
use serde_json::Value;

use crate::types::ToolOutput;

use super::{
    LlmWorkbenchContext,
    actions::{
        DatapanelCardSuggestionInput, database_diagram_suggestion, datapanel_card_suggestion,
        required_trimmed, sql_audit_llm_action, sql_operation_suggestion,
    },
};

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
    pub(super) fn new(content: String, actions: Vec<WorkbenchActionSuggestion>) -> Self {
        let waiting_for_user = !actions.is_empty();

        Self {
            content,
            actions,
            tool_steps: Vec::new(),
            waiting_for_user,
        }
    }

    pub(super) fn with_tool_steps(mut self, tool_steps: Vec<WorkbenchToolStep>) -> Self {
        self.tool_steps = tool_steps;
        self.waiting_for_user = !self.actions.is_empty();
        self
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
        z_key: Option<String>,
        #[serde(default)]
        series: Vec<DatapanelChartSeries>,
        #[serde(default)]
        group_keys: Vec<String>,
        #[serde(default)]
        value_key: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
    },
    CreateDatabaseDiagram {
        title: String,
        #[serde(default)]
        description: Option<String>,
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
                z_key,
                series,
                group_keys,
                value_key,
                limit,
            } => datapanel_card_suggestion(
                context,
                DatapanelCardSuggestionInput {
                    title,
                    description,
                    display,
                    sql,
                    chart_type,
                    x_key,
                    y_keys,
                    z_key,
                    series,
                    group_keys,
                    value_key,
                    limit,
                },
            ),
            Self::CreateDatabaseDiagram { title, description } => {
                database_diagram_suggestion(context, title, description)
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
