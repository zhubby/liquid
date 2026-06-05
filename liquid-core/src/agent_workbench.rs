use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConversation {
    pub id: String,
    pub owner_user_id: String,
    pub title: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAgentConversationRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateAgentConversationRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMessageRole {
    User,
    Assistant,
    Tool,
    System,
}

impl AgentMessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: String,
    pub conversation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub role: AgentMessageRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDashboardContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_view: Option<AgentActiveView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_sql_audit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_range: Option<AgentDateRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentActiveView {
    Ai,
    Bi,
    Databases,
    SqlAudits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentDateRange {
    #[serde(rename = "last_7_days")]
    Last7Days,
    #[serde(rename = "last_30_days")]
    Last30Days,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAgentTurnRequest {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_database_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard_context: Option<AgentDashboardContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_request_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTurnStatus {
    Queued,
    Running,
    Completed,
    Blocked,
    Failed,
    Cancelled,
}

impl AgentTurnStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Blocked | Self::Failed | Self::Cancelled
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTurn {
    pub id: String,
    pub conversation_id: String,
    pub status: AgentTurnStatus,
    pub user_message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_database_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard_context: Option<AgentDashboardContext>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    pub completed_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEventType {
    TurnStarted,
    MessageCreated,
    AssistantDelta,
    ToolCallStarted,
    ToolCallFinished,
    ResourceCreated,
    ResourceUpdated,
    ActionProposed,
    TurnCompleted,
    TurnFailed,
}

impl AgentEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TurnStarted => "turn_started",
            Self::MessageCreated => "message_created",
            Self::AssistantDelta => "assistant_delta",
            Self::ToolCallStarted => "tool_call_started",
            Self::ToolCallFinished => "tool_call_finished",
            Self::ResourceCreated => "resource_created",
            Self::ResourceUpdated => "resource_updated",
            Self::ActionProposed => "action_proposed",
            Self::TurnCompleted => "turn_completed",
            Self::TurnFailed => "turn_failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentEventRecord {
    pub seq: i32,
    pub turn_id: String,
    #[serde(rename = "type")]
    pub event_type: AgentEventType,
    pub payload: Value,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentActionKind {
    CreateSqlAudit,
    ApproveSqlAudit,
    RejectSqlAudit,
    ExecuteSqlAudit,
    CreateManagedDatabase,
    UpdateManagedDatabase,
    DeleteManagedDatabase,
    StartDatabaseBackup,
    StartDatabaseRestore,
}

impl AgentActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CreateSqlAudit => "create_sql_audit",
            Self::ApproveSqlAudit => "approve_sql_audit",
            Self::RejectSqlAudit => "reject_sql_audit",
            Self::ExecuteSqlAudit => "execute_sql_audit",
            Self::CreateManagedDatabase => "create_managed_database",
            Self::UpdateManagedDatabase => "update_managed_database",
            Self::DeleteManagedDatabase => "delete_managed_database",
            Self::StartDatabaseBackup => "start_database_backup",
            Self::StartDatabaseRestore => "start_database_restore",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentActionStatus {
    Proposed,
    Applied,
    Rejected,
    Failed,
    Superseded,
}

impl AgentActionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
            Self::Superseded => "superseded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentResourceKind {
    SqlAudit,
    ManagedDatabase,
    DatabaseBackup,
    DatabaseRestore,
}

impl AgentResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SqlAudit => "sql_audit",
            Self::ManagedDatabase => "managed_database",
            Self::DatabaseBackup => "database_backup",
            Self::DatabaseRestore => "database_restore",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentAction {
    pub id: String,
    pub conversation_id: String,
    pub turn_id: String,
    pub kind: AgentActionKind,
    pub status: AgentActionStatus,
    pub title: String,
    pub description: String,
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_kind: Option<AgentResourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    pub requires_confirmation: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateAgentActionRequest {
    pub kind: AgentActionKind,
    pub title: String,
    pub description: String,
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_kind: Option<AgentResourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    #[serde(default = "default_requires_confirmation")]
    pub requires_confirmation: bool,
}

fn default_requires_confirmation() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentActionDecisionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentCapability {
    pub name: String,
    pub description: String,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentCapabilitiesResponse {
    pub mode: String,
    pub capabilities: Vec<AgentCapability>,
}
