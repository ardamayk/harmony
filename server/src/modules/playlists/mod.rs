//! Playlists (server/internal/modules/playlists). Every user has a default
//! Favorites playlist, and a user playlist disappears when its last Track
//! is removed.

mod repository;

use crate::app::AppState;
use crate::auth;
use crate::gojson;
use crate::modules::library::{LibraryReader, Track};
use crate::respond::{ApiError, ApiResult};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use repository::{Playlist, PlaylistError, Repository};
use serde::{Deserialize, Serialize};

pub const DEFAULT_FAVORITES_NAME: &str = "Favorites";

#[derive(Serialize)]
struct PlaylistList {
    items: Vec<Playlist>,
    total: usize,
}

#[derive(Clone, Serialize)]
pub struct PlaylistDetail {
    #[serde(flatten)]
    playlist: Playlist,
    tracks: Vec<Track>,
}

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

/// Playlist rules over the repository and the library.
pub struct Service {
    repo: Repository,
    tracks: LibraryReader,
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("playlist not found")]
    PlaylistNotFound,
    #[error("track not found")]
    TrackNotFound,
    #[error("playlist name is required")]
    NameRequired,
    /// A database error the create handler words like the Go driver.
    #[error("{}", gojson::sqlite_error(.0))]
    Create(sqlx::Error),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl From<PlaylistError> for ServiceError {
    fn from(error: PlaylistError) -> Self {
        match error {
            PlaylistError::NotFound => ServiceError::PlaylistNotFound,
            PlaylistError::Database(error) => ServiceError::Database(error),
        }
    }
}

impl Service {
    pub fn new(state: &AppState) -> Self {
        Self {
            repo: Repository::new(state.db.clone()),
            tracks: LibraryReader::new(state.db.clone()),
        }
    }

    pub async fn default_favorites(&self, user_id: &str) -> Result<Playlist, ServiceError> {
        Ok(self.repo.ensure_default(user_id).await?)
    }

    async fn list(&self, user_id: &str) -> Result<PlaylistList, ServiceError> {
        self.repo.ensure_default(user_id).await?;
        let items = self.repo.list(user_id).await?;
        Ok(PlaylistList {
            total: items.len(),
            items,
        })
    }

    async fn create(&self, user_id: &str, name: &str) -> Result<Playlist, ServiceError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ServiceError::NameRequired);
        }
        self.repo.ensure_default(user_id).await?;
        let id = self
            .repo
            .create(user_id, name)
            .await
            .map_err(ServiceError::Create)?;
        Ok(self.repo.get(user_id, &id).await?)
    }

    pub async fn get(
        &self,
        user_id: &str,
        playlist_id: &str,
    ) -> Result<PlaylistDetail, ServiceError> {
        self.repo.ensure_default(user_id).await?;
        let playlist = self.repo.get(user_id, playlist_id).await?;
        let mut tracks = Vec::new();
        for track_id in self.repo.track_ids(playlist_id).await? {
            // Hidden or removed Tracks are skipped, not errors.
            if let Ok(track) = self.tracks.get_track(&track_id).await {
                tracks.push(track);
            }
        }
        Ok(PlaylistDetail { playlist, tracks })
    }

    pub async fn add_track(
        &self,
        user_id: &str,
        playlist_id: &str,
        track_id: &str,
    ) -> Result<PlaylistDetail, ServiceError> {
        self.get(user_id, playlist_id).await?;
        // Any failure to read the Track counts as not found, as in Go.
        if self.tracks.get_track(track_id).await.is_err() {
            return Err(ServiceError::TrackNotFound);
        }
        self.repo.append_track(playlist_id, track_id).await?;
        self.get(user_id, playlist_id).await
    }

    pub async fn remove_track(
        &self,
        user_id: &str,
        playlist_id: &str,
        track_id: &str,
    ) -> Result<PlaylistDetail, ServiceError> {
        let mut detail = self.get(user_id, playlist_id).await?;
        self.repo.remove_track(playlist_id, track_id).await?;
        if detail.playlist.is_default || self.repo.count_tracks(playlist_id).await? > 0 {
            return self.get(user_id, playlist_id).await;
        }
        self.repo.delete_user_playlist(user_id, playlist_id).await?;
        detail.playlist.track_count = 0;
        detail.tracks = Vec::new();
        Ok(detail)
    }
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
