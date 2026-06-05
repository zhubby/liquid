use async_trait::async_trait;
use liquid_core::{
    AuthResponse, CreateManagedDatabaseRequest, LoginRequest, ManagedDatabase, PublicUser,
    RegisterRequest, UpdateManagedDatabaseRequest,
};

use crate::error::StorageError;

#[async_trait]
pub trait LiquidStore: Send + Sync {
    async fn register_user(&self, request: RegisterRequest) -> Result<AuthResponse, StorageError>;
    async fn login_user(&self, request: LoginRequest) -> Result<AuthResponse, StorageError>;
    async fn authenticate_token(&self, token: &str) -> Result<Option<PublicUser>, StorageError>;
    async fn revoke_token(&self, token: &str) -> Result<(), StorageError>;
    async fn list_managed_databases(
        &self,
        owner_user_id: &str,
    ) -> Result<Vec<ManagedDatabase>, StorageError>;
    async fn create_managed_database(
        &self,
        owner_user_id: &str,
        request: CreateManagedDatabaseRequest,
    ) -> Result<ManagedDatabase, StorageError>;
    async fn update_managed_database(
        &self,
        owner_user_id: &str,
        id: &str,
        request: UpdateManagedDatabaseRequest,
    ) -> Result<ManagedDatabase, StorageError>;
    async fn delete_managed_database(
        &self,
        owner_user_id: &str,
        id: &str,
    ) -> Result<(), StorageError>;
}
