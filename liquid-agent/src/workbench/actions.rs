use anyhow::{Result, bail};
use liquid_core::{
    AgentActionKind, AgentResourceKind, DatapanelCardKind, DatapanelChartConfig,
    DatapanelChartSeries, DatapanelChartType,
};
use serde::Deserialize;
use serde_json::json;

use super::{LlmWorkbenchContext, prompt::known_sql_audit_id, response::WorkbenchActionSuggestion};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DatapanelCardSuggestionInput {
    pub(super) title: String,
    #[serde(default)]
    pub(super) description: Option<String>,
    pub(super) display: DatapanelCardKind,
    pub(super) sql: String,
    #[serde(default)]
    pub(super) chart_type: Option<DatapanelChartType>,
    #[serde(default)]
    pub(super) x_key: Option<String>,
    #[serde(default)]
    pub(super) y_keys: Vec<String>,
    #[serde(default)]
    pub(super) z_key: Option<String>,
    #[serde(default)]
    pub(super) series: Vec<DatapanelChartSeries>,
    #[serde(default)]
    pub(super) group_keys: Vec<String>,
    #[serde(default)]
    pub(super) value_key: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<usize>,
}

pub(super) fn database_diagram_suggestion(
    context: &LlmWorkbenchContext,
    title: String,
    description: Option<String>,
) -> Result<WorkbenchActionSuggestion> {
    let Some(database) = context.managed_database.as_ref() else {
        bail!("create_database_diagram requires a selected managed database");
    };
    let title = required_trimmed("title", title)?;
    let description = optional_trimmed(description);

    Ok(WorkbenchActionSuggestion {
        kind: AgentActionKind::CreateDatabaseDiagram,
        title: title.clone(),
        description: description.clone().unwrap_or_else(|| {
            "Create a database design from the selected database catalog.".to_owned()
        }),
        payload: json!({
            "managed_database_id": database.id,
            "managed_database_name": database.name,
            "title": title,
            "description": description,
        }),
        resource_kind: Some(AgentResourceKind::DatabaseDiagram),
        resource_id: None,
        requires_confirmation: true,
    })
}

pub(super) fn sql_operation_suggestion(
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

pub(super) fn datapanel_card_suggestion(
    context: &LlmWorkbenchContext,
    input: DatapanelCardSuggestionInput,
) -> Result<WorkbenchActionSuggestion> {
    let Some(database_id) = context
        .managed_database
        .as_ref()
        .map(|database| &database.id)
    else {
        bail!("create_datapanel_card requires a selected managed database");
    };
    let DatapanelCardSuggestionInput {
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
    } = input;
    let sql = required_trimmed("sql", sql)?;
    let title = required_trimmed("title", title)?;
    let description = optional_trimmed(description);
    let chart = match display {
        DatapanelCardKind::Table => None,
        DatapanelCardKind::Chart => Some(datapanel_chart_config(
            chart_type, x_key, y_keys, z_key, series, group_keys, value_key,
        )?),
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

fn datapanel_chart_config(
    chart_type: Option<DatapanelChartType>,
    x_key: Option<String>,
    y_keys: Vec<String>,
    z_key: Option<String>,
    series: Vec<DatapanelChartSeries>,
    group_keys: Vec<String>,
    value_key: Option<String>,
) -> Result<DatapanelChartConfig> {
    let chart_type = chart_type.ok_or_else(|| anyhow::anyhow!("chart_type is required"))?;
    let x_key = optional_trimmed(x_key);
    let mut y_keys = trimmed_keys("y_key", y_keys)?;
    let z_key = optional_trimmed(z_key);
    let mut series = trimmed_series(series)?;
    let group_keys = trimmed_keys("group_key", group_keys)?;
    let value_key = optional_trimmed(value_key);

    match chart_type {
        DatapanelChartType::Line
        | DatapanelChartType::Bar
        | DatapanelChartType::Area
        | DatapanelChartType::Pie
        | DatapanelChartType::Radar
        | DatapanelChartType::RadialBar
        | DatapanelChartType::Funnel => {
            require_config_key("x_key", x_key.as_deref())?;
            require_config_keys("y_keys", &y_keys)?;
            series.clear();
        }
        DatapanelChartType::Scatter => {
            require_config_key("x_key", x_key.as_deref())?;
            require_config_keys("y_keys", &y_keys)?;
            series.clear();
        }
        DatapanelChartType::Composed => {
            require_config_key("x_key", x_key.as_deref())?;

            if series.is_empty() {
                bail!("series is required");
            }

            if y_keys.is_empty() {
                y_keys = series.iter().map(|item| item.key.clone()).collect();
            }
        }
        DatapanelChartType::Treemap | DatapanelChartType::Sunburst => {
            require_config_keys("group_keys", &group_keys)?;
            require_config_key("value_key", value_key.as_deref())?;
            y_keys.clear();
            series.clear();
        }
    }

    Ok(DatapanelChartConfig {
        chart_type,
        x_key,
        y_keys: non_empty_vec(y_keys),
        z_key,
        series: non_empty_vec(series),
        group_keys: non_empty_vec(group_keys),
        value_key,
    })
}

pub(super) fn sql_audit_llm_action(
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

pub(super) fn sql_audit_action(
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

pub(super) fn required_trimmed(field: &str, value: String) -> Result<String> {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        bail!("{field} is required");
    }

    Ok(trimmed.to_owned())
}

pub(super) fn optional_trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn non_empty_vec<T>(values: Vec<T>) -> Option<Vec<T>> {
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn trimmed_keys(field: &str, values: Vec<String>) -> Result<Vec<String>> {
    values
        .into_iter()
        .map(|value| required_trimmed(field, value))
        .collect::<Result<Vec<_>>>()
}

fn trimmed_series(values: Vec<DatapanelChartSeries>) -> Result<Vec<DatapanelChartSeries>> {
    values
        .into_iter()
        .map(|value| {
            Ok(DatapanelChartSeries {
                key: required_trimmed("series.key", value.key)?,
                kind: value.kind,
            })
        })
        .collect::<Result<Vec<_>>>()
}

fn require_config_key(field: &str, value: Option<&str>) -> Result<()> {
    if value.is_none() {
        bail!("{field} is required");
    }

    Ok(())
}

fn require_config_keys(field: &str, keys: &[String]) -> Result<()> {
    if keys.is_empty() {
        bail!("{field} is required");
    }

    Ok(())
}
