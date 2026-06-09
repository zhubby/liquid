use std::{error::Error, fmt};

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use liquid_storage::{ManagedDatabasePoolError, StorageError};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}

#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    message: String,
    details: Option<Value>,
}

impl ApiError {
    pub(crate) fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn internal(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
            details: None,
        }
    }

    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn conflict_with_details(message: impl Into<String>, details: Value) -> Self {
        Self::conflict(message).with_details(details)
    }

    pub(crate) fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl Error for ApiError {}

impl From<StorageError> for ApiError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::DuplicateEmail | StorageError::DuplicateManagedDatabaseName => Self {
                status: StatusCode::CONFLICT,
                message: error.to_string(),
                details: None,
            },
            StorageError::InvalidCredentials => Self {
                status: StatusCode::UNAUTHORIZED,
                message: error.to_string(),
                details: None,
            },
            StorageError::NotFound => Self {
                status: StatusCode::NOT_FOUND,
                message: error.to_string(),
                details: None,
            },
            StorageError::Conflict(_) => Self {
                status: StatusCode::CONFLICT,
                message: error.to_string(),
                details: None,
            },
            StorageError::Validation(_) => Self {
                status: StatusCode::BAD_REQUEST,
                message: error.to_string(),
                details: None,
            },
            StorageError::Database(_) | StorageError::Crypto(_) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "internal storage error".to_owned(),
                details: None,
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
                details: None,
            },
            ManagedDatabasePoolError::Invalidated => Self {
                status: StatusCode::CONFLICT,
                message: error.to_string(),
                details: None,
            },
            ManagedDatabasePoolError::InvalidConnection(_) => Self {
                status: StatusCode::BAD_REQUEST,
                message: error.to_string(),
                details: None,
            },
            ManagedDatabasePoolError::Secret(_) | ManagedDatabasePoolError::Loader(_) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "managed database connection error".to_owned(),
                details: None,
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
                details: self.details,
            }),
        )
            .into_response()
    }
}
