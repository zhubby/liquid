use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::{
    AgentActionKind, AgentActionStatus, AgentActiveView, AgentDateRange, AgentMessageRole,
    AgentResourceKind, AgentTurnStatus, ManagedDatabaseEngine, ManagedDatabaseSslMode,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChatConversation {
    pub id: String,
    pub title: String,
    pub selected_database: Option<ChatManagedDatabaseSummary>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateChatConversationRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdateChatConversationRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChatTurnDashboardContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub active_view: Option<AgentActiveView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub selected_sql_audit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub date_range: Option<AgentDateRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateChatTurnRequest {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub managed_database_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub dashboard_context: Option<ChatTurnDashboardContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub client_request_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChatActionDecisionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChatManagedDatabaseSummary {
    pub id: String,
    pub name: String,
    pub engine: ManagedDatabaseEngine,
    pub host: String,
    pub port: i32,
    pub database: String,
    pub username: String,
    pub ssl_mode: ManagedDatabaseSslMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ChatMessageStatus {
    Complete,
    Streaming,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChatMessage {
    pub id: String,
    pub role: AgentMessageRole,
    pub status: ChatMessageStatus,
    pub content: String,
    pub parts: Vec<ChatMessagePart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub turn_id: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum ChatMessagePart {
    Text {
        text: String,
    },
    Markdown {
        markdown: String,
    },
    Code {
        language: Option<String>,
        code: String,
    },
    ActionRef {
        action_id: String,
    },
    Error {
        code: ChatErrorCode,
        message: String,
    },
    Status {
        stage: ChatStreamStage,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChatTurn {
    pub id: String,
    pub conversation_id: String,
    pub status: AgentTurnStatus,
    pub input_message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub output_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error_code: Option<ChatErrorCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ChatStreamStage {
    Thinking,
    LoadingContext,
    ProposingAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ChatErrorCode {
    ProviderNotConfigured,
    ProviderRequestFailed,
    InvalidModelResponse,
    InvalidActionIntent,
    StorageError,
    TurnCancelled,
}

impl ChatErrorCode {
    pub fn message_key(self) -> &'static str {
        match self {
            Self::ProviderNotConfigured => "workspace.providerNotConfigured",
            Self::ProviderRequestFailed => "workspace.providerRequestFailed",
            Self::InvalidModelResponse => "workspace.invalidModelResponse",
            Self::InvalidActionIntent => "workspace.invalidActionIntent",
            Self::StorageError => "workspace.storageError",
            Self::TurnCancelled => "workspace.turnCancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
#[ts(export)]
pub enum ChatStreamEvent {
    TurnStarted {
        turn_id: String,
    },
    MessageCreated {
        message: ChatMessage,
    },
    AssistantDelta {
        message_id: String,
        delta: String,
        accumulated: Option<String>,
    },
    AssistantDone {
        message: ChatMessage,
    },
    StatusChanged {
        stage: ChatStreamStage,
    },
    ActionProposed {
        action: ChatAction,
    },
    ActionUpdated {
        action: ChatAction,
    },
    TurnCompleted {
        turn: ChatTurn,
    },
    TurnFailed {
        turn_id: String,
        error_code: ChatErrorCode,
        message_key: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChatAction {
    pub id: String,
    pub turn_id: String,
    pub kind: AgentActionKind,
    pub status: AgentActionStatus,
    pub title: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resource_kind: Option<AgentResourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resource_id: Option<String>,
    pub requires_confirmation: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub preview: Option<ChatActionPreview>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum ChatActionPreview {
    SqlAudit {
        sql: String,
        database_name: Option<String>,
        context: Option<String>,
    },
}
