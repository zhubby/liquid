use serde::{Deserialize, Serialize};

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
