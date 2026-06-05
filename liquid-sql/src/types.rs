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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PgSqlAnalysis {
    pub statements: Vec<PgSqlStatement>,
    pub findings: Vec<PgSqlFinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<PgSqlMetadataReport>,
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
    pub fn new(
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlMetadataOptions {
    pub enabled: bool,
    pub explain_enabled: bool,
    pub runtime_enabled: bool,
    pub allow_explain_analyze: bool,
    pub statement_timeout_ms: u64,
    pub lock_timeout_ms: u64,
    pub large_table_threshold_bytes: i64,
    pub high_estimated_rows_threshold: i64,
    pub high_total_cost_threshold: i64,
}

impl Default for PgSqlMetadataOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            explain_enabled: true,
            runtime_enabled: true,
            allow_explain_analyze: false,
            statement_timeout_ms: 2_000,
            lock_timeout_ms: 250,
            large_table_threshold_bytes: 1_073_741_824,
            high_estimated_rows_threshold: 100_000,
            high_total_cost_threshold: 100_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PgSqlMetadataStatus {
    NotRequested,
    Available,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PgSqlMetadataReport {
    pub status: PgSqlMetadataStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statements: Vec<PgSqlStatementMetadata>,
}

impl PgSqlMetadataReport {
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: PgSqlMetadataStatus::Unavailable,
            warnings: vec![message.into()],
            statements: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PgSqlStatementMetadata {
    pub statement_index: usize,
    pub metadata_status: PgSqlMetadataStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<PgSqlRelationMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<PgSqlIndexMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<PgSqlConstraintMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<PgSqlColumnMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub privileges: Vec<PgSqlPrivilegeMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rls: Vec<PgSqlRlsMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locks: Vec<PgSqlLockMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<PgSqlPlanMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl PgSqlStatementMetadata {
    pub fn new(statement_index: usize) -> Self {
        Self {
            statement_index,
            metadata_status: PgSqlMetadataStatus::Available,
            relations: Vec::new(),
            indexes: Vec::new(),
            constraints: Vec::new(),
            columns: Vec::new(),
            privileges: Vec::new(),
            rls: Vec::new(),
            locks: Vec::new(),
            plan: None,
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlRelationRef {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PgSqlRelationMetadata {
    pub oid: i64,
    pub schema: String,
    pub name: String,
    pub kind: String,
    pub owner: String,
    pub total_size_bytes: i64,
    pub relation_size_bytes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_rows: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_rows: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dead_rows: Option<i64>,
    pub is_partitioned: bool,
    pub partition_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlIndexMetadata {
    pub relation_oid: i64,
    pub index_oid: i64,
    pub schema: String,
    pub name: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
    pub is_primary: bool,
    pub is_valid: bool,
    pub is_ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub predicate: Option<String>,
    pub definition: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlConstraintMetadata {
    pub relation_oid: i64,
    pub name: String,
    pub kind: String,
    pub columns: Vec<String>,
    pub is_validated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlColumnMetadata {
    pub relation_oid: i64,
    pub name: String,
    pub is_nullable: bool,
    pub has_default: bool,
    pub is_identity: bool,
    pub is_generated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlPrivilegeMetadata {
    pub relation_oid: i64,
    pub action: String,
    pub allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlRlsMetadata {
    pub relation_oid: i64,
    pub enabled: bool,
    pub forced: bool,
    pub current_role_bypasses_rls: bool,
    pub policy_count: i64,
    pub applicable_policy_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgSqlLockMetadata {
    pub relation_oid: i64,
    pub expected_mode: String,
    pub conflicting_granted_locks: i64,
    pub conflicting_waiting_locks: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longest_conflict_age_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PgSqlPlanMetadata {
    pub statement_index: usize,
    pub total_cost: f64,
    pub plan_rows: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<PgSqlPlanNodeMetadata>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PgSqlPlanNodeMetadata {
    pub node_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation_name: Option<String>,
    pub total_cost: f64,
    pub plan_rows: i64,
}
