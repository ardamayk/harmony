//! HTTP for Soulseek Acquisition: parse the request, call the service,
//! respond. Every route answers 404 `soulseek_acquisition_disabled` while
//! slskd is not configured.

use super::service::{Sharing, SoulseekError, Status};
use crate::app::AppState;
use crate::respond::{ApiError, ApiResult};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/soulseek/status", get(status))
        .route("/api/v1/soulseek/searches", post(unimplemented))
        .route("/api/v1/soulseek/searches/{search_id}", get(unimplemented))
        .route(
            "/api/v1/soulseek/acquisitions",
            get(unimplemented).post(unimplemented),
        )
        .route(
            "/api/v1/soulseek/acquisitions/{acquisition_id}",
            get(unimplemented).delete(unimplemented),
        )
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    slskd: SlskdStatus,
    soulseek: SoulseekLogin,
    sharing: SharingStatus,
}

#[derive(Serialize)]
struct SlskdStatus {
    reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SoulseekLogin {
    connected: bool,
    logged_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SharingStatus {
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared_file_count: Option<u64>,
}

async fn status(State(state): State<AppState>) -> ApiResult<Json<StatusResponse>> {
    let Status { slskd, sharing } = state.soulseek.status().await?;
    let (state, shared_file_count) = match sharing {
        Sharing::Shared { file_count } => ("shared", file_count),
        Sharing::NotShared => ("not_shared", None),
        Sharing::Unknown => ("unknown", None),
    };
    let reachable = slskd.is_some();
    let (version, soulseek) = match slskd {
        Some(application) => (
            application.version,
            SoulseekLogin {
                connected: application.connected,
                logged_in: application.logged_in,
                username: application.username.filter(|_| application.logged_in),
            },
        ),
        None => (
            None,
            SoulseekLogin {
                connected: false,
                logged_in: false,
                username: None,
            },
        ),
    };
    Ok(Json(StatusResponse {
        slskd: SlskdStatus { reachable, version },
        soulseek,
        sharing: SharingStatus {
            state,
            shared_file_count,
        },
    }))
}
