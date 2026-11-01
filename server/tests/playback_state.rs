//! The Shared Playback State over the assembled router: a claim makes the
//! device Active, reports from it land, remote commands raise the revision,
//! a stray report answers 409 with the state, and the queue event stream
//! carries `playback-state` frames (ADR 0040).

use axum::Router;
use axum::body::{Body, BodyDataStream};
use axum::http::{Method, Request, StatusCode, header};
use earthly_music_server::app::{self, AppState};
use earthly_music_server::config::Config;
use earthly_music_server::{db, dependencies};
use futures_util::StreamExt;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use tower::ServiceExt;

const STATE: &str = "/api/v1/playback/state";
const COMMANDS: &str = "/api/v1/playback/state/commands";
const EVENTS: &str = "/api/v1/playback/queue/events";
const TRACK: &str = "0190c5a0-0000-7000-8000-00000000000a";

struct Harness {
    owner: Router,
    guest: Router,
    _directory: tempfile::TempDir,
}

async fn harness() -> Harness {
    let directory = tempfile::tempdir().unwrap();
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let pool = db::open_and_migrate(
        directory.path().join("app.db").to_str().unwrap(),
        &migrations,
    )
    .await
    .expect("database");
    let managed = directory.path().join("managed");
    let mut config = Config::from_lookup(|name| match name {
        "MANAGED_STORAGE_PATH" => Some(managed.to_string_lossy().into_owned()),
        "RADIO_BROWSER_BASE_URL" => Some("http://127.0.0.1:9".to_owned()),
        "ARTIST_INFO_ENABLED" | "LYRICS_LOOKUP_ENABLED" => Some("false".to_owned()),
        "APP_VERSION" => Some("playback-state-test".to_owned()),
        "GUEST_ADDR" => Some("127.0.0.1:18091".to_owned()),
        "PUBLIC_URL" => Some("https://music.example.com".to_owned()),
        _ => None,
    })
    .expect("config");
    config.cors_origins = vec!["https://app.example".to_owned()];
    for statement in [
        "INSERT INTO artists (id, name, name_sort) VALUES ('ar', 'A', 'a')".to_owned(),
        "INSERT INTO albums (id, artist_id, title, title_sort) VALUES ('al', 'ar', 'B', 'b')"
            .to_owned(),
        format!(
            "INSERT INTO tracks (id, album_id, title, title_sort, artist_name, format, file_path)
             VALUES ('{TRACK}', 'al', 'One', 'one', 'A', 'flac', '/1.flac')"
        ),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(statement))
            .execute(&pool)
            .await
            .unwrap();
    }
    let state = AppState::new(config, dependencies::probe_system().await, pool);
    Harness {
        owner: app::router(state.clone()),
        guest: app::guest_router(state).expect("configured"),
        _directory: directory,
    }
}

fn request(method: Method, uri: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1:8090");
    match body {
        Some(body) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let body = tokio::time::timeout(Duration::from_secs(5), response.into_body().collect())
        .await
        .expect("body ends")
        .expect("body")
        .to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn command(harness: &Harness, body: Value) -> (StatusCode, Value) {
    send(&harness.owner, request(Method::POST, COMMANDS, Some(body))).await
}

/// A request as a browser on the Guest Listener's public address sends it.
fn guest_request(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "music.example.com")
        .body(Body::empty())
        .unwrap()
}

fn claim(device: &str, playing: bool) -> Value {
    json!({
        "type": "claim",
        "deviceId": device,
        "deviceName": "Desk",
        "queueItemId": "q1",
        "trackId": TRACK,
        "positionMs": 0,
        "playing": playing,
    })
}

/// Reads server-sent events, keeping each raw frame.
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

    /// The next event's raw frame, its name and its data, or `None` when the
    /// stream ended.
    async fn next(&mut self) -> Option<(String, String, Value)> {
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
                let Some(event) = field("event: ") else {
                    continue;
                };
                let data = field("data: ").expect("an event needs a data line");
                return Some((
                    frame,
                    event,
                    serde_json::from_str(&data).expect("JSON data"),
                ));
            }
            let chunk = tokio::time::timeout(Duration::from_secs(5), self.stream.next())
                .await
                .expect("an event in time")?
                .expect("chunk");
            self.buffer.push_str(std::str::from_utf8(&chunk).unwrap());
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_claim_makes_the_device_active_and_reports_follow() {
    let harness = harness().await;
    let (status, snapshot) = command(&harness, claim("d1", true)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(snapshot["revision"], 1);
    assert_eq!(snapshot["activeDeviceId"], "d1");
    assert_eq!(snapshot["activeDeviceName"], "Desk");
    assert_eq!(snapshot["playing"], true);
    assert_eq!(snapshot["queueItemId"], "q1");
    assert_eq!(snapshot["trackId"], TRACK);
    assert_eq!(snapshot["remoteCommandRevision"], 0);
    assert!(snapshot["skipRequest"].is_null());
    assert!(snapshot["serverTimeMs"].is_number());
    assert_eq!(snapshot["positionAtMs"], snapshot["serverTimeMs"]);

    // A heartbeat from the Active Device advances the playhead.
    let (status, snapshot) = command(
        &harness,
        json!({
            "type": "report",
            "deviceId": "d1",
            "basedOnRevision": 1,
            "queueItemId": "q1",
            "trackId": TRACK,
            "positionMs": 1_500,
            "playing": true,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(snapshot["revision"], 2);
    assert_eq!(snapshot["positionMs"], 1_500);
    assert_eq!(snapshot["playing"], true);

    // A remote pause freezes the playhead and raises the command revision.
    let (status, snapshot) = command(&harness, json!({ "type": "pause" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(snapshot["playing"], false);
    assert_eq!(snapshot["remoteCommandRevision"], snapshot["revision"]);

    let revision = snapshot["revision"].as_i64().unwrap();
    let (status, snapshot) =
        command(&harness, json!({ "type": "seek", "positionMs": 9_000 })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(snapshot["positionMs"], 9_000);
    assert_eq!(snapshot["revision"], revision + 1);

    // A skip is relayed for the Active Device to carry out once.
    let (status, snapshot) =
        command(&harness, json!({ "type": "skip", "direction": "next" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        snapshot["skipRequest"],
        json!({ "revision": snapshot["revision"], "direction": "next" })
    );

    // The read answers the same state.
    let (status, read) = send(&harness.owner, request(Method::GET, STATE, None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["revision"], snapshot["revision"]);
    assert_eq!(read["skipRequest"], snapshot["skipRequest"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_report_from_a_device_that_is_not_active_answers_409_with_the_state() {
    let harness = harness().await;
    let (_, claimed) = command(&harness, claim("d1", true)).await;
    let (status, body) = command(
        &harness,
        json!({
            "type": "report",
            "deviceId": "d2",
            "basedOnRevision": 1,
            "queueItemId": "q1",
            "trackId": TRACK,
            "positionMs": 100,
            "playing": true,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "playback_device_not_active");
    assert_eq!(body["code"], "playback_device_not_active");
    // The body carries the state as it stands, so the device can follow it.
    assert_eq!(body["state"]["activeDeviceId"], claimed["activeDeviceId"]);
    assert_eq!(body["state"]["revision"], claimed["revision"]);
    assert_eq!(body["state"]["playing"], claimed["playing"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_report_older_than_the_latest_remote_command_changes_nothing() {
    let harness = harness().await;
    let (_, claimed) = command(&harness, claim("d1", true)).await;
    let (_, paused) = command(&harness, json!({ "type": "pause" })).await;
    assert_eq!(paused["playing"], false);
    // The heartbeat was already flying when the pause landed: accepted, but
    // the paused state stands.
    let (status, snapshot) = command(
        &harness,
        json!({
            "type": "report",
            "deviceId": "d1",
            "basedOnRevision": claimed["revision"],
            "queueItemId": "q1",
            "trackId": TRACK,
            "positionMs": 4_000,
            "playing": true,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Accepted, but the paused state stands, a heartbeat in flight or not.
    assert_eq!(snapshot["revision"], paused["revision"]);
    assert_eq!(snapshot["playing"], paused["playing"]);
    assert_eq!(snapshot["positionMs"], paused["positionMs"]);
    assert_eq!(
        snapshot["remoteCommandRevision"],
        paused["remoteCommandRevision"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_track_outside_the_library_is_stored_as_null_so_its_device_can_go_on_reporting() {
    let harness = harness().await;
    let mut stray = claim("d1", true);
    stray["trackId"] = json!("0190c5a0-0000-7000-8000-00000000ffff");
    let (status, state) = command(&harness, stray).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state["trackId"], Value::Null);

    let (status, state) = command(&harness, claim("d1", true)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = command(
        &harness,
        json!({
            "type": "report",
            "deviceId": "d1",
            "basedOnRevision": state["revision"],
            "queueItemId": "q1",
            "trackId": "0190c5a0-0000-7000-8000-00000000ffff",
            "positionMs": 1000,
            "playing": true,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, read) = send(&harness.owner, request(Method::GET, STATE, None)).await;
    assert_eq!(read["trackId"], Value::Null);
    assert_eq!(read["positionMs"], 1000);
}

#[tokio::test]
async fn commands_are_refused_outside_the_contract() {
    let harness = harness().await;
    let mut claim = claim("d1", true);
    claim["deviceId"] = json!("");
    let (status, _) = command(&harness, claim).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = command(&harness, json!({ "type": "seek", "positionMs": -1 })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = command(&harness, json!({ "type": "explode" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let response = harness
        .owner
        .clone()
        .oneshot(request(Method::POST, COMMANDS, None))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_queue_event_stream_carries_the_state_on_connect_and_each_change() {
    let harness = harness().await;
    let mut events = Events::connect(&harness.owner, request(Method::GET, EVENTS, None)).await;

    // First the Queue revision, with its ID, then the empty state without one.
    let (frame, event, data) = events.next().await.expect("queue frame");
    assert_eq!(event, "queue-invalidated");
    assert!(frame.starts_with("id: "), "{frame}");
    assert!(data["revision"].is_string());
    let (frame, event, data) = events.next().await.expect("playback state frame");
    assert_eq!(event, "playback-state");
    assert!(!frame.starts_with("id: "), "{frame}");
    assert_eq!(data["revision"], 0);

    // A claim reaches the stream as a full snapshot.
    let (_, claimed) = command(&harness, claim("d1", true)).await;
    let (_, event, data) = events.next().await.expect("snapshot after claim");
    assert_eq!(event, "playback-state");
    assert_eq!(data, claimed);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_guest_gate_refuses_both_state_routes() {
    let harness = harness().await;
    for (method, uri) in [(Method::GET, STATE), (Method::POST, COMMANDS)] {
        let label = format!("{method} {uri}");
        let (status, body) = send(&harness.guest, guest_request(method, uri)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{label}");
        assert_eq!(body["code"], "not_found");
    }
}
