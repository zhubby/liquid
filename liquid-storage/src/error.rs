#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("email already registered")]
    DuplicateEmail,
    #[error("managed database name already exists")]
    DuplicateManagedDatabaseName,
    #[error("invalid email or password")]
    InvalidCredentials,
    #[error("record not found")]
    NotFound,
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    Database(#[source] sqlx::Error),
    #[error("{0}")]
    Crypto(String),
}

impl From<sqlx::Error> for StorageError {
    fn from(error: sqlx::Error) -> Self {
        map_database_error(error)
    }
}

pub(crate) fn map_database_error(error: sqlx::Error) -> StorageError {
    let sqlx::Error::Database(database_error) = &error else {
        return StorageError::Database(error);
    };

    if database_error.code().as_deref() == Some("23505") {
        return match database_error.constraint() {
            Some("users_email_unique_idx") => StorageError::DuplicateEmail,
            Some("managed_databases_owner_name_unique_idx") => {
                StorageError::DuplicateManagedDatabaseName
            }
            _ => StorageError::Database(error),
        };
    }

    StorageError::Database(error)
}
