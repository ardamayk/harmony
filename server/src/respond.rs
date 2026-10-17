//! JSON responses and the API error shape shared by every module
//! (server/internal/api/respond).

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ErrorBody {
    pub error: String,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub field: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

/// An API error: a status plus `{error, code, message, field?, reason?}`,
/// where `error` repeats `code` for older clients.
#[derive(Clone, Debug, PartialEq)]
pub struct ApiError {
    pub status: StatusCode,
    /// Boxed so `Result<T, ApiError>` stays small.
    pub body: Box<ErrorBody>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            body: Box::new(ErrorBody {
                error: code.to_owned(),
                code: code.to_owned(),
                message: message.into(),
                field: String::new(),
                reason: String::new(),
            }),
        }
    }

    pub fn with_field(mut self, field: &str) -> Self {
        self.body.field = field.to_owned();
        self
    }

    pub fn with_reason(mut self, reason: &str) -> Self {
        self.body.reason = reason.to_owned();
        self
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    /// Logs the cause and answers 500 without leaking it.
    pub fn internal(cause: impl std::fmt::Display) -> Self {
        tracing::error!(error = %cause, "request failed");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "internal server error",
        )
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        Self::internal(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(*self.body)).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// A JSON body with an explicit status.
pub fn json<T: Serialize>(status: StatusCode, value: T) -> Response {
    (status, Json(value)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_body_omits_empty_field_and_reason() {
        let body = serde_json::to_value(ApiError::bad_request("size is wrong").body).expect("json");
        assert_eq!(
            body,
            serde_json::json!({ "error": "bad_request", "code": "bad_request", "message": "size is wrong" })
        );
        let body = serde_json::to_value(
            ApiError::bad_request("x")
                .with_field("size")
                .with_reason("enum")
                .body,
        )
        .expect("json");
        assert_eq!(body["field"], "size");
        assert_eq!(body["reason"], "enum");
    }
}
