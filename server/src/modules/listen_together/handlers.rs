//! HTTP for Listen Together. The Owner's listener and the Guest Listener
//! mount different handlers, so a Guest request cannot end in an Owner one:
//! who a client is follows from which listener it reached.

use super::gate;
use super::room_state::{Command, RoomState};
use super::service::{self, Client, Feed, GuestSession, RoomError, Subscription};
use crate::app::AppState;
use crate::respond::ApiError;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Extension, Json, Router};
use futures_util::StreamExt;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::time::Duration;
use tokio::sync::watch;

const SESSION: &str = "/api/v1/listen-together/session";
const JOIN: &str = "/api/v1/listen-together/join";
const LEAVE: &str = "/api/v1/listen-together/leave";
const ROOM: &str = "/api/v1/listen-together/room";
const COMMANDS: &str = "/api/v1/listen-together/room/commands";
const EVENTS: &str = "/api/v1/listen-together/room/events";
const CLOCK: &str = "/api/v1/listen-together/room/clock";

const KEEP_ALIVE: Duration = Duration::from_secs(15);
/// Comfortably more than a full Room Queue of 500 ids.
const MAX_COMMAND_BYTES: usize = 64 * 1024;
const MAX_JOIN_BYTES: usize = 1024;
const MAX_TOKEN_CHARS: usize = 128;

pub fn owner_routes() -> Router<AppState> {
    Router::new()
        .route(SESSION, get(owner_session))
        .route(JOIN, any(guest_only))
        .route(LEAVE, any(guest_only))
        .route(ROOM, get(owner_room).post(open_room).delete(close_room))
        .route(COMMANDS, post(owner_command))
        .route(EVENTS, get(owner_events))
        .route(CLOCK, get(clock))
}

/// Opening and closing a Room are not here: the Guest Listener has no such
/// routes at all.
pub fn guest_routes() -> Router<AppState> {
    Router::new()
        .route(SESSION, get(guest_session))
        .route(JOIN, post(join))
        .route(LEAVE, post(leave))
        .route(ROOM, get(guest_room))
        .route(COMMANDS, post(guest_command))
        .route(EVENTS, get(guest_events))
        .route(CLOCK, get(clock))
}

impl From<RoomError> for ApiError {
    fn from(error: RoomError) -> Self {
        let message = error.to_string();
        match error {
            RoomError::Disabled => {
                Self::new(StatusCode::NOT_FOUND, "listen_together_disabled", message)
            }
            RoomError::AlreadyOpen => Self::new(StatusCode::CONFLICT, "conflict", message),
            RoomError::NotFound => Self::not_found(message),
            RoomError::InviteInvalid => Self::new(StatusCode::NOT_FOUND, "invite_invalid", message),
            RoomError::Full => Self::new(StatusCode::CONFLICT, "room_full", message),
            RoomError::SessionEnded => Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message),
            RoomError::RateLimited => {
                Self::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", message)
            }
            RoomError::BadCommand(_) => Self::bad_request(message),
            RoomError::UnknownTrack => Self::bad_request(message).with_field("trackIds"),
            RoomError::Library(cause) => Self::internal(cause),
        }
    }
}

fn role(role: &'static str) -> Response {
    Json(serde_json::json!({ "role": role })).into_response()
}

async fn owner_session() -> Response {
    role("owner")
}

async fn guest_session(State(state): State<AppState>, headers: HeaderMap) -> Response {
    role(if gate::guest_session(&state, &headers).is_some() {
        "guest"
    } else {
        "none"
    })
}

async fn guest_only() -> ApiError {
    ApiError::not_found("this route exists only on the Guest Listener")
}

async fn clock() -> Response {
    Json(serde_json::json!({ "serverTimeMs": service::now_ms() })).into_response()
}

async fn owner_room(State(state): State<AppState>) -> Result<Json<RoomState>, ApiError> {
    Ok(Json(state.listen_together.room(Client::Owner)?))
}

async fn guest_room(
    State(state): State<AppState>,
    Extension(session): Extension<GuestSession>,
) -> Result<Json<RoomState>, ApiError> {
    Ok(Json(state.listen_together.room(Client::Guest(&session))?))
}

async fn open_room(State(state): State<AppState>) -> Result<Response, ApiError> {
    let room = state.listen_together.open()?;
    Ok((StatusCode::CREATED, Json(room)).into_response())
}

async fn close_room(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state.listen_together.close()?;
    Ok(StatusCode::NO_CONTENT)
}

async fn decode<T: DeserializeOwned>(body: Body, limit: usize) -> Result<T, ApiError> {
    let bytes: Bytes = axum::body::to_bytes(body, limit)
        .await
        .map_err(|_| ApiError::bad_request("request body is too large"))?;
    serde_json::from_slice(&bytes).map_err(|_| ApiError::bad_request("invalid JSON body"))
}

async fn owner_command(
    State(state): State<AppState>,
    body: Body,
) -> Result<Json<RoomState>, ApiError> {
    let command: Command = decode(body, MAX_COMMAND_BYTES).await?;
    Ok(Json(
        state
            .listen_together
            .command(Client::Owner, command)
            .await?,
    ))
}

async fn guest_command(
    State(state): State<AppState>,
    Extension(session): Extension<GuestSession>,
    body: Body,
) -> Result<Json<RoomState>, ApiError> {
    let command: Command = decode(body, MAX_COMMAND_BYTES).await?;
    Ok(Json(
        state
            .listen_together
            .command(Client::Guest(&session), command)
            .await?,
    ))
}

#[derive(Deserialize)]
struct JoinBody {
    token: String,
}

fn session_cookie(
    state: &AppState,
    value: &str,
    attributes: &str,
) -> Result<HeaderValue, ApiError> {
    let secure = if gate::is_secure(state) {
        "; Secure"
    } else {
        ""
    };
    let name = gate::cookie_name(state);
    HeaderValue::from_str(&format!(
        "{name}={value}; Path=/; HttpOnly; SameSite=Strict{secure}{attributes}"
    ))
    .map_err(ApiError::internal)
}

async fn join(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let JoinBody { token } = decode(body, MAX_JOIN_BYTES).await?;
    if token.is_empty() || token.chars().count() > MAX_TOKEN_CHARS {
        return Err(ApiError::bad_request("token has the wrong length").with_field("token"));
    }
    let current = gate::session_cookie(&state, &headers);
    let joined = state.listen_together.join(&token, current)?;
    let cookie = session_cookie(&state, &joined.session_token, "")?;
    Ok(([(header::SET_COOKIE, cookie)], Json(joined.state)).into_response())
}

async fn leave(
    State(state): State<AppState>,
    Extension(session): Extension<GuestSession>,
) -> Result<Response, ApiError> {
    state.listen_together.leave(&session)?;
    let cookie = session_cookie(&state, "", "; Max-Age=0")?;
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response())
}

async fn owner_events(State(state): State<AppState>) -> Result<Response, ApiError> {
    let subscription = state.listen_together.subscribe(Client::Owner)?;
    Ok(event_stream(subscription, None))
}

async fn guest_events(
    State(state): State<AppState>,
    Extension(session): Extension<GuestSession>,
) -> Result<Response, ApiError> {
    let subscription = state.listen_together.subscribe(Client::Guest(&session))?;
    Ok(event_stream(subscription, Some(session)))
}

/// A frame, and whether the stream ends after it. `room-closed` needs a data
/// line: EventSource drops an event without one.
fn frame(feed: &Feed, invite_url: Option<&String>) -> (Bytes, bool) {
    match feed {
        Feed::Closed => (
            Bytes::from_static(b"event: room-closed\ndata: {}\n\n"),
            true,
        ),
        Feed::State(state) => {
            let revision = state.revision;
            let data = match invite_url {
                Some(invite_url) => serde_json::to_string(&RoomState {
                    invite_url: Some(invite_url.clone()),
                    ..RoomState::clone(state)
                }),
                None => serde_json::to_string(&**state),
            }
            .expect("Room State encodes");
            (
                Bytes::from(format!(
                    "id: {revision}\nevent: room-state\ndata: {data}\n\n"
                )),
                false,
            )
        }
    }
}

async fn session_ended(revoked: &mut Option<watch::Receiver<bool>>) {
    match revoked {
        Some(revoked) => {
            let _ = revoked.wait_for(|revoked| *revoked).await;
        }
        None => std::future::pending().await,
    }
}

struct EventStream {
    subscription: Subscription,
    revoked: Option<watch::Receiver<bool>>,
    keep_alive: tokio::time::Interval,
}

impl EventStream {
    /// The next frame, or `None` when the session ended with the Room open.
    /// A snapshot waiting in the channel loses to a revocation, so a Guest
    /// who is out receives nothing more.
    async fn next(&mut self) -> Option<(Bytes, bool)> {
        tokio::select! {
            biased;
            () = session_ended(&mut self.revoked) => {
                let feed = self.subscription.events.borrow().clone();
                matches!(feed, Feed::Closed).then(|| frame(&feed, None))
            }
            changed = self.subscription.events.changed() => {
                if changed.is_err() {
                    return Some(frame(&Feed::Closed, None));
                }
                let feed = self.subscription.events.borrow_and_update().clone();
                Some(frame(&feed, self.subscription.invite_url.as_ref()))
            }
            _ = self.keep_alive.tick() => Some((Bytes::from_static(b": keep-alive\n\n"), false)),
        }
    }
}

/// The current snapshot first, then each later one, with a keep-alive
/// comment every 15s. Snapshots coalesce, so a slow client skips to the
/// latest; `room-closed` is last.
fn event_stream(mut subscription: Subscription, session: Option<GuestSession>) -> Response {
    let current = subscription.events.borrow_and_update().clone();
    let (first, ended) = frame(&current, subscription.invite_url.as_ref());
    let keep_alive = tokio::time::interval_at(tokio::time::Instant::now() + KEEP_ALIVE, KEEP_ALIVE);
    let stream = EventStream {
        subscription,
        revoked: session.as_ref().map(GuestSession::revocation),
        keep_alive,
    };
    let later = futures_util::stream::unfold((!ended).then_some(stream), |stream| async move {
        let mut stream = stream?;
        let (chunk, ended) = stream.next().await?;
        Some((
            Ok::<_, std::convert::Infallible>(chunk),
            (!ended).then_some(stream),
        ))
    });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_closed_has_a_data_line_and_ends_the_stream() {
        let (bytes, ended) = frame(&Feed::Closed, None);
        assert_eq!(
            bytes,
            Bytes::from_static(b"event: room-closed\ndata: {}\n\n")
        );
        assert!(ended);
    }
}
