//! HTTP for Library Search: validate the query, call the service, respond.

use super::rank::Query;
use super::service::SearchResponse;
use crate::app::AppState;
use crate::auth;
use crate::modules::listen_together::GuestSession;
use crate::query::GoQuery;
use crate::respond::{ApiError, ApiResult};
use crate::searchdata;
use axum::extract::State;
use axum::routing::get;
use axum::{Extension, Json, Router};

const MAX_QUERY_CHARACTERS: usize = 200;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/library/search", get(search))
}

async fn search(
    State(state): State<AppState>,
    guest: Option<Extension<GuestSession>>,
    query: GoQuery,
) -> ApiResult<Json<SearchResponse>> {
    let raw = query.get("q");
    if raw.trim().is_empty() {
        return Err(ApiError::bad_request("query parameter q is required"));
    }
    if raw.chars().count() > MAX_QUERY_CHARACTERS {
        return Err(ApiError::bad_request(format!(
            "query is longer than {MAX_QUERY_CHARACTERS} characters"
        )));
    }
    let prepared = Query::new(&searchdata::prepare_query(raw));
    if prepared.is_empty() {
        return Err(ApiError::bad_request(
            "query has no searchable letters or digits",
        ));
    }
    let all = match query.values("all").as_deref() {
        None => false,
        Some(["true"]) => true,
        Some(["false"]) => false,
        Some(_) => {
            return Err(ApiError::bad_request(
                "query parameter all must be true or false",
            ));
        }
    };
    let response = state
        .search
        // Playlists stay private to the Owner (ADR 0037).
        .search(auth::current_user().id, &prepared, all, guest.is_none())
        .await
        .map_err(|error| {
            tracing::error!(%error, "Library Search failed");
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Library Search is unavailable",
            )
        })?;
    Ok(Json(response))
}
