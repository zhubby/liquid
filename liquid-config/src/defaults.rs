pub(crate) const DEFAULT_API_ADDR: &str = "127.0.0.1:3001";
pub(crate) const DEFAULT_CORS_ORIGIN: &str = "http://localhost:3000";
pub(crate) const DEFAULT_DATABASE_URL: &str = "postgres://postgres:postgres@localhost:5432/liquid";
pub(crate) const DEFAULT_DATABASE_MAX_CONNECTIONS: u32 = 5;
pub(crate) const DEFAULT_DATABASE_AUTO_MIGRATE: bool = true;
pub(crate) const DEFAULT_AUTH_TOKEN_TTL_SECONDS: i64 = 60 * 60 * 24 * 7;
pub(crate) const DEFAULT_ENCRYPTION_KEY: &str = "liquid-development-encryption-key-change-me";
pub(crate) const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com";
pub(crate) const DEFAULT_WORKBENCH_MAX_TOOL_ROUNDS: usize = 10;
pub(crate) const DEFAULT_SQL_MANAGED_POOL_MAX_CONNECTIONS: u32 = 2;
pub(crate) const DEFAULT_SQL_MANAGED_POOL_IDLE_TTL_SECONDS: u64 = 10 * 60;
pub(crate) const DEFAULT_SQL_MANAGED_POOL_REAP_INTERVAL_SECONDS: u64 = 60;
pub(crate) const DEFAULT_SQL_MANAGED_POOL_ACQUIRE_TIMEOUT_SECONDS: u64 = 10;
pub(crate) const DEFAULT_BACKUP_S3_PREFIX: &str = "liquid/database-backups";
pub(crate) const DEFAULT_BACKUP_S3_REGION: &str = "us-east-1";
pub(crate) const DEFAULT_BACKUP_S3_PATH_STYLE: bool = false;
pub(crate) const DEFAULT_BACKUP_WORK_DIR: &str = "/tmp/liquid-backups";
pub(crate) const DEFAULT_BACKUP_WORKER_CONCURRENCY: usize = 1;

pub fn default_config_toml() -> String {
    format!(
        r#"[api]
addr = "{DEFAULT_API_ADDR}"
cors_origin = "{DEFAULT_CORS_ORIGIN}"

[database]
url = "{DEFAULT_DATABASE_URL}"
max_connections = {DEFAULT_DATABASE_MAX_CONNECTIONS}
auto_migrate = {DEFAULT_DATABASE_AUTO_MIGRATE}

[auth]
token_ttl_seconds = {DEFAULT_AUTH_TOKEN_TTL_SECONDS}

[security]
encryption_key = "{DEFAULT_ENCRYPTION_KEY}"

[llm]
base_url = "{DEFAULT_OPENAI_BASE_URL}"
api_mode = "chat_completions"

[workbench]
max_tool_rounds = {DEFAULT_WORKBENCH_MAX_TOOL_ROUNDS}
# max_output_tokens = 4000

[sql]
metadata = "auto"
execution = "readonly"
managed_pool_max_connections = {DEFAULT_SQL_MANAGED_POOL_MAX_CONNECTIONS}
managed_pool_idle_ttl_seconds = {DEFAULT_SQL_MANAGED_POOL_IDLE_TTL_SECONDS}
managed_pool_reap_interval_seconds = {DEFAULT_SQL_MANAGED_POOL_REAP_INTERVAL_SECONDS}
managed_pool_acquire_timeout_seconds = {DEFAULT_SQL_MANAGED_POOL_ACQUIRE_TIMEOUT_SECONDS}

[backup]
s3_prefix = "{DEFAULT_BACKUP_S3_PREFIX}"
s3_region = "{DEFAULT_BACKUP_S3_REGION}"
s3_path_style = {DEFAULT_BACKUP_S3_PATH_STYLE}
work_dir = "{DEFAULT_BACKUP_WORK_DIR}"
worker_concurrency = {DEFAULT_BACKUP_WORKER_CONCURRENCY}
"#
    )
}
