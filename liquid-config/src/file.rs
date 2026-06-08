use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct FileConfig {
    pub(crate) api: Option<FileApiConfig>,
    pub(crate) database: Option<FileDatabaseConfig>,
    pub(crate) auth: Option<FileAuthConfig>,
    pub(crate) security: Option<FileSecurityConfig>,
    pub(crate) llm: Option<FileLlmConfig>,
    pub(crate) sql: Option<FileSqlConfig>,
    pub(crate) backup: Option<FileBackupConfig>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct FileApiConfig {
    pub(crate) addr: Option<String>,
    pub(crate) cors_origin: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct FileDatabaseConfig {
    pub(crate) url: Option<String>,
    pub(crate) max_connections: Option<u32>,
    pub(crate) auto_migrate: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct FileAuthConfig {
    pub(crate) token_ttl_seconds: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct FileSecurityConfig {
    pub(crate) encryption_key: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct FileLlmConfig {
    pub(crate) api_key: Option<String>,
    pub(crate) base_url: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) api_mode: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct FileSqlConfig {
    pub(crate) metadata: Option<String>,
    pub(crate) execution: Option<String>,
    pub(crate) managed_pool_max_connections: Option<u32>,
    pub(crate) managed_pool_idle_ttl_seconds: Option<u64>,
    pub(crate) managed_pool_reap_interval_seconds: Option<u64>,
    pub(crate) managed_pool_acquire_timeout_seconds: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct FileBackupConfig {
    pub(crate) s3_bucket: Option<String>,
    pub(crate) s3_prefix: Option<String>,
    pub(crate) s3_region: Option<String>,
    pub(crate) s3_endpoint: Option<String>,
    pub(crate) s3_path_style: Option<bool>,
    pub(crate) work_dir: Option<String>,
    pub(crate) worker_concurrency: Option<usize>,
}

pub(crate) fn read_file_config(path: &Path) -> Result<FileConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file: {}", path.display()))?;

    toml::from_str(&content)
        .with_context(|| format!("failed to parse config file: {}", path.display()))
}
