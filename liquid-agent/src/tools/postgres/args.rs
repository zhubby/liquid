use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use liquid_sql::{PgSqlAnalysis, PgSqlAnalysisRequest, PgSqlStatementKind, analyze_postgres_sql};
use serde_json::Value;

use super::config::PostgresToolContext;

pub(super) fn validate_single_statement(
    sql: &str,
    tool_name: &str,
) -> Result<(PgSqlAnalysis, PgSqlStatementKind, String)> {
    let executable_sql = strip_trailing_semicolon(sql);
    let analysis = analyze_postgres_sql(PgSqlAnalysisRequest::new(&executable_sql));

    if let Some(parse_error) = &analysis.parse_error {
        bail!(
            "{tool_name} requires valid PostgreSQL SQL: {}",
            parse_error.message
        );
    }

    if analysis.statements.len() != 1 {
        bail!(
            "{tool_name} requires exactly one statement; got {}",
            analysis.statements.len()
        );
    }

    let statement_kind = analysis.statements[0].kind.clone();
    Ok((analysis, statement_kind, executable_sql))
}

pub(super) fn explain_tool_supported(kind: &PgSqlStatementKind) -> bool {
    matches!(
        kind,
        PgSqlStatementKind::Select
            | PgSqlStatementKind::Insert
            | PgSqlStatementKind::Update
            | PgSqlStatementKind::Delete
            | PgSqlStatementKind::Merge
    )
}

pub(super) fn optional_bool_arg(arguments: &Value, name: &str) -> Result<Option<bool>> {
    match arguments.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_bool()
            .map(Some)
            .ok_or_else(|| anyhow!("{name} must be a boolean")),
    }
}

pub(super) fn optional_string_arg(arguments: &Value, name: &str) -> Result<Option<String>> {
    match arguments.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| Ok(Some(value.to_owned())))
            .unwrap_or_else(|| {
                if value.is_string() {
                    Ok(None)
                } else {
                    Err(anyhow!("{name} must be a string"))
                }
            }),
    }
}

pub(super) fn required_string_arg(
    arguments: &Value,
    name: &str,
    tool_name: &str,
) -> Result<String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("{tool_name} requires a non-empty {name} argument"))
}

pub(super) fn limit_arg(
    arguments: &Value,
    context: &PostgresToolContext,
    tool_name: &str,
) -> Result<usize> {
    match arguments.get("limit") {
        None | Some(Value::Null) => Ok(context.default_limit.min(context.max_limit)),
        Some(value) => {
            let requested = value
                .as_u64()
                .ok_or_else(|| anyhow!("{tool_name} limit must be a positive integer"))?
                as usize;

            Ok(requested.clamp(1, context.max_limit))
        }
    }
}

pub(super) fn relation_kind_codes(arguments: &Value) -> Result<Vec<String>> {
    let Some(value) = arguments.get("kinds") else {
        return Ok(Vec::new());
    };
    let Some(values) = value.as_array() else {
        bail!("kinds must be an array of strings");
    };

    let mut codes = Vec::new();
    for value in values {
        let Some(kind) = value.as_str() else {
            bail!("kinds must be an array of strings");
        };
        let code = match kind.trim().to_ascii_lowercase().as_str() {
            "r" | "table" | "ordinary_table" => "r",
            "p" | "partitioned" | "partitioned_table" => "p",
            "v" | "view" => "v",
            "m" | "matview" | "materialized_view" => "m",
            "f" | "foreign" | "foreign_table" => "f",
            other => bail!("unsupported PostgreSQL relation kind: {other}"),
        };

        if !codes.iter().any(|existing| existing == code) {
            codes.push(code.to_owned());
        }
    }

    Ok(codes)
}

pub(super) fn relation_name(schema: Option<&str>, name: &str) -> String {
    schema
        .map(|schema| format!("{schema}.{name}"))
        .unwrap_or_else(|| name.to_owned())
}

pub(super) fn elapsed_ms(started_at: Instant) -> u64 {
    started_at
        .elapsed()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn strip_trailing_semicolon(sql: &str) -> String {
    sql.trim().trim_end_matches(';').trim().to_owned()
}
