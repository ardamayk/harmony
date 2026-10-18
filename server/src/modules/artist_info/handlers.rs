//! HTTP for Artist photos, biographies and popular Tracks.

use super::service::ArtistInfoError;
use crate::app::AppState;
use crate::modules::library::Track;
use crate::query::GoQuery;
use crate::respond::{ApiError, ApiResult};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

/// Popular Tracks shown when the client names no limit, and the most it may.
const DEFAULT_POPULAR_TRACKS: usize = 10;
const MAX_POPULAR_TRACKS: usize = 50;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/library/artists/{artist_id}/photo", get(get_photo))
        .route(
            "/api/v1/library/artists/{artist_id}/biography",
            get(get_biography),
        )
        .route(
            "/api/v1/library/artists/{artist_id}/popular-tracks",
            get(get_popular_tracks),
        )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BiographyBody {
    text: String,
    source_url: String,
}

fn api_error(error: ArtistInfoError) -> ApiError {
    match error {
        ArtistInfoError::NotFound => ApiError::not_found("artist not found"),
        other => ApiError::internal(other),
    }
}

async fn get_photo(
    State(state): State<AppState>,
    Path(artist_id): Path<String>,
) -> ApiResult<Response> {
    let photo = state
        .artist_info
        .photo(&artist_id)
        .await
        .map_err(api_error)?
        .ok_or_else(|| ApiError::not_found("artist photo not found"))?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, photo.media_type),
            (header::CACHE_CONTROL, "public, max-age=86400".to_owned()),
        ],
        photo.data,
    )
        .into_response())
}

async fn get_biography(
    State(state): State<AppState>,
    Path(artist_id): Path<String>,
) -> ApiResult<Json<BiographyBody>> {
    let biography = state
        .artist_info
        .biography(&artist_id)
        .await
        .map_err(api_error)?
        .ok_or_else(|| ApiError::not_found("artist biography not found"))?;
    Ok(Json(BiographyBody {
        text: biography.text,
        source_url: biography.source_url,
    }))
}

#[derive(Serialize)]
struct PopularTracksBody {
    items: Vec<Track>,
}

async fn get_popular_tracks(
    State(state): State<AppState>,
    Path(artist_id): Path<String>,
    query: GoQuery,
) -> ApiResult<Json<PopularTracksBody>> {
    let limit = query
        .get("limit")
        .parse()
        .unwrap_or(DEFAULT_POPULAR_TRACKS)
        .clamp(1, MAX_POPULAR_TRACKS);
    let items = state
        .artist_info
        .popular_tracks(&artist_id, limit)
        .await
        .map_err(api_error)?;
    Ok(Json(PopularTracksBody { items }))
}
