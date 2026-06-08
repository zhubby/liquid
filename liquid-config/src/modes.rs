use std::str::FromStr;

use anyhow::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LlmApiMode {
    #[default]
    ChatCompletions,
    Responses,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SqlMetadataMode {
    #[default]
    Auto,
    Off,
    Required,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SqlExecutionMode {
    Off,
    #[default]
    Readonly,
    WriteGated,
}

impl FromStr for SqlMetadataMode {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Ok(Self::Auto),
            "off" | "disabled" | "false" => Ok(Self::Off),
            "required" | "require" | "on" | "true" => Ok(Self::Required),
            other => Err(anyhow::anyhow!(
                "invalid LIQUID_SQL_METADATA: {other}; expected auto, off, or required"
            )),
        }
    }
}

impl FromStr for SqlExecutionMode {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "readonly" | "read_only" | "read-only" | "on" | "true" => Ok(Self::Readonly),
            "off" | "disabled" | "false" => Ok(Self::Off),
            "write_gated" | "write-gated" | "write" | "gated" => Ok(Self::WriteGated),
            other => Err(anyhow::anyhow!(
                "invalid LIQUID_SQL_EXECUTION: {other}; expected off, readonly, or write_gated"
            )),
        }
    }
}

impl FromStr for LlmApiMode {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "chat" | "chat_completions" | "chat-completions" => Ok(Self::ChatCompletions),
            "responses" | "response" => Ok(Self::Responses),
            other => Err(anyhow::anyhow!(
                "invalid OPENAI_API_MODE: {other}; expected chat_completions or responses"
            )),
        }
    }
}
