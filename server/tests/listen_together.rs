//! Listen Together through both assembled routers: what the Guest Listener
//! refuses, how a Guest gets in and out, and the Room State a client sees.

use axum::Router;
use axum::body::{Body, BodyDataStream};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use earthly_music_server::app::{self, AppState};
use earthly_music_server::config::Config;
use earthly_music_server::modules::library::LibraryService;
use earthly_music_server::modules::listen_together::{ListenTogether, Timings};
use earthly_music_server::{db, dependencies, gojson};
use futures_util::StreamExt;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tower::ServiceExt;

const PUBLIC_HOST: &str = "music.example.com";
const PUBLIC_ORIGIN: &str = "https://music.example.com";
const COOKIE_NAME: &str = "__Host-harmony_guest";
const TRACK_A: &str = "0190c5a0-0000-7000-8000-00000000000a";
const TRACK_B: &str = "0190c5a0-0000-7000-8000-00000000000b";
const TRACK_C: &str = "0190c5a0-0000-7000-8000-00000000000c";
const LARGE_TRACK: &str = "0190c5a0-0000-7000-8000-00000000000d";
const LARGE_TRACK_BYTES: usize = 16 * 1024 * 1024;
const ROOM: &str = "/api/v1/listen-together/room";
const COMMANDS: &str = "/api/v1/listen-together/room/commands";
const EVENTS: &str = "/api/v1/listen-together/room/events";
const SESSION: &str = "/api/v1/listen-together/session";

struct Harness {
    owner: Router,
    guest: Router,
    state: AppState,
    _directory: tempfile::TempDir,
}

async fn harness_with(timings: Timings, max_guests: &str) -> Harness {
    let directory = tempfile::tempdir().unwrap();
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let pool = db::open_and_migrate(
        directory.path().join("app.db").to_str().unwrap(),
        &migrations,
    )
    .await
    .expect("database");
    let large_file = directory.path().join("large.flac");
    std::fs::write(&large_file, vec![0_u8; LARGE_TRACK_BYTES]).unwrap();
    let managed = directory.path().join("managed");
    let config = Config::from_lookup(|name| match name {
        "MANAGED_STORAGE_PATH" => Some(managed.to_string_lossy().into_owned()),
        "RADIO_BROWSER_BASE_URL" => Some("http://127.0.0.1:9".to_owned()),
        "ARTIST_INFO_ENABLED" | "LYRICS_LOOKUP_ENABLED" => Some("false".to_owned()),
        "APP_VERSION" => Some("listen-together-test".to_owned()),
        "GUEST_ADDR" => Some("127.0.0.1:18091".to_owned()),
        "PUBLIC_URL" => Some(PUBLIC_ORIGIN.to_owned()),
        "LISTEN_TOGETHER_MAX_GUESTS" => Some(max_guests.to_owned()),
        _ => None,
    })
    .expect("config");
    for statement in [
        "INSERT INTO artists (id, name, name_sort) VALUES ('ar', 'Skillet', 'skillet')".to_owned(),
        "INSERT INTO albums (id, artist_id, title, title_sort) VALUES ('al', 'ar', 'Awake', 'awake')".to_owned(),
        format!(
            "INSERT INTO tracks (id, album_id, title, title_sort, artist_name, format, file_path, duration_ms, lyrics)
             VALUES ('{TRACK_A}', 'al', 'Hero', 'hero', 'Skillet', 'flac', '/owner/secret/a.flac', 186600, ''),
                    ('{TRACK_B}', 'al', 'Monster', 'monster', 'Skillet', 'flac', '/owner/secret/b.flac', 178000, ''),
                    ('{TRACK_C}', 'al', 'Lucy', 'lucy', 'Skillet', 'flac', '/owner/secret/c.flac', 222000, ''),
                    ('{LARGE_TRACK}', 'al', 'Long', 'long', 'Skillet', 'flac', '{}', 600000, '')",
            large_file.display()
        ),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(statement))
            .execute(&pool)
            .await
            .unwrap();
    }
    let listen_together =
        ListenTogether::with_timings(&config, LibraryService::new(pool.clone()), timings);
    let mut state = AppState::new(config, dependencies::probe_system().await, pool);
    state.listen_together = Arc::new(listen_together);
    Harness {
        owner: app::router(state.clone()),
        guest: app::guest_router(state.clone()).expect("configured"),
        state,
        _directory: directory,
    }
}

async fn harness() -> Harness {
    harness_with(Timings::default(), "8").await
}

fn on_owner(method: Method, uri: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1:8090");
    with_body(builder, body)
}

fn on_guest(method: Method, uri: &str, cookie: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, PUBLIC_HOST);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    with_body(builder, body)
}

fn with_body(builder: axum::http::request::Builder, body: Option<Value>) -> Request<Body> {
    match body {
        Some(body) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let body = tokio::time::timeout(Duration::from_secs(5), response.into_body().collect())
        .await
        .expect("body ends")
        .expect("body")
        .to_bytes();
    (
        status,
        headers,
        serde_json::from_slice(&body).unwrap_or(Value::Null),
    )
}

async fn status_of(router: &Router, request: Request<Body>) -> StatusCode {
    send(router, request).await.0
}

async fn open_room(harness: &Harness) -> Value {
    let (status, _, room) = send(&harness.owner, on_owner(Method::POST, ROOM, None)).await;
    assert_eq!(status, StatusCode::CREATED);
    room
}

fn token(room: &Value) -> String {
    let url = room["inviteUrl"].as_str().expect("inviteUrl");
    let (base, token) = url.split_once('#').expect("fragment");
    assert_eq!(base, format!("{PUBLIC_ORIGIN}/join"));
    token.to_owned()
}

async fn try_join(router: &Router, token: &str) -> (StatusCode, HeaderMap, Value) {
    send(
        router,
        on_guest(
            Method::POST,
            "/api/v1/listen-together/join",
            None,
            Some(json!({ "token": token })),
        ),
    )
    .await
}

/// The `name=value` a browser would send back.
fn cookie_from(headers: &HeaderMap) -> String {
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("Set-Cookie")
        .to_str()
        .unwrap();
    cookie.split(';').next().unwrap().to_owned()
}

async fn join(harness: &Harness, token: &str) -> (String, Value) {
    let (status, headers, room) = try_join(&harness.guest, token).await;
    assert_eq!(status, StatusCode::OK);
    (cookie_from(&headers), room)
}

async fn owner_command(harness: &Harness, command: Value) -> (StatusCode, Value) {
    let (status, _, room) = send(
        &harness.owner,
        on_owner(Method::POST, COMMANDS, Some(command)),
    )
    .await;
    (status, room)
}

async fn guest_command(harness: &Harness, cookie: &str, command: Value) -> (StatusCode, Value) {
    let (status, _, room) = send(
        &harness.guest,
        on_guest(Method::POST, COMMANDS, Some(cookie), Some(command)),
    )
    .await;
    (status, room)
}

async fn guest_room(harness: &Harness, cookie: &str) -> (StatusCode, Value) {
    let (status, _, room) = send(
        &harness.guest,
        on_guest(Method::GET, ROOM, Some(cookie), None),
    )
    .await;
    (status, room)
}

async fn role(harness: &Harness, cookie: Option<&str>) -> String {
    let (_, _, session) = send(&harness.guest, on_guest(Method::GET, SESSION, cookie, None)).await;
    session["role"].as_str().unwrap().to_owned()
}

/// Reads server-sent events from a response body.
struct Events {
    stream: BodyDataStream,
    buffer: String,
}

impl Events {
    async fn connect(router: &Router, request: Request<Body>) -> Self {
        let response = router.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        Self {
            stream: response.into_body().into_data_stream(),
            buffer: String::new(),
        }
    }

    /// The next event's name and data, or `None` when the stream ended.
    async fn next(&mut self) -> Option<(String, Value)> {
        loop {
            if let Some((frame, rest)) = self.buffer.split_once("\n\n") {
                let (frame, rest) = (frame.to_owned(), rest.to_owned());
                self.buffer = rest;
                let field = |name: &str| {
                    frame
                        .lines()
                        .find_map(|line| line.strip_prefix(name))
                        .map(str::to_owned)
                };
                if let Some(event) = field("event: ") {
                    let data = field("data: ").expect("an event needs a data line");
                    return Some((event, serde_json::from_str(&data).expect("JSON data")));
                }
                continue;
            }
            let chunk = tokio::time::timeout(Duration::from_secs(5), self.stream.next())
                .await
                .expect("an event in time")?
                .expect("chunk");
            self.buffer.push_str(std::str::from_utf8(&chunk).unwrap());
        }
    }
}

/// Every operation a Guest may reach without a session.
const PUBLIC_OPERATIONS: &[(&str, &str)] = &[
    ("get", "/api/v1/health"),
    ("get", "/api/v1/listen-together/session"),
    ("post", "/api/v1/listen-together/join"),
];

/// Every operation a Guest may reach with a session.
const SESSION_OPERATIONS: &[(&str, &str)] = &[
    ("get", "/api/v1/me"),
    ("get", "/api/v1/preferences"),
    ("get", "/api/v1/library/artists"),
    ("get", "/api/v1/library/artists/{artistId}"),
    ("get", "/api/v1/library/artists/{artistId}/photo"),
    ("get", "/api/v1/library/artists/{artistId}/biography"),
    ("get", "/api/v1/library/artists/{artistId}/popular-tracks"),
    ("get", "/api/v1/library/tracks/play-counts"),
    ("get", "/api/v1/library/albums"),
    ("get", "/api/v1/library/albums/{albumId}"),
    ("get", "/api/v1/library/albums/{albumId}/online-genres"),
    ("get", "/api/v1/library/albums/{albumId}/cover"),
    ("get", "/api/v1/library/tracks"),
    ("get", "/api/v1/library/tracks/{trackId}"),
    ("get", "/api/v1/library/search"),
    ("get", "/api/v1/library/tracks/{trackId}/lyrics"),
    ("get", "/api/v1/library/tracks/{trackId}/waveform"),
    ("get", "/api/v1/tracks/{trackId}/stream"),
    ("get", "/api/v1/listen-together/room"),
    ("get", "/api/v1/listen-together/room/events"),
    ("get", "/api/v1/listen-together/room/clock"),
    ("post", "/api/v1/listen-together/room/commands"),
    ("post", "/api/v1/listen-together/leave"),
];

fn documented_operations() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../packages/contracts/openapi.yaml"),
    )
    .expect("read contract");
    let contract: Value = serde_yaml_ng::from_str(&text).expect("parse contract");
    let mut operations = Vec::new();
    for (path, item) in contract["paths"].as_object().expect("paths") {
        for method in ["get", "post", "put", "patch", "delete", "head"] {
            if item.get(method).is_some() {
                operations.push((method.to_owned(), path.clone()));
            }
        }
    }
    operations
}

fn probe(method: &str, path: &str, cookie: Option<&str>) -> Request<Body> {
    let parameter = regex::Regex::new(r"\{[^}]+\}").unwrap();
    let uri = parameter.replace_all(path, "contract-probe");
    let method = Method::from_bytes(method.to_uppercase().as_bytes()).unwrap();
    let body = (method != Method::GET).then(|| json!({}));
    on_guest(method, &uri, cookie, body)
}

/// Every documented operation that is not on a list above is refused, even
/// with a live session, so a new route fails closed until it is listed.
#[tokio::test(flavor = "multi_thread")]
async fn every_other_documented_operation_is_refused_on_the_guest_listener() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let (cookie, _) = join(&harness, &token(&room)).await;
    let is_listed = |method: &str, path: &str| {
        PUBLIC_OPERATIONS
            .iter()
            .chain(SESSION_OPERATIONS)
            .any(|&(listed_method, listed_path)| listed_method == method && listed_path == path)
    };
    let mut refused = 0;
    for (method, path) in documented_operations() {
        if is_listed(&method, &path) {
            continue;
        }
        let (status, _, body) = send(&harness.guest, probe(&method, &path, Some(&cookie))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
        // A HEAD response has no body to read a code from.
        if method != "head" {
            assert_eq!(body["code"], "not_found", "{method} {path}");
        }
        refused += 1;
    }
    assert!(refused > 50, "only {refused} operations refused");
    assert_eq!(guest_room(&harness, &cookie).await.0, StatusCode::OK);
    // A method the contract does not list on an allowed route is refused too.
    for (method, path) in [
        (Method::POST, ROOM),
        (Method::DELETE, ROOM),
        (Method::PATCH, "/api/v1/preferences"),
        (Method::HEAD, "/api/v1/library/tracks"),
        (Method::OPTIONS, COMMANDS),
    ] {
        assert_eq!(
            status_of(
                &harness.guest,
                on_guest(method.clone(), path, Some(&cookie), None)
            )
            .await,
            StatusCode::NOT_FOUND,
            "{method} {path}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_lists_above_name_documented_operations() {
    let documented = documented_operations();
    for (method, path) in PUBLIC_OPERATIONS.iter().chain(SESSION_OPERATIONS) {
        assert!(
            documented
                .iter()
                .any(|(documented_method, documented_path)| {
                    documented_method == method && documented_path == path
                }),
            "{method} {path}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn session_operations_need_a_live_session() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let (cookie, _) = join(&harness, &token(&room)).await;
    for &(method, path) in SESSION_OPERATIONS {
        for cookie in [None, Some("__Host-harmony_guest=forged")] {
            let (status, _, body) = send(&harness.guest, probe(method, path, cookie)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
            assert_eq!(body["code"], "unauthorized", "{method} {path}");
        }
    }
    // HEAD on a stream is the one HEAD on the allowlist.
    let head = on_guest(
        Method::HEAD,
        "/api/v1/tracks/contract-probe/stream",
        None,
        None,
    );
    assert_eq!(
        status_of(&harness.guest, head).await,
        StatusCode::UNAUTHORIZED
    );
    let head = on_guest(
        Method::HEAD,
        &format!("/api/v1/tracks/{LARGE_TRACK}/stream"),
        Some(&cookie),
        None,
    );
    assert_eq!(status_of(&harness.guest, head).await, StatusCode::OK);
    // The session is what opens them: the same reads pass with it.
    for path in [
        "/api/v1/me",
        "/api/v1/preferences",
        "/api/v1/library/artists",
        "/api/v1/library/albums",
        "/api/v1/library/tracks",
        "/api/v1/library/search?q=hero",
    ] {
        let request = on_guest(Method::GET, path, Some(&cookie), None);
        assert_eq!(
            status_of(&harness.guest, request).await,
            StatusCode::OK,
            "{path}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn library_search_hides_every_playlist_from_a_guest() {
    let harness = harness().await;
    // More than one result group holds, so a capped group cannot hide the rest.
    for number in 0..7 {
        sqlx::query("INSERT INTO playlists (id, user_id, name) VALUES (?, ?, ?)")
            .bind(format!("playlist-{number}"))
            .bind(earthly_music_server::auth::DEFAULT_USER_ID)
            .bind(format!("Hero Mix {number}"))
            .execute(&harness.state.db)
            .await
            .unwrap();
    }
    let room = open_room(&harness).await;
    let (cookie, _) = join(&harness, &token(&room)).await;
    let path = "/api/v1/library/search?q=hero";

    let (_, _, owner) = send(&harness.owner, on_owner(Method::GET, path, None)).await;
    let (status, _, guest) = send(
        &harness.guest,
        on_guest(Method::GET, path, Some(&cookie), None),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner["total"], 8);
    assert_eq!(guest["total"], 1);
    assert_eq!(guest["playlists"], json!([]));
    assert_eq!(guest["bestMatch"]["type"], "track");
    assert!(!guest.to_string().contains("Hero Mix"), "{guest}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_never_learns_where_the_owners_files_are() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let (cookie, _) = join(&harness, &token(&room)).await;
    for path in [
        "/api/v1/library/tracks".to_owned(),
        format!("/api/v1/library/tracks/{TRACK_A}"),
        "/api/v1/library/albums/al".to_owned(),
        "/api/v1/library/search?q=hero".to_owned(),
    ] {
        let (status, _, body) = send(
            &harness.guest,
            on_guest(Method::GET, &path, Some(&cookie), None),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{path}");
        let text = body.to_string();
        assert!(text.contains("Hero"), "{path}: {text}");
        assert!(!text.contains("/owner/secret"), "{path}: {text}");
    }
    let (_, _, track) = send(
        &harness.owner,
        on_owner(
            Method::GET,
            &format!("/api/v1/library/tracks/{TRACK_A}"),
            None,
        ),
    )
    .await;
    assert_eq!(track["filePath"], "/owner/secret/a.flac");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_guest_listener_serves_the_web_client_but_no_docs_or_other_api_paths() {
    let harness = harness().await;
    for path in ["/", "/join", "/library/albums"] {
        let (_, _, body) = send(&harness.guest, on_guest(Method::GET, path, None, None)).await;
        // Only the gate answers with JSON; without a built Web Client the
        // static handler has an empty 404.
        assert_eq!(body, Value::Null, "{path}");
    }
    for path in [
        "/docs",
        "/docs/index.html",
        "/api",
        "/api/docs",
        "/api/docs/scalar/a.js",
        "/api/docs/openapi.yaml",
        "/api/v1/nothing",
    ] {
        let (status, _, body) = send(&harness.guest, on_guest(Method::GET, path, None, None)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(body["code"], "not_found", "{path}");
    }
    let post = on_guest(Method::POST, "/join", None, Some(json!({})));
    assert_eq!(status_of(&harness.guest, post).await, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forged_host_or_origin_is_refused() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let (cookie, _) = join(&harness, &token(&room)).await;
    let health = "/api/v1/health";
    for host in [
        "evil.example",
        "music.example.com.evil.example",
        "127.0.0.1:18091",
        "music.example.com:8443",
    ] {
        let request = Request::builder()
            .uri(health)
            .header(header::HOST, host)
            .body(Body::empty())
            .unwrap();
        let (status, _, body) = send(&harness.guest, request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{host}");
        assert_eq!(body["code"], "forbidden_host", "{host}");
    }
    let no_host = Request::builder().uri(health).body(Body::empty()).unwrap();
    assert_eq!(
        status_of(&harness.guest, no_host).await,
        StatusCode::FORBIDDEN
    );
    // An absolute request target does not stand in for a Host header.
    let absolute = Request::builder()
        .uri(format!("{PUBLIC_ORIGIN}{health}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        status_of(&harness.guest, absolute).await,
        StatusCode::FORBIDDEN
    );
    let host_443 = Request::builder()
        .uri(health)
        .header(header::HOST, "music.example.com:443")
        .body(Body::empty())
        .unwrap();
    assert_eq!(status_of(&harness.guest, host_443).await, StatusCode::OK);

    for origin in [
        "https://evil.example",
        "http://music.example.com",
        "https://music.example.com:8443",
        "null",
    ] {
        for method in [Method::GET, Method::POST] {
            let body = (method == Method::POST).then(|| json!({ "type": "pause" }));
            let uri = if method == Method::GET {
                ROOM
            } else {
                COMMANDS
            };
            let mut request = on_guest(method.clone(), uri, Some(&cookie), body);
            request
                .headers_mut()
                .insert(header::ORIGIN, origin.parse().unwrap());
            let (status, _, body) = send(&harness.guest, request).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {origin}");
            assert_eq!(body["code"], "forbidden_origin", "{method} {origin}");
        }
    }
    let mut own = on_guest(
        Method::POST,
        COMMANDS,
        Some(&cookie),
        Some(json!({ "type": "pause" })),
    );
    own.headers_mut()
        .insert(header::ORIGIN, PUBLIC_ORIGIN.parse().unwrap());
    assert_eq!(status_of(&harness.guest, own).await, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cross_site_form_cannot_send_a_command() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let (cookie, _) = join(&harness, &token(&room)).await;
    let request = Request::builder()
        .method(Method::POST)
        .uri(COMMANDS)
        .header(header::HOST, PUBLIC_HOST)
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(r#"{"type":"pause"}"#))
        .unwrap();
    assert_eq!(
        status_of(&harness.guest, request).await,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_owners_listener_is_owner_and_has_no_join_or_leave() {
    let harness = harness().await;
    let (_, _, session) = send(&harness.owner, on_owner(Method::GET, SESSION, None)).await;
    assert_eq!(session, json!({ "role": "owner" }));
    assert_eq!(role(&harness, None).await, "none");
    let room = open_room(&harness).await;
    for path in [
        "/api/v1/listen-together/join",
        "/api/v1/listen-together/leave",
    ] {
        let request = on_owner(Method::POST, path, Some(json!({ "token": token(&room) })));
        let (status, _, body) = send(&harness.owner, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(body["code"], "not_found", "{path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn one_room_at_a_time() {
    let harness = harness().await;
    assert_eq!(
        status_of(&harness.owner, on_owner(Method::GET, ROOM, None)).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        status_of(&harness.owner, on_owner(Method::DELETE, ROOM, None)).await,
        StatusCode::NOT_FOUND
    );
    let room = open_room(&harness).await;
    assert_eq!(room["revision"], 1);
    assert_eq!(room["currentIndex"], Value::Null);
    assert_eq!(room["maxGuests"], 8);
    let (status, _, body) = send(&harness.owner, on_owner(Method::POST, ROOM, None)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "conflict");
    let (_, _, again) = send(&harness.owner, on_owner(Method::GET, ROOM, None)).await;
    assert_eq!(again["inviteUrl"], room["inviteUrl"]);
    assert_eq!(
        status_of(&harness.owner, on_owner(Method::DELETE, ROOM, None)).await,
        StatusCode::NO_CONTENT
    );
    let reopened = open_room(&harness).await;
    assert_ne!(token(&reopened), token(&room));
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_guest_address_there_is_no_room_and_no_capability() {
    let directory = tempfile::tempdir().unwrap();
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let pool = db::open_and_migrate(
        directory.path().join("app.db").to_str().unwrap(),
        &migrations,
    )
    .await
    .expect("database");
    let managed = directory.path().join("managed");
    let config = Config::from_lookup(|name| match name {
        "MANAGED_STORAGE_PATH" => Some(managed.to_string_lossy().into_owned()),
        _ => None,
    })
    .expect("config");
    let state = AppState::new(config, dependencies::probe_system().await, pool);
    assert!(app::guest_router(state.clone()).is_none());
    let owner = app::router(state);
    let (status, _, body) = send(&owner, on_owner(Method::POST, ROOM, None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "listen_together_disabled");
    let (_, _, health) = send(&owner, on_owner(Method::GET, "/api/v1/health", None)).await;
    let capabilities = health["capabilities"].to_string();
    assert!(
        !capabilities.contains("listen-together.v1"),
        "{capabilities}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn guest_health_tells_only_what_the_web_client_needs() {
    let harness = harness().await;
    let (_, _, owner) = send(
        &harness.owner,
        on_owner(Method::GET, "/api/v1/health", None),
    )
    .await;
    let capabilities: Vec<&str> = owner["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(capabilities.contains(&"listen-together.v1"));
    assert!(capabilities.contains(&"managed-import.v1"));

    let (status, _, guest) = send(
        &harness.guest,
        on_guest(Method::GET, "/api/v1/health", None, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(guest["status"], "ok");
    assert_eq!(guest["version"], "listen-together-test");
    assert_eq!(guest["dependencies"], json!([]));
    assert!(guest.get("externalSource").is_none());
    let capabilities = guest["capabilities"].as_array().unwrap();
    assert!(capabilities.contains(&json!("api.v1")));
    assert!(capabilities.contains(&json!("listen-together.v1")));
    assert!(!capabilities.contains(&json!("managed-import.v1")));
    assert!(!capabilities.contains(&json!("playback.queue-events.v1")));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invalid_invite_is_refused() {
    let harness = harness().await;
    // No Room at all looks the same as a wrong token.
    let (status, _, body) = try_join(&harness.guest, "anything").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "invite_invalid");
    let room = open_room(&harness).await;
    let good = token(&room);
    let wrong = format!("{}x", &good[..good.len() - 1]);
    let (status, headers, body) = try_join(&harness.guest, &wrong).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "invite_invalid");
    assert!(!headers.contains_key(header::SET_COOKIE));
    for invalid in [String::new(), "x".repeat(129)] {
        let (status, _, _) = try_join(&harness.guest, &invalid).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let not_json = on_guest(
        Method::POST,
        "/api/v1/listen-together/join",
        None,
        Some(json!({ "invite": "x" })),
    );
    assert_eq!(
        status_of(&harness.guest, not_json).await,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_session_cookie_is_host_only_http_only_and_strict() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let (status, headers, _) = try_join(&harness.guest, &token(&room)).await;
    assert_eq!(status, StatusCode::OK);
    let cookie = headers[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with(&format!("{COOKIE_NAME}=")), "{cookie}");
    let value = cookie_from(&headers);
    assert_eq!(value.len(), COOKIE_NAME.len() + 1 + 43, "{value}");
    assert!(
        cookie.ends_with("; Path=/; HttpOnly; SameSite=Strict; Secure"),
        "{cookie}"
    );
    assert!(!cookie.contains("Domain"), "{cookie}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_ninth_guest_is_refused_and_leaving_frees_a_place() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let invite = token(&room);
    let mut cookies = Vec::new();
    for count in 1..=8 {
        let (cookie, joined) = join(&harness, &invite).await;
        assert_eq!(joined["guestCount"], count);
        cookies.push(cookie);
    }
    let (status, headers, body) = try_join(&harness.guest, &invite).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "room_full");
    assert!(!headers.contains_key(header::SET_COOKIE));

    let cookie = cookies.pop().unwrap();
    let leave = on_guest(
        Method::POST,
        "/api/v1/listen-together/leave",
        Some(&cookie),
        None,
    );
    let (status, headers, _) = send(&harness.guest, leave).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let cleared = headers[header::SET_COOKIE].to_str().unwrap();
    assert!(
        cleared.starts_with(&format!("{COOKIE_NAME}=;")),
        "{cleared}"
    );
    assert!(cleared.contains("Max-Age=0"), "{cleared}");
    assert_eq!(
        guest_room(&harness, &cookie).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, _, owner) = send(&harness.owner, on_owner(Method::GET, ROOM, None)).await;
    assert_eq!(owner["guestCount"], 7);
    let (_, rejoined) = join(&harness, &invite).await;
    assert_eq!(rejoined["guestCount"], 8);
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_joins_never_pass_the_limit() {
    let harness = harness_with(Timings::default(), "3").await;
    let room = open_room(&harness).await;
    let invite = token(&room);
    let attempts: Vec<_> = (0..16)
        .map(|_| {
            let router = harness.guest.clone();
            let invite = invite.clone();
            tokio::spawn(async move { try_join(&router, &invite).await.0 })
        })
        .collect();
    let mut joined = 0;
    let mut refused = 0;
    for attempt in attempts {
        match attempt.await.unwrap() {
            StatusCode::OK => joined += 1,
            StatusCode::CONFLICT => refused += 1,
            status => panic!("unexpected status {status}"),
        }
    }
    assert_eq!((joined, refused), (3, 13));
    let (_, _, owner) = send(&harness.owner, on_owner(Method::GET, ROOM, None)).await;
    assert_eq!(owner["guestCount"], 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn opening_the_link_again_keeps_the_same_place() {
    let harness = harness_with(Timings::default(), "1").await;
    let room = open_room(&harness).await;
    let invite = token(&room);
    let (cookie, _) = join(&harness, &invite).await;
    let request = on_guest(
        Method::POST,
        "/api/v1/listen-together/join",
        Some(&cookie),
        Some(json!({ "token": invite })),
    );
    let (status, headers, again) = send(&harness.guest, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cookie_from(&headers), cookie);
    assert_eq!(again["guestCount"], 1);
    // A wrong token with a live cookie is still wrong.
    let request = on_guest(
        Method::POST,
        "/api/v1/listen-together/join",
        Some(&cookie),
        Some(json!({ "token": "wrong" })),
    );
    assert_eq!(
        status_of(&harness.guest, request).await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn invite_url_is_never_in_a_guest_response() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let invite = token(&room);
    let (cookie, joined) = join(&harness, &invite).await;
    assert!(joined.get("inviteUrl").is_none());
    assert!(
        guest_room(&harness, &cookie)
            .await
            .1
            .get("inviteUrl")
            .is_none()
    );
    let (_, after_command) = guest_command(
        &harness,
        &cookie,
        json!({ "type": "setLyricsOpen", "open": true }),
    )
    .await;
    assert_eq!(after_command["lyricsOpen"], true);
    assert!(after_command.get("inviteUrl").is_none());
    let (_, owner_view) =
        owner_command(&harness, json!({ "type": "setLyricsOpen", "open": false })).await;
    assert_eq!(owner_view["inviteUrl"], room["inviteUrl"]);

    let mut events = Events::connect(
        &harness.guest,
        on_guest(Method::GET, EVENTS, Some(&cookie), None),
    )
    .await;
    let (_, first) = events.next().await.unwrap();
    assert!(first.get("inviteUrl").is_none());
    assert!(!first.to_string().contains(&invite));
    owner_command(&harness, json!({ "type": "setLyricsOpen", "open": true })).await;
    let (_, second) = events.next().await.unwrap();
    assert_eq!(second["lyricsOpen"], true);
    assert!(second.get("inviteUrl").is_none());

    let mut owner_events =
        Events::connect(&harness.owner, on_owner(Method::GET, EVENTS, None)).await;
    let (_, first) = owner_events.next().await.unwrap();
    assert_eq!(first["inviteUrl"], room["inviteUrl"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshots_reach_every_client_and_room_closed_ends_the_stream() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let (cookie, _) = join(&harness, &token(&room)).await;
    let mut guest = Events::connect(
        &harness.guest,
        on_guest(Method::GET, EVENTS, Some(&cookie), None),
    )
    .await;
    let mut owner = Events::connect(&harness.owner, on_owner(Method::GET, EVENTS, None)).await;

    let (event, first) = guest.next().await.unwrap();
    assert_eq!(event, "room-state");
    assert_eq!(first["guestCount"], 1);
    let (_, owner_first) = owner.next().await.unwrap();
    assert_eq!(owner_first["revision"], first["revision"]);

    let (status, played) = guest_command(
        &harness,
        &cookie,
        json!({ "type": "playTracks", "trackIds": [TRACK_A, TRACK_B], "index": 0 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for events in [&mut guest, &mut owner] {
        let (event, snapshot) = events.next().await.unwrap();
        assert_eq!(event, "room-state");
        assert_eq!(snapshot["revision"], played["revision"]);
        assert_eq!(snapshot["trackIds"], json!([TRACK_A, TRACK_B]));
        assert_eq!(snapshot["currentIndex"], 0);
        assert_eq!(snapshot["playing"], true);
    }
    assert!(played["revision"].as_i64() > first["revision"].as_i64());

    // A second Guest changes guestCount, which everyone sees.
    let (_, joined) = join(&harness, &token(&room)).await;
    assert_eq!(joined["guestCount"], 2);
    let (_, snapshot) = owner.next().await.unwrap();
    assert_eq!(snapshot["guestCount"], 2);

    assert_eq!(
        status_of(&harness.owner, on_owner(Method::DELETE, ROOM, None)).await,
        StatusCode::NO_CONTENT
    );
    for events in [&mut guest, &mut owner] {
        loop {
            let (event, data) = events.next().await.unwrap();
            if event == "room-closed" {
                assert_eq!(data, json!({}));
                break;
            }
        }
        assert!(
            events.next().await.is_none(),
            "room-closed comes once, last"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn snapshots_coalesce_for_a_slow_client() {
    let harness = harness().await;
    open_room(&harness).await;
    let mut events = Events::connect(&harness.owner, on_owner(Method::GET, EVENTS, None)).await;
    events.next().await.unwrap();
    for position_ms in [1, 2, 3] {
        owner_command(
            &harness,
            json!({ "type": "playTracks", "trackIds": [TRACK_A], "index": 0 }),
        )
        .await;
        owner_command(
            &harness,
            json!({ "type": "seek", "positionMs": position_ms }),
        )
        .await;
    }
    let (_, snapshot) = events.next().await.unwrap();
    assert_eq!(snapshot["positionMs"], 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_the_room_revokes_everything_at_once() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let invite = token(&room);
    let (cookie, _) = join(&harness, &invite).await;
    assert_eq!(role(&harness, Some(&cookie)).await, "guest");
    let stream_uri = format!("/api/v1/tracks/{LARGE_TRACK}/stream");
    let response = harness
        .guest
        .clone()
        .oneshot(on_guest(Method::GET, &stream_uri, Some(&cookie), None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut audio = response.into_body().into_data_stream();
    let first = audio.next().await.expect("a chunk").expect("audio");

    assert_eq!(
        status_of(&harness.owner, on_owner(Method::DELETE, ROOM, None)).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(role(&harness, Some(&cookie)).await, "none");
    assert_eq!(
        guest_room(&harness, &cookie).await.0,
        StatusCode::UNAUTHORIZED
    );
    let read = on_guest(Method::GET, "/api/v1/library/tracks", Some(&cookie), None);
    assert_eq!(
        status_of(&harness.guest, read).await,
        StatusCode::UNAUTHORIZED
    );
    let (status, _, body) = try_join(&harness.guest, &invite).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "invite_invalid");
    // The audio already on its way stops too.
    let mut received = first.len();
    while let Some(chunk) = audio.next().await {
        received += chunk.expect("audio").len();
    }
    assert!(received < LARGE_TRACK_BYTES, "received {received} bytes");
    // A new Room does not revive an old session or link.
    open_room(&harness).await;
    assert_eq!(role(&harness, Some(&cookie)).await, "none");
    assert_eq!(
        try_join(&harness.guest, &invite).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_who_leaves_gets_nothing_more_on_another_tab() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let (cookie, _) = join(&harness, &token(&room)).await;
    let mut events = Events::connect(
        &harness.guest,
        on_guest(Method::GET, EVENTS, Some(&cookie), None),
    )
    .await;
    events.next().await.unwrap();
    let leave = on_guest(
        Method::POST,
        "/api/v1/listen-together/leave",
        Some(&cookie),
        None,
    );
    assert_eq!(
        status_of(&harness.guest, leave).await,
        StatusCode::NO_CONTENT
    );
    owner_command(&harness, json!({ "type": "setLyricsOpen", "open": true })).await;
    assert!(events.next().await.is_none());
}

fn short_room() -> Timings {
    Timings {
        room_lifetime: Duration::from_millis(400),
        guest_grace: Duration::from_secs(60),
        sweep_interval: Duration::from_millis(10),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_room_closes_itself() {
    let harness = harness_with(short_room(), "8").await;
    let room = open_room(&harness).await;
    let invite = token(&room);
    let (cookie, _) = join(&harness, &invite).await;
    let mut events = Events::connect(
        &harness.guest,
        on_guest(Method::GET, EVENTS, Some(&cookie), None),
    )
    .await;
    events.next().await.unwrap();
    loop {
        let (event, _) = events.next().await.expect("room-closed before the end");
        if event == "room-closed" {
            break;
        }
    }
    assert!(events.next().await.is_none());
    assert_eq!(
        guest_room(&harness, &cookie).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        status_of(&harness.owner, on_owner(Method::GET, ROOM, None)).await,
        StatusCode::NOT_FOUND
    );
    let (status, _, body) = try_join(&harness.guest, &invite).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "invite_invalid");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_room_is_closed_even_between_sweeps() {
    let timings = Timings {
        room_lifetime: Duration::from_millis(50),
        guest_grace: Duration::from_secs(60),
        sweep_interval: Duration::from_secs(3600),
    };
    let harness = harness_with(timings, "8").await;
    let room = open_room(&harness).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        try_join(&harness.guest, &token(&room)).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_gone_for_the_grace_period_loses_their_place() {
    let timings = Timings {
        room_lifetime: Duration::from_secs(600),
        guest_grace: Duration::from_millis(600),
        sweep_interval: Duration::from_millis(10),
    };
    let harness = harness_with(timings, "8").await;
    let room = open_room(&harness).await;
    let invite = token(&room);
    let (stays, _) = join(&harness, &invite).await;
    let (never_connected, _) = join(&harness, &invite).await;
    let mut events = Events::connect(
        &harness.guest,
        on_guest(Method::GET, EVENTS, Some(&stays), None),
    )
    .await;
    events.next().await.unwrap();
    tokio::time::sleep(Duration::from_millis(900)).await;

    // An open stream keeps its Guest in; the other one is out, and everyone
    // is told.
    assert_eq!(guest_room(&harness, &stays).await.0, StatusCode::OK);
    assert_eq!(
        guest_room(&harness, &never_connected).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, snapshot) = events.next().await.unwrap();
    assert_eq!(snapshot["guestCount"], 1);

    // A reconnect inside the grace period keeps the place.
    drop(events);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut events = Events::connect(
        &harness.guest,
        on_guest(Method::GET, EVENTS, Some(&stays), None),
    )
    .await;
    events.next().await.unwrap();
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(guest_room(&harness, &stays).await.0, StatusCode::OK);

    drop(events);
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(role(&harness, Some(&stays)).await, "none");
    let (_, _, owner) = send(&harness.owner, on_owner(Method::GET, ROOM, None)).await;
    assert_eq!(owner["guestCount"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn play_tracks_takes_only_library_tracks_and_keeps_duplicates() {
    let harness = harness().await;
    open_room(&harness).await;
    let (status, body) = owner_command(
        &harness,
        json!({ "type": "playTracks", "trackIds": [TRACK_A, "0190c5a0-0000-7000-8000-0000000000ff"], "index": 0 }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["field"], "trackIds");
    let (_, _, room) = send(&harness.owner, on_owner(Method::GET, ROOM, None)).await;
    assert_eq!(room["trackIds"], json!([]));

    for command in [
        json!({ "type": "playTracks", "trackIds": [], "index": 0 }),
        json!({ "type": "playTracks", "trackIds": [TRACK_A], "index": 1 }),
        json!({ "type": "seek", "positionMs": -1 }),
        json!({ "type": "nonsense" }),
    ] {
        assert_eq!(
            owner_command(&harness, command.clone()).await.0,
            StatusCode::BAD_REQUEST,
            "{command}"
        );
    }

    let (status, room) = owner_command(
        &harness,
        json!({ "type": "playTracks", "trackIds": [TRACK_A, TRACK_B, TRACK_A], "index": 2 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(room["trackIds"], json!([TRACK_A, TRACK_B, TRACK_A]));
    assert_eq!(room["currentIndex"], 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_skip_reported_by_every_client_counts_once() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let (cookie, _) = join(&harness, &token(&room)).await;
    owner_command(
        &harness,
        json!({ "type": "playTracks", "trackIds": [TRACK_A, TRACK_B, TRACK_C], "index": 0 }),
    )
    .await;
    let skip = json!({ "type": "skip", "direction": "next", "fromTrackId": TRACK_A });
    let (_, first) = guest_command(&harness, &cookie, skip.clone()).await;
    assert_eq!(first["currentIndex"], 1);
    let (status, second) = owner_command(&harness, skip).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["currentIndex"], 1);
    assert_eq!(second["revision"], first["revision"]);

    // Past the end the Room stops, and a second report changes nothing.
    owner_command(&harness, json!({ "type": "skip", "direction": "next" })).await;
    let last = json!({ "type": "skip", "direction": "next", "fromTrackId": TRACK_C });
    let (_, stopped) = owner_command(&harness, last.clone()).await;
    assert_eq!(stopped["playing"], false);
    assert_eq!(stopped["currentIndex"], 2);
    let (_, again) = guest_command(&harness, &cookie, last).await;
    assert_eq!(again["revision"], stopped["revision"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_keeps_the_position_and_resume_restarts_the_clock() {
    let harness = harness().await;
    open_room(&harness).await;
    owner_command(
        &harness,
        json!({ "type": "playTracks", "trackIds": [TRACK_A], "index": 0 }),
    )
    .await;
    owner_command(&harness, json!({ "type": "seek", "positionMs": 10_000 })).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (_, paused) = owner_command(&harness, json!({ "type": "pause" })).await;
    assert_eq!(paused["playing"], false);
    let position = paused["positionMs"].as_i64().unwrap();
    assert!(position >= 10_050, "{position}");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (_, _, same) = send(&harness.owner, on_owner(Method::GET, ROOM, None)).await;
    assert_eq!(same["positionMs"], position);
    let (_, resumed) = owner_command(&harness, json!({ "type": "resume" })).await;
    assert_eq!(resumed["playing"], true);
    assert_eq!(resumed["positionMs"], position);
    assert!(
        resumed["positionAtMs"].as_i64() >= Some(paused["positionAtMs"].as_i64().unwrap() + 50)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn each_client_has_its_own_command_limit() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let invite = token(&room);
    let (first, _) = join(&harness, &invite).await;
    let (second, _) = join(&harness, &invite).await;
    let mut limited = None;
    for _ in 0..60 {
        let (status, body) = guest_command(&harness, &first, json!({ "type": "pause" })).await;
        if status == StatusCode::TOO_MANY_REQUESTS {
            limited = Some(body);
            break;
        }
    }
    assert_eq!(limited.expect("limited")["code"], "rate_limited");
    assert_eq!(
        guest_command(&harness, &second, json!({ "type": "pause" }))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        owner_command(&harness, json!({ "type": "pause" })).await.0,
        StatusCode::OK
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_stream_reaches_only_library_tracks() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let (cookie, _) = join(&harness, &token(&room)).await;
    for track in ["not-a-track", "..%2F..%2Fetc%2Fpasswd"] {
        let uri = format!("/api/v1/tracks/{track}/stream");
        let request = on_guest(Method::GET, &uri, Some(&cookie), None);
        assert_eq!(
            status_of(&harness.guest, request).await,
            StatusCode::NOT_FOUND
        );
    }
    let radio = on_guest(
        Method::GET,
        "/api/v1/radio/stations/x/stream",
        Some(&cookie),
        None,
    );
    assert_eq!(
        status_of(&harness.guest, radio).await,
        StatusCode::NOT_FOUND
    );
    let external = on_guest(
        Method::GET,
        "/api/v1/external/tracks/x/stream",
        Some(&cookie),
        None,
    );
    assert_eq!(
        status_of(&harness.guest, external).await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_clock_answers_both_listeners() {
    let harness = harness().await;
    let (status, _, clock) = send(
        &harness.owner,
        on_owner(Method::GET, "/api/v1/listen-together/room/clock", None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(clock["serverTimeMs"].as_i64().unwrap() > 1_700_000_000_000);
    let room = open_room(&harness).await;
    let (cookie, _) = join(&harness, &token(&room)).await;
    let (status, _, clock) = send(
        &harness.guest,
        on_guest(
            Method::GET,
            "/api/v1/listen-together/room/clock",
            Some(&cookie),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(clock["serverTimeMs"].is_i64());
    assert_eq!(harness.state.config.listen_together_max_guests, 8);
}

#[tokio::test(flavor = "multi_thread")]
async fn plain_http_on_loopback_uses_the_plain_cookie() {
    let directory = tempfile::tempdir().unwrap();
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let pool = db::open_and_migrate(
        directory.path().join("app.db").to_str().unwrap(),
        &migrations,
    )
    .await
    .expect("database");
    let managed = directory.path().join("managed");
    let config = Config::from_lookup(|name| match name {
        "MANAGED_STORAGE_PATH" => Some(managed.to_string_lossy().into_owned()),
        "GUEST_ADDR" => Some("127.0.0.1:18091".to_owned()),
        "PUBLIC_URL" => Some("http://localhost:18091".to_owned()),
        _ => None,
    })
    .expect("config");
    let state = AppState::new(config, dependencies::probe_system().await, pool);
    let owner = app::router(state.clone());
    let guest = app::guest_router(state).expect("configured");
    let (_, _, room) = send(&owner, on_owner(Method::POST, ROOM, None)).await;
    let url = room["inviteUrl"].as_str().unwrap();
    assert!(url.starts_with("http://localhost:18091/join#"), "{url}");
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/listen-together/join")
        .header(header::HOST, "localhost:18091")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "token": url.rsplit_once('#').unwrap().1 }).to_string(),
        ))
        .unwrap();
    let (status, headers, _) = send(&guest, request).await;
    assert_eq!(status, StatusCode::OK);
    let cookie = headers[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with("harmony_guest="), "{cookie}");
    assert!(!cookie.contains("Secure"), "{cookie}");
}

/// Reads `uri` as a Guest until the limit answers, and returns how many passed.
async fn passes_until_limited(harness: &Harness, cookie: &str, uri: &str, at_most: usize) -> usize {
    for sent in 0..at_most {
        let request = on_guest(Method::GET, uri, Some(cookie), None);
        let (status, _, body) = send(&harness.guest, request).await;
        if status == StatusCode::TOO_MANY_REQUESTS {
            assert_eq!(body["code"], "rate_limited", "{uri}");
            return sent;
        }
    }
    panic!("{uri} is still open after {at_most} reads");
}

async fn guest_read(harness: &Harness, cookie: &str, uri: &str) -> StatusCode {
    status_of(
        &harness.guest,
        on_guest(Method::GET, uri, Some(cookie), None),
    )
    .await
}

const LOOKUPS: &[&str] = &[
    "/api/v1/library/artists/ar/biography",
    "/api/v1/library/artists/ar/popular-tracks",
    "/api/v1/library/albums/al/online-genres",
    "/api/v1/library/tracks/play-counts?ids=x",
];
const ME: &str = "/api/v1/me";
const CLOCK: &str = "/api/v1/listen-together/room/clock";
const ARTIST_PHOTO: &str = "/api/v1/library/artists/ar/photo";

fn waveform_uri() -> String {
    format!("/api/v1/library/tracks/{TRACK_A}/waveform")
}

#[tokio::test(flavor = "multi_thread")]
async fn library_reads_are_limited_per_guest_and_never_on_the_owners_listener() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let invite = token(&room);
    let (first, _) = join(&harness, &invite).await;
    let (second, _) = join(&harness, &invite).await;

    let passed = passes_until_limited(&harness, &first, ME, 20_000).await;
    assert!(passed >= 1000, "only {passed} reads passed");

    assert_eq!(guest_read(&harness, &second, ME).await, StatusCode::OK);
    for _ in 0..1100 {
        assert_eq!(
            status_of(&harness.owner, on_owner(Method::GET, ME, None)).await,
            StatusCode::OK
        );
    }
    // Browsing cannot cost a Guest their place in the Room's sync.
    assert_eq!(guest_read(&harness, &first, CLOCK).await, StatusCode::OK);
    assert_eq!(guest_room(&harness, &first).await.0, StatusCode::OK);
    assert_eq!(
        guest_command(&harness, &first, json!({ "type": "pause" }))
            .await
            .0,
        StatusCode::OK
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(guest_read(&harness, &first, ME).await, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn work_and_outside_lookups_share_a_stricter_limit() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let invite = token(&room);
    let (first, _) = join(&harness, &invite).await;
    let (second, _) = join(&harness, &invite).await;

    let waveform = waveform_uri();
    let passed = passes_until_limited(&harness, &first, &waveform, 200).await;
    assert!((40..100).contains(&passed), "{passed} waveforms passed");

    let lyrics = format!("/api/v1/library/tracks/{TRACK_A}/lyrics");
    for uri in LOOKUPS.iter().copied().chain([lyrics.as_str()]) {
        let (status, _, body) = send(
            &harness.guest,
            on_guest(Method::GET, uri, Some(&first), None),
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{uri}");
        assert_eq!(body["code"], "rate_limited", "{uri}");
    }
    // A Guest out of lookups still browses, and the others are untouched.
    assert_eq!(guest_read(&harness, &first, ME).await, StatusCode::OK);
    assert_ne!(
        guest_read(&harness, &first, ARTIST_PHOTO).await,
        StatusCode::TOO_MANY_REQUESTS
    );
    for uri in LOOKUPS {
        assert_ne!(
            guest_read(&harness, &second, uri).await,
            StatusCode::TOO_MANY_REQUESTS,
            "{uri}"
        );
    }
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_ne!(
        guest_read(&harness, &first, &waveform).await,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_of_artist_photos_fits_and_leaves_lookups_alone() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let (cookie, _) = join(&harness, &token(&room)).await;
    // The Artists page asks for 100 photos at once.
    for _ in 0..100 {
        assert_ne!(
            guest_read(&harness, &cookie, ARTIST_PHOTO).await,
            StatusCode::TOO_MANY_REQUESTS
        );
    }
    let passed = passes_until_limited(&harness, &cookie, ARTIST_PHOTO, 200).await;
    assert!(passed >= 20, "only {passed} more photos passed");
    assert_ne!(
        guest_read(&harness, &cookie, &waveform_uri()).await,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn waveform_polling_and_room_reads_stay_within_their_limits() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let (cookie, _) = join(&harness, &token(&room)).await;
    // A Guest spends their whole library budget; clock samples still pass.
    passes_until_limited(&harness, &cookie, ME, 20_000).await;
    for _ in 0..10 {
        assert_eq!(guest_read(&harness, &cookie, CLOCK).await, StatusCode::OK);
    }
    // useTrackWaveform stops at 30 polls.
    let waveform = waveform_uri();
    for _ in 0..31 {
        assert_ne!(
            guest_read(&harness, &cookie, &waveform).await,
            StatusCode::TOO_MANY_REQUESTS
        );
    }
}

/// Tracks with files of their own; `file_path` is unique.
async fn playable_tracks(harness: &Harness, count: usize) -> Vec<String> {
    let mut tracks = Vec::new();
    for number in 0..count {
        let id = format!("0190c5a0-0000-7000-8000-0000000001{number:02}");
        let file = harness._directory.path().join(format!("{number}.flac"));
        std::fs::write(&file, vec![0_u8; 1024 * 1024]).unwrap();
        sqlx::query(
            "INSERT INTO tracks (id, album_id, title, title_sort, artist_name, format, file_path, duration_ms, lyrics)
             VALUES (?, 'al', 'Song', 'song', 'Skillet', 'flac', ?, 1000, '')",
        )
        .bind(&id)
        .bind(file.to_string_lossy().into_owned())
        .execute(&harness.state.db)
        .await
        .unwrap();
        tracks.push(id);
    }
    tracks
}

async fn open_stream(
    harness: &Harness,
    cookie: &str,
    track: &str,
    range: &str,
) -> axum::http::Response<Body> {
    let uri = format!("/api/v1/tracks/{track}/stream");
    let mut request = on_guest(Method::GET, &uri, Some(cookie), None);
    request
        .headers_mut()
        .insert(header::RANGE, range.parse().unwrap());
    harness.guest.clone().oneshot(request).await.unwrap()
}

/// Reads to the end, and returns how many bytes came.
async fn drain(response: axum::http::Response<Body>) -> usize {
    let mut body = response.into_body().into_data_stream();
    let mut received = 0;
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(5), body.next())
        .await
        .expect("the body ends")
    {
        received += chunk.expect("audio").len();
    }
    received
}

#[tokio::test(flavor = "multi_thread")]
async fn audio_streams_are_limited_per_guest_and_a_closed_one_frees_its_place() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let invite = token(&room);
    let (cookie, _) = join(&harness, &invite).await;
    let (other, _) = join(&harness, &invite).await;
    let tracks = playable_tracks(&harness, 5).await;

    let mut open = Vec::new();
    for track in &tracks[..4] {
        let response = open_stream(&harness, &cookie, track, "bytes=0-").await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        open.push(response);
    }
    let refused = open_stream(&harness, &cookie, &tracks[4], "bytes=0-").await;
    assert_eq!(refused.status(), StatusCode::TOO_MANY_REQUESTS);
    let body = refused.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["code"], "rate_limited");

    // HEAD sends no audio, another Guest has their own places, and the Owner
    // has no limit.
    let head = on_guest(
        Method::HEAD,
        &format!("/api/v1/tracks/{}/stream", tracks[4]),
        Some(&cookie),
        None,
    );
    assert_eq!(status_of(&harness.guest, head).await, StatusCode::OK);
    let response = open_stream(&harness, &other, &tracks[4], "bytes=0-").await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    let mut owner_streams = Vec::new();
    for _ in 0..8 {
        let uri = format!("/api/v1/tracks/{}/stream", tracks[4]);
        let response = harness
            .owner
            .clone()
            .oneshot(on_owner(Method::GET, &uri, None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        owner_streams.push(response);
    }

    // A connection that goes away, or a Track sent to its end, frees a place.
    drop(open.pop());
    let replacement = open_stream(&harness, &cookie, &tracks[4], "bytes=0-").await;
    assert_eq!(replacement.status(), StatusCode::PARTIAL_CONTENT);
    let refused = open_stream(&harness, &cookie, &tracks[3], "bytes=0-").await;
    assert_eq!(refused.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(drain(open.pop().unwrap()).await, 1024 * 1024);
    let reopened = open_stream(&harness, &cookie, &tracks[3], "bytes=0-").await;
    assert_eq!(reopened.status(), StatusCode::PARTIAL_CONTENT);
}

/// A seek is a new range request, and the previous one may stay open for a
/// while in the player or the proxy. It is never refused.
#[tokio::test(flavor = "multi_thread")]
async fn seeking_is_never_refused_and_replaces_the_oldest_stream_of_that_track() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let (cookie, _) = join(&harness, &token(&room)).await;
    let others = playable_tracks(&harness, 1).await;
    let hold = open_stream(&harness, &cookie, &others[0], "bytes=0-").await;

    let mut seeks = Vec::new();
    for position in 0..40 {
        let range = format!("bytes={}-", position * 1024);
        let response = open_stream(&harness, &cookie, LARGE_TRACK, &range).await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT, "{position}");
        seeks.push(response);
    }
    let latest = seeks.pop().unwrap();
    let superseded = seeks.remove(0);
    assert!(drain(superseded).await < LARGE_TRACK_BYTES);
    assert_eq!(drain(latest).await, LARGE_TRACK_BYTES - 39 * 1024);
    // The other Track's stream was not the one replaced.
    assert_eq!(drain(hold).await, 1024 * 1024);
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_a_guest_still_ends_a_counted_stream() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let (cookie, _) = join(&harness, &token(&room)).await;
    let response = open_stream(&harness, &cookie, LARGE_TRACK, "bytes=0-").await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        status_of(&harness.owner, on_owner(Method::DELETE, ROOM, None)).await,
        StatusCode::NO_CONTENT
    );
    assert!(drain(response).await < LARGE_TRACK_BYTES);
}

#[tokio::test(flavor = "multi_thread")]
async fn joins_share_one_limit_whoever_sends_them() {
    let harness = harness().await;
    let room = open_room(&harness).await;
    let invite = token(&room);
    let mut passed = 0;
    let (status, body) = loop {
        let (status, _, body) = try_join(&harness.guest, "wrong").await;
        if status != StatusCode::NOT_FOUND {
            break (status, body);
        }
        passed += 1;
        assert!(passed <= 20, "still open after {passed} joins");
    };
    assert_eq!(passed, 20);
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], "rate_limited");
    // The limit is on the route, so even the right token waits.
    let (status, headers, body) = try_join(&harness.guest, &invite).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], "rate_limited");
    assert!(!headers.contains_key(header::SET_COOKIE));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_join_limit_outlives_a_room() {
    let harness = harness().await;
    // With no Room open, attempts still count.
    for _ in 0..20 {
        assert_eq!(
            try_join(&harness.guest, "anything").await.0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        try_join(&harness.guest, "anything").await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    let room = open_room(&harness).await;
    assert_eq!(
        try_join(&harness.guest, &token(&room)).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_room_lasts_as_long_as_the_operator_set() {
    let directory = tempfile::tempdir().unwrap();
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let pool = db::open_and_migrate(
        directory.path().join("app.db").to_str().unwrap(),
        &migrations,
    )
    .await
    .expect("database");
    let managed = directory.path().join("managed");
    let config = Config::from_lookup(|name| match name {
        "MANAGED_STORAGE_PATH" => Some(managed.to_string_lossy().into_owned()),
        "GUEST_ADDR" => Some("127.0.0.1:18091".to_owned()),
        "PUBLIC_URL" => Some(PUBLIC_ORIGIN.to_owned()),
        "LISTEN_TOGETHER_ROOM_HOURS" => Some("2".to_owned()),
        _ => None,
    })
    .expect("config");
    assert_eq!(
        Timings::for_config(&config).room_lifetime,
        Duration::from_secs(2 * 60 * 60)
    );
    assert_eq!(
        Timings::default().room_lifetime,
        Duration::from_secs(24 * 60 * 60)
    );
    let state = AppState::new(config, dependencies::probe_system().await, pool);
    let earliest = SystemTime::now() + Duration::from_secs(2 * 60 * 60 - 60);
    let latest = SystemTime::now() + Duration::from_secs(2 * 60 * 60 + 60);
    let (_, _, room) = send(&app::router(state), on_owner(Method::POST, ROOM, None)).await;
    // RFC 3339 times in UTC order as text at this distance.
    let expires_at = room["expiresAt"].as_str().unwrap();
    assert!(
        expires_at > gojson::rfc3339_nano(earliest).as_str(),
        "{expires_at}"
    );
    assert!(
        expires_at < gojson::rfc3339_nano(latest).as_str(),
        "{expires_at}"
    );
}
