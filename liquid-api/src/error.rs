use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use liquid_storage::{ManagedDatabasePoolError, StorageError};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub(crate) fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    pub(crate) fn internal(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }

    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    pub(crate) fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }
}

impl From<StorageError> for ApiError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::DuplicateEmail | StorageError::DuplicateManagedDatabaseName => Self {
                status: StatusCode::CONFLICT,
                message: error.to_string(),
            },
            StorageError::InvalidCredentials => Self {
                status: StatusCode::UNAUTHORIZED,
                message: error.to_string(),
            },
            StorageError::NotFound => Self {
                status: StatusCode::NOT_FOUND,
                message: error.to_string(),
            },
            StorageError::Conflict(_) => Self {
                status: StatusCode::CONFLICT,
                message: error.to_string(),
            },
            StorageError::Validation(_) => Self {
                status: StatusCode::BAD_REQUEST,
                message: error.to_string(),
            },
            StorageError::Database(_) | StorageError::Crypto(_) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "internal storage error".to_owned(),
            },
        }
    }
}

impl From<ManagedDatabasePoolError> for ApiError {
    fn from(error: ManagedDatabasePoolError) -> Self {
        match error {
            ManagedDatabasePoolError::NotFound => Self {
                status: StatusCode::NOT_FOUND,
                message: error.to_string(),
            },
            ManagedDatabasePoolError::Invalidated => Self {
                status: StatusCode::CONFLICT,
                message: error.to_string(),
            },
            ManagedDatabasePoolError::InvalidConnection(_) => Self {
                status: StatusCode::BAD_REQUEST,
                message: error.to_string(),
            },
            ManagedDatabasePoolError::Secret(_) | ManagedDatabasePoolError::Loader(_) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "managed database connection error".to_owned(),
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}
