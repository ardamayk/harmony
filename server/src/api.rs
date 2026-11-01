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
    "playback.shared-session.v1",
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

/// Advertised only when `GUEST_ADDR` and `PUBLIC_URL` give Guests an address
/// (ADR 0037).
pub const LISTEN_TOGETHER_CAPABILITY: &str = "listen-together.v1";

pub fn server_capabilities(
    report: &dependencies::Report,
    external_source: bool,
    listen_together: bool,
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
    if listen_together {
        capabilities.push(LISTEN_TOGETHER_CAPABILITY.to_owned());
    }
    capabilities
}

/// What a Guest's Web Client may rely on. Everything else in the full list
/// is a behavior the Guest Listener refuses.
fn guest_capabilities(report: &dependencies::Report) -> Vec<String> {
    let mut capabilities = vec!["api.v1".to_owned(), LISTEN_TOGETHER_CAPABILITY.to_owned()];
    if report.has(dependencies::FFMPEG) {
        capabilities.push(TRACK_WAVEFORM_CAPABILITY.to_owned());
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
            state.config.listen_together.is_some(),
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

/// Health on the Guest Listener: the deployment's dependencies and External
/// Source stay with the Owner.
pub async fn get_guest_health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: state.config.version.clone(),
        capabilities: guest_capabilities(&state.dependencies),
        dependencies: Vec::new(),
        external_source: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The Shared Playback State belongs to the Owner's devices only: a
    /// Guest's Web Client must not learn it is advertised, and the Guest
    /// Listener's gate refuses both routes (see `listen_together::gate`).
    #[test]
    fn the_shared_session_capability_stays_away_from_guests() {
        let capabilities = server_capabilities(&dependencies::Report(Vec::new()), false, false);
        assert!(capabilities.contains(&"playback.shared-session.v1".to_owned()));
        let guests = guest_capabilities(&dependencies::Report(Vec::new()));
        assert!(!guests.contains(&"playback.shared-session.v1".to_owned()));
    }
}
