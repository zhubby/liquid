use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LlmProviderKind {
    OpenaiCompatible,
}

impl LlmProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenaiCompatible => "openai_compatible",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LlmProviderApiMode {
    ChatCompletions,
    Responses,
}

impl LlmProviderApiMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Responses => "responses",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct LlmProviderSettings {
    pub provider: LlmProviderKind,
    pub base_url: String,
    pub model: String,
    pub api_mode: LlmProviderApiMode,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct LlmProviderSettingsResponse {
    pub settings: Option<LlmProviderSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct UpdateLlmProviderSettingsRequest {
    pub provider: LlmProviderKind,
    pub base_url: String,
    pub model: String,
    pub api_mode: LlmProviderApiMode,
    #[ts(optional)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLlmProviderSettings {
    pub provider: LlmProviderKind,
    pub base_url: String,
    pub model: String,
    pub api_mode: LlmProviderApiMode,
    pub api_key: Option<String>,
}
