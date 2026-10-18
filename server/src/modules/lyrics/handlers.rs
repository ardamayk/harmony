//! HTTP for Track lyrics.

use super::service::LyricsError;
use crate::app::AppState;
use crate::respond::{ApiError, ApiResult};
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/library/tracks/{track_id}/lyrics",
        get(get_track_lyrics),
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackLyricsBody {
    track_id: String,
    lyrics: String,
}

async fn get_track_lyrics(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
) -> ApiResult<Json<TrackLyricsBody>> {
    let lyrics = state
        .lyrics
        .lyrics(&track_id)
        .await
        .map_err(|error| match error {
            LyricsError::NotFound => ApiError::not_found("track not found"),
            other => ApiError::internal(other),
        })?;
    Ok(Json(TrackLyricsBody { track_id, lyrics }))
}
