use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use liquid_storage::StorageError;
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
