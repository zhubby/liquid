use std::net::SocketAddr;

use crate::defaults::DEFAULT_WORKBENCH_MAX_TOOL_ROUNDS;
use crate::modes::{LlmApiMode, SqlExecutionMode, SqlMetadataMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidConfig {
    pub api_addr: SocketAddr,
    pub cors_origin: String,
    pub database: DatabaseConfig,
    pub auth: AuthConfig,
    pub security: SecurityConfig,
    pub sql_metadata: SqlMetadataMode,
    pub sql_execution: SqlExecutionMode,
    pub managed_database_pool: ManagedDatabasePoolConfig,
    pub database_backup: DatabaseBackupConfig,
    pub llm: LlmConfig,
    pub workbench: WorkbenchConfig,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkbenchConfig {
    pub max_tool_rounds: usize,
    pub max_output_tokens: Option<u32>,
}

impl Default for WorkbenchConfig {
    fn default() -> Self {
        Self {
            max_tool_rounds: DEFAULT_WORKBENCH_MAX_TOOL_ROUNDS,
            max_output_tokens: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedDatabasePoolConfig {
    pub max_connections: u32,
    pub idle_ttl_seconds: u64,
    pub reap_interval_seconds: u64,
    pub acquire_timeout_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseBackupConfig {
    pub s3_bucket: Option<String>,
    pub s3_prefix: String,
    pub s3_region: String,
    pub s3_endpoint: Option<String>,
    pub s3_path_style: bool,
    pub work_dir: String,
    pub worker_concurrency: usize,
}
