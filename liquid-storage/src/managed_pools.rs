use std::{collections::HashMap, sync::Arc, time::Instant};

use async_trait::async_trait;
use liquid_core::{
    ManagedDatabaseConnectionLoader, ManagedDatabaseConnectionLoaderError,
    ManagedDatabaseConnectionSpec, ManagedDatabaseEngine, ManagedDatabasePoolKey,
    ManagedDatabasePoolPolicy, ManagedDatabaseSslMode,
};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
};
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};

#[derive(Debug, thiserror::Error)]
pub enum ManagedDatabasePoolError {
    #[error("managed database not found")]
    NotFound,
    #[error("managed database pool was invalidated while loading")]
    Invalidated,
    #[error("invalid managed database connection: {0}")]
    InvalidConnection(String),
    #[error("managed database secret error: {0}")]
    Secret(String),
    #[error("managed database connection loader failed: {0}")]
    Loader(String),
}

#[async_trait]
pub trait ManagedDatabasePoolConnector: Send + Sync {
    async fn connect(
        &self,
        spec: &ManagedDatabaseConnectionSpec,
        policy: &ManagedDatabasePoolPolicy,
    ) -> Result<PgPool, ManagedDatabasePoolError>;
}

impl From<ManagedDatabaseConnectionLoaderError> for ManagedDatabasePoolError {
    fn from(error: ManagedDatabaseConnectionLoaderError) -> Self {
        match error {
            ManagedDatabaseConnectionLoaderError::NotFound => Self::NotFound,
            ManagedDatabaseConnectionLoaderError::InvalidConnection(message) => {
                Self::InvalidConnection(message)
            }
            ManagedDatabaseConnectionLoaderError::Secret(message) => Self::Secret(message),
            ManagedDatabaseConnectionLoaderError::Backend(message) => Self::Loader(message),
        }
    }
}

#[derive(Clone)]
pub struct ManagedDatabasePoolManager {
    inner: Arc<ManagedDatabasePoolManagerInner>,
}

struct ManagedDatabasePoolManagerInner {
    loader: Arc<dyn ManagedDatabaseConnectionLoader>,
    connector: Arc<dyn ManagedDatabasePoolConnector>,
    policy: ManagedDatabasePoolPolicy,
    pools: Mutex<HashMap<ManagedDatabasePoolKey, ManagedDatabasePoolEntry>>,
}

enum ManagedDatabasePoolEntry {
    Ready(CachedManagedDatabasePool),
    Loading(watch::Sender<()>),
}

struct CachedManagedDatabasePool {
    pool: PgPool,
    last_used_at: Instant,
}

impl ManagedDatabasePoolManager {
    pub fn new(
        loader: Arc<dyn ManagedDatabaseConnectionLoader>,
        policy: ManagedDatabasePoolPolicy,
    ) -> Self {
        Self::with_connector(loader, Arc::new(SqlxManagedDatabasePoolConnector), policy)
    }

    pub fn with_connector(
        loader: Arc<dyn ManagedDatabaseConnectionLoader>,
        connector: Arc<dyn ManagedDatabasePoolConnector>,
        policy: ManagedDatabasePoolPolicy,
    ) -> Self {
        Self {
            inner: Arc::new(ManagedDatabasePoolManagerInner {
                loader,
                connector,
                policy,
                pools: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn policy(&self) -> &ManagedDatabasePoolPolicy {
        &self.inner.policy
    }

    pub fn spawn_reaper(&self) -> JoinHandle<()> {
        let manager = self.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(manager.policy().reap_interval);

            loop {
                interval.tick().await;
                manager.reap_idle().await;
            }
        })
    }

    pub async fn get_pool(
        &self,
        key: ManagedDatabasePoolKey,
    ) -> Result<PgPool, ManagedDatabasePoolError> {
        self.get_or_create_pool(key).await
    }

    pub async fn create_pool(
        &self,
        key: ManagedDatabasePoolKey,
    ) -> Result<PgPool, ManagedDatabasePoolError> {
        self.get_or_create_pool(key).await
    }

    async fn get_or_create_pool(
        &self,
        key: ManagedDatabasePoolKey,
    ) -> Result<PgPool, ManagedDatabasePoolError> {
        loop {
            match self.next_pool_action(&key).await {
                PoolAction::Use(pool) => return Ok(pool),
                PoolAction::Wait(mut receiver) => {
                    let _ = receiver.changed().await;
                }
                PoolAction::Load(sender) => return self.load_and_insert_pool(key, sender).await,
            }
        }
    }

    pub async fn invalidate(&self, key: &ManagedDatabasePoolKey) -> bool {
        let removed = self.inner.pools.lock().await.remove(key);

        match removed {
            Some(ManagedDatabasePoolEntry::Ready(cached)) => {
                close_pool_background(cached.pool);
                true
            }
            Some(ManagedDatabasePoolEntry::Loading(sender)) => {
                let _ = sender.send(());
                true
            }
            None => false,
        }
    }

    pub async fn reap_idle(&self) -> usize {
        let now = Instant::now();
        let pool_idle_ttl = self.policy().pool_idle_ttl;
        let mut removed = Vec::new();

        {
            let mut pools = self.inner.pools.lock().await;
            let expired_keys = pools
                .iter()
                .filter_map(|(key, entry)| match entry {
                    ManagedDatabasePoolEntry::Ready(cached)
                        if now.duration_since(cached.last_used_at) >= pool_idle_ttl =>
                    {
                        Some(key.clone())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();

            for key in expired_keys {
                if let Some(ManagedDatabasePoolEntry::Ready(cached)) = pools.remove(&key) {
                    removed.push(cached.pool);
                }
            }
        }

        let removed_count = removed.len();

        for pool in removed {
            close_pool_background(pool);
        }

        removed_count
    }

    pub async fn cached_pool_count(&self) -> usize {
        self.inner
            .pools
            .lock()
            .await
            .values()
            .filter(|entry| matches!(entry, ManagedDatabasePoolEntry::Ready(_)))
            .count()
    }

    async fn next_pool_action(&self, key: &ManagedDatabasePoolKey) -> PoolAction {
        let mut pools = self.inner.pools.lock().await;

        match pools.get_mut(key) {
            Some(ManagedDatabasePoolEntry::Ready(cached)) => {
                cached.last_used_at = Instant::now();
                PoolAction::Use(cached.pool.clone())
            }
            Some(ManagedDatabasePoolEntry::Loading(sender)) => PoolAction::Wait(sender.subscribe()),
            None => {
                let (sender, _) = watch::channel(());
                pools.insert(
                    key.clone(),
                    ManagedDatabasePoolEntry::Loading(sender.clone()),
                );
                PoolAction::Load(sender)
            }
        }
    }

    async fn load_and_insert_pool(
        &self,
        key: ManagedDatabasePoolKey,
        sender: watch::Sender<()>,
    ) -> Result<PgPool, ManagedDatabasePoolError> {
        let result = self.load_pool(&key).await;

        match result {
            Ok(pool) => {
                let insert_result = {
                    let mut pools = self.inner.pools.lock().await;
                    let still_loading = matches!(
                        pools.get(&key),
                        Some(ManagedDatabasePoolEntry::Loading(current))
                            if current.same_channel(&sender)
                    );

                    if still_loading {
                        pools.insert(
                            key,
                            ManagedDatabasePoolEntry::Ready(CachedManagedDatabasePool {
                                pool: pool.clone(),
                                last_used_at: Instant::now(),
                            }),
                        );
                        Ok(())
                    } else {
                        Err(ManagedDatabasePoolError::Invalidated)
                    }
                };

                let _ = sender.send(());

                match insert_result {
                    Ok(()) => Ok(pool),
                    Err(error) => {
                        close_pool_background(pool);
                        Err(error)
                    }
                }
            }
            Err(error) => {
                {
                    let mut pools = self.inner.pools.lock().await;
                    let still_loading = matches!(
                        pools.get(&key),
                        Some(ManagedDatabasePoolEntry::Loading(current))
                            if current.same_channel(&sender)
                    );

                    if still_loading {
                        pools.remove(&key);
                    }
                }

                let _ = sender.send(());
                Err(error)
            }
        }
    }

    async fn load_pool(
        &self,
        key: &ManagedDatabasePoolKey,
    ) -> Result<PgPool, ManagedDatabasePoolError> {
        let spec = self
            .inner
            .loader
            .load_managed_database_connection(key)
            .await?;

        match spec.engine {
            ManagedDatabaseEngine::Postgres => {
                self.inner.connector.connect(&spec, self.policy()).await
            }
        }
    }
}

enum PoolAction {
    Use(PgPool),
    Wait(watch::Receiver<()>),
    Load(watch::Sender<()>),
}

#[derive(Debug, Default)]
struct SqlxManagedDatabasePoolConnector;

#[async_trait]
impl ManagedDatabasePoolConnector for SqlxManagedDatabasePoolConnector {
    async fn connect(
        &self,
        spec: &ManagedDatabaseConnectionSpec,
        policy: &ManagedDatabasePoolPolicy,
    ) -> Result<PgPool, ManagedDatabasePoolError> {
        postgres_pool(spec, policy).await
    }
}

async fn postgres_pool(
    spec: &ManagedDatabaseConnectionSpec,
    policy: &ManagedDatabasePoolPolicy,
) -> Result<PgPool, ManagedDatabasePoolError> {
    let options = PgConnectOptions::new_without_pgpass()
        .host(&spec.host)
        .port(spec.port)
        .username(&spec.username)
        .password(&spec.password)
        .database(&spec.database)
        .ssl_mode(pg_ssl_mode(spec.ssl_mode))
        .application_name("liquid-managed-database");

    let pool = PgPoolOptions::new()
        .max_connections(policy.max_connections.max(1))
        .min_connections(0)
        .acquire_timeout(policy.acquire_timeout)
        .idle_timeout(Some(policy.connection_idle_timeout))
        .max_lifetime(Some(policy.connection_max_lifetime))
        .test_before_acquire(true)
        .connect_with(options)
        .await
        .map_err(|error| connection_error(spec, error))?;

    let connection = pool
        .acquire()
        .await
        .map_err(|error| connection_error(spec, error))?;
    drop(connection);

    Ok(pool)
}

fn pg_ssl_mode(mode: ManagedDatabaseSslMode) -> PgSslMode {
    match mode {
        ManagedDatabaseSslMode::Disable => PgSslMode::Disable,
        ManagedDatabaseSslMode::Prefer => PgSslMode::Prefer,
        ManagedDatabaseSslMode::Require => PgSslMode::Require,
    }
}

fn connection_error(
    spec: &ManagedDatabaseConnectionSpec,
    error: sqlx::Error,
) -> ManagedDatabasePoolError {
    let mut message = error.to_string();

    if !spec.password.is_empty() {
        message = message.replace(&spec.password, "[redacted]");
    }

    ManagedDatabasePoolError::InvalidConnection(message)
}

fn close_pool_background(pool: PgPool) {
    tokio::spawn(async move {
        pool.close().await;
    });
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use async_trait::async_trait;
    use liquid_core::ManagedDatabaseConnectionLoaderError;

    use super::*;

    #[derive(Default)]
    struct FakeConnectionLoader {
        calls: AtomicUsize,
        delay: Duration,
    }

    #[derive(Default)]
    struct FakePoolConnector {
        calls: AtomicUsize,
        fail: AtomicBool,
    }

    #[async_trait]
    impl ManagedDatabaseConnectionLoader for FakeConnectionLoader {
        async fn load_managed_database_connection(
            &self,
            _key: &ManagedDatabasePoolKey,
        ) -> Result<ManagedDatabaseConnectionSpec, ManagedDatabaseConnectionLoaderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);

            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }

            Ok(test_spec())
        }
    }

    #[async_trait]
    impl ManagedDatabasePoolConnector for FakePoolConnector {
        async fn connect(
            &self,
            spec: &ManagedDatabaseConnectionSpec,
            policy: &ManagedDatabasePoolPolicy,
        ) -> Result<PgPool, ManagedDatabasePoolError> {
            self.calls.fetch_add(1, Ordering::SeqCst);

            if self.fail.load(Ordering::SeqCst) {
                return Err(ManagedDatabasePoolError::InvalidConnection(
                    "could not connect".to_owned(),
                ));
            }

            Ok(lazy_test_pool(spec, policy))
        }
    }

    #[tokio::test]
    async fn get_pool_loads_lazily_and_reuses_cached_pool() {
        let loader = Arc::new(FakeConnectionLoader::default());
        let connector = Arc::new(FakePoolConnector::default());
        let manager = test_manager(loader.clone(), connector.clone());
        let key = test_key();

        let first = manager.get_pool(key.clone()).await.unwrap();
        let second = manager.get_pool(key).await.unwrap();

        assert_eq!(loader.calls.load(Ordering::SeqCst), 1);
        assert_eq!(connector.calls.load(Ordering::SeqCst), 1);
        assert_eq!(manager.cached_pool_count().await, 1);
        assert_eq!(first.connect_options().get_database(), Some("warehouse"));
        assert_eq!(second.connect_options().get_database(), Some("warehouse"));
    }

    #[tokio::test]
    async fn create_pool_actively_loads_and_caches_pool() {
        let loader = Arc::new(FakeConnectionLoader::default());
        let connector = Arc::new(FakePoolConnector::default());
        let manager = test_manager(loader.clone(), connector.clone());
        let key = test_key();

        let pool = manager.create_pool(key.clone()).await.unwrap();
        let cached = manager.get_pool(key).await.unwrap();

        assert_eq!(loader.calls.load(Ordering::SeqCst), 1);
        assert_eq!(connector.calls.load(Ordering::SeqCst), 1);
        assert_eq!(pool.connect_options().get_database(), Some("warehouse"));
        assert_eq!(cached.connect_options().get_database(), Some("warehouse"));
    }

    #[tokio::test]
    async fn concurrent_get_pool_only_loads_once_for_same_key() {
        let loader = Arc::new(FakeConnectionLoader {
            calls: AtomicUsize::new(0),
            delay: Duration::from_millis(25),
        });
        let connector = Arc::new(FakePoolConnector::default());
        let manager = test_manager(loader.clone(), connector.clone());
        let key = test_key();

        let (first, second) =
            tokio::join!(manager.get_pool(key.clone()), manager.get_pool(key.clone()));

        assert!(first.unwrap().connect_options().get_database().is_some());
        assert!(second.unwrap().connect_options().get_database().is_some());
        assert_eq!(loader.calls.load(Ordering::SeqCst), 1);
        assert_eq!(connector.calls.load(Ordering::SeqCst), 1);
        assert_eq!(manager.cached_pool_count().await, 1);
    }

    #[tokio::test]
    async fn invalidate_removes_and_closes_ready_pool() {
        let loader = Arc::new(FakeConnectionLoader::default());
        let connector = Arc::new(FakePoolConnector::default());
        let manager = test_manager(loader, connector);
        let key = test_key();
        let pool = manager.get_pool(key.clone()).await.unwrap();

        assert!(!pool.is_closed());

        assert!(manager.invalidate(&key).await);

        wait_until_pool_closed(&pool).await;
        assert_eq!(manager.cached_pool_count().await, 0);
    }

    #[tokio::test]
    async fn connector_failure_is_not_cached() {
        let loader = Arc::new(FakeConnectionLoader::default());
        let connector = Arc::new(FakePoolConnector::default());
        connector.fail.store(true, Ordering::SeqCst);
        let manager = test_manager(loader.clone(), connector.clone());
        let key = test_key();

        let error = manager.get_pool(key.clone()).await.unwrap_err();

        assert!(matches!(
            error,
            ManagedDatabasePoolError::InvalidConnection(_)
        ));
        assert_eq!(loader.calls.load(Ordering::SeqCst), 1);
        assert_eq!(connector.calls.load(Ordering::SeqCst), 1);
        assert_eq!(manager.cached_pool_count().await, 0);

        connector.fail.store(false, Ordering::SeqCst);
        manager.get_pool(key).await.unwrap();

        assert_eq!(loader.calls.load(Ordering::SeqCst), 2);
        assert_eq!(connector.calls.load(Ordering::SeqCst), 2);
        assert_eq!(manager.cached_pool_count().await, 1);
    }

    #[tokio::test]
    async fn reap_idle_closes_expired_pools() {
        let loader = Arc::new(FakeConnectionLoader::default());
        let connector = Arc::new(FakePoolConnector::default());
        let manager = ManagedDatabasePoolManager::with_connector(
            loader,
            connector,
            ManagedDatabasePoolPolicy {
                pool_idle_ttl: Duration::from_millis(1),
                ..test_policy()
            },
        );
        let key = test_key();
        let pool = manager.get_pool(key).await.unwrap();

        tokio::time::sleep(Duration::from_millis(5)).await;

        assert_eq!(manager.reap_idle().await, 1);
        wait_until_pool_closed(&pool).await;
        assert_eq!(manager.cached_pool_count().await, 0);
    }

    fn test_manager(
        loader: Arc<FakeConnectionLoader>,
        connector: Arc<FakePoolConnector>,
    ) -> ManagedDatabasePoolManager {
        ManagedDatabasePoolManager::with_connector(loader, connector, test_policy())
    }

    fn test_key() -> ManagedDatabasePoolKey {
        ManagedDatabasePoolKey::new("user-1", "db-1")
    }

    fn test_spec() -> ManagedDatabaseConnectionSpec {
        ManagedDatabaseConnectionSpec {
            engine: ManagedDatabaseEngine::Postgres,
            host: "localhost".to_owned(),
            port: 1,
            database: "warehouse".to_owned(),
            username: "readonly".to_owned(),
            password: "secret".to_owned(),
            ssl_mode: ManagedDatabaseSslMode::Disable,
        }
    }

    fn test_policy() -> ManagedDatabasePoolPolicy {
        ManagedDatabasePoolPolicy {
            acquire_timeout: Duration::from_millis(50),
            connection_idle_timeout: Duration::from_millis(50),
            connection_max_lifetime: Duration::from_secs(1),
            ..ManagedDatabasePoolPolicy::default()
        }
    }

    fn lazy_test_pool(
        spec: &ManagedDatabaseConnectionSpec,
        policy: &ManagedDatabasePoolPolicy,
    ) -> PgPool {
        let options = PgConnectOptions::new_without_pgpass()
            .host(&spec.host)
            .port(spec.port)
            .username(&spec.username)
            .password(&spec.password)
            .database(&spec.database)
            .ssl_mode(pg_ssl_mode(spec.ssl_mode))
            .application_name("liquid-managed-database-test");

        PgPoolOptions::new()
            .max_connections(policy.max_connections.max(1))
            .min_connections(0)
            .acquire_timeout(policy.acquire_timeout)
            .idle_timeout(Some(policy.connection_idle_timeout))
            .max_lifetime(Some(policy.connection_max_lifetime))
            .test_before_acquire(true)
            .connect_lazy_with(options)
    }

    async fn wait_until_pool_closed(pool: &PgPool) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while !pool.is_closed() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
}
