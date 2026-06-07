use std::{fmt, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ManagedDatabaseEngine {
    Postgres,
}

impl ManagedDatabaseEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ManagedDatabaseSslMode {
    Disable,
    #[default]
    Prefer,
    Require,
}

impl ManagedDatabaseSslMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Prefer => "prefer",
            Self::Require => "require",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct ManagedDatabase {
    pub id: String,
    pub name: String,
    pub engine: ManagedDatabaseEngine,
    pub host: String,
    pub port: i32,
    pub database: String,
    pub username: String,
    pub tags: Vec<String>,
    pub ssl_mode: ManagedDatabaseSslMode,
    pub has_password: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct CurrentManagedDatabaseResponse {
    pub database: Option<ManagedDatabase>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct SetCurrentManagedDatabaseRequest {
    pub managed_database_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct ManagedDatabaseConnectionTestResponse {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct CreateManagedDatabaseRequest {
    pub name: String,
    pub engine: ManagedDatabaseEngine,
    pub host: String,
    pub port: i32,
    pub database: String,
    pub username: String,
    pub password: String,
    #[serde(default)]
    #[ts(optional)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub ssl_mode: ManagedDatabaseSslMode,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub struct UpdateManagedDatabaseRequest {
    #[ts(optional)]
    pub name: Option<String>,
    #[ts(optional)]
    pub host: Option<String>,
    #[ts(optional)]
    pub port: Option<i32>,
    #[ts(optional)]
    pub database: Option<String>,
    #[ts(optional)]
    pub username: Option<String>,
    #[ts(optional)]
    pub password: Option<String>,
    #[ts(optional)]
    pub tags: Option<Vec<String>>,
    #[ts(optional)]
    pub ssl_mode: Option<ManagedDatabaseSslMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ManagedDatabasePoolKey {
    pub owner_user_id: String,
    pub database_id: String,
}

impl ManagedDatabasePoolKey {
    pub fn new(owner_user_id: impl Into<String>, database_id: impl Into<String>) -> Self {
        Self {
            owner_user_id: owner_user_id.into(),
            database_id: database_id.into(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ManagedDatabaseConnectionSpec {
    pub engine: ManagedDatabaseEngine,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    pub ssl_mode: ManagedDatabaseSslMode,
}

impl fmt::Debug for ManagedDatabaseConnectionSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagedDatabaseConnectionSpec")
            .field("engine", &self.engine)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("database", &self.database)
            .field("username", &self.username)
            .field("password", &"[redacted]")
            .field("ssl_mode", &self.ssl_mode)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedDatabasePoolPolicy {
    pub max_connections: u32,
    pub pool_idle_ttl: Duration,
    pub reap_interval: Duration,
    pub acquire_timeout: Duration,
    pub connection_idle_timeout: Duration,
    pub connection_max_lifetime: Duration,
}

impl Default for ManagedDatabasePoolPolicy {
    fn default() -> Self {
        Self {
            max_connections: 2,
            pool_idle_ttl: Duration::from_secs(10 * 60),
            reap_interval: Duration::from_secs(60),
            acquire_timeout: Duration::from_secs(10),
            connection_idle_timeout: Duration::from_secs(60),
            connection_max_lifetime: Duration::from_secs(30 * 60),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedDatabaseConnectionLoaderError {
    NotFound,
    InvalidConnection(String),
    Secret(String),
    Backend(String),
}

impl fmt::Display for ManagedDatabaseConnectionLoaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(formatter, "managed database not found"),
            Self::InvalidConnection(message) => write!(formatter, "{message}"),
            Self::Secret(message) => write!(formatter, "{message}"),
            Self::Backend(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for ManagedDatabaseConnectionLoaderError {}

#[async_trait]
pub trait ManagedDatabaseConnectionLoader: Send + Sync {
    async fn load_managed_database_connection(
        &self,
        key: &ManagedDatabasePoolKey,
    ) -> Result<ManagedDatabaseConnectionSpec, ManagedDatabaseConnectionLoaderError>;
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, time::Duration};

    use super::*;

    #[test]
    fn managed_database_pool_key_hashes_owner_and_database() {
        let key = ManagedDatabasePoolKey::new("user-1", "db-1");
        let same = ManagedDatabasePoolKey::new("user-1", "db-1");
        let different_owner = ManagedDatabasePoolKey::new("user-2", "db-1");

        let mut keys = HashSet::new();
        keys.insert(key);
        keys.insert(same);
        keys.insert(different_owner);

        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn managed_database_pool_policy_defaults_are_bounded() {
        let policy = ManagedDatabasePoolPolicy::default();

        assert_eq!(policy.max_connections, 2);
        assert_eq!(policy.pool_idle_ttl, Duration::from_secs(600));
        assert_eq!(policy.reap_interval, Duration::from_secs(60));
        assert_eq!(policy.acquire_timeout, Duration::from_secs(10));
        assert!(policy.connection_idle_timeout <= policy.pool_idle_ttl);
        assert!(policy.connection_max_lifetime > policy.connection_idle_timeout);
    }

    #[test]
    fn managed_database_connection_spec_carries_postgres_ssl_mode() {
        let spec = ManagedDatabaseConnectionSpec {
            engine: ManagedDatabaseEngine::Postgres,
            host: "db.internal".to_owned(),
            port: 5432,
            database: "warehouse".to_owned(),
            username: "readonly".to_owned(),
            password: "secret".to_owned(),
            ssl_mode: ManagedDatabaseSslMode::Require,
        };

        assert_eq!(spec.engine.as_str(), "postgres");
        assert_eq!(spec.ssl_mode.as_str(), "require");
    }

    #[test]
    fn managed_database_connection_spec_debug_redacts_password() {
        let spec = ManagedDatabaseConnectionSpec {
            engine: ManagedDatabaseEngine::Postgres,
            host: "db.internal".to_owned(),
            port: 5432,
            database: "warehouse".to_owned(),
            username: "readonly".to_owned(),
            password: "secret".to_owned(),
            ssl_mode: ManagedDatabaseSslMode::Require,
        };

        let debug = format!("{spec:?}");

        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("secret"));
    }
}
