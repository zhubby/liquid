use async_trait::async_trait;
use liquid_core::{
    ApproveSqlAuditRequest, AuthResponse, CreateManagedDatabaseRequest, LoginRequest,
    ManagedDatabase, ManagedDatabaseConnectionLoader, ManagedDatabaseConnectionLoaderError,
    ManagedDatabaseConnectionSpec, ManagedDatabasePoolKey, PublicUser, RegisterRequest,
    RejectSqlAuditRequest, SqlAuditExecutionResult, SqlAuditRecord, SqlAuditStatus,
    UpdateManagedDatabaseRequest,
};
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::{
    auth,
    crypto::PasswordCipher,
    error::StorageError,
    managed_databases,
    options::StorageOptions,
    sql_audits,
    traits::{CreateSqlAuditRecord, LiquidStore},
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

    pub fn decrypt_managed_database_password(
        &self,
        encrypted_password: &str,
    ) -> Result<String, StorageError> {
        self.cipher.decrypt(encrypted_password)
    }
}

#[async_trait]
impl ManagedDatabaseConnectionLoader for Storage {
    async fn load_managed_database_connection(
        &self,
        key: &ManagedDatabasePoolKey,
    ) -> Result<ManagedDatabaseConnectionSpec, ManagedDatabaseConnectionLoaderError> {
        managed_databases::load_managed_database_connection(self, key)
            .await
            .map_err(managed_database_loader_error)
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

    async fn list_managed_databases(
        &self,
        owner_user_id: &str,
    ) -> Result<Vec<ManagedDatabase>, StorageError> {
        managed_databases::list_managed_databases(self, owner_user_id).await
    }

    async fn create_managed_database(
        &self,
        owner_user_id: &str,
        request: CreateManagedDatabaseRequest,
    ) -> Result<ManagedDatabase, StorageError> {
        managed_databases::create_managed_database(self, owner_user_id, request).await
    }

    async fn update_managed_database(
        &self,
        owner_user_id: &str,
        id: &str,
        request: UpdateManagedDatabaseRequest,
    ) -> Result<ManagedDatabase, StorageError> {
        managed_databases::update_managed_database(self, owner_user_id, id, request).await
    }

    async fn delete_managed_database(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<(), StorageError> {
        managed_databases::delete_managed_database(self, owner_user_id, id).await
    }

    async fn create_sql_audit(
        &self,
        owner_user_id: &str,
        managed_database_id: &str,
        record: CreateSqlAuditRecord,
    ) -> Result<SqlAuditRecord, StorageError> {
        sql_audits::create_sql_audit(self, owner_user_id, managed_database_id, record).await
    }

    async fn list_sql_audits(
        &self,
        owner_user_id: &str,
        managed_database_id: Option<&str>,
        status: Option<SqlAuditStatus>,
        limit: i64,
    ) -> Result<Vec<SqlAuditRecord>, StorageError> {
        sql_audits::list_sql_audits(self, owner_user_id, managed_database_id, status, limit).await
    }

    async fn get_sql_audit(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<SqlAuditRecord, StorageError> {
        sql_audits::get_sql_audit(self, owner_user_id, id).await
    }

    async fn approve_sql_audit(
        &self,
        owner_user_id: &str,
        id: &str,
        request: ApproveSqlAuditRequest,
    ) -> Result<SqlAuditRecord, StorageError> {
        sql_audits::approve_sql_audit(self, owner_user_id, id, request).await
    }

    async fn reject_sql_audit(
        &self,
        owner_user_id: &str,
        id: &str,
        request: RejectSqlAuditRequest,
    ) -> Result<SqlAuditRecord, StorageError> {
        sql_audits::reject_sql_audit(self, owner_user_id, id, request).await
    }

    async fn start_sql_audit_execution(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<SqlAuditRecord, StorageError> {
        sql_audits::start_sql_audit_execution(self, owner_user_id, id).await
    }

    async fn complete_sql_audit_execution(
        &self,
        owner_user_id: &str,
        id: &str,
        result: SqlAuditExecutionResult,
    ) -> Result<SqlAuditRecord, StorageError> {
        sql_audits::complete_sql_audit_execution(self, owner_user_id, id, result).await
    }

    async fn fail_sql_audit_execution(
        &self,
        owner_user_id: &str,
        id: &str,
        error: String,
    ) -> Result<SqlAuditRecord, StorageError> {
        sql_audits::fail_sql_audit_execution(self, owner_user_id, id, error).await
    }
}

fn managed_database_loader_error(error: StorageError) -> ManagedDatabaseConnectionLoaderError {
    match error {
        StorageError::NotFound => ManagedDatabaseConnectionLoaderError::NotFound,
        StorageError::Conflict(message) => ManagedDatabaseConnectionLoaderError::Backend(message),
        StorageError::Validation(message) => {
            ManagedDatabaseConnectionLoaderError::InvalidConnection(message)
        }
        StorageError::Crypto(message) => ManagedDatabaseConnectionLoaderError::Secret(message),
        StorageError::Database(error) => {
            ManagedDatabaseConnectionLoaderError::Backend(error.to_string())
        }
        other => ManagedDatabaseConnectionLoaderError::Backend(other.to_string()),
    }
}
