use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Datapanel {
    pub id: String,
    pub conversation_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    pub cards: Vec<DatapanelCard>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatapanelCard {
    pub id: String,
    pub panel_id: String,
    pub managed_database_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_action_id: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    pub kind: DatapanelCardKind,
    pub sql: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub chart: Option<DatapanelChartConfig>,
    pub layout: DatapanelCardLayout,
    pub result: DatapanelQueryResult,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DatapanelCardKind {
    Table,
    Chart,
}

impl DatapanelCardKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Chart => "chart",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DatapanelChartType {
    Line,
    Bar,
    Area,
    Pie,
    Scatter,
    Radar,
    RadialBar,
    Composed,
    Treemap,
    Funnel,
    Sunburst,
}

impl DatapanelChartType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Bar => "bar",
            Self::Area => "area",
            Self::Pie => "pie",
            Self::Scatter => "scatter",
            Self::Radar => "radar",
            Self::RadialBar => "radial_bar",
            Self::Composed => "composed",
            Self::Treemap => "treemap",
            Self::Funnel => "funnel",
            Self::Sunburst => "sunburst",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DatapanelChartSeriesKind {
    Line,
    Bar,
    Area,
}

impl DatapanelChartSeriesKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Bar => "bar",
            Self::Area => "area",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatapanelChartSeries {
    pub key: String,
    pub kind: DatapanelChartSeriesKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatapanelChartConfig {
    pub chart_type: DatapanelChartType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub x_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub y_keys: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub z_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub series: Option<Vec<DatapanelChartSeries>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub group_keys: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub value_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatapanelCardLayout {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatapanelQueryResult {
    pub columns: Vec<String>,
    #[ts(type = "Array<Record<string, unknown>>")]
    pub rows: Vec<Value>,
    pub row_count: i32,
    pub truncated: bool,
    pub elapsed_ms: i64,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub refreshed_at: OffsetDateTime,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateDatapanelRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateDatapanelCardRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateDatapanelLayoutRequest {
    pub cards: Vec<DatapanelCardLayoutUpdate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatapanelCardLayoutUpdate {
    pub card_id: String,
    pub layout: DatapanelCardLayout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateDatapanelCardRequest {
    pub managed_database_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_action_id: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    pub kind: DatapanelCardKind,
    pub sql: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub chart: Option<DatapanelChartConfig>,
    pub layout: DatapanelCardLayout,
    pub result: DatapanelQueryResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SaveDatapanelTableCardRequest {
    pub managed_database_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    pub sql: String,
    pub result: DatapanelQueryResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatapanelExport {
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub exported_at: OffsetDateTime,
    pub panel: Datapanel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_config_deserializes_existing_xy_shape() {
        let chart = serde_json::from_value::<DatapanelChartConfig>(serde_json::json!({
            "chart_type": "line",
            "x_key": "day",
            "y_keys": ["risk_count"]
        }))
        .unwrap();

        assert_eq!(chart.chart_type, DatapanelChartType::Line);
        assert_eq!(chart.x_key.as_deref(), Some("day"));
        assert_eq!(
            chart.y_keys.as_deref(),
            Some(&["risk_count".to_owned()][..])
        );
        assert!(chart.series.is_none());
        assert!(chart.group_keys.is_none());
    }

    #[test]
    fn chart_config_deserializes_hierarchy_shape_without_xy_keys() {
        let chart = serde_json::from_value::<DatapanelChartConfig>(serde_json::json!({
            "chart_type": "sunburst",
            "group_keys": ["region", "product"],
            "value_key": "revenue"
        }))
        .unwrap();

        assert_eq!(chart.chart_type, DatapanelChartType::Sunburst);
        assert!(chart.x_key.is_none());
        assert!(chart.y_keys.is_none());
        assert_eq!(
            chart.group_keys.as_deref(),
            Some(&["region".to_owned(), "product".to_owned()][..])
        );
        assert_eq!(chart.value_key.as_deref(), Some("revenue"));
    }
}
