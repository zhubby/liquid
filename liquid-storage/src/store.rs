use async_trait::async_trait;
use liquid_core::{
    AuditedDatabase, AuthResponse, CreateAuditedDatabaseRequest, LoginRequest, PublicUser,
    RegisterRequest, UpdateAuditedDatabaseRequest,
};
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::{
    audited_databases, auth, crypto::PasswordCipher, error::StorageError, options::StorageOptions,
    traits::LiquidStore,
};

#[derive(Debug, Clone)]
pub struct Storage {
    pub(crate) pool: PgPool,
    pub(crate) token_ttl_seconds: i64,
    pub(crate) cipher: PasswordCipher,
}

impl Storage {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        Self::connect_with_options(StorageOptions::new(database_url)).await
    }

    pub async fn connect_with_options(options: StorageOptions) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(options.max_connections)
            .connect(&options.database_url)
            .await?;

        Ok(Self {
            pool,
            token_ttl_seconds: options.token_ttl_seconds,
            cipher: PasswordCipher::new(&options.encryption_key),
        })
    }

    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations").run(&self.pool).await
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn decrypt_audited_database_password(
        &self,
        encrypted_password: &str,
    ) -> Result<String, StorageError> {
        self.cipher.decrypt(encrypted_password)
    }
}

#[async_trait]
impl LiquidStore for Storage {
    async fn register_user(&self, request: RegisterRequest) -> Result<AuthResponse, StorageError> {
        auth::register_user(self, request).await
    }

    async fn login_user(&self, request: LoginRequest) -> Result<AuthResponse, StorageError> {
        auth::login_user(self, request).await
    }

    async fn authenticate_token(&self, token: &str) -> Result<Option<PublicUser>, StorageError> {
        auth::authenticate_token(self, token).await
    }

    async fn revoke_token(&self, token: &str) -> Result<(), StorageError> {
        auth::revoke_token(self, token).await
    }

    async fn list_audited_databases(
        &self,
        owner_user_id: &str,
    ) -> Result<Vec<AuditedDatabase>, StorageError> {
        audited_databases::list_audited_databases(self, owner_user_id).await
    }

    async fn create_audited_database(
        &self,
        owner_user_id: &str,
        request: CreateAuditedDatabaseRequest,
    ) -> Result<AuditedDatabase, StorageError> {
        audited_databases::create_audited_database(self, owner_user_id, request).await
    }

    async fn update_audited_database(
        &self,
        owner_user_id: &str,
        id: &str,
        request: UpdateAuditedDatabaseRequest,
    ) -> Result<AuditedDatabase, StorageError> {
        audited_databases::update_audited_database(self, owner_user_id, id, request).await
    }

    async fn delete_audited_database(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<(), StorageError> {
        audited_databases::delete_audited_database(self, owner_user_id, id).await
    }
}
