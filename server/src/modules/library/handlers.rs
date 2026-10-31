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

/// `limit` 1..=500 (default 50) and `offset` >= 0, as the contract bounds
/// them; anything else is refused. An empty value counts as unset.
fn pagination(query: &GoQuery) -> ApiResult<(i64, i64)> {
    let limit = match query.get("limit") {
        "" => 50,
        raw => raw
            .parse()
            .ok()
            .filter(|limit| (1..=500).contains(limit))
            .ok_or_else(|| {
                ApiError::bad_request("limit must be a whole number from 1 to 500")
                    .with_field("limit")
            })?,
    };
    let offset = match query.get("offset") {
        "" => 0,
        raw => raw
            .parse()
            .ok()
            .filter(|offset| *offset >= 0)
            .ok_or_else(|| {
                ApiError::bad_request("offset must be a whole number from 0").with_field("offset")
            })?,
    };
    Ok((limit, offset))
}

/// The `artistId` filter, or "" without one. `parse_str` also takes the
/// UUID's other spellings, which no stored id uses.
fn artist_filter(query: &GoQuery) -> ApiResult<&str> {
    let artist_id = query.get("artistId");
    if artist_id.is_empty() || (artist_id.len() == 36 && uuid::Uuid::parse_str(artist_id).is_ok()) {
        return Ok(artist_id);
    }
    Err(ApiError::bad_request("artistId must be a UUID").with_field("artistId"))
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
    let (limit, offset) = pagination(&query)?;
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
    let (limit, offset) = pagination(&query)?;
    let artist_id = artist_filter(&query)?;
    let page = Service::new(state.db)
        .list_albums(limit, offset, artist_id, query.get("q"))
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
    let (limit, offset) = pagination(&query)?;
    let artist_id = artist_filter(&query)?;
    let page = Service::new(state.db)
        .list_tracks(limit, offset, artist_id, query.get("q"))
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
    fn pagination_defaults_and_keeps_valid_values() {
        let query = GoQuery::parse;
        assert_eq!(pagination(&query("")), Ok((50, 0)));
        assert_eq!(pagination(&query("limit=&offset=")), Ok((50, 0)));
        assert_eq!(pagination(&query("limit=100&offset=20")), Ok((100, 20)));
        assert_eq!(pagination(&query("limit=1&offset=0")), Ok((1, 0)));
        assert_eq!(pagination(&query("limit=500")), Ok((500, 0)));
        assert_eq!(pagination(&query("limit=10&limit=20")), Ok((10, 0)));
    }

    #[test]
    fn pagination_refuses_what_the_contract_does_not_allow() {
        for (raw, field) in [
            ("limit=abc", "limit"),
            ("limit=-1", "limit"),
            ("limit=0", "limit"),
            ("limit=501", "limit"),
            ("limit=1.5", "limit"),
            ("limit=9223372036854775808", "limit"),
            ("offset=abc", "offset"),
            ("offset=-1", "offset"),
            ("offset=1.5", "offset"),
            ("offset=9223372036854775808", "offset"),
        ] {
            let error = pagination(&GoQuery::parse(raw)).expect_err(raw);
            assert_eq!(error.status, StatusCode::BAD_REQUEST, "{raw}");
            assert_eq!(error.body.field, field, "{raw}");
        }
    }

    #[test]
    fn artist_filter_takes_only_a_hyphenated_uuid() {
        let id = "67e55044-10b1-426f-9247-bb680e5fe0c8";
        let valid = format!("artistId={id}");
        assert_eq!(artist_filter(&GoQuery::parse(&valid)), Ok(id));
        assert_eq!(artist_filter(&GoQuery::parse("")), Ok(""));
        for raw in [
            "artistId=abc",
            "artistId=67e5504410b1426f9247bb680e5fe0c8",
            "artistId=%7B67e55044-10b1-426f-9247-bb680e5fe0c8%7D",
            "artistId=67e55044-10b1-426f-9247-bb680e5fe0cg",
        ] {
            let error = artist_filter(&GoQuery::parse(raw)).expect_err(raw);
            assert_eq!(error.status, StatusCode::BAD_REQUEST, "{raw}");
            assert_eq!(error.body.field, "artistId", "{raw}");
        }
    }
}
