//! HTTP for Soulseek Acquisition: parse the request, call the service,
//! respond. Every route answers 404 `soulseek_acquisition_disabled` while
//! slskd is not configured.

use super::service::SoulseekError;
use crate::app::AppState;
use crate::respond::{ApiError, ApiResult};
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;

pub fn routes() -> Router<AppState> {
    // Every path under the prefix, until the routes themselves exist.
    Router::new().nest("/api/v1/soulseek", Router::new().fallback(unimplemented))
}

impl From<SoulseekError> for ApiError {
    fn from(error: SoulseekError) -> Self {
        let (status, code) = match &error {
            SoulseekError::Disabled => (StatusCode::NOT_FOUND, "soulseek_acquisition_disabled"),
            SoulseekError::NotImplemented => (StatusCode::NOT_IMPLEMENTED, "not_implemented"),
        };
        ApiError::new(status, code, error.to_string())
    }
}

async fn unimplemented(State(state): State<AppState>) -> ApiResult<Response> {
    state.soulseek.ensure_enabled()?;
    Err(SoulseekError::NotImplemented.into())
}
