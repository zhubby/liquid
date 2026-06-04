use std::{env, net::SocketAddr, str::FromStr};

use anyhow::{Context, Result};

const DEFAULT_API_ADDR: &str = "127.0.0.1:3001";
const DEFAULT_DATABASE_URL: &str = "postgres://postgres:postgres@localhost:5432/liquid";
const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidConfig {
    pub api_addr: SocketAddr,
    pub database_url: String,
    pub llm: LlmConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: Option<String>,
    pub api_mode: LlmApiMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LlmApiMode {
    #[default]
    ChatCompletions,
    Responses,
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

impl LiquidConfig {
    pub fn from_env() -> Result<Self> {
        Self::from_env_values(|key| env::var(key).ok())
    }

    fn from_env_values<F>(get: F) -> Result<Self>
    where
        F: Fn(&str) -> Option<String>,
    {
        let api_addr = get("LIQUID_API_ADDR").unwrap_or_else(|| DEFAULT_API_ADDR.to_owned());
        let database_url = get("DATABASE_URL").unwrap_or_else(|| DEFAULT_DATABASE_URL.to_owned());
        let api_key = get("OPENAI_API_KEY").and_then(non_empty);
        let base_url = get("OPENAI_BASE_URL")
            .and_then(non_empty)
            .unwrap_or_else(|| DEFAULT_OPENAI_BASE_URL.to_owned());
        let model = get("OPENAI_MODEL").and_then(non_empty);
        let api_mode = get("OPENAI_API_MODE")
            .as_deref()
            .unwrap_or_default()
            .parse()?;

        Ok(Self {
            api_addr: api_addr
                .parse()
                .with_context(|| format!("invalid LIQUID_API_ADDR: {api_addr}"))?,
            database_url,
            llm: LlmConfig {
                api_key,
                base_url,
                model,
                api_mode,
            },
        })
    }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let addr: SocketAddr = DEFAULT_API_ADDR.parse().expect("default api addr");

        assert_eq!(addr.port(), 3001);
        assert!(DEFAULT_DATABASE_URL.starts_with("postgres://"));
    }

    #[test]
    fn llm_defaults_to_openai_compatible_chat_completions() {
        let config = LiquidConfig::from_env_values(|_| None).unwrap();

        assert_eq!(config.llm.api_key, None);
        assert_eq!(config.llm.base_url, DEFAULT_OPENAI_BASE_URL);
        assert_eq!(config.llm.model, None);
        assert_eq!(config.llm.api_mode, LlmApiMode::ChatCompletions);
    }

    #[test]
    fn parses_llm_env_values() {
        let config = LiquidConfig::from_env_values(|key| match key {
            "OPENAI_API_KEY" => Some(" key ".to_owned()),
            "OPENAI_BASE_URL" => Some("https://llm.example.test".to_owned()),
            "OPENAI_MODEL" => Some("gpt-test".to_owned()),
            "OPENAI_API_MODE" => Some("responses".to_owned()),
            _ => None,
        })
        .unwrap();

        assert_eq!(config.llm.api_key.as_deref(), Some("key"));
        assert_eq!(config.llm.base_url, "https://llm.example.test");
        assert_eq!(config.llm.model.as_deref(), Some("gpt-test"));
        assert_eq!(config.llm.api_mode, LlmApiMode::Responses);
    }

    #[test]
    fn rejects_invalid_llm_api_mode() {
        let error = LiquidConfig::from_env_values(|key| match key {
            "OPENAI_API_MODE" => Some("legacy".to_owned()),
            _ => None,
        })
        .unwrap_err();

        assert!(error.to_string().contains("invalid OPENAI_API_MODE"));
    }
}
