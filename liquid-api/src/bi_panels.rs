use std::time::Instant;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, patch, post},
};
use liquid_core::{
    BiPanel, BiPanelCard, BiPanelExport, BiQueryResult, ManagedDatabasePoolKey,
    UpdateBiPanelCardRequest, UpdateBiPanelLayoutRequest, UpdateBiPanelRequest,
};
use liquid_sql::{PgSqlAnalysisRequest, PgSqlStatementKind, analyze_postgres_sql};
use serde_json::Value;
use sqlx::Row;
use time::OffsetDateTime;

use crate::{auth::authenticated_user, error::ApiError, state::ApiState};

const DEFAULT_BI_QUERY_LIMIT: usize = 100;
const MAX_BI_QUERY_LIMIT: usize = 1_000;

pub(crate) fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/api/v1/chat/conversations/{conversation_id}/bi-panel",
            get(get_conversation_bi_panel).patch(update_conversation_bi_panel),
        )
        .route("/api/v1/bi-panels/{panel_id}/layout", patch(update_layout))
        .route(
            "/api/v1/bi-panels/{panel_id}/cards/{card_id}",
            patch(update_card).delete(delete_card),
        )
        .route(
            "/api/v1/bi-panels/{panel_id}/cards/{card_id}/refresh",
            post(refresh_card),
        )
        .route("/api/v1/bi-panels/{panel_id}/export", get(export_panel))
}

async fn get_conversation_bi_panel(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
) -> Result<Json<BiPanel>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let panel = state
        .store
        .get_or_create_bi_panel(&user.id, &conversation_id)
        .await?;

    Ok(Json(panel))
}

async fn update_conversation_bi_panel(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(conversation_id): Path<String>,
    Json(request): Json<UpdateBiPanelRequest>,
) -> Result<Json<BiPanel>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let panel = state
        .store
        .get_or_create_bi_panel(&user.id, &conversation_id)
        .await?;
    let panel = state
        .store
        .update_bi_panel(&user.id, &panel.id, request)
        .await?;

    Ok(Json(panel))
}

async fn update_layout(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(panel_id): Path<String>,
    Json(request): Json<UpdateBiPanelLayoutRequest>,
) -> Result<Json<BiPanel>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let panel = state
        .store
        .update_bi_panel_layout(&user.id, &panel_id, request.cards)
        .await?;

    Ok(Json(panel))
}

async fn update_card(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((panel_id, card_id)): Path<(String, String)>,
    Json(request): Json<UpdateBiPanelCardRequest>,
) -> Result<Json<BiPanelCard>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let card = state
        .store
        .update_bi_panel_card(&user.id, &panel_id, &card_id, request)
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
        .delete_bi_panel_card(&user.id, &panel_id, &card_id)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn refresh_card(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((panel_id, card_id)): Path<(String, String)>,
) -> Result<Json<BiPanelCard>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let card = state
        .store
        .get_bi_panel_card(&user.id, &panel_id, &card_id)
        .await?;
    let result = materialize_bi_query(
        &state,
        &user.id,
        &card.managed_database_id,
        &card.sql,
        DEFAULT_BI_QUERY_LIMIT,
    )
    .await?;
    let card = state
        .store
        .update_bi_panel_card_result(&user.id, &panel_id, &card_id, result)
        .await?;

    Ok(Json(card))
}

async fn export_panel(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(panel_id): Path<String>,
) -> Result<Json<BiPanelExport>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let export = state.store.export_bi_panel(&user.id, &panel_id).await?;

    Ok(Json(export))
}

pub(crate) async fn materialize_bi_query(
    state: &ApiState,
    owner_user_id: &str,
    managed_database_id: &str,
    sql: &str,
    limit: usize,
) -> Result<BiQueryResult, ApiError> {
    let executable_sql = validate_readonly_select(sql)?;
    let limit = limit.clamp(1, MAX_BI_QUERY_LIMIT);
    let fetch_limit = limit.saturating_add(1).min(MAX_BI_QUERY_LIMIT + 1);
    let pool = state
        .managed_database_pools
        .get_pool(ManagedDatabasePoolKey::new(
            owner_user_id.to_owned(),
            managed_database_id.to_owned(),
        ))
        .await?;
    let started_at = Instant::now();
    let wrapped_sql = format!(
        "select to_jsonb(liquid_row) as row from ({}) liquid_row limit {}",
        executable_sql, fetch_limit
    );
    let mut transaction = pool.begin().await.map_err(|error| {
        ApiError::internal(anyhow::anyhow!(
            "failed to start BI query transaction: {error}"
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
        .map_err(|error| ApiError::bad_request(format!("BI query failed: {error}")))?;

    transaction.rollback().await.map_err(|error| {
        ApiError::internal(anyhow::anyhow!(
            "failed to roll back BI query transaction: {error}"
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

    Ok(BiQueryResult {
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
        return Err(ApiError::bad_request("BI card SQL is required"));
    }

    let analysis = analyze_postgres_sql(PgSqlAnalysisRequest::new(trimmed));

    if analysis.statements.len() != 1 {
        return Err(ApiError::bad_request(
            "BI card SQL must contain exactly one PostgreSQL statement",
        ));
    }

    if !matches!(&analysis.statements[0].kind, PgSqlStatementKind::Select) {
        return Err(ApiError::bad_request(
            "BI card SQL must be a SELECT statement",
        ));
    }

    if analysis
        .findings
        .iter()
        .any(|finding| finding.rule_id == "select_for_locking")
    {
        return Err(ApiError::bad_request(
            "BI card SQL cannot request row locks",
        ));
    }

    Ok(strip_trailing_semicolon(trimmed))
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
