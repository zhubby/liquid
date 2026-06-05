use std::{env, fs, net::SocketAddr, path::Path, str::FromStr};

use anyhow::{Context, Result};
use serde::Deserialize;

const DEFAULT_API_ADDR: &str = "127.0.0.1:3001";
const DEFAULT_CORS_ORIGIN: &str = "http://localhost:3000";
const DEFAULT_DATABASE_URL: &str = "postgres://postgres:postgres@localhost:5432/liquid";
const DEFAULT_DATABASE_MAX_CONNECTIONS: u32 = 5;
const DEFAULT_DATABASE_AUTO_MIGRATE: bool = true;
const DEFAULT_AUTH_TOKEN_TTL_SECONDS: i64 = 60 * 60 * 24 * 7;
const DEFAULT_ENCRYPTION_KEY: &str = "liquid-development-encryption-key-change-me";
const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidConfig {
    pub api_addr: SocketAddr,
    pub cors_origin: String,
    pub database: DatabaseConfig,
    pub auth: AuthConfig,
    pub security: SecurityConfig,
    pub sql_metadata: SqlMetadataMode,
    pub sql_execution: SqlExecutionMode,
    pub llm: LlmConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub auto_migrate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthConfig {
    pub token_ttl_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityConfig {
    pub encryption_key: String,
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

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    api: Option<FileApiConfig>,
    database: Option<FileDatabaseConfig>,
    auth: Option<FileAuthConfig>,
    security: Option<FileSecurityConfig>,
    llm: Option<FileLlmConfig>,
    sql: Option<FileSqlConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct FileApiConfig {
    addr: Option<String>,
    cors_origin: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FileDatabaseConfig {
    url: Option<String>,
    max_connections: Option<u32>,
    auto_migrate: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct FileAuthConfig {
    token_ttl_seconds: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct FileSecurityConfig {
    encryption_key: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FileLlmConfig {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    api_mode: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FileSqlConfig {
    metadata: Option<String>,
    execution: Option<String>,
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

impl LiquidConfig {
    pub fn from_env() -> Result<Self> {
        Self::from_env_values(None::<FileConfig>, |key| env::var(key).ok())
    }

    pub fn from_file_and_env(path: Option<&Path>) -> Result<Self> {
        let file_config = match path {
            Some(path) => Some(read_file_config(path)?),
            None => None,
        };

        Self::from_env_values(file_config, |key| env::var(key).ok())
    }

    fn from_env_values<F>(file_config: Option<FileConfig>, get: F) -> Result<Self>
    where
        F: Fn(&str) -> Option<String>,
    {
        let file_config = file_config.unwrap_or_default();
        let file_api = file_config.api.unwrap_or_default();
        let file_database = file_config.database.unwrap_or_default();
        let file_auth = file_config.auth.unwrap_or_default();
        let file_security = file_config.security.unwrap_or_default();
        let file_llm = file_config.llm.unwrap_or_default();
        let file_sql = file_config.sql.unwrap_or_default();

        let api_addr = env_or_file(
            get("LIQUID_API_ADDR"),
            file_api.addr,
            DEFAULT_API_ADDR.to_owned(),
        );
        let cors_origin = env_or_file(
            get("LIQUID_CORS_ORIGIN"),
            file_api.cors_origin,
            DEFAULT_CORS_ORIGIN.to_owned(),
        );
        let database_url = env_or_file(
            get("LIQUID_DATABASE_URL").or_else(|| get("DATABASE_URL")),
            file_database.url,
            DEFAULT_DATABASE_URL.to_owned(),
        );
        let max_connections = parse_u32(
            "LIQUID_DATABASE_MAX_CONNECTIONS",
            get("LIQUID_DATABASE_MAX_CONNECTIONS"),
            file_database.max_connections,
            DEFAULT_DATABASE_MAX_CONNECTIONS,
        )?;
        let auto_migrate = parse_bool(
            "LIQUID_DATABASE_AUTO_MIGRATE",
            get("LIQUID_DATABASE_AUTO_MIGRATE"),
            file_database.auto_migrate,
            DEFAULT_DATABASE_AUTO_MIGRATE,
        )?;
        let token_ttl_seconds = parse_i64(
            "LIQUID_AUTH_TOKEN_TTL_SECONDS",
            get("LIQUID_AUTH_TOKEN_TTL_SECONDS"),
            file_auth.token_ttl_seconds,
            DEFAULT_AUTH_TOKEN_TTL_SECONDS,
        )?;
        let encryption_key = env_or_file(
            get("LIQUID_ENCRYPTION_KEY"),
            file_security.encryption_key,
            DEFAULT_ENCRYPTION_KEY.to_owned(),
        );
        let api_key = get("OPENAI_API_KEY")
            .or(file_llm.api_key)
            .and_then(non_empty);
        let base_url = get("OPENAI_BASE_URL")
            .or(file_llm.base_url)
            .and_then(non_empty)
            .unwrap_or_else(|| DEFAULT_OPENAI_BASE_URL.to_owned());
        let model = get("OPENAI_MODEL").or(file_llm.model).and_then(non_empty);
        let api_mode = get("OPENAI_API_MODE")
            .or(file_llm.api_mode)
            .as_deref()
            .unwrap_or_default()
            .parse()?;
        let sql_metadata = get("LIQUID_SQL_METADATA")
            .or(file_sql.metadata)
            .as_deref()
            .unwrap_or_default()
            .parse()?;
        let sql_execution = get("LIQUID_SQL_EXECUTION")
            .or(file_sql.execution)
            .as_deref()
            .unwrap_or_default()
            .parse()?;

        if token_ttl_seconds <= 0 {
            anyhow::bail!("LIQUID_AUTH_TOKEN_TTL_SECONDS must be positive");
        }

        Ok(Self {
            api_addr: api_addr
                .parse()
                .with_context(|| format!("invalid LIQUID_API_ADDR: {api_addr}"))?,
            cors_origin,
            database: DatabaseConfig {
                url: database_url,
                max_connections,
                auto_migrate,
            },
            auth: AuthConfig { token_ttl_seconds },
            security: SecurityConfig { encryption_key },
            sql_metadata,
            sql_execution,
            llm: LlmConfig {
                api_key,
                base_url,
                model,
                api_mode,
            },
        })
    }
}

fn read_file_config(path: &Path) -> Result<FileConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file: {}", path.display()))?;

    toml::from_str(&content)
        .with_context(|| format!("failed to parse config file: {}", path.display()))
}

fn env_or_file(
    env_value: Option<String>,
    file_value: Option<String>,
    default_value: String,
) -> String {
    env_value
        .or(file_value)
        .and_then(non_empty)
        .unwrap_or(default_value)
}

fn parse_u32(
    env_name: &str,
    env_value: Option<String>,
    file_value: Option<u32>,
    default_value: u32,
) -> Result<u32> {
    match env_value.and_then(non_empty) {
        Some(value) => value
            .parse()
            .with_context(|| format!("invalid {env_name}: {value}")),
        None => Ok(file_value.unwrap_or(default_value)),
    }
}

fn parse_i64(
    env_name: &str,
    env_value: Option<String>,
    file_value: Option<i64>,
    default_value: i64,
) -> Result<i64> {
    match env_value.and_then(non_empty) {
        Some(value) => value
            .parse()
            .with_context(|| format!("invalid {env_name}: {value}")),
        None => Ok(file_value.unwrap_or(default_value)),
    }
}

fn parse_bool(
    env_name: &str,
    env_value: Option<String>,
    file_value: Option<bool>,
    default_value: bool,
) -> Result<bool> {
    match env_value.and_then(non_empty) {
        Some(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => anyhow::bail!("invalid {env_name}: {value}; expected true or false"),
        },
        None => Ok(file_value.unwrap_or(default_value)),
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
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn defaults_are_valid() {
        let config = LiquidConfig::from_env_values(None, |_| None).unwrap();
        let addr: SocketAddr = DEFAULT_API_ADDR.parse().expect("default api addr");

        assert_eq!(addr.port(), 3001);
        assert!(DEFAULT_DATABASE_URL.starts_with("postgres://"));
        assert!(config.auth.token_ttl_seconds > 0);
    }

    #[test]
    fn llm_defaults_to_openai_compatible_chat_completions() {
        let config = LiquidConfig::from_env_values(None, |_| None).unwrap();

        assert_eq!(config.llm.api_key, None);
        assert_eq!(config.llm.base_url, DEFAULT_OPENAI_BASE_URL);
        assert_eq!(config.llm.model, None);
        assert_eq!(config.llm.api_mode, LlmApiMode::ChatCompletions);
        assert_eq!(config.sql_metadata, SqlMetadataMode::Auto);
        assert_eq!(config.sql_execution, SqlExecutionMode::Readonly);
    }

    #[test]
    fn parses_llm_env_values() {
        let config = LiquidConfig::from_env_values(None, |key| match key {
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
    fn reads_toml_config_file_values() {
        let path = temp_config_path("liquid-config-file-values.toml");
        fs::write(
            &path,
            r#"
[api]
addr = "127.0.0.1:3131"
cors_origin = "http://localhost:4000"

[database]
url = "postgres://liquid:liquid@localhost:5432/app"
max_connections = 9
auto_migrate = false

[auth]
token_ttl_seconds = 3600

[security]
encryption_key = "test-key"

[llm]
api_mode = "responses"

[sql]
metadata = "off"
execution = "off"
"#,
        )
        .unwrap();

        let config = LiquidConfig::from_file_and_env(Some(&path)).unwrap();

        assert_eq!(config.api_addr.port(), 3131);
        assert_eq!(config.cors_origin, "http://localhost:4000");
        assert_eq!(
            config.database.url,
            "postgres://liquid:liquid@localhost:5432/app"
        );
        assert_eq!(config.database.max_connections, 9);
        assert!(!config.database.auto_migrate);
        assert_eq!(config.auth.token_ttl_seconds, 3600);
        assert_eq!(config.security.encryption_key, "test-key");
        assert_eq!(config.llm.api_mode, LlmApiMode::Responses);
        assert_eq!(config.sql_metadata, SqlMetadataMode::Off);
        assert_eq!(config.sql_execution, SqlExecutionMode::Off);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn env_values_override_toml_config() {
        let file_config = FileConfig {
            database: Some(FileDatabaseConfig {
                url: Some("postgres://file".to_owned()),
                max_connections: Some(2),
                auto_migrate: Some(false),
            }),
            ..FileConfig::default()
        };
        let config = LiquidConfig::from_env_values(Some(file_config), |key| match key {
            "LIQUID_DATABASE_URL" => Some("postgres://env".to_owned()),
            "LIQUID_DATABASE_MAX_CONNECTIONS" => Some("7".to_owned()),
            "LIQUID_DATABASE_AUTO_MIGRATE" => Some("true".to_owned()),
            _ => None,
        })
        .unwrap();

        assert_eq!(config.database.url, "postgres://env");
        assert_eq!(config.database.max_connections, 7);
        assert!(config.database.auto_migrate);
    }

    #[test]
    fn parses_sql_metadata_mode() {
        let config = LiquidConfig::from_env_values(None, |key| match key {
            "LIQUID_SQL_METADATA" => Some("required".to_owned()),
            _ => None,
        })
        .unwrap();

        assert_eq!(config.sql_metadata, SqlMetadataMode::Required);
    }

    #[test]
    fn parses_sql_execution_mode() {
        let config = LiquidConfig::from_env_values(None, |key| match key {
            "LIQUID_SQL_EXECUTION" => Some("write_gated".to_owned()),
            _ => None,
        })
        .unwrap();

        assert_eq!(config.sql_execution, SqlExecutionMode::WriteGated);
    }

    #[test]
    fn rejects_invalid_sql_metadata_mode() {
        let error = LiquidConfig::from_env_values(None, |key| match key {
            "LIQUID_SQL_METADATA" => Some("sometimes".to_owned()),
            _ => None,
        })
        .unwrap_err();

        assert!(error.to_string().contains("invalid LIQUID_SQL_METADATA"));
    }

    #[test]
    fn rejects_invalid_sql_execution_mode() {
        let error = LiquidConfig::from_env_values(None, |key| match key {
            "LIQUID_SQL_EXECUTION" => Some("sometimes".to_owned()),
            _ => None,
        })
        .unwrap_err();

        assert!(error.to_string().contains("invalid LIQUID_SQL_EXECUTION"));
    }

    fn temp_config_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }
}
