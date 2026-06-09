use std::{fs, net::SocketAddr, path::PathBuf};

use crate::{
    LiquidConfig, LlmApiMode, SqlExecutionMode, SqlMetadataMode, default_config_toml,
    defaults::*,
    file::{FileConfig, FileDatabaseConfig, read_file_config},
};

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
    assert_eq!(
        config.workbench.max_tool_rounds,
        DEFAULT_WORKBENCH_MAX_TOOL_ROUNDS
    );
    assert_eq!(config.workbench.max_output_tokens, None);
    assert_eq!(config.sql_metadata, SqlMetadataMode::Auto);
    assert_eq!(config.sql_execution, SqlExecutionMode::Readonly);
    assert_eq!(config.database_backup.s3_bucket, None);
    assert_eq!(config.database_backup.s3_prefix, DEFAULT_BACKUP_S3_PREFIX);
    assert_eq!(
        config.database_backup.worker_concurrency,
        DEFAULT_BACKUP_WORKER_CONCURRENCY
    );
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

[workbench]
max_tool_rounds = 12
max_output_tokens = 4096

[sql]
metadata = "off"
execution = "off"
managed_pool_max_connections = 4
managed_pool_idle_ttl_seconds = 120
managed_pool_reap_interval_seconds = 15
managed_pool_acquire_timeout_seconds = 3

[backup]
s3_bucket = "liquid-backups"
s3_prefix = "custom/prefix"
s3_region = "ap-east-1"
s3_endpoint = "http://localhost:9000"
s3_path_style = true
work_dir = "/tmp/liquid-test-backups"
worker_concurrency = 2
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
    assert_eq!(config.workbench.max_tool_rounds, 12);
    assert_eq!(config.workbench.max_output_tokens, Some(4096));
    assert_eq!(config.sql_metadata, SqlMetadataMode::Off);
    assert_eq!(config.sql_execution, SqlExecutionMode::Off);
    assert_eq!(config.managed_database_pool.max_connections, 4);
    assert_eq!(config.managed_database_pool.idle_ttl_seconds, 120);
    assert_eq!(config.managed_database_pool.reap_interval_seconds, 15);
    assert_eq!(config.managed_database_pool.acquire_timeout_seconds, 3);
    assert_eq!(
        config.database_backup.s3_bucket.as_deref(),
        Some("liquid-backups")
    );
    assert_eq!(config.database_backup.s3_prefix, "custom/prefix");
    assert_eq!(config.database_backup.s3_region, "ap-east-1");
    assert_eq!(
        config.database_backup.s3_endpoint.as_deref(),
        Some("http://localhost:9000")
    );
    assert!(config.database_backup.s3_path_style);
    assert_eq!(config.database_backup.work_dir, "/tmp/liquid-test-backups");
    assert_eq!(config.database_backup.worker_concurrency, 2);

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
fn parses_managed_database_pool_env_values() {
    let config = LiquidConfig::from_env_values(None, |key| match key {
        "LIQUID_SQL_MANAGED_POOL_MAX_CONNECTIONS" => Some("3".to_owned()),
        "LIQUID_SQL_MANAGED_POOL_IDLE_TTL_SECONDS" => Some("90".to_owned()),
        "LIQUID_SQL_MANAGED_POOL_REAP_INTERVAL_SECONDS" => Some("9".to_owned()),
        "LIQUID_SQL_MANAGED_POOL_ACQUIRE_TIMEOUT_SECONDS" => Some("2".to_owned()),
        _ => None,
    })
    .unwrap();

    assert_eq!(config.managed_database_pool.max_connections, 3);
    assert_eq!(config.managed_database_pool.idle_ttl_seconds, 90);
    assert_eq!(config.managed_database_pool.reap_interval_seconds, 9);
    assert_eq!(config.managed_database_pool.acquire_timeout_seconds, 2);
}

#[test]
fn parses_workbench_env_values() {
    let config = LiquidConfig::from_env_values(None, |key| match key {
        "LIQUID_WORKBENCH_MAX_TOOL_ROUNDS" => Some("15".to_owned()),
        "LIQUID_WORKBENCH_MAX_OUTPUT_TOKENS" => Some("8192".to_owned()),
        _ => None,
    })
    .unwrap();

    assert_eq!(config.workbench.max_tool_rounds, 15);
    assert_eq!(config.workbench.max_output_tokens, Some(8192));
}

#[test]
fn parses_database_backup_env_values() {
    let config = LiquidConfig::from_env_values(None, |key| match key {
        "LIQUID_BACKUP_S3_BUCKET" => Some("env-bucket".to_owned()),
        "LIQUID_BACKUP_S3_PREFIX" => Some("env-prefix".to_owned()),
        "LIQUID_BACKUP_S3_REGION" => Some("eu-west-1".to_owned()),
        "LIQUID_BACKUP_S3_ENDPOINT" => Some("http://localhost:9000".to_owned()),
        "LIQUID_BACKUP_S3_PATH_STYLE" => Some("true".to_owned()),
        "LIQUID_BACKUP_WORK_DIR" => Some("/tmp/liquid-env-backups".to_owned()),
        "LIQUID_BACKUP_WORKER_CONCURRENCY" => Some("3".to_owned()),
        _ => None,
    })
    .unwrap();

    assert_eq!(
        config.database_backup.s3_bucket.as_deref(),
        Some("env-bucket")
    );
    assert_eq!(config.database_backup.s3_prefix, "env-prefix");
    assert_eq!(config.database_backup.s3_region, "eu-west-1");
    assert_eq!(
        config.database_backup.s3_endpoint.as_deref(),
        Some("http://localhost:9000")
    );
    assert!(config.database_backup.s3_path_style);
    assert_eq!(config.database_backup.work_dir, "/tmp/liquid-env-backups");
    assert_eq!(config.database_backup.worker_concurrency, 3);
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

#[test]
fn rejects_zero_managed_database_pool_values() {
    let error = LiquidConfig::from_env_values(None, |key| match key {
        "LIQUID_SQL_MANAGED_POOL_MAX_CONNECTIONS" => Some("0".to_owned()),
        _ => None,
    })
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("LIQUID_SQL_MANAGED_POOL_MAX_CONNECTIONS")
    );
}

#[test]
fn rejects_zero_workbench_values() {
    let error = LiquidConfig::from_env_values(None, |key| match key {
        "LIQUID_WORKBENCH_MAX_TOOL_ROUNDS" => Some("0".to_owned()),
        _ => None,
    })
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("LIQUID_WORKBENCH_MAX_TOOL_ROUNDS")
    );

    let error = LiquidConfig::from_env_values(None, |key| match key {
        "LIQUID_WORKBENCH_MAX_OUTPUT_TOKENS" => Some("0".to_owned()),
        _ => None,
    })
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("LIQUID_WORKBENCH_MAX_OUTPUT_TOKENS")
    );
}

#[test]
fn generated_default_config_is_valid() {
    let path = temp_config_path("liquid-default-config.toml");
    fs::write(&path, default_config_toml()).unwrap();

    let file_config = read_file_config(&path).unwrap();
    let config = LiquidConfig::from_env_values(Some(file_config), |_| None).unwrap();

    assert_eq!(config.api_addr, DEFAULT_API_ADDR.parse().unwrap());
    assert_eq!(config.cors_origin, DEFAULT_CORS_ORIGIN);
    assert_eq!(config.database.url, DEFAULT_DATABASE_URL);
    assert_eq!(
        config.database.max_connections,
        DEFAULT_DATABASE_MAX_CONNECTIONS
    );
    assert_eq!(config.database.auto_migrate, DEFAULT_DATABASE_AUTO_MIGRATE);
    assert_eq!(
        config.auth.token_ttl_seconds,
        DEFAULT_AUTH_TOKEN_TTL_SECONDS
    );
    assert_eq!(config.security.encryption_key, DEFAULT_ENCRYPTION_KEY);
    assert_eq!(config.llm.base_url, DEFAULT_OPENAI_BASE_URL);
    assert_eq!(config.llm.api_mode, LlmApiMode::ChatCompletions);
    assert_eq!(
        config.workbench.max_tool_rounds,
        DEFAULT_WORKBENCH_MAX_TOOL_ROUNDS
    );
    assert_eq!(config.workbench.max_output_tokens, None);
    assert_eq!(config.sql_metadata, SqlMetadataMode::Auto);
    assert_eq!(config.sql_execution, SqlExecutionMode::Readonly);
    assert_eq!(
        config.managed_database_pool.max_connections,
        DEFAULT_SQL_MANAGED_POOL_MAX_CONNECTIONS
    );
    assert_eq!(
        config.managed_database_pool.idle_ttl_seconds,
        DEFAULT_SQL_MANAGED_POOL_IDLE_TTL_SECONDS
    );
    assert_eq!(
        config.managed_database_pool.reap_interval_seconds,
        DEFAULT_SQL_MANAGED_POOL_REAP_INTERVAL_SECONDS
    );
    assert_eq!(
        config.managed_database_pool.acquire_timeout_seconds,
        DEFAULT_SQL_MANAGED_POOL_ACQUIRE_TIMEOUT_SECONDS
    );

    let _ = fs::remove_file(path);
}

fn temp_config_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}
