//! HTTP for library reads: parse the request, call the service, respond.

use super::covers::{self, LARGEST_COVER_SIZE};
use super::repository::LibraryError;
use super::service::Service;
use super::types::{self, Track};
use super::waveform::WaveformError;
use crate::app::AppState;
use crate::modules::artist_info::ArtistInfoError;
use crate::query::GoQuery;
use crate::respond::{ApiError, ApiResult};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/library/artists", get(list_artists))
        .route("/api/v1/library/artists/{artist_id}", get(get_artist))
        .route("/api/v1/library/albums", get(list_albums))
        .route(
            "/api/v1/library/albums/{album_id}/cover",
            get(get_album_cover),
        )
        .route("/api/v1/library/albums/{album_id}", get(get_album))
        .route("/api/v1/library/tracks", get(list_tracks))
        .route("/api/v1/library/tracks/{track_id}", get(get_track))
        .route(
            "/api/v1/library/tracks/{track_id}/waveform",
            get(get_track_waveform),
        )
}

/// `limit` 1..=500 (default 50) and `offset` >= 0; invalid values fall back
/// to the defaults rather than failing, like the Go server.
fn pagination(query: &GoQuery) -> (i64, i64) {
    let number = |key: &str| query.get(key).parse::<i64>().ok();
    let limit = number("limit")
        .filter(|limit| (1..=500).contains(limit))
        .unwrap_or(50);
    let offset = number("offset").filter(|offset| *offset >= 0).unwrap_or(0);
    (limit, offset)
}

fn not_found(message: &str) -> impl FnOnce(LibraryError) -> ApiError + '_ {
    move |error| match error {
        LibraryError::NotFound => ApiError::not_found(message),
        other => ApiError::internal(other),
    }
}

async fn list_artists(
    State(state): State<AppState>,
    query: GoQuery,
) -> ApiResult<Json<types::Page<types::Artist>>> {
    let (limit, offset) = pagination(&query);
    let page = Service::new(state.db)
        .list_artists(limit, offset, query.get("q"))
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(page))
}

async fn get_artist(
    State(state): State<AppState>,
    Path(artist_id): Path<String>,
) -> ApiResult<Json<types::Artist>> {
    let artist = Service::new(state.db)
        .get_artist(&artist_id)
        .await
        .map_err(not_found("artist not found"))?;
    Ok(Json(artist))
}

async fn list_albums(
    State(state): State<AppState>,
    query: GoQuery,
) -> ApiResult<Json<types::Page<types::Album>>> {
    let (limit, offset) = pagination(&query);
    let page = Service::new(state.db)
        .list_albums(limit, offset, query.get("artistId"), query.get("q"))
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(page))
}

async fn get_album(
    State(state): State<AppState>,
    Path(album_id): Path<String>,
) -> ApiResult<Json<types::AlbumDetail>> {
    let album = Service::new(state.db)
        .get_album(&album_id)
        .await
        .map_err(not_found("album not found"))?;
    Ok(Json(album))
}

async fn get_album_cover(
    State(state): State<AppState>,
    Path(album_id): Path<String>,
    query: GoQuery,
) -> ApiResult<Response> {
    let size = match query.get("size") {
        "" => 0,
        raw => raw
            .parse::<i64>()
            .ok()
            .filter(|size| covers::COVER_SIZES.contains(size))
            .ok_or_else(|| ApiError::bad_request("size must be one of 64, 128, 256, 512, 1024"))?,
    };
    if size == LARGEST_COVER_SIZE {
        match state.artist_info.sharper_cover(&album_id).await {
            Ok(Some(cover)) => return Ok(cover_response(cover.media_type, cover.data)),
            Ok(None) | Err(ArtistInfoError::NotFound) => {}
            // The Album's own cover still answers.
            Err(error) => tracing::warn!(album_id, %error, "sharper cover lookup failed"),
        }
    }
    let cover = Service::new(state.db)
        .album_cover(&album_id, size)
        .await
        .map_err(not_found("album cover not found"))?;
    Ok(cover_response(cover.media_type, cover.data))
}

fn cover_response(media_type: String, data: Vec<u8>) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, media_type),
            (header::CACHE_CONTROL, "public, max-age=86400".to_owned()),
        ],
        data,
    )
        .into_response()
}

async fn list_tracks(
    State(state): State<AppState>,
    query: GoQuery,
) -> ApiResult<Json<types::Page<Track>>> {
    let (limit, offset) = pagination(&query);
    let page = Service::new(state.db)
        .list_tracks(limit, offset, query.get("artistId"), query.get("q"))
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(page))
}

async fn get_track(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
) -> ApiResult<Json<Track>> {
    let track = Service::new(state.db)
        .get_track(&track_id)
        .await
        .map_err(not_found("track not found"))?;
    Ok(Json(track))
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
        let query = GoQuery::parse;
        assert_eq!(pagination(&query("")), (50, 0));
        assert_eq!(pagination(&query("limit=100&offset=20")), (100, 20));
        assert_eq!(pagination(&query("limit=0&offset=-1")), (50, 0));
        assert_eq!(pagination(&query("limit=501&offset=x")), (50, 0));
        assert_eq!(pagination(&query("limit=10&limit=20")), (10, 0));
    }
}
