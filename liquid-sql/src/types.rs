use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlAnalysisRequest {
    pub sql: String,
    pub options: PgSqlRuleOptions,
}

impl PgSqlAnalysisRequest {
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            options: PgSqlRuleOptions::default(),
        }
    }

    pub fn with_options(mut self, options: PgSqlRuleOptions) -> Self {
        self.options = options;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlRuleOptions {
    pub max_insert_rows: usize,
    pub check_destructive_ddl: bool,
    pub check_dml_scope: bool,
    pub check_broad_projection: bool,
    pub check_joins: bool,
    pub check_transaction_controls: bool,
}

impl Default for PgSqlRuleOptions {
    fn default() -> Self {
        Self {
            max_insert_rows: 1_000,
            check_destructive_ddl: true,
            check_dml_scope: true,
            check_broad_projection: true,
            check_joins: true,
            check_transaction_controls: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlAnalysis {
    pub statements: Vec<PgSqlStatement>,
    pub findings: Vec<PgSqlFinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<PgSqlParseError>,
}

impl PgSqlAnalysis {
    pub fn parse_ok(&self) -> bool {
        self.parse_error.is_none()
    }

    pub fn risk_floor(&self) -> u8 {
        self.findings
            .iter()
            .map(|finding| finding.severity.risk_floor())
            .max()
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlStatement {
    pub index: usize,
    pub kind: PgSqlStatementKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PgSqlStatementKind {
    Select,
    Insert,
    Update,
    Delete,
    Merge,
    Create,
    Alter,
    Drop,
    Truncate,
    Security,
    Transaction,
    Control,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlFinding {
    pub rule_id: String,
    pub severity: PgSqlRiskSeverity,
    pub title: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statement_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

impl PgSqlFinding {
    pub(crate) fn new(
        rule_id: impl Into<String>,
        severity: PgSqlRiskSeverity,
        title: impl Into<String>,
        detail: impl Into<String>,
        statement_index: Option<usize>,
        evidence: Option<String>,
    ) -> Self {
        Self {
            rule_id: rule_id.into(),
            severity,
            title: title.into(),
            detail: detail.into(),
            statement_index,
            evidence,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PgSqlRiskSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl PgSqlRiskSeverity {
    pub fn risk_floor(&self) -> u8 {
        match self {
            Self::Low => 25,
            Self::Medium => 50,
            Self::High => 80,
            Self::Critical => 95,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlParseError {
    pub message: String,
}
