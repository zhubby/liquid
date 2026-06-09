use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::{
    defaults::*,
    file::{FileConfig, read_file_config},
    types::{
        AuthConfig, DatabaseBackupConfig, DatabaseConfig, LiquidConfig, LlmConfig,
        ManagedDatabasePoolConfig, SecurityConfig, WorkbenchConfig,
    },
};

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

    pub(crate) fn from_env_values<F>(file_config: Option<FileConfig>, get: F) -> Result<Self>
    where
        F: Fn(&str) -> Option<String>,
    {
        let file_config = file_config.unwrap_or_default();
        let file_api = file_config.api.unwrap_or_default();
        let file_database = file_config.database.unwrap_or_default();
        let file_auth = file_config.auth.unwrap_or_default();
        let file_security = file_config.security.unwrap_or_default();
        let file_llm = file_config.llm.unwrap_or_default();
        let file_workbench = file_config.workbench.unwrap_or_default();
        let file_sql = file_config.sql.unwrap_or_default();
        let file_backup = file_config.backup.unwrap_or_default();

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
        let workbench_max_tool_rounds = parse_usize(
            "LIQUID_WORKBENCH_MAX_TOOL_ROUNDS",
            get("LIQUID_WORKBENCH_MAX_TOOL_ROUNDS"),
            file_workbench.max_tool_rounds,
            DEFAULT_WORKBENCH_MAX_TOOL_ROUNDS,
        )?;
        let workbench_max_output_tokens = parse_optional_u32(
            "LIQUID_WORKBENCH_MAX_OUTPUT_TOKENS",
            get("LIQUID_WORKBENCH_MAX_OUTPUT_TOKENS"),
            file_workbench.max_output_tokens,
        )?;
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
        let managed_pool_max_connections = parse_u32(
            "LIQUID_SQL_MANAGED_POOL_MAX_CONNECTIONS",
            get("LIQUID_SQL_MANAGED_POOL_MAX_CONNECTIONS"),
            file_sql.managed_pool_max_connections,
            DEFAULT_SQL_MANAGED_POOL_MAX_CONNECTIONS,
        )?;
        let managed_pool_idle_ttl_seconds = parse_u64(
            "LIQUID_SQL_MANAGED_POOL_IDLE_TTL_SECONDS",
            get("LIQUID_SQL_MANAGED_POOL_IDLE_TTL_SECONDS"),
            file_sql.managed_pool_idle_ttl_seconds,
            DEFAULT_SQL_MANAGED_POOL_IDLE_TTL_SECONDS,
        )?;
        let managed_pool_reap_interval_seconds = parse_u64(
            "LIQUID_SQL_MANAGED_POOL_REAP_INTERVAL_SECONDS",
            get("LIQUID_SQL_MANAGED_POOL_REAP_INTERVAL_SECONDS"),
            file_sql.managed_pool_reap_interval_seconds,
            DEFAULT_SQL_MANAGED_POOL_REAP_INTERVAL_SECONDS,
        )?;
        let managed_pool_acquire_timeout_seconds = parse_u64(
            "LIQUID_SQL_MANAGED_POOL_ACQUIRE_TIMEOUT_SECONDS",
            get("LIQUID_SQL_MANAGED_POOL_ACQUIRE_TIMEOUT_SECONDS"),
            file_sql.managed_pool_acquire_timeout_seconds,
            DEFAULT_SQL_MANAGED_POOL_ACQUIRE_TIMEOUT_SECONDS,
        )?;
        let backup_s3_bucket = get("LIQUID_BACKUP_S3_BUCKET")
            .or(file_backup.s3_bucket)
            .and_then(non_empty);
        let backup_s3_prefix = env_or_file(
            get("LIQUID_BACKUP_S3_PREFIX"),
            file_backup.s3_prefix,
            DEFAULT_BACKUP_S3_PREFIX.to_owned(),
        );
        let backup_s3_region = env_or_file(
            get("LIQUID_BACKUP_S3_REGION"),
            file_backup.s3_region,
            DEFAULT_BACKUP_S3_REGION.to_owned(),
        );
        let backup_s3_endpoint = get("LIQUID_BACKUP_S3_ENDPOINT")
            .or(file_backup.s3_endpoint)
            .and_then(non_empty);
        let backup_s3_path_style = parse_bool(
            "LIQUID_BACKUP_S3_PATH_STYLE",
            get("LIQUID_BACKUP_S3_PATH_STYLE"),
            file_backup.s3_path_style,
            DEFAULT_BACKUP_S3_PATH_STYLE,
        )?;
        let backup_work_dir = env_or_file(
            get("LIQUID_BACKUP_WORK_DIR"),
            file_backup.work_dir,
            DEFAULT_BACKUP_WORK_DIR.to_owned(),
        );
        let backup_work_dir = expand_backup_work_dir(&backup_work_dir)?;
        let backup_worker_concurrency = parse_usize(
            "LIQUID_BACKUP_WORKER_CONCURRENCY",
            get("LIQUID_BACKUP_WORKER_CONCURRENCY"),
            file_backup.worker_concurrency,
            DEFAULT_BACKUP_WORKER_CONCURRENCY,
        )?;

        if token_ttl_seconds <= 0 {
            anyhow::bail!("LIQUID_AUTH_TOKEN_TTL_SECONDS must be positive");
        }
        if workbench_max_tool_rounds == 0 {
            anyhow::bail!("LIQUID_WORKBENCH_MAX_TOOL_ROUNDS must be positive");
        }
        if workbench_max_output_tokens == Some(0) {
            anyhow::bail!("LIQUID_WORKBENCH_MAX_OUTPUT_TOKENS must be positive");
        }
        if managed_pool_max_connections == 0 {
            anyhow::bail!("LIQUID_SQL_MANAGED_POOL_MAX_CONNECTIONS must be positive");
        }
        if managed_pool_idle_ttl_seconds == 0 {
            anyhow::bail!("LIQUID_SQL_MANAGED_POOL_IDLE_TTL_SECONDS must be positive");
        }
        if managed_pool_reap_interval_seconds == 0 {
            anyhow::bail!("LIQUID_SQL_MANAGED_POOL_REAP_INTERVAL_SECONDS must be positive");
        }
        if managed_pool_acquire_timeout_seconds == 0 {
            anyhow::bail!("LIQUID_SQL_MANAGED_POOL_ACQUIRE_TIMEOUT_SECONDS must be positive");
        }
        if backup_worker_concurrency == 0 {
            anyhow::bail!("LIQUID_BACKUP_WORKER_CONCURRENCY must be positive");
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
            managed_database_pool: ManagedDatabasePoolConfig {
                max_connections: managed_pool_max_connections,
                idle_ttl_seconds: managed_pool_idle_ttl_seconds,
                reap_interval_seconds: managed_pool_reap_interval_seconds,
                acquire_timeout_seconds: managed_pool_acquire_timeout_seconds,
            },
            database_backup: DatabaseBackupConfig {
                s3_bucket: backup_s3_bucket,
                s3_prefix: backup_s3_prefix,
                s3_region: backup_s3_region,
                s3_endpoint: backup_s3_endpoint,
                s3_path_style: backup_s3_path_style,
                work_dir: backup_work_dir,
                worker_concurrency: backup_worker_concurrency,
            },
            llm: LlmConfig {
                api_key,
                base_url,
                model,
                api_mode,
            },
            workbench: WorkbenchConfig {
                max_tool_rounds: workbench_max_tool_rounds,
                max_output_tokens: workbench_max_output_tokens,
            },
        })
    }
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

fn expand_backup_work_dir(value: &str) -> Result<String> {
    let Some(rest) = value.strip_prefix("~/") else {
        return Ok(value.to_owned());
    };

    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .context("could not determine user home directory for LIQUID_BACKUP_WORK_DIR")?;

    Ok(home.join(rest).display().to_string())
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

fn parse_optional_u32(
    env_name: &str,
    env_value: Option<String>,
    file_value: Option<u32>,
) -> Result<Option<u32>> {
    match env_value.and_then(non_empty) {
        Some(value) => value
            .parse()
            .map(Some)
            .with_context(|| format!("invalid {env_name}: {value}")),
        None => Ok(file_value),
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

fn parse_u64(
    env_name: &str,
    env_value: Option<String>,
    file_value: Option<u64>,
    default_value: u64,
) -> Result<u64> {
    match env_value.and_then(non_empty) {
        Some(value) => value
            .parse()
            .with_context(|| format!("invalid {env_name}: {value}")),
        None => Ok(file_value.unwrap_or(default_value)),
    }
}

fn parse_usize(
    env_name: &str,
    env_value: Option<String>,
    file_value: Option<usize>,
    default_value: usize,
) -> Result<usize> {
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
