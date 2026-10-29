//! Health and identity (server/internal/api/handlers.go).

use crate::app::AppState;
use crate::auth;
use crate::dependencies::{self, Dependency};
use axum::Json;
use axum::extract::State;
use serde::Serialize;

/// The named behaviors this release advertises to separately released
/// Playback Clients (ADR 0006). Every entry must be documented in the
/// HealthResponse contract; clients ignore unknown entries.
const SERVER_CAPABILITIES: &[&str] = &[
    "api.v1",
    "playback.queue-events.v1",
    "managed-import.v1",
    "managed-import-batches.v1",
    "managed-track-deletion.v1",
    "managed-track-replacement.v1",
    "managed-album-deletion.v1",
];

/// Advertised only when ffmpeg is present, since waveforms are decoded from
/// the managed file on first request.
pub const TRACK_WAVEFORM_CAPABILITY: &str = "track-waveform.v1";

/// Advertised only when the operator configured `EXTERNAL_SOURCE_SEARCH`
/// (ADR 0032). A missing yt-dlp shows as its Server Dependency instead.
pub const EXTERNAL_SOURCE_CAPABILITY: &str = "external-source.v1";

/// Advertised only when the operator configured `SLSKD_URL`, `SLSKD_API_KEY`
/// and `SLSKD_DOWNLOADS_PATH` (ADR 0036).
pub const SOULSEEK_ACQUISITION_CAPABILITY: &str = "soulseek-acquisition.v1";

pub fn server_capabilities(
    report: &dependencies::Report,
    external_source: bool,
    soulseek_acquisition: bool,
) -> Vec<String> {
    let mut capabilities: Vec<String> = SERVER_CAPABILITIES
        .iter()
        .map(|&capability| capability.to_owned())
        .collect();
    if report.has(dependencies::FFMPEG) {
        capabilities.push(TRACK_WAVEFORM_CAPABILITY.to_owned());
    }
    if external_source {
        capabilities.push(EXTERNAL_SOURCE_CAPABILITY.to_owned());
    }
    if soulseek_acquisition {
        capabilities.push(SOULSEEK_ACQUISITION_CAPABILITY.to_owned());
    }
    capabilities
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    status: &'static str,
    version: String,
    capabilities: Vec<String>,
    dependencies: Vec<Dependency>,
    /// Present only with `external-source.v1`.
    #[serde(skip_serializing_if = "Option::is_none")]
    external_source: Option<ExternalSourceInfo>,
}

/// What the clients show of the External Source (ADR 0032).
#[derive(Serialize)]
pub struct ExternalSourceInfo {
    name: String,
}

pub async fn get_health(State(state): State<AppState>) -> Json<HealthResponse> {
    let external_source_enabled = state.external_source.is_enabled();
    Json(HealthResponse {
        status: "ok",
        version: state.config.version.clone(),
        capabilities: server_capabilities(
            &state.dependencies,
            external_source_enabled,
            state.soulseek.is_enabled(),
        ),
        dependencies: state.dependencies.0.clone(),
        external_source: state
            .config
            .external_source
            .as_ref()
            .filter(|_| external_source_enabled)
            .map(|source| ExternalSourceInfo {
                name: source.name.clone(),
            }),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserResponse {
    id: &'static str,
    username: &'static str,
    #[serde(skip_serializing_if = "str::is_empty")]
    display_name: &'static str,
}

pub async fn get_me() -> Json<UserResponse> {
    let user = auth::current_user();
    Json(UserResponse {
        id: user.id,
        username: user.username,
        display_name: user.display_name,
    })
}
