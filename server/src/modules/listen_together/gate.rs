//! The Guest Listener's allowlist. Anything not named here is refused, so a
//! route added later stays out of a Guest's reach until someone adds it.

use super::limits::Budget;
use super::service::{AudioStreamSlot, GuestSession};
use crate::app::AppState;
use crate::respond::ApiError;
use axum::body::Body;
use axum::extract::{MatchedPath, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::oneshot;

/// Open to anyone who reaches the Guest Listener, as (method, route).
const PUBLIC_ROUTES: &[(&str, &str)] = &[
    ("GET", "/api/v1/health"),
    ("GET", "/api/v1/listen-together/session"),
    ("POST", "/api/v1/listen-together/join"),
];

const TRACK_STREAM: &str = "/api/v1/tracks/{track_id}/stream";
const ARTIST_PHOTO: &str = "/api/v1/library/artists/{artist_id}/photo";

/// Reads that run ffmpeg or may call an outside service.
const LOOKUP_ROUTES: &[&str] = &[
    "/api/v1/library/artists/{artist_id}/biography",
    "/api/v1/library/artists/{artist_id}/popular-tracks",
    "/api/v1/library/albums/{album_id}/online-genres",
    "/api/v1/library/tracks/play-counts",
    "/api/v1/library/tracks/{track_id}/lyrics",
    "/api/v1/library/tracks/{track_id}/waveform",
];

/// Open to a live Guest session, as (method, route).
const SESSION_ROUTES: &[(&str, &str)] = &[
    ("GET", "/api/v1/me"),
    ("GET", "/api/v1/preferences"),
    ("GET", "/api/v1/library/artists"),
    ("GET", "/api/v1/library/artists/{artist_id}"),
    ("GET", ARTIST_PHOTO),
    ("GET", "/api/v1/library/artists/{artist_id}/biography"),
    ("GET", "/api/v1/library/artists/{artist_id}/popular-tracks"),
    ("GET", "/api/v1/library/albums"),
    ("GET", "/api/v1/library/albums/{album_id}"),
    ("GET", "/api/v1/library/albums/{album_id}/online-genres"),
    ("GET", "/api/v1/library/albums/{album_id}/cover"),
    ("GET", "/api/v1/library/tracks"),
    ("GET", "/api/v1/library/tracks/play-counts"),
    ("GET", "/api/v1/library/tracks/{track_id}"),
    ("GET", "/api/v1/library/tracks/{track_id}/lyrics"),
    ("GET", "/api/v1/library/tracks/{track_id}/waveform"),
    ("GET", "/api/v1/library/search"),
    ("GET", TRACK_STREAM),
    ("HEAD", TRACK_STREAM),
    ("GET", "/api/v1/listen-together/room"),
    ("GET", "/api/v1/listen-together/room/events"),
    ("GET", "/api/v1/listen-together/room/clock"),
    ("POST", "/api/v1/listen-together/room/commands"),
    ("POST", "/api/v1/listen-together/leave"),
];

#[derive(Debug, PartialEq)]
enum Access {
    Public,
    Session,
    Denied,
}

/// `route` is the matched route pattern; a request that matched none falls
/// through to the Web Client's static files, for browser navigation only.
fn classify(method: &str, route: Option<&str>, path: &str) -> Access {
    let Some(route) = route else {
        return if matches!(method, "GET" | "HEAD") && is_web_client_path(path) {
            Access::Public
        } else {
            Access::Denied
        };
    };
    if PUBLIC_ROUTES.contains(&(method, route)) {
        Access::Public
    } else if SESSION_ROUTES.contains(&(method, route)) {
        Access::Session
    } else {
        Access::Denied
    }
}

/// What a session route draws on, for `SESSION_ROUTES` only. Room commands
/// have their own limit, and leaving ends the session. A read this does not
/// name counts as a library read, so a route added later has a limit too.
fn budget(method: &str, route: &str) -> Option<Budget> {
    if method == "POST" {
        return None;
    }
    Some(if route == ARTIST_PHOTO {
        Budget::Photos
    } else if LOOKUP_ROUTES.contains(&route) {
        Budget::Lookups
    } else if route.starts_with("/api/v1/listen-together/") {
        Budget::Room
    } else {
        Budget::Library
    })
}

fn is_web_client_path(path: &str) -> bool {
    let under = |prefix: &str| {
        path.strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    };
    !under("/api") && !under("/docs")
}

/// The Guest session a request's cookie stands for.
pub fn guest_session(state: &AppState, headers: &HeaderMap) -> Option<GuestSession> {
    state
        .listen_together
        .authenticate(session_cookie(state, headers)?)
}

/// The value of this listener's Guest cookie.
pub fn session_cookie<'a>(state: &AppState, headers: &'a HeaderMap) -> Option<&'a str> {
    let name = cookie_name(state);
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

/// `__Host-` needs HTTPS, and makes the browser refuse the cookie from a
/// sibling subdomain or a wider path.
pub fn cookie_name(state: &AppState) -> &'static str {
    if is_secure(state) {
        "__Host-harmony_guest"
    } else {
        "harmony_guest"
    }
}

pub fn is_secure(state: &AppState) -> bool {
    state
        .config
        .listen_together
        .as_ref()
        .is_some_and(|settings| settings.secure)
}

/// The deny-by-default check every Guest Listener request passes. It runs
/// after routing, so it sees the route pattern rather than a path a request
/// could spell differently.
pub async fn guest_gate(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|route| route.as_str().to_owned());
    let access = classify(
        request.method().as_str(),
        route.as_deref(),
        request.uri().path(),
    );
    match access {
        Access::Denied => ApiError::not_found("not found").into_response(),
        Access::Public => next.run(request).await,
        Access::Session => {
            let Some(session) = guest_session(&state, request.headers()) else {
                return ApiError::new(
                    StatusCode::UNAUTHORIZED,
                    "unauthorized",
                    "join the Room through its Invite Link first",
                )
                .into_response();
            };
            let spent = route
                .as_deref()
                .and_then(|route| budget(request.method().as_str(), route))
                .map_or(Ok(()), |budget| {
                    state.listen_together.take_read(&session, budget)
                });
            if let Err(error) = spent {
                return ApiError::from(error).into_response();
            }
            // HEAD sends no audio, so it needs no place.
            let audio = if route.as_deref() == Some(TRACK_STREAM) && request.method() == Method::GET
            {
                match state
                    .listen_together
                    .open_audio_stream(&session, request.uri().path())
                {
                    Ok(audio) => Some(audio),
                    Err(error) => return ApiError::from(error).into_response(),
                }
            } else {
                None
            };
            request.extensions_mut().insert(session.clone());
            let response = next.run(request).await;
            match route.as_deref() {
                Some(TRACK_STREAM) => end_with_session(response, session, audio),
                Some(route) if route.starts_with("/api/v1/library/") => {
                    without_file_paths(response).await
                }
                _ => response,
            }
        }
    }
}

/// A Track can take hours to send; revoking a Guest stops it as well. The
/// audio stream's place lasts as long as the body does.
fn end_with_session(
    response: Response,
    session: GuestSession,
    audio: Option<(AudioStreamSlot, oneshot::Receiver<()>)>,
) -> Response {
    let (parts, body) = response.into_parts();
    let stream = body
        .into_data_stream()
        .take_until(Box::pin(session.revoked()));
    let Some((slot, superseded)) = audio else {
        return Response::from_parts(parts, Body::from_stream(stream));
    };
    let stream = stream.take_until(superseded).map(move |chunk| {
        let _held = &slot;
        chunk
    });
    Response::from_parts(parts, Body::from_stream(stream))
}

/// A Track's `filePath` says where the Owner's files live, and no client
/// reads it. The field stays, empty, so the JSON keeps the contract's shape.
async fn without_file_paths(response: Response) -> Response {
    let is_json = response
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|value| value.as_bytes().starts_with(b"application/json"));
    if !is_json {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        return ApiError::internal("read a library response").into_response();
    };
    let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    blank_file_paths(&mut value);
    parts.headers.remove(header::CONTENT_LENGTH);
    let body = serde_json::to_vec(&value).expect("JSON value encodes");
    Response::from_parts(parts, Body::from(body))
}

fn blank_file_paths(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(blank_file_paths),
        Value::Object(fields) => {
            for (name, field) in fields.iter_mut() {
                if name == "filePath" {
                    *field = Value::String(String::new());
                } else {
                    blank_file_paths(field);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_route_is_open_only_for_its_listed_method() {
        let classify = |method, route| classify(method, Some(route), "/");
        assert_eq!(classify("GET", "/api/v1/health"), Access::Public);
        assert_eq!(classify("HEAD", "/api/v1/health"), Access::Denied);
        assert_eq!(classify("POST", "/api/v1/health"), Access::Denied);
        assert_eq!(classify("GET", TRACK_STREAM), Access::Session);
        assert_eq!(classify("HEAD", TRACK_STREAM), Access::Session);
        assert_eq!(classify("GET", "/api/v1/preferences"), Access::Session);
        assert_eq!(classify("PATCH", "/api/v1/preferences"), Access::Denied);
        assert_eq!(classify("GET", "/api/v1/playback/queue"), Access::Denied);
        // The Shared Playback State is the Owner's; ADR 0040 keeps it from
        // Guests on both of its routes.
        assert_eq!(classify("GET", "/api/v1/playback/state"), Access::Denied);
        assert_eq!(
            classify("POST", "/api/v1/playback/state/commands"),
            Access::Denied
        );
        assert_eq!(
            classify("POST", "/api/v1/listen-together/room"),
            Access::Denied
        );
        assert_eq!(
            classify("DELETE", "/api/v1/listen-together/room"),
            Access::Denied
        );
    }

    #[test]
    fn every_session_read_has_a_budget_and_only_named_routes_are_lookups() {
        for &(method, route) in SESSION_ROUTES {
            let expected = match (method, route) {
                ("POST", _) => None,
                (_, ARTIST_PHOTO) => Some(Budget::Photos),
                (_, "/api/v1/listen-together/room")
                | (_, "/api/v1/listen-together/room/events")
                | (_, "/api/v1/listen-together/room/clock") => Some(Budget::Room),
                (_, route) if LOOKUP_ROUTES.contains(&route) => Some(Budget::Lookups),
                _ => Some(Budget::Library),
            };
            assert_eq!(budget(method, route), expected, "{method} {route}");
        }
        for route in LOOKUP_ROUTES {
            assert!(SESSION_ROUTES.contains(&("GET", route)), "{route}");
        }
        assert_eq!(budget("HEAD", TRACK_STREAM), Some(Budget::Library));
        assert_eq!(budget("GET", TRACK_STREAM), Some(Budget::Library));
    }

    #[test]
    fn unmatched_requests_reach_only_the_web_client() {
        let classify = |method, path| classify(method, None, path);
        for path in [
            "/",
            "/join",
            "/library/albums",
            "/assets/index.js",
            "/apiary",
            "/docsy",
        ] {
            assert_eq!(classify("GET", path), Access::Public, "{path}");
        }
        for path in [
            "/api",
            "/api/",
            "/api/docs",
            "/api/v1/nope",
            "/docs",
            "/docs/x",
        ] {
            assert_eq!(classify("GET", path), Access::Denied, "{path}");
        }
        assert_eq!(classify("POST", "/join"), Access::Denied);
    }

    #[test]
    fn blanks_every_file_path_at_any_depth() {
        let mut value = serde_json::json!({
            "filePath": "/a",
            "tracks": [{ "id": "1", "filePath": "/b" }],
            "items": [{ "album": { "filePath": "/c" } }],
        });
        blank_file_paths(&mut value);
        assert_eq!(
            value,
            serde_json::json!({
                "filePath": "",
                "tracks": [{ "id": "1", "filePath": "" }],
                "items": [{ "album": { "filePath": "" } }],
            })
        );
    }
}
