mod analysis;
mod ast;
mod rules;
mod types;

#[cfg(test)]
mod tests;

pub use analysis::analyze_postgres_sql;
pub use types::{
    PgSqlAnalysis, PgSqlAnalysisRequest, PgSqlFinding, PgSqlParseError, PgSqlRiskSeverity,
    PgSqlRuleOptions, PgSqlStatement, PgSqlStatementKind,
};
