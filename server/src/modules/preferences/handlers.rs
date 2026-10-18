//! HTTP for preferences: parse the request, call the service, respond.

use super::service::{Service, ServiceError};
use super::types::{UserPreferences, UserPreferencesPatch};
use crate::app::AppState;
use crate::auth;
use crate::gojson;
use crate::respond::{ApiError, ApiResult};
use axum::body::Bytes;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/preferences",
        get(get_preferences).patch(patch_preferences),
    )
}

async fn get_preferences(State(state): State<AppState>) -> ApiResult<Json<UserPreferences>> {
    let preferences = Service::new(state.db)
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
    let preferences = Service::new(state.db)
        .update(auth::current_user().id, &patch)
        .await
        .map_err(|error| match error {
            ServiceError::Invalid(message) => ApiError::bad_request(message),
            other => ApiError::internal(other),
        })?;
    Ok(Json(preferences))
}
