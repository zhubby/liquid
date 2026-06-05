use std::pin::Pin;

use anyhow::Result;
use futures_core::Stream;
use liquid_core::SqlAuditReport;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type AgentStream = Pin<Box<dyn Stream<Item = Result<AgentEvent>> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Started,
    ToolCallStarted {
        id: String,
        name: String,
    },
    ToolCallFinished {
        id: String,
        name: String,
        output: ToolOutput,
    },
    Completed {
        report: SqlAuditReport,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolOutput {
    pub content: String,
}

impl ToolOutput {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
        }
    }

    pub fn json(value: Value) -> Self {
        Self::new(value.to_string())
    }
}
