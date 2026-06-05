mod analysis;
mod ast;
mod metadata;
mod postgres;
mod rules;
mod types;

#[cfg(test)]
mod tests;

pub use analysis::analyze_postgres_sql;
pub use metadata::{
    PgSqlMetadataError, PgSqlMetadataProvider, PgSqlStatementMetadataRequest,
    analyze_postgres_sql_with_metadata,
};
pub use postgres::{PgSqlDatabaseMetadataProvider, analyze_postgres_sql_with_database};
pub use types::{
    PgSqlAnalysis, PgSqlAnalysisRequest, PgSqlColumnMetadata, PgSqlConstraintMetadata,
    PgSqlFinding, PgSqlIndexMetadata, PgSqlLockMetadata, PgSqlMetadataOptions, PgSqlMetadataReport,
    PgSqlMetadataStatus, PgSqlParseError, PgSqlPlanMetadata, PgSqlPlanNodeMetadata,
    PgSqlPrivilegeMetadata, PgSqlRelationMetadata, PgSqlRelationRef, PgSqlRiskSeverity,
    PgSqlRlsMetadata, PgSqlRuleOptions, PgSqlStatement, PgSqlStatementKind, PgSqlStatementMetadata,
};
