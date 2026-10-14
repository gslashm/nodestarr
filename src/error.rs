//! Application error type and its HTTP mapping.
//!
//! Every API handler returns [`Result<T>`]; errors become a JSON body of the form
//! `{"error": {"code": "...", "message": "..."}}` with a matching status code.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

pub type Result<T, E = AppError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("authentication required")]
    Unauthorized,
    #[error("{0}")]
    Forbidden(String),
    #[error("{0} not found")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("too many attempts, try again later")]
    RateLimited,
    /// A call to something outside Berth failed (registry, Proxmox, an app API).
    #[error("{0}")]
    Upstream(String),
    #[error("Docker: {0}")]
    Docker(String),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl AppError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }
    pub fn not_found(what: impl Into<String>) -> Self {
        Self::NotFound(what.into())
    }
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::Forbidden(msg.into())
    }
    pub fn upstream(msg: impl Into<String>) -> Self {
        Self::Upstream(msg.into())
    }

    pub fn status(&self) -> StatusCode {
        match self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            AppError::Upstream(_) => StatusCode::BAD_GATEWAY,
            AppError::Docker(_) => StatusCode::BAD_GATEWAY,
            AppError::Db(_) | AppError::Io(_) | AppError::Other(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            AppError::BadRequest(_) => "bad_request",
            AppError::Unauthorized => "unauthorized",
            AppError::Forbidden(_) => "forbidden",
            AppError::NotFound(_) => "not_found",
            AppError::Conflict(_) => "conflict",
            AppError::RateLimited => "rate_limited",
            AppError::Upstream(_) => "upstream_error",
            AppError::Docker(_) => "docker_error",
            AppError::Db(_) => "database_error",
            AppError::Io(_) => "io_error",
            AppError::Other(_) => "internal_error",
        }
    }
}

impl From<bollard::errors::Error> for AppError {
    fn from(err: bollard::errors::Error) -> Self {
        use bollard::errors::Error as E;
        match err {
            E::DockerResponseServerError {
                status_code,
                message,
            } => match status_code {
                400 => AppError::BadRequest(message),
                404 => AppError::NotFound(message),
                409 => AppError::Conflict(message),
                _ => AppError::Docker(message),
            },
            other => AppError::Docker(other.to_string()),
        }
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        AppError::Upstream(crate::util::describe_reqwest_error(&err))
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::BadRequest(format!("invalid JSON: {err}"))
    }
}

impl From<tokio::task::JoinError> for AppError {
    fn from(err: tokio::task::JoinError) -> Self {
        AppError::Other(anyhow::anyhow!("background task failed: {err}"))
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        if status.is_server_error() {
            tracing::warn!(error = %self, "request failed");
        }
        let body = json!({ "error": { "code": self.code(), "message": self.to_string() } });
        (status, Json(body)).into_response()
    }
}

/// Returns true when a Docker error just means "already in that state"
/// (HTTP 304), which container actions treat as success.
pub fn is_not_modified(err: &bollard::errors::Error) -> bool {
    matches!(
        err,
        bollard::errors::Error::DockerResponseServerError {
            status_code: 304,
            ..
        }
    )
}

/// True for Docker "no such container/image" errors.
pub fn is_not_found(err: &bollard::errors::Error) -> bool {
    matches!(
        err,
        bollard::errors::Error::DockerResponseServerError {
            status_code: 404,
            ..
        }
    )
}
