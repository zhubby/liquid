use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export)]
pub struct AuditSummary {
    pub total_queries: u64,
    pub flagged_queries: u64,
    pub high_risk_queries: u64,
    pub average_latency_ms: f64,
    pub audit_score: u8,
    pub risk_breakdown: Vec<RiskBreakdown>,
    pub trend: Vec<AuditTrendPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct RiskBreakdown {
    pub label: String,
    pub count: u64,
    pub severity: RiskSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RiskSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct AuditTrendPoint {
    pub day: String,
    pub audited: u64,
    pub flagged: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SqlAuditRequest {
    pub sql: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub context: Option<String>,
}

impl SqlAuditRequest {
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            schema: None,
            context: None,
        }
    }

    pub fn with_schema(mut self, schema: impl Into<String>) -> Self {
        self.schema = Some(schema.into());
        self
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateSqlAuditRequest {
    pub sql: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub execution_purpose: Option<String>,
}

impl CreateSqlAuditRequest {
    pub fn into_audit_request(&self) -> SqlAuditRequest {
        SqlAuditRequest {
            sql: self.sql.clone(),
            schema: self.schema.clone(),
            context: self.context.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SqlAuditReport {
    pub summary: String,
    pub risk_score: u8,
    #[serde(default)]
    pub findings: Vec<SqlAuditFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SqlAuditFinding {
    pub title: String,
    pub severity: RiskSeverity,
    pub explanation: String,
    pub recommendation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SqlAuditStatus {
    Audited,
    PendingApproval,
    Approved,
    Rejected,
    Blocked,
    Executing,
    Executed,
    ExecutionFailed,
}

impl SqlAuditStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Audited => "audited",
            Self::PendingApproval => "pending_approval",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Blocked => "blocked",
            Self::Executing => "executing",
            Self::Executed => "executed",
            Self::ExecutionFailed => "execution_failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SqlStatementKind {
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

impl SqlStatementKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Insert => "insert",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Merge => "merge",
            Self::Create => "create",
            Self::Alter => "alter",
            Self::Drop => "drop",
            Self::Truncate => "truncate",
            Self::Security => "security",
            Self::Transaction => "transaction",
            Self::Control => "control",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ApproveSqlAuditRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RejectSqlAuditRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SqlAuditExecutionResult {
    pub statement_kind: SqlStatementKind,
    pub affected_rows: u64,
    pub elapsed_ms: u64,
    pub risk_floor: u8,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub findings: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SqlAuditRecord {
    pub id: String,
    pub owner_user_id: String,
    pub managed_database_id: String,
    pub managed_database_name: String,
    pub managed_database_engine: String,
    pub managed_database_host: String,
    pub managed_database_port: i32,
    pub managed_database_database: String,
    pub managed_database_username: String,
    pub managed_database_ssl_mode: String,
    pub sql: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub execution_purpose: Option<String>,
    pub status: SqlAuditStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub statement_kind: Option<SqlStatementKind>,
    pub risk_score: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report: Option<SqlAuditReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "unknown")]
    pub deterministic_analysis: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub approved_by_user_id: Option<String>,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "string")]
    pub approved_at: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub approval_comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub rejected_by_user_id: Option<String>,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "string")]
    pub rejected_at: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub rejection_comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub execution_result: Option<SqlAuditExecutionResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub execution_error: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated_at: OffsetDateTime,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "string")]
    pub executed_at: Option<OffsetDateTime>,
}

impl AuditSummary {
    pub fn sample() -> Self {
        Self {
            total_queries: 12_846,
            flagged_queries: 438,
            high_risk_queries: 37,
            average_latency_ms: 86.4,
            audit_score: 92,
            risk_breakdown: vec![
                RiskBreakdown {
                    label: "PII exposure".to_owned(),
                    count: 144,
                    severity: RiskSeverity::High,
                },
                RiskBreakdown {
                    label: "Cartesian joins".to_owned(),
                    count: 96,
                    severity: RiskSeverity::Medium,
                },
                RiskBreakdown {
                    label: "DDL mutation".to_owned(),
                    count: 31,
                    severity: RiskSeverity::Critical,
                },
                RiskBreakdown {
                    label: "Unbounded scans".to_owned(),
                    count: 167,
                    severity: RiskSeverity::Low,
                },
            ],
            trend: vec![
                AuditTrendPoint {
                    day: "Mon".to_owned(),
                    audited: 1840,
                    flagged: 68,
                },
                AuditTrendPoint {
                    day: "Tue".to_owned(),
                    audited: 1935,
                    flagged: 71,
                },
                AuditTrendPoint {
                    day: "Wed".to_owned(),
                    audited: 2018,
                    flagged: 82,
                },
                AuditTrendPoint {
                    day: "Thu".to_owned(),
                    audited: 1762,
                    flagged: 49,
                },
                AuditTrendPoint {
                    day: "Fri".to_owned(),
                    audited: 2114,
                    flagged: 76,
                },
                AuditTrendPoint {
                    day: "Sat".to_owned(),
                    audited: 1588,
                    flagged: 44,
                },
                AuditTrendPoint {
                    day: "Sun".to_owned(),
                    audited: 1589,
                    flagged: 48,
                },
            ],
        }
    }
}
