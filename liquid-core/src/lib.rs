use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicUser {
    pub id: String,
    pub email: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegisterRequest {
    pub email: String,
    pub display_name: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthResponse {
    pub token: String,
    pub token_type: String,
    pub expires_in_seconds: i64,
    pub user: PublicUser,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CurrentUserResponse {
    pub user: PublicUser,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditedDatabaseEngine {
    Postgres,
}

impl AuditedDatabaseEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditedDatabaseSslMode {
    Disable,
    #[default]
    Prefer,
    Require,
}

impl AuditedDatabaseSslMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Prefer => "prefer",
            Self::Require => "require",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditedDatabase {
    pub id: String,
    pub name: String,
    pub engine: AuditedDatabaseEngine,
    pub host: String,
    pub port: i32,
    pub database: String,
    pub username: String,
    pub ssl_mode: AuditedDatabaseSslMode,
    pub has_password: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateAuditedDatabaseRequest {
    pub name: String,
    pub engine: AuditedDatabaseEngine,
    pub host: String,
    pub port: i32,
    pub database: String,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub ssl_mode: AuditedDatabaseSslMode,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateAuditedDatabaseRequest {
    pub name: Option<String>,
    pub host: Option<String>,
    pub port: Option<i32>,
    pub database: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub ssl_mode: Option<AuditedDatabaseSslMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditSummary {
    pub total_queries: u64,
    pub flagged_queries: u64,
    pub high_risk_queries: u64,
    pub average_latency_ms: f64,
    pub audit_score: u8,
    pub risk_breakdown: Vec<RiskBreakdown>,
    pub trend: Vec<AuditTrendPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RiskBreakdown {
    pub label: String,
    pub count: u64,
    pub severity: RiskSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditTrendPoint {
    pub day: String,
    pub audited: u64,
    pub flagged: u64,
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
