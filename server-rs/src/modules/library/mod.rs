//! Library reads: Artists, Albums, Tracks, covers, lyrics and waveforms
//! (server/internal/modules/library). Ingestion, deletion and replacement
//! belong to Managed Import.

mod covers;
mod genres;
mod repository;
mod types;
mod waveform;

use crate::app::AppState;
use crate::respond::{ApiError, ApiResult};
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
pub use genres::split_genres;
pub use repository::{LibraryError, Repository};
use serde::Deserialize;
use std::collections::HashMap;
pub use types::Track;
use waveform::WaveformError;
pub use waveform::WaveformService;

/// What other modules may read from the library (Go's TrackAccess).
#[derive(Clone)]
pub struct LibraryReader {
    repo: Repository,
}

impl LibraryReader {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self {
            repo: Repository::new(pool),
        }
    }

    pub async fn get_track(&self, track_id: &str) -> Result<Track, LibraryError> {
        self.repo.get_track(track_id).await
    }

    pub async fn get_track_file_path(&self, track_id: &str) -> Result<String, LibraryError> {
        self.repo.get_track_file_path(track_id).await
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/library/artists", get(list_artists))
        .route("/api/v1/library/albums", get(list_albums))
        .route(
            "/api/v1/library/albums/{album_id}/cover",
            get(get_album_cover),
        )
        .route("/api/v1/library/albums/{album_id}", get(get_album))
        .route("/api/v1/library/tracks", get(list_tracks))
        .route("/api/v1/library/tracks/{track_id}", get(get_track))
        .route(
            "/api/v1/library/tracks/{track_id}/lyrics",
            get(get_track_lyrics),
        )
        .route(
            "/api/v1/library/tracks/{track_id}/waveform",
            get(get_track_waveform),
        )
}

/// `limit` 1..=500 (default 50) and `offset` >= 0; invalid values fall back
/// to the defaults rather than failing, like the Go server.
fn pagination(query: &HashMap<String, String>) -> (i64, i64) {
    let number = |key: &str| query.get(key).and_then(|value| value.parse::<i64>().ok());
    let limit = number("limit")
        .filter(|limit| (1..=500).contains(limit))
        .unwrap_or(50);
    let offset = number("offset").filter(|offset| *offset >= 0).unwrap_or(0);
    (limit, offset)
}

fn text<'a>(query: &'a HashMap<String, String>, key: &str) -> &'a str {
    query.get(key).map(String::as_str).unwrap_or_default()
}

fn not_found(message: &str) -> impl FnOnce(LibraryError) -> ApiError + '_ {
    move |error| match error {
        LibraryError::NotFound => ApiError::not_found(message),
        other => ApiError::internal(other),
    }
}

async fn list_artists(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult<Json<types::Page<types::Artist>>> {
    let (limit, offset) = pagination(&query);
    let page = Repository::new(state.db)
        .list_artists(limit, offset, text(&query, "q"))
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(page))
}

async fn list_albums(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult<Json<types::Page<types::Album>>> {
    let (limit, offset) = pagination(&query);
    let page = Repository::new(state.db)
        .list_albums(limit, offset, text(&query, "artistId"), text(&query, "q"))
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(page))
}

async fn get_album(
    State(state): State<AppState>,
    Path(album_id): Path<String>,
) -> ApiResult<Json<types::AlbumDetail>> {
    let album = Repository::new(state.db)
        .get_album(&album_id)
        .await
        .map_err(not_found("album not found"))?;
    Ok(Json(album))
}

#[derive(Deserialize)]
struct CoverQuery {
    size: Option<String>,
}

async fn get_album_cover(
    State(state): State<AppState>,
    Path(album_id): Path<String>,
    Query(query): Query<CoverQuery>,
) -> ApiResult<Response> {
    let size = match query.size.as_deref() {
        None | Some("") => 0,
        Some(raw) => raw
            .parse::<i64>()
            .ok()
            .filter(|size| covers::COVER_SIZES.contains(size))
            .ok_or_else(|| ApiError::bad_request("size must be one of 64, 128, 256, 512, 1024"))?,
    };
    let cover = covers::album_cover(&Repository::new(state.db), &album_id, size)
        .await
        .map_err(not_found("album cover not found"))?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, cover.media_type),
            (header::CACHE_CONTROL, "public, max-age=86400".to_owned()),
        ],
        cover.data,
    )
        .into_response())
}

async fn list_tracks(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult<Json<types::Page<Track>>> {
    let (limit, offset) = pagination(&query);
    let page = Repository::new(state.db)
        .list_tracks(limit, offset, text(&query, "artistId"), text(&query, "q"))
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(page))
}

async fn get_track(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
) -> ApiResult<Json<Track>> {
    let track = Repository::new(state.db)
        .get_track(&track_id)
        .await
        .map_err(not_found("track not found"))?;
    Ok(Json(track))
}

async fn get_track_lyrics(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
) -> ApiResult<Json<types::TrackLyrics>> {
    let lyrics = Repository::new(state.db)
        .get_track_lyrics(&track_id)
        .await
        .map_err(not_found("track not found"))?;
    Ok(Json(types::TrackLyrics { track_id, lyrics }))
}

/// Cached peaks, 202 while they are generated, 404 for an unknown Track or
/// a missing file, and 503 without ffmpeg.
async fn get_track_waveform(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
) -> Response {
    match state.waveforms.get(&track_id).await {
        Ok(waveform) => (
            [(header::CACHE_CONTROL, "private, no-cache")],
            Json(waveform),
        )
            .into_response(),
        Err(WaveformError::Pending) => (
            StatusCode::ACCEPTED,
            [(header::RETRY_AFTER, "2")],
            Json(types::TrackWaveformPending {
                status: "pending",
                retry_after_seconds: 2,
            }),
        )
            .into_response(),
        Err(WaveformError::Library(LibraryError::NotFound)) => {
            ApiError::not_found("track or track file not found").into_response()
        }
        Err(WaveformError::Unavailable) => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "dependency_unavailable",
            "waveforms need ffmpeg on the Music Server",
        )
        .into_response(),
        Err(other) => ApiError::internal(other).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_falls_back_to_defaults() {
        let query = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect()
        };
        assert_eq!(pagination(&query(&[])), (50, 0));
        assert_eq!(
            pagination(&query(&[("limit", "100"), ("offset", "20")])),
            (100, 20)
        );
        assert_eq!(
            pagination(&query(&[("limit", "0"), ("offset", "-1")])),
            (50, 0)
        );
        assert_eq!(
            pagination(&query(&[("limit", "501"), ("offset", "x")])),
            (50, 0)
        );
    }
}
