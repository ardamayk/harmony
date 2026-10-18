//! HTTP for playback: parse the request, call the service, respond.

use super::repository::QueueError;
use super::serve_file;
use super::service::Service;
use super::sources;
use crate::app::AppState;
use crate::auth;
use crate::gojson;
use crate::modules::library::LibraryError;
use crate::query::GoQuery;
use crate::respond::ApiError;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::value::RawValue;
use std::time::Duration;

const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const KEEP_ALIVE: Duration = Duration::from_secs(15);

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/playback/queue",
            get(get_queue)
                .put(replace_queue)
                .patch(reorder_queue)
                .delete(clear_queue),
        )
        .route("/api/v1/playback/queue/items", post(append_item))
        .route(
            "/api/v1/playback/queue/items/{item_id}",
            delete(remove_item),
        )
        .route("/api/v1/playback/queue/events", get(stream_queue_events))
        .route(
            "/api/v1/tracks/{track_id}/stream",
            get(stream_track).head(stream_track),
        )
}

/// The body as Go's decodeQueueRequest reads it: at most 1 MiB, then one
/// strict json.Unmarshal (trailing data is an error here, unlike Decode).
async fn decode<T: serde::de::DeserializeOwned + Default>(body: Body) -> Result<T, ApiError> {
    let mut stream = body.into_data_stream();
    let mut data = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| ApiError::bad_request("could not read queue request body"))?;
        data.extend_from_slice(&chunk);
        if data.len() > MAX_REQUEST_BYTES {
            return Err(ApiError::bad_request(format!(
                "queue request exceeds {MAX_REQUEST_BYTES} bytes"
            )));
        }
    }
    match serde_json::from_slice::<Option<T>>(&data) {
        Ok(value) => Ok(value.unwrap_or_default()),
        Err(_) => Err(ApiError::bad_request("invalid JSON body")),
    }
}

fn internal(error: impl std::fmt::Display) -> Response {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        error.to_string(),
    )
    .into_response()
}

async fn respond_mutation(
    state: &AppState,
    user_id: &str,
    result: Result<(), QueueError>,
) -> Response {
    let service = Service::new(state);
    match result {
        Ok(()) => match service.get_queue(user_id).await {
            Ok(queue) => {
                state
                    .queue_events
                    .publish(user_id, &queue.revision, &queue.event_sequence);
                Json(queue).into_response()
            }
            Err(error) => internal(error),
        },
        Err(QueueError::RevisionConflict) => match service.get_queue(user_id).await {
            Ok(queue) => (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "conflict",
                    "code": "queue_revision_conflict",
                    "message": "queue changed since supplied revision",
                    "queue": queue,
                })),
            )
                .into_response(),
            Err(error) => internal(error),
        },
        Err(
            error @ (QueueError::LimitExceeded
            | QueueError::InvalidSource(_)
            | QueueError::InvalidOrder),
        ) => ApiError::bad_request(error.to_string()).into_response(),
        Err(QueueError::TrackNotFound) => ApiError::not_found("track not found").into_response(),
        Err(QueueError::NotFound) => ApiError::not_found("queue item not found").into_response(),
        Err(error) => internal(error),
    }
}

async fn get_queue(State(state): State<AppState>) -> Response {
    match Service::new(&state)
        .get_queue(auth::current_user().id)
        .await
    {
        Ok(queue) => Json(queue).into_response(),
        Err(error) => internal(error),
    }
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ReplaceBody {
    #[serde(deserialize_with = "gojson::nullable_strings")]
    track_ids: Option<Vec<String>>,
    #[serde(deserialize_with = "gojson::raw")]
    source: Option<Box<RawValue>>,
    #[serde(deserialize_with = "gojson::nullable")]
    revision: String,
}

async fn replace_queue(State(state): State<AppState>, body: Body) -> Response {
    let user_id = auth::current_user().id;
    let body: ReplaceBody = match decode(body).await {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if body.revision.is_empty() {
        return ApiError::bad_request("revision is required").into_response();
    }
    let source = match sources::parse(body.source.as_deref()) {
        Ok(source) => source,
        Err(error) => return ApiError::bad_request(error).into_response(),
    };
    let track_ids = body.track_ids.unwrap_or_default();
    let result = Service::new(&state)
        .replace(user_id, &body.revision, &track_ids, &source)
        .await;
    respond_mutation(&state, user_id, result).await
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct AppendBody {
    #[serde(deserialize_with = "gojson::nullable")]
    track_id: String,
    #[serde(deserialize_with = "gojson::raw")]
    source: Option<Box<RawValue>>,
    #[serde(deserialize_with = "gojson::nullable")]
    revision: String,
}

async fn append_item(State(state): State<AppState>, body: Body) -> Response {
    let user_id = auth::current_user().id;
    let body: AppendBody = match decode(body).await {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if body.track_id.is_empty() {
        return ApiError::bad_request("trackId is required").into_response();
    }
    if body.revision.is_empty() {
        return ApiError::bad_request("revision is required").into_response();
    }
    let source = match sources::parse(body.source.as_deref()) {
        Ok(source) => source,
        Err(error) => return ApiError::bad_request(error).into_response(),
    };
    let result = Service::new(&state)
        .append(user_id, &body.revision, &body.track_id, &source)
        .await;
    respond_mutation(&state, user_id, result).await
}

fn if_match(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

async fn remove_item(
    State(state): State<AppState>,
    Path(item_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let user_id = auth::current_user().id;
    let Some(revision) = if_match(&headers) else {
        return ApiError::bad_request("If-Match revision is required").into_response();
    };
    let result = Service::new(&state)
        .remove(user_id, &revision, &item_id)
        .await;
    respond_mutation(&state, user_id, result).await
}

async fn clear_queue(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: GoQuery,
) -> Response {
    let user_id = auth::current_user().id;
    let Some(revision) = if_match(&headers) else {
        return ApiError::bad_request("If-Match revision is required").into_response();
    };
    let result = Service::new(&state)
        .clear(user_id, &revision, query.get("afterItemId"))
        .await;
    respond_mutation(&state, user_id, result).await
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ReorderBody {
    #[serde(deserialize_with = "gojson::nullable_strings")]
    item_ids: Option<Vec<String>>,
    #[serde(deserialize_with = "gojson::nullable")]
    revision: String,
}

async fn reorder_queue(State(state): State<AppState>, body: Body) -> Response {
    let user_id = auth::current_user().id;
    let body: ReorderBody = match decode(body).await {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    let (Some(item_ids), false) = (body.item_ids, body.revision.is_empty()) else {
        return ApiError::bad_request("itemIds and revision are required").into_response();
    };
    let result = Service::new(&state)
        .reorder(user_id, &body.revision, &item_ids)
        .await;
    respond_mutation(&state, user_id, result).await
}

fn queue_event(revision: &str, sequence: &str) -> Bytes {
    let data = gojson::to_string(&serde_json::json!({
        "invalidates": ["queue"],
        "revision": revision,
        "sequence": sequence,
    }));
    Bytes::from(format!(
        "id: {sequence}\nevent: queue-invalidated\ndata: {data}\n\n"
    ))
}

/// The current revision first, then every later invalidation, with a
/// keep-alive comment every 15s.
async fn stream_queue_events(State(state): State<AppState>) -> Response {
    let user_id = auth::current_user().id;
    let subscription = state.queue_events.subscribe(user_id);
    let queue = match Service::new(&state).get_queue(user_id).await {
        Ok(queue) => queue,
        Err(error) => return internal(error),
    };
    let first = queue_event(&queue.revision, &queue.event_sequence);
    let keep_alive = tokio::time::interval_at(tokio::time::Instant::now() + KEEP_ALIVE, KEEP_ALIVE);
    let later = futures_util::stream::unfold(
        (subscription, keep_alive),
        |(mut subscription, mut keep_alive)| async move {
            let chunk = tokio::select! {
                changed = subscription.events.changed() => {
                    changed.ok()?;
                    let event = subscription.events.borrow_and_update().clone()?;
                    queue_event(&event.revision, &event.sequence)
                }
                _ = keep_alive.tick() => Bytes::from_static(b": keep-alive\n\n"),
            };
            Some((
                Ok::<_, std::convert::Infallible>(chunk),
                (subscription, keep_alive),
            ))
        },
    );
    let stream = futures_util::stream::once(async move { Ok(first) }).chain(later);
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    (headers, Body::from_stream(stream)).into_response()
}

async fn stream_track(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let path = match Service::new(&state).track_file_path(&track_id).await {
        Ok(path) => path,
        Err(LibraryError::NotFound) => {
            return ApiError::not_found("track not found").into_response();
        }
        Err(error) => return internal(error),
    };
    match serve_file::serve(&method, &headers, std::path::Path::new(&path)).await {
        Ok(response) => response,
        Err(serve_file::ServeError::NotFound) => {
            ApiError::not_found("track file not found").into_response()
        }
        Err(serve_file::ServeError::Io(error)) => internal(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_events_frame_like_the_go_server() {
        assert_eq!(
            queue_event("12", "40"),
            Bytes::from_static(b"id: 40\nevent: queue-invalidated\ndata: {\"invalidates\":[\"queue\"],\"revision\":\"12\",\"sequence\":\"40\"}\n\n")
        );
    }
}
