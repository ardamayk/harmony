//! HTTP for playlists: parse the request, call the service, respond.

use super::repository::Playlist;
use super::service::{PlaylistDetail, PlaylistList, Service, ServiceError};
use crate::app::AppState;
use crate::auth;
use crate::gojson;
use crate::respond::{ApiError, ApiResult};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/playlists",
            get(list_playlists).post(create_playlist),
        )
        .route("/api/v1/playlists/{playlist_id}", get(get_playlist))
        .route("/api/v1/playlists/{playlist_id}/tracks", post(add_track))
        .route(
            "/api/v1/playlists/{playlist_id}/tracks/{track_id}",
            delete(remove_track),
        )
}
fn not_found(error: ServiceError) -> ApiError {
    match error {
        ServiceError::PlaylistNotFound => ApiError::not_found("playlist not found"),
        ServiceError::TrackNotFound => ApiError::not_found("track not found"),
        other => ApiError::internal(other),
    }
}

async fn list_playlists(State(state): State<AppState>) -> ApiResult<Json<PlaylistList>> {
    let list = Service::new(&state)
        .list(auth::current_user().id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(list))
}

#[derive(Default, Deserialize)]
struct CreateBody {
    #[serde(default, deserialize_with = "gojson::nullable")]
    name: String,
}

/// Any failure to create answers 400 with its message, as the Go handler does.
async fn create_playlist(
    State(state): State<AppState>,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<Playlist>)> {
    let body: CreateBody = gojson::decode_body(&body, "invalid JSON body")?;
    let playlist = Service::new(&state)
        .create(auth::current_user().id, &body.name)
        .await
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok((StatusCode::CREATED, Json(playlist)))
}

async fn get_playlist(
    State(state): State<AppState>,
    Path(playlist_id): Path<String>,
) -> ApiResult<Json<PlaylistDetail>> {
    let detail = Service::new(&state)
        .get(auth::current_user().id, &playlist_id)
        .await
        .map_err(not_found)?;
    Ok(Json(detail))
}

#[derive(Default, Deserialize)]
struct AddTrackBody {
    #[serde(rename = "trackId", default, deserialize_with = "gojson::nullable")]
    track_id: String,
}

async fn add_track(
    State(state): State<AppState>,
    Path(playlist_id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<PlaylistDetail>> {
    let body: AddTrackBody = gojson::decode_body(&body, "invalid JSON body")?;
    if body.track_id.is_empty() {
        return Err(ApiError::bad_request("trackId is required"));
    }
    let detail = Service::new(&state)
        .add_track(auth::current_user().id, &playlist_id, &body.track_id)
        .await
        .map_err(not_found)?;
    Ok(Json(detail))
}

async fn remove_track(
    State(state): State<AppState>,
    Path((playlist_id, track_id)): Path<(String, String)>,
) -> ApiResult<Json<PlaylistDetail>> {
    let detail = Service::new(&state)
        .remove_track(auth::current_user().id, &playlist_id, &track_id)
        .await
        .map_err(not_found)?;
    Ok(Json(detail))
}
