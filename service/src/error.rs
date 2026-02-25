use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Serial port error: {0}")]
    Serial(#[from] tokio_serial::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("No reply from device (timeout)")]
    Timeout,

    #[error("Invalid reply from device")]
    InvalidReply,

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),
}

impl AppError {
    pub fn should_disconnect(&self) -> bool {
        matches!(self, AppError::Serial(_) | AppError::Io(_) | AppError::Timeout | AppError::InvalidReply)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Serial(_) | AppError::Io(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            AppError::Timeout => (StatusCode::GATEWAY_TIMEOUT, self.to_string()),
            AppError::InvalidReply => (StatusCode::BAD_GATEWAY, self.to_string()),
            AppError::InvalidParameter(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

pub type ApiResult<T> = Result<T, AppError>;
