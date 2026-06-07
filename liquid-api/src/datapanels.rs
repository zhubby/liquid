use std::time::Instant;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, patch, post},
};
use liquid_core::{
    CreateDatapanelCardRequest, Datapanel, DatapanelCard, DatapanelCardKind, DatapanelCardLayout,
    DatapanelExport, DatapanelPreview, DatapanelPreviewLink, DatapanelQueryResult,
    ManagedDatabasePoolKey, SaveDatapanelTableCardRequest, UpdateDatapanelCardRequest,
    UpdateDatapanelLayoutRequest, UpdateDatapanelRequest,
};
use liquid_sql::{PgSqlAnalysisRequest, PgSqlStatementKind, analyze_postgres_sql};
use serde_json::Value;
use sqlx::Row;
use time::OffsetDateTime;

use crate::{auth::authenticated_user, error::ApiError, state::ApiState};

const DEFAULT_DATAPANEL_QUERY_LIMIT: usize = 100;
const MAX_DATAPANEL_QUERY_LIMIT: usize = 1_000;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/api/v1/chat/conversations/{conversation_id}/datapanel",
            get(get_conversation_datapanel).patch(update_conversation_datapanel),
        )
        .route(
            "/api/v1/chat/conversations/{conversation_id}/datapanel/cards",
            post(save_conversation_datapanel_table_card),
        )
        .route("/api/v1/datapanels/{panel_id}/layout", patch(update_layout))
        .route(
            "/api/v1/datapanels/{panel_id}/cards/{card_id}",
            patch(update_card).delete(delete_card),
        )
        .route(
            "/api/v1/datapanels/{panel_id}/cards/{card_id}/refresh",
            post(refresh_card),
        )
        .route("/api/v1/datapanels/{panel_id}/export", get(export_panel))
        .route(
            "/api/v1/datapanels/{panel_id}/preview",
            post(create_preview),
        )
        .route("/api/v1/datapanel-previews/{slug}", get(get_preview))
}

async fn get_conversation_datapanel(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
) -> Result<Json<Datapanel>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let panel = state
        .store
        .get_or_create_datapanel(&user.id, &conversation_id)
        .await?;

    Ok(Json(panel))
}

async fn update_conversation_datapanel(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Json(request): Json<UpdateDatapanelRequest>,
) -> Result<Json<Datapanel>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let panel = state
        .store
        .get_or_create_datapanel(&user.id, &conversation_id)
        .await?;
    let panel = state
        .store
        .update_datapanel(&user.id, &panel.id, request)
        .await?;

    Ok(Json(panel))
}

async fn save_conversation_datapanel_table_card(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Json(request): Json<SaveDatapanelTableCardRequest>,
) -> Result<Json<DatapanelCard>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let sql = validate_readonly_select(&request.sql)?;
    let panel = state
        .store
        .get_or_create_datapanel(&user.id, &conversation_id)
        .await?;
    let card = state
        .store
        .create_datapanel_card(
            &user.id,
            &panel.id,
            CreateDatapanelCardRequest {
                managed_database_id: request.managed_database_id,
                source_action_id: None,
                title: request.title,
                description: request.description,
                kind: DatapanelCardKind::Table,
                sql,
                chart: None,
                layout: next_table_card_layout(&panel),
                result: request.result,
            },
        )
        .await?;

    Ok(Json(card))
}

async fn update_layout(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(panel_id): Path<String>,
    Json(request): Json<UpdateDatapanelLayoutRequest>,
) -> Result<Json<Datapanel>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let panel = state
        .store
        .update_datapanel_layout(&user.id, &panel_id, request.cards)
        .await?;

    Ok(Json(panel))
}

async fn update_card(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((panel_id, card_id)): Path<(String, String)>,
    Json(request): Json<UpdateDatapanelCardRequest>,
) -> Result<Json<DatapanelCard>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let card = state
        .store
        .update_datapanel_card(&user.id, &panel_id, &card_id, request)
        .await?;

    Ok(Json(card))
}

async fn delete_card(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((panel_id, card_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    state
        .store
        .delete_datapanel_card(&user.id, &panel_id, &card_id)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn refresh_card(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((panel_id, card_id)): Path<(String, String)>,
) -> Result<Json<DatapanelCard>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let card = state
        .store
        .get_datapanel_card(&user.id, &panel_id, &card_id)
        .await?;
    let result = materialize_datapanel_query(
        &state,
        &user.id,
        &card.managed_database_id,
        &card.sql,
        DEFAULT_DATAPANEL_QUERY_LIMIT,
    )
    .await?;
    let card = state
        .store
        .update_datapanel_card_result(&user.id, &panel_id, &card_id, result)
        .await?;

    Ok(Json(card))
}

async fn export_panel(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(panel_id): Path<String>,
) -> Result<Json<DatapanelExport>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let export = state.store.export_datapanel(&user.id, &panel_id).await?;

    Ok(Json(export))
}

async fn create_preview(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(panel_id): Path<String>,
) -> Result<Json<DatapanelPreviewLink>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let preview = state
        .store
        .create_datapanel_preview(&user.id, &panel_id)
        .await?;

    Ok(Json(preview))
}

async fn get_preview(
    State(state): State<ApiState>,
    Path(slug): Path<String>,
) -> Result<Json<DatapanelPreview>, ApiError> {
    let preview = state.store.get_datapanel_preview(&slug).await?;

    Ok(Json(preview))
}

pub(crate) async fn materialize_datapanel_query(
    state: &ApiState,
    owner_user_id: &str,
    managed_database_id: &str,
    sql: &str,
    limit: usize,
) -> Result<DatapanelQueryResult, ApiError> {
    let pool = state
        .managed_database_pools
        .get_pool(ManagedDatabasePoolKey::new(
            owner_user_id.to_owned(),
            managed_database_id.to_owned(),
        ))
        .await?;
    materialize_datapanel_query_with_pool(pool, sql, limit).await
}

pub(crate) async fn materialize_datapanel_query_with_pool(
    pool: sqlx::PgPool,
    sql: &str,
    limit: usize,
) -> Result<DatapanelQueryResult, ApiError> {
    let executable_sql = validate_readonly_select(sql)?;
    let limit = limit.clamp(1, MAX_DATAPANEL_QUERY_LIMIT);
    let fetch_limit = limit.saturating_add(1).min(MAX_DATAPANEL_QUERY_LIMIT + 1);
    let started_at = Instant::now();
    let wrapped_sql = format!(
        "select to_jsonb(liquid_row) as row from ({}) liquid_row limit {}",
        executable_sql, fetch_limit
    );
    let mut transaction = pool.begin().await.map_err(|error| {
        ApiError::internal(anyhow::anyhow!(
            "failed to start datapanel query transaction: {error}"
        ))
    })?;

    sqlx::query("set transaction read only")
        .execute(&mut *transaction)
        .await
        .map_err(|error| {
            ApiError::bad_request(format!("failed to mark query read-only: {error}"))
        })?;
    sqlx::query("set local statement_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .map_err(|error| ApiError::bad_request(format!("failed to set query timeout: {error}")))?;

    let rows = sqlx::query(&wrapped_sql)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|error| ApiError::bad_request(format!("datapanel query failed: {error}")))?;

    transaction.rollback().await.map_err(|error| {
        ApiError::internal(anyhow::anyhow!(
            "failed to roll back datapanel query transaction: {error}"
        ))
    })?;

    let mut row_values = rows
        .into_iter()
        .map(|row| row.get::<Value, _>("row"))
        .collect::<Vec<_>>();
    let truncated = row_values.len() > limit;

    if truncated {
        row_values.truncate(limit);
    }

    let row_count = row_values.len() as i32;

    Ok(DatapanelQueryResult {
        columns: json_columns(&row_values),
        rows: row_values,
        row_count,
        truncated,
        elapsed_ms: started_at.elapsed().as_millis().min(i64::MAX as u128) as i64,
        refreshed_at: OffsetDateTime::now_utc(),
    })
}

fn validate_readonly_select(sql: &str) -> Result<String, ApiError> {
    let trimmed = sql.trim();

    if trimmed.is_empty() {
        return Err(ApiError::bad_request("Datapanel card SQL is required"));
    }

    let analysis = analyze_postgres_sql(PgSqlAnalysisRequest::new(trimmed));

    if analysis.statements.len() != 1 {
        return Err(ApiError::bad_request(
            "Datapanel card SQL must contain exactly one PostgreSQL statement",
        ));
    }

    if !matches!(&analysis.statements[0].kind, PgSqlStatementKind::Select) {
        return Err(ApiError::bad_request(
            "Datapanel card SQL must be a SELECT statement",
        ));
    }

    if analysis
        .findings
        .iter()
        .any(|finding| finding.rule_id == "select_for_locking")
    {
        return Err(ApiError::bad_request(
            "Datapanel card SQL cannot request row locks",
        ));
    }

    Ok(strip_trailing_semicolon(trimmed))
}

fn next_table_card_layout(panel: &Datapanel) -> DatapanelCardLayout {
    DatapanelCardLayout {
        x: 0,
        y: panel
            .cards
            .iter()
            .map(|card| card.layout.y + card.layout.h)
            .max()
            .unwrap_or(0),
        w: 12,
        h: 5,
    }
}

fn strip_trailing_semicolon(sql: &str) -> String {
    sql.trim_end()
        .strip_suffix(';')
        .unwrap_or(sql)
        .trim()
        .to_owned()
}

fn json_columns(rows: &[Value]) -> Vec<String> {
    let mut columns = Vec::new();

    for row in rows {
        let Some(object) = row.as_object() else {
            continue;
        };

        for key in object.keys() {
            if !columns.iter().any(|column| column == key) {
                columns.push(key.clone());
            }
        }
    }

    columns
}
