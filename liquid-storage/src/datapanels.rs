use liquid_core::{
    CreateDatapanelCardRequest, Datapanel, DatapanelCard, DatapanelCardKind, DatapanelCardLayout,
    DatapanelCardLayoutUpdate, DatapanelChartConfig, DatapanelChartType, DatapanelExport,
    DatapanelQueryResult, UpdateDatapanelCardRequest, UpdateDatapanelRequest,
};
use serde_json::Value;
use time::OffsetDateTime;

use crate::{
    error::{StorageError, map_database_error},
    store::Storage,
    validation::required_string,
};

const DATAPANEL_COLUMNS: &str = r#"
id::text,
conversation_id::text,
title,
description,
created_at,
updated_at
"#;

const DATAPANEL_CARD_COLUMNS: &str = r#"
id::text,
panel_id::text,
managed_database_id::text,
source_action_id::text,
title,
description,
kind,
sql,
chart,
layout,
result,
created_at,
updated_at
"#;

pub(crate) async fn get_or_create_datapanel(
    storage: &Storage,
    owner_user_id: &str,
    conversation_id: &str,
) -> Result<Datapanel, StorageError> {
    let row = sqlx::query_as::<_, DatapanelRow>(&format!(
        r#"
        insert into datapanels (conversation_id, owner_user_id, title)
        select id, owner_user_id, title || ' Datapanel'
        from agent_conversations
        where id = $2::uuid
          and owner_user_id = $1::uuid
        on conflict (conversation_id) do update
        set updated_at = datapanels.updated_at
        returning {DATAPANEL_COLUMNS}
        "#
    ))
    .bind(owner_user_id)
    .bind(conversation_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    let panel = row.ok_or(StorageError::NotFound)?.try_into()?;
    panel_with_cards(storage, owner_user_id, panel).await
}

pub(crate) async fn update_datapanel(
    storage: &Storage,
    owner_user_id: &str,
    panel_id: &str,
    request: UpdateDatapanelRequest,
) -> Result<Datapanel, StorageError> {
    let title = request
        .title
        .map(|value| required_string("title", &value))
        .transpose()?;
    let description_present = request.description.is_some();
    let description = blank_to_none(request.description);
    let row = sqlx::query_as::<_, DatapanelRow>(&format!(
        r#"
        update datapanels
        set title = coalesce($3, title),
            description = case when $4 then $5 else description end,
            updated_at = now()
        where id = $1::uuid
          and owner_user_id = $2::uuid
        returning {DATAPANEL_COLUMNS}
        "#
    ))
    .bind(panel_id)
    .bind(owner_user_id)
    .bind(title)
    .bind(description_present)
    .bind(description)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    let panel = row.ok_or(StorageError::NotFound)?.try_into()?;
    panel_with_cards(storage, owner_user_id, panel).await
}

pub(crate) async fn create_datapanel_card(
    storage: &Storage,
    owner_user_id: &str,
    panel_id: &str,
    request: CreateDatapanelCardRequest,
) -> Result<DatapanelCard, StorageError> {
    validate_card_request(&request)?;
    let title = required_string("title", &request.title)?;
    let description = blank_to_none(request.description);
    let layout = checked_json("layout", &request.layout)?;
    let chart = checked_optional_json("chart", &request.chart)?;
    let result = checked_json("result", &request.result)?;
    let row = sqlx::query_as::<_, DatapanelCardRow>(&format!(
        r#"
        insert into datapanel_cards (
            panel_id,
            owner_user_id,
            managed_database_id,
            source_action_id,
            title,
            description,
            kind,
            sql,
            chart,
            layout,
            result
        )
        select
            p.id,
            p.owner_user_id,
            $3::uuid,
            $4::uuid,
            $5,
            $6,
            $7,
            $8,
            $9,
            $10,
            $11
        from datapanels p
        join managed_databases d
          on d.id = $3::uuid
         and d.owner_user_id = p.owner_user_id
        where p.id = $2::uuid
          and p.owner_user_id = $1::uuid
        returning {DATAPANEL_CARD_COLUMNS}
        "#
    ))
    .bind(owner_user_id)
    .bind(panel_id)
    .bind(request.managed_database_id)
    .bind(request.source_action_id)
    .bind(title)
    .bind(description)
    .bind(request.kind.as_str())
    .bind(required_string("sql", &request.sql)?)
    .bind(chart)
    .bind(layout)
    .bind(result)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

pub(crate) async fn get_datapanel_card(
    storage: &Storage,
    owner_user_id: &str,
    panel_id: &str,
    card_id: &str,
) -> Result<DatapanelCard, StorageError> {
    fetch_card(storage, owner_user_id, panel_id, card_id).await
}

pub(crate) async fn update_datapanel_card(
    storage: &Storage,
    owner_user_id: &str,
    panel_id: &str,
    card_id: &str,
    request: UpdateDatapanelCardRequest,
) -> Result<DatapanelCard, StorageError> {
    let title = request
        .title
        .map(|value| required_string("title", &value))
        .transpose()?;
    let description_present = request.description.is_some();
    let description = blank_to_none(request.description);
    let row = sqlx::query_as::<_, DatapanelCardRow>(&format!(
        r#"
        update datapanel_cards
        set title = coalesce($4, title),
            description = case when $5 then $6 else description end,
            updated_at = now()
        where id = $3::uuid
          and panel_id = $2::uuid
          and owner_user_id = $1::uuid
        returning {DATAPANEL_CARD_COLUMNS}
        "#
    ))
    .bind(owner_user_id)
    .bind(panel_id)
    .bind(card_id)
    .bind(title)
    .bind(description_present)
    .bind(description)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

pub(crate) async fn update_datapanel_layout(
    storage: &Storage,
    owner_user_id: &str,
    panel_id: &str,
    layouts: Vec<DatapanelCardLayoutUpdate>,
) -> Result<Datapanel, StorageError> {
    let mut transaction = storage.pool.begin().await.map_err(map_database_error)?;

    for update in layouts {
        validate_layout(&update.layout)?;
        let layout = checked_json("layout", &update.layout)?;
        let result = sqlx::query(
            r#"
            update datapanel_cards
            set layout = $4,
                updated_at = now()
            where owner_user_id = $1::uuid
              and panel_id = $2::uuid
              and id = $3::uuid
            "#,
        )
        .bind(owner_user_id)
        .bind(panel_id)
        .bind(update.card_id)
        .bind(layout)
        .execute(&mut *transaction)
        .await
        .map_err(map_database_error)?;

        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound);
        }
    }

    sqlx::query(
        r#"
        update datapanels
        set updated_at = now()
        where id = $2::uuid
          and owner_user_id = $1::uuid
        "#,
    )
    .bind(owner_user_id)
    .bind(panel_id)
    .execute(&mut *transaction)
    .await
    .map_err(map_database_error)?;

    transaction.commit().await.map_err(map_database_error)?;
    fetch_panel(storage, owner_user_id, panel_id).await
}

pub(crate) async fn update_datapanel_card_result(
    storage: &Storage,
    owner_user_id: &str,
    panel_id: &str,
    card_id: &str,
    result: DatapanelQueryResult,
) -> Result<DatapanelCard, StorageError> {
    let result = checked_json("result", &result)?;
    let row = sqlx::query_as::<_, DatapanelCardRow>(&format!(
        r#"
        update datapanel_cards
        set result = $4,
            updated_at = now()
        where id = $3::uuid
          and panel_id = $2::uuid
          and owner_user_id = $1::uuid
        returning {DATAPANEL_CARD_COLUMNS}
        "#
    ))
    .bind(owner_user_id)
    .bind(panel_id)
    .bind(card_id)
    .bind(result)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

pub(crate) async fn delete_datapanel_card(
    storage: &Storage,
    owner_user_id: &str,
    panel_id: &str,
    card_id: &str,
) -> Result<(), StorageError> {
    let result = sqlx::query(
        r#"
        delete from datapanel_cards
        where id = $3::uuid
          and panel_id = $2::uuid
          and owner_user_id = $1::uuid
        "#,
    )
    .bind(owner_user_id)
    .bind(panel_id)
    .bind(card_id)
    .execute(&storage.pool)
    .await
    .map_err(map_database_error)?;

    if result.rows_affected() == 0 {
        return Err(StorageError::NotFound);
    }

    Ok(())
}

pub(crate) async fn export_datapanel(
    storage: &Storage,
    owner_user_id: &str,
    panel_id: &str,
) -> Result<DatapanelExport, StorageError> {
    Ok(DatapanelExport {
        exported_at: OffsetDateTime::now_utc(),
        panel: fetch_panel(storage, owner_user_id, panel_id).await?,
    })
}

async fn fetch_panel(
    storage: &Storage,
    owner_user_id: &str,
    panel_id: &str,
) -> Result<Datapanel, StorageError> {
    let row = sqlx::query_as::<_, DatapanelRow>(&format!(
        r#"
        select {DATAPANEL_COLUMNS}
        from datapanels
        where id = $2::uuid
          and owner_user_id = $1::uuid
        "#
    ))
    .bind(owner_user_id)
    .bind(panel_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    let panel = row.ok_or(StorageError::NotFound)?.try_into()?;
    panel_with_cards(storage, owner_user_id, panel).await
}

async fn panel_with_cards(
    storage: &Storage,
    owner_user_id: &str,
    mut panel: Datapanel,
) -> Result<Datapanel, StorageError> {
    panel.cards = list_cards(storage, owner_user_id, &panel.id).await?;
    Ok(panel)
}

async fn list_cards(
    storage: &Storage,
    owner_user_id: &str,
    panel_id: &str,
) -> Result<Vec<DatapanelCard>, StorageError> {
    let rows = sqlx::query_as::<_, DatapanelCardRow>(&format!(
        r#"
        select {DATAPANEL_CARD_COLUMNS}
        from datapanel_cards
        where owner_user_id = $1::uuid
          and panel_id = $2::uuid
        order by (layout->>'y')::int, (layout->>'x')::int, created_at
        "#
    ))
    .bind(owner_user_id)
    .bind(panel_id)
    .fetch_all(&storage.pool)
    .await
    .map_err(map_database_error)?;

    rows.into_iter().map(DatapanelCard::try_from).collect()
}

async fn fetch_card(
    storage: &Storage,
    owner_user_id: &str,
    panel_id: &str,
    card_id: &str,
) -> Result<DatapanelCard, StorageError> {
    let row = sqlx::query_as::<_, DatapanelCardRow>(&format!(
        r#"
        select {DATAPANEL_CARD_COLUMNS}
        from datapanel_cards
        where owner_user_id = $1::uuid
          and panel_id = $2::uuid
          and id = $3::uuid
        "#
    ))
    .bind(owner_user_id)
    .bind(panel_id)
    .bind(card_id)
    .fetch_optional(&storage.pool)
    .await
    .map_err(map_database_error)?;

    row.ok_or(StorageError::NotFound)?.try_into()
}

fn validate_card_request(request: &CreateDatapanelCardRequest) -> Result<(), StorageError> {
    required_string("sql", &request.sql)?;
    validate_layout(&request.layout)?;

    if request.kind == DatapanelCardKind::Chart {
        let chart = request.chart.as_ref().ok_or_else(|| {
            StorageError::Validation("chart cards require chart configuration".to_owned())
        })?;
        validate_chart(chart)?;
    }

    Ok(())
}

fn validate_layout(layout: &DatapanelCardLayout) -> Result<(), StorageError> {
    if layout.x < 0 || layout.y < 0 || layout.w <= 0 || layout.h <= 0 || layout.w > 12 {
        return Err(StorageError::Validation(
            "invalid Datapanel card layout".to_owned(),
        ));
    }

    Ok(())
}

fn validate_chart(chart: &DatapanelChartConfig) -> Result<(), StorageError> {
    let y_keys = chart.y_keys.as_deref().unwrap_or(&[]);
    let series = chart.series.as_deref().unwrap_or(&[]);
    let group_keys = chart.group_keys.as_deref().unwrap_or(&[]);

    match chart.chart_type {
        DatapanelChartType::Line
        | DatapanelChartType::Bar
        | DatapanelChartType::Area
        | DatapanelChartType::Pie
        | DatapanelChartType::Scatter
        | DatapanelChartType::Radar
        | DatapanelChartType::RadialBar
        | DatapanelChartType::Funnel => {
            required_chart_key("x_key", chart.x_key.as_ref())?;
            required_chart_keys("y_key", y_keys)?;
        }
        DatapanelChartType::Composed => {
            required_chart_key("x_key", chart.x_key.as_ref())?;

            if series.is_empty() {
                return Err(StorageError::Validation(
                    "chart cards require at least one series".to_owned(),
                ));
            }
        }
        DatapanelChartType::Treemap | DatapanelChartType::Sunburst => {
            required_chart_keys("group_key", group_keys)?;
            required_chart_key("value_key", chart.value_key.as_ref())?;
        }
    }

    if let Some(z_key) = &chart.z_key {
        required_string("z_key", z_key)?;
    }

    for key in y_keys {
        required_string("y_key", key)?;
    }

    for item in series {
        required_string("series.key", &item.key)?;
    }

    for key in group_keys {
        required_string("group_key", key)?;
    }

    Ok(())
}

fn required_chart_key(field: &str, value: Option<&String>) -> Result<(), StorageError> {
    match value {
        Some(value) => required_string(field, value).map(|_| ()),
        None => Err(StorageError::Validation(format!(
            "chart cards require {field}"
        ))),
    }
}

fn required_chart_keys(field: &str, values: &[String]) -> Result<(), StorageError> {
    if values.is_empty() {
        return Err(StorageError::Validation(format!(
            "chart cards require at least one {field}"
        )));
    }

    for value in values {
        required_string(field, value)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use liquid_core::{DatapanelChartSeries, DatapanelChartSeriesKind};

    use super::*;

    #[test]
    fn validate_chart_accepts_composed_series() {
        let chart = DatapanelChartConfig {
            chart_type: DatapanelChartType::Composed,
            x_key: Some("day".to_owned()),
            y_keys: Some(vec!["revenue".to_owned(), "cost".to_owned()]),
            z_key: None,
            series: Some(vec![
                DatapanelChartSeries {
                    key: "revenue".to_owned(),
                    kind: DatapanelChartSeriesKind::Bar,
                },
                DatapanelChartSeries {
                    key: "cost".to_owned(),
                    kind: DatapanelChartSeriesKind::Line,
                },
            ]),
            group_keys: None,
            value_key: None,
        };

        validate_chart(&chart).unwrap();
    }

    #[test]
    fn validate_chart_accepts_hierarchy_config() {
        let chart = DatapanelChartConfig {
            chart_type: DatapanelChartType::Sunburst,
            x_key: None,
            y_keys: None,
            z_key: None,
            series: None,
            group_keys: Some(vec!["region".to_owned(), "product".to_owned()]),
            value_key: Some("revenue".to_owned()),
        };

        validate_chart(&chart).unwrap();
    }

    #[test]
    fn validate_chart_rejects_composed_without_series() {
        let chart = DatapanelChartConfig {
            chart_type: DatapanelChartType::Composed,
            x_key: Some("day".to_owned()),
            y_keys: Some(vec!["revenue".to_owned()]),
            z_key: None,
            series: None,
            group_keys: None,
            value_key: None,
        };

        let error = validate_chart(&chart).unwrap_err();

        assert_eq!(error.to_string(), "chart cards require at least one series");
    }

    #[test]
    fn validate_chart_rejects_hierarchy_without_value_key() {
        let chart = DatapanelChartConfig {
            chart_type: DatapanelChartType::Treemap,
            x_key: None,
            y_keys: None,
            z_key: None,
            series: None,
            group_keys: Some(vec!["region".to_owned(), "product".to_owned()]),
            value_key: None,
        };

        let error = validate_chart(&chart).unwrap_err();

        assert_eq!(error.to_string(), "chart cards require value_key");
    }
}

fn checked_json<T: serde::Serialize>(field: &str, value: &T) -> Result<Value, StorageError> {
    serde_json::to_value(value)
        .map_err(|error| StorageError::Validation(format!("{field} is invalid: {error}")))
}

fn checked_optional_json<T: serde::Serialize>(
    field: &str,
    value: &Option<T>,
) -> Result<Option<Value>, StorageError> {
    value
        .as_ref()
        .map(|value| checked_json(field, value))
        .transpose()
}

fn blank_to_none(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[derive(sqlx::FromRow)]
struct DatapanelRow {
    id: String,
    conversation_id: String,
    title: String,
    description: Option<String>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
struct DatapanelCardRow {
    id: String,
    panel_id: String,
    managed_database_id: String,
    source_action_id: Option<String>,
    title: String,
    description: Option<String>,
    kind: String,
    sql: String,
    chart: Option<Value>,
    layout: Value,
    result: Value,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl TryFrom<DatapanelRow> for Datapanel {
    type Error = StorageError;

    fn try_from(row: DatapanelRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            conversation_id: row.conversation_id,
            title: row.title,
            description: row.description,
            cards: Vec::new(),
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

impl TryFrom<DatapanelCardRow> for DatapanelCard {
    type Error = StorageError;

    fn try_from(row: DatapanelCardRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            panel_id: row.panel_id,
            managed_database_id: row.managed_database_id,
            source_action_id: row.source_action_id,
            title: row.title,
            description: row.description,
            kind: parse_card_kind(&row.kind)?,
            sql: row.sql,
            chart: row
                .chart
                .map(serde_json::from_value)
                .transpose()
                .map_err(json_error)?,
            layout: serde_json::from_value(row.layout).map_err(json_error)?,
            result: serde_json::from_value(row.result).map_err(json_error)?,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

fn parse_card_kind(value: &str) -> Result<DatapanelCardKind, StorageError> {
    match value {
        "table" => Ok(DatapanelCardKind::Table),
        "chart" => Ok(DatapanelCardKind::Chart),
        other => Err(StorageError::Validation(format!(
            "unsupported Datapanel card kind: {other}"
        ))),
    }
}

fn json_error(error: serde_json::Error) -> StorageError {
    StorageError::Validation(error.to_string())
}
