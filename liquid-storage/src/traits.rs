use async_trait::async_trait;
use liquid_core::{
    AuditedDatabase, AuthResponse, CreateAuditedDatabaseRequest, LoginRequest, PublicUser,
    RegisterRequest, UpdateAuditedDatabaseRequest,
};

use crate::error::StorageError;

#[async_trait]
pub trait LiquidStore: Send + Sync {
    async fn register_user(&self, request: RegisterRequest) -> Result<AuthResponse, StorageError>;
    async fn login_user(&self, request: LoginRequest) -> Result<AuthResponse, StorageError>;
    async fn authenticate_token(&self, token: &str) -> Result<Option<PublicUser>, StorageError>;
    async fn revoke_token(&self, token: &str) -> Result<(), StorageError>;
    async fn list_audited_databases(
        &self,
        owner_user_id: &str,
    ) -> Result<Vec<AuditedDatabase>, StorageError>;
    async fn create_audited_database(
        &self,
        owner_user_id: &str,
        request: CreateAuditedDatabaseRequest,
    ) -> Result<AuditedDatabase, StorageError>;
    async fn update_audited_database(
        &self,
        owner_user_id: &str,
        id: &str,
        request: UpdateAuditedDatabaseRequest,
    ) -> Result<AuditedDatabase, StorageError>;
    async fn delete_audited_database(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<(), StorageError>;
}
