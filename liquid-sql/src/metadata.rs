pub mod explain;
pub mod runtime;
pub mod schema;

use async_trait::async_trait;
use pg_query::{NodeEnum, protobuf::RawStmt};

use crate::{
    analysis,
    metadata::{explain::inspect_explain, runtime::inspect_runtime, schema::inspect_schema},
    types::{
        PgSqlAnalysis, PgSqlAnalysisRequest, PgSqlFinding, PgSqlMetadataOptions,
        PgSqlMetadataReport, PgSqlMetadataStatus, PgSqlRiskSeverity, PgSqlStatementMetadata,
    },
};

#[async_trait]
pub trait PgSqlMetadataProvider: Send + Sync {
    async fn metadata_for_statement(
        &self,
        request: PgSqlStatementMetadataRequest<'_>,
    ) -> Result<PgSqlStatementMetadata, PgSqlMetadataError>;
}

#[derive(Debug, Clone, Copy)]
pub struct PgSqlStatementMetadataRequest<'a> {
    pub statement_index: usize,
    pub sql: &'a str,
    pub node: &'a NodeEnum,
    pub options: &'a PgSqlMetadataOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgSqlMetadataError {
    pub message: String,
}

impl PgSqlMetadataError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PgSqlMetadataError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PgSqlMetadataError {}

pub async fn analyze_postgres_sql_with_metadata<P>(
    request: PgSqlAnalysisRequest,
    provider: &P,
    options: PgSqlMetadataOptions,
) -> PgSqlAnalysis
where
    P: PgSqlMetadataProvider + ?Sized,
{
    let sql = request.sql.trim().to_owned();
    let mut analysis = analysis::analyze_postgres_sql(request);

    if !options.enabled || !analysis.parse_ok() {
        return analysis;
    }

    let parsed = match pg_query::parse(&sql) {
        Ok(parsed) => parsed,
        Err(_) => return analysis,
    };

    let mut report = PgSqlMetadataReport {
        status: PgSqlMetadataStatus::Available,
        warnings: Vec::new(),
        statements: Vec::new(),
    };

    for (index, raw_stmt) in parsed.protobuf.stmts.iter().enumerate() {
        let Some(node) = raw_stmt.stmt.as_deref().and_then(|stmt| stmt.node.as_ref()) else {
            continue;
        };

        match provider
            .metadata_for_statement(PgSqlStatementMetadataRequest {
                statement_index: index,
                sql: statement_sql(&sql, raw_stmt),
                node,
                options: &options,
            })
            .await
        {
            Ok(statement_metadata) => {
                inspect_schema(index, node, &statement_metadata, &options, &mut analysis);
                inspect_explain(index, node, &statement_metadata, &options, &mut analysis);
                inspect_runtime(index, node, &statement_metadata, &options, &mut analysis);
                report.statements.push(statement_metadata);
            }
            Err(error) => {
                report.status = PgSqlMetadataStatus::Partial;
                report.warnings.push(error.message.clone());
                analysis.findings.push(PgSqlFinding::new(
                    "metadata_unavailable",
                    PgSqlRiskSeverity::Low,
                    "Metadata unavailable",
                    "PostgreSQL metadata could not be collected for this statement.",
                    Some(index),
                    Some(error.message),
                ));
            }
        }
    }

    if report.statements.is_empty() && !report.warnings.is_empty() {
        report.status = PgSqlMetadataStatus::Unavailable;
    }

    analysis.metadata = Some(report);
    analysis
}

fn statement_sql<'a>(sql: &'a str, raw_stmt: &RawStmt) -> &'a str {
    let start = usize::try_from(raw_stmt.stmt_location)
        .ok()
        .filter(|start| *start < sql.len())
        .unwrap_or(0);
    let length = usize::try_from(raw_stmt.stmt_len).ok().unwrap_or(0);

    if length == 0 {
        sql[start..].trim()
    } else {
        let end = start.saturating_add(length).min(sql.len());
        sql[start..end].trim()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;

    use async_trait::async_trait;

    use super::*;
    use crate::types::PgSqlStatementMetadata;

    #[derive(Default)]
    pub(crate) struct MockMetadataProvider {
        pub statements: BTreeMap<usize, PgSqlStatementMetadata>,
        pub error: Option<PgSqlMetadataError>,
    }

    #[async_trait]
    impl PgSqlMetadataProvider for MockMetadataProvider {
        async fn metadata_for_statement(
            &self,
            request: PgSqlStatementMetadataRequest<'_>,
        ) -> Result<PgSqlStatementMetadata, PgSqlMetadataError> {
            if let Some(error) = &self.error {
                return Err(error.clone());
            }

            Ok(self
                .statements
                .get(&request.statement_index)
                .cloned()
                .unwrap_or_else(|| PgSqlStatementMetadata::new(request.statement_index)))
        }
    }
}
