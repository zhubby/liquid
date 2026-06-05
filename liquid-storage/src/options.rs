const DEFAULT_TOKEN_TTL_SECONDS: i64 = 60 * 60 * 24 * 7;
const DEFAULT_ENCRYPTION_KEY: &str = "liquid-development-encryption-key-change-me";

#[derive(Debug, Clone)]
pub struct StorageOptions {
    pub database_url: String,
    pub max_connections: u32,
    pub token_ttl_seconds: i64,
    pub encryption_key: String,
}

impl StorageOptions {
    pub fn new(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            max_connections: 5,
            token_ttl_seconds: DEFAULT_TOKEN_TTL_SECONDS,
            encryption_key: DEFAULT_ENCRYPTION_KEY.to_owned(),
        }
    }

    pub fn with_max_connections(mut self, max_connections: u32) -> Self {
        self.max_connections = max_connections;
        self
    }

    pub fn with_token_ttl_seconds(mut self, token_ttl_seconds: i64) -> Self {
        self.token_ttl_seconds = token_ttl_seconds;
        self
    }

    pub fn with_encryption_key(mut self, encryption_key: impl Into<String>) -> Self {
        self.encryption_key = encryption_key.into();
        self
    }
}
