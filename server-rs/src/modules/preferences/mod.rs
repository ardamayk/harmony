//! User preferences: theme, layout and Player Bar settings
//! (server/internal/modules/preferences).

mod repository;
mod rules;
mod types;

use crate::app::AppState;
use crate::auth;
use crate::gojson;
use crate::respond::{ApiError, ApiResult};
use axum::body::Bytes;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
pub use repository::Repository;
use types::{UserPreferences, UserPreferencesPatch};

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/preferences",
        get(get_preferences).patch(patch_preferences),
    )
}

async fn get_preferences(State(state): State<AppState>) -> ApiResult<Json<UserPreferences>> {
    let preferences = Repository::new(state.db)
        .get(auth::current_user().id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(preferences))
}

async fn patch_preferences(
    State(state): State<AppState>,
    body: Bytes,
) -> ApiResult<Json<UserPreferences>> {
    let patch: UserPreferencesPatch = gojson::decode_body(&body, "invalid JSON body")?;
    rules::validate_playback_patch(patch.playback.as_ref()).map_err(ApiError::bad_request)?;
    let preferences = Repository::new(state.db)
        .update(auth::current_user().id, |current| {
            rules::apply_patch(current, &patch)
        })
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(preferences))
}
