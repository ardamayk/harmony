//! Soulseek Acquisition through the assembled router (ADR 0036), with a
//! fake slskd: a small HTTP server on loopback that answers scripted JSON
//! and records every request. Nothing here reaches the Soulseek network.

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Method, Request, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use earthly_music_server::app::{self, AppState};
use earthly_music_server::config::{self, Config, ConfigError, SoulseekConfig};
use earthly_music_server::db;
use earthly_music_server::dependencies::{Dependency, Report};
use earthly_music_server::modules::soulseek;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;

const CAPABILITY: &str = "soulseek-acquisition.v1";
/// A FLAC file that passes the Strict Import Profile.
const FLAC: &[u8] = include_bytes!("../testdata/strict-import.flac");
/// A file named .flac that is not audio.
const NOT_AUDIO: &[u8] = include_bytes!("../testdata/not-audio.flac");
const COVER: &[u8] = include_bytes!("../testdata/cover.png");
const API_KEY: &str = "test-key";

/// One request the fake slskd received.
#[derive(Clone, Debug)]
struct Recorded {
    method: Method,
    /// Path and query, such as `/api/v0/searches/<id>?includeResponses=true`.
    uri: String,
    api_key: Option<String>,
    body: Value,
}

#[derive(Default)]
struct FakeState {
    /// Replies by `"<METHOD> <path>"`; anything else answers 404.
    replies: HashMap<String, (StatusCode, Value)>,
    requests: Vec<Recorded>,
    /// slskd's download list, in the order queued.
    transfers: Vec<FakeTransfer>,
    /// What a queued download does, by remote path, else by user.
    file_behaviors: HashMap<String, Behavior>,
    user_behaviors: HashMap<String, Behavior>,
    /// The bytes a successful download writes, by file name.
    contents: HashMap<String, Vec<u8>>,
    /// slskd's download directory.
    downloads: Option<PathBuf>,
}

/// What the fake does with a queued download.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Behavior {
    /// Completes at once and writes the file.
    Succeed,
    /// Completes with slskd's `Errored` flag.
    Fail,
    /// Stays queued at the remote user.
    Queue,
    /// Stays in progress with some bytes until [`FakeSlskd::finish_held`].
    Hold,
    /// slskd refuses to queue it (the user is offline).
    Offline,
}

#[derive(Clone, Debug)]
struct FakeTransfer {
    id: String,
    username: String,
    filename: String,
    size: u64,
    state: &'static str,
    bytes: u64,
}

impl FakeTransfer {
    fn json(&self) -> Value {
        json!({
            "id": self.id,
            "username": self.username,
            "direction": "Download",
            "filename": self.filename,
            "size": self.size,
            "state": self.state,
            "bytesTransferred": self.bytes,
            "percentComplete": 0
        })
    }
}

impl FakeState {
    fn behavior(&self, username: &str, filename: &str) -> Behavior {
        self.file_behaviors
            .get(filename)
            .or_else(|| self.user_behaviors.get(username))
            .copied()
            .unwrap_or(Behavior::Succeed)
    }

    /// Writes a finished download where slskd puts it by default:
    /// `<downloads>/<remote folder name>/<file name>`.
    fn land(&self, filename: &str) {
        let downloads = self.downloads.as_ref().expect("downloads directory");
        let mut parts = filename.rsplit('\\');
        let name = parts.next().unwrap();
        let folder = downloads.join(parts.next().unwrap());
        std::fs::create_dir_all(&folder).unwrap();
        let content = self
            .contents
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.as_bytes().to_vec());
        std::fs::write(folder.join(name), content).unwrap();
    }
}

/// A fake slskd on 127.0.0.1 with an ephemeral port.
#[derive(Clone)]
struct FakeSlskd {
    url: url::Url,
    state: Arc<Mutex<FakeState>>,
}

impl FakeSlskd {
    async fn start() -> Self {
        let state: Arc<Mutex<FakeState>> = Arc::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake slskd");
        let address = listener.local_addr().expect("address");
        let router = axum::Router::new()
            .fallback(answer)
            .with_state(state.clone());
        tokio::spawn(async move { axum::serve(listener, router).await });
        Self {
            url: url::Url::parse(&format!("http://{address}")).expect("url"),
            state,
        }
    }

    fn reply(&self, method: Method, path: &str, status: StatusCode, body: Value) {
        self.state
            .lock()
            .unwrap()
            .replies
            .insert(format!("{method} {path}"), (status, body));
    }

    fn requests(&self) -> Vec<Recorded> {
        self.state.lock().unwrap().requests.clone()
    }

    fn requests_to(&self, method: Method, prefix: &str) -> Vec<Recorded> {
        self.requests()
            .into_iter()
            .filter(|request| request.method == method && request.uri.starts_with(prefix))
            .collect()
    }

    fn serve_downloads(&self, downloads: &Path) {
        self.state.lock().unwrap().downloads = Some(downloads.to_owned());
    }

    fn user_behaves(&self, username: &str, behavior: Behavior) {
        self.state
            .lock()
            .unwrap()
            .user_behaviors
            .insert(username.to_owned(), behavior);
    }

    fn file_behaves(&self, filename: &str, behavior: Behavior) {
        self.state
            .lock()
            .unwrap()
            .file_behaviors
            .insert(filename.to_owned(), behavior);
    }

    /// The bytes a successful download of `name` writes.
    fn content(&self, name: &str, data: &[u8]) {
        self.state
            .lock()
            .unwrap()
            .contents
            .insert(name.to_owned(), data.to_vec());
    }

    fn transfers(&self) -> Vec<FakeTransfer> {
        self.state.lock().unwrap().transfers.clone()
    }

    /// Completes every held download and writes its file.
    fn finish_held(&self) {
        let mut state = self.state.lock().unwrap();
        let held: Vec<usize> = (0..state.transfers.len())
            .filter(|index| state.transfers[*index].state == "InProgress")
            .collect();
        for index in held {
            let filename = state.transfers[index].filename.clone();
            state.land(&filename);
            let transfer = &mut state.transfers[index];
            transfer.state = "Completed, Succeeded";
            transfer.bytes = transfer.size;
        }
    }

    /// Answers a search for the Album with [`album_responses`].
    fn knows_album_search(&self, search_id: &str) {
        self.reply(
            Method::GET,
            &format!("/api/v0/searches/{search_id}"),
            StatusCode::OK,
            search(search_id, "Daft Punk Discovery", true, album_responses()),
        );
    }

    /// slskd logged in to Soulseek as `earthly-listener`.
    fn logged_in(&self) {
        self.reply(
            Method::GET,
            "/api/v0/application",
            StatusCode::OK,
            application(true, true),
        );
    }
}

async fn answer(
    State(state): State<Arc<Mutex<FakeState>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut state = state.lock().unwrap();
    state.requests.push(Recorded {
        method: method.clone(),
        uri: uri.to_string(),
        api_key: headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    });
    if let Some(rest) = uri.path().strip_prefix("/api/v0/transfers/downloads/") {
        return transfers(&mut state, &method, rest, &body);
    }
    match state.replies.get(&format!("{method} {}", uri.path())) {
        Some((status, body)) => (*status, axum::Json(body.clone())).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// slskd's transfers API: queue, list and remove one user's downloads.
fn transfers(state: &mut FakeState, method: &Method, rest: &str, body: &[u8]) -> Response {
    let mut segments = rest.split('/');
    let username = segments.next().unwrap_or_default().replace("%20", " ");
    let id = segments.next();
    match (method.clone(), id) {
        (Method::POST, None) => {
            let files: Vec<Value> = serde_json::from_slice(body).unwrap_or_default();
            let mut enqueued = Vec::new();
            for file in files {
                let filename = file["filename"].as_str().unwrap().to_owned();
                let size = file["size"].as_u64().unwrap();
                let behavior = state.behavior(&username, &filename);
                if behavior == Behavior::Offline {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        axum::Json(json!(format!("User {username} appears to be offline"))),
                    )
                        .into_response();
                }
                let (transfer_state, bytes) = match behavior {
                    Behavior::Succeed => {
                        state.land(&filename);
                        ("Completed, Succeeded", size)
                    }
                    Behavior::Fail => ("Completed, Errored", 0),
                    Behavior::Queue => ("Queued, Remotely", 0),
                    Behavior::Hold => ("InProgress", size / 2),
                    Behavior::Offline => unreachable!(),
                };
                let transfer = FakeTransfer {
                    id: uuid::Uuid::new_v4().to_string(),
                    username: username.clone(),
                    filename,
                    size,
                    state: transfer_state,
                    bytes,
                };
                enqueued.push(transfer.json());
                state.transfers.push(transfer);
            }
            (
                StatusCode::CREATED,
                axum::Json(json!({"enqueued": enqueued, "failed": []})),
            )
                .into_response()
        }
        (Method::GET, None) => {
            let files: Vec<Value> = state
                .transfers
                .iter()
                .filter(|transfer| transfer.username == username)
                .map(FakeTransfer::json)
                .collect();
            if files.is_empty() {
                return StatusCode::NOT_FOUND.into_response();
            }
            axum::Json(json!({
                "username": username,
                "directories": [{"directory": "any", "fileCount": files.len(), "files": files}]
            }))
            .into_response()
        }
        (Method::DELETE, Some(id)) => {
            let before = state.transfers.len();
            state.transfers.retain(|transfer| transfer.id != id);
            if state.transfers.len() == before {
                StatusCode::NOT_FOUND.into_response()
            } else {
                StatusCode::NO_CONTENT.into_response()
            }
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// slskd's `GET /api/v0/application` state, trimmed to what matters here.
fn application(connected: bool, logged_in: bool) -> Value {
    let state = match (connected, logged_in) {
        (true, true) => "Connected, LoggedIn",
        (true, false) => "Connected",
        _ => "Disconnected",
    };
    json!({
        "version": {"full": "0.24.1.0+abc", "current": "0.24.1", "latest": null, "isCanary": false},
        "pendingReconnect": false,
        "server": {
            "address": "vps.slsknet.org:2271",
            "state": state,
            "isConnected": connected,
            "isLoggedIn": logged_in,
            "isTransitioning": false
        },
        "user": {"username": if logged_in { json!("earthly-listener") } else { Value::Null }},
        "shares": {"ready": true, "directories": 4, "files": 61}
    })
}

/// slskd's `GET /api/v0/shares`: shares grouped by host.
fn share(local_path: &str, excluded: bool, files: u64) -> Value {
    json!({
        "id": "6f1b",
        "alias": "music",
        "isExcluded": excluded,
        "localPath": local_path,
        "raw": local_path,
        "remotePath": "music",
        "directories": 3,
        "files": files
    })
}

struct Harness {
    router: axum::Router,
    state: AppState,
    managed: PathBuf,
    downloads: PathBuf,
    slskd: Option<url::Url>,
    /// The acquisition worker, once started.
    worker: Option<tokio::task::AbortHandle>,
    directory: Arc<tempfile::TempDir>,
    _downloads: Arc<tempfile::TempDir>,
}

/// The router, with Soulseek Acquisition configured against `slskd` when
/// given.
async fn harness(slskd: Option<&url::Url>) -> Harness {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let downloads = Arc::new(tempfile::tempdir().unwrap());
    std::fs::create_dir(directory.path().join("managed")).unwrap();
    app_in(directory, downloads, slskd.cloned()).await
}

/// An app on the database, Managed Storage and slskd download directory
/// of `directory` and `downloads`, as after a restart.
async fn app_in(
    directory: Arc<tempfile::TempDir>,
    downloads: Arc<tempfile::TempDir>,
    slskd: Option<url::Url>,
) -> Harness {
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
        _ => None,
    })
    .expect("config");
    config.soulseek = slskd
        .as_ref()
        .map(|url| soulseek_config(url, downloads.path()));
    let state = AppState::new(config, Report::default(), pool);
    Harness {
        router: app::router(state.clone()),
        state,
        managed,
        downloads: downloads.path().to_owned(),
        slskd,
        worker: None,
        directory,
        _downloads: downloads,
    }
}

/// Polls fast; a source stalls only after `stall`.
fn timing(stall: Duration) -> soulseek::WorkerTiming {
    soulseek::WorkerTiming {
        poll_interval: Duration::from_millis(20),
        stall_timeout: stall,
    }
}

impl Harness {
    /// Starts the acquisition worker, as the server does at startup.
    async fn start_worker(&mut self, stall: Duration) {
        self.state
            .managed_import
            .start()
            .await
            .expect("Managed Import startup");
        self.worker = self.state.soulseek.start(timing(stall));
    }

    /// Stops this app's worker and opens a new app on the same files.
    async fn restart(mut self) -> Harness {
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
        app_in(
            self.directory.clone(),
            self._downloads.clone(),
            self.slskd.clone(),
        )
        .await
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
    }
}

fn soulseek_config(url: &url::Url, downloads: &Path) -> SoulseekConfig {
    SoulseekConfig {
        slskd_url: url.clone(),
        api_key: API_KEY.to_owned(),
        downloads_path: downloads.to_owned(),
    }
}

/// Nothing listens on the discard port.
fn unreachable_slskd() -> url::Url {
    url::Url::parse("http://127.0.0.1:9").unwrap()
}

async fn send(router: &axum::Router, method: Method, uri: &str) -> (StatusCode, Value) {
    let body = (method == Method::POST).then(|| json!({}));
    send_json(router, method, uri, body).await
}

async fn send_json(
    router: &axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(body) => {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    let response = router
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

const ROUTES: [(Method, &str); 7] = [
    (Method::GET, "/api/v1/soulseek/status"),
    (Method::POST, "/api/v1/soulseek/searches"),
    (Method::GET, "/api/v1/soulseek/searches/s1"),
    (Method::POST, "/api/v1/soulseek/acquisitions"),
    (Method::GET, "/api/v1/soulseek/acquisitions"),
    (Method::GET, "/api/v1/soulseek/acquisitions/a1"),
    (Method::DELETE, "/api/v1/soulseek/acquisitions/a1"),
];

async fn capabilities(router: &axum::Router) -> Vec<Value> {
    let (status, health) = send(router, Method::GET, "/api/v1/health").await;
    assert_eq!(status, StatusCode::OK);
    health["capabilities"].as_array().unwrap().clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_disabled_feature_answers_404_and_is_not_advertised() {
    let harness = harness(None).await;
    for (method, uri) in ROUTES {
        let (status, body) = send(&harness.router, method.clone(), uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
        assert_eq!(
            body["code"], "soulseek_acquisition_disabled",
            "{method} {uri}"
        );
    }
    assert!(
        !capabilities(&harness.router)
            .await
            .contains(&Value::from(CAPABILITY))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_configured_feature_is_advertised_and_its_routes_answer() {
    let harness = harness(Some(&unreachable_slskd())).await;
    assert!(
        capabilities(&harness.router)
            .await
            .contains(&Value::from(CAPABILITY))
    );
    for (method, uri) in ROUTES {
        let (_, body) = send(&harness.router, method.clone(), uri).await;
        assert_ne!(
            body["code"], "soulseek_acquisition_disabled",
            "{method} {uri}"
        );
    }
}

#[test]
fn a_partial_configuration_fails_startup() {
    let partial = Config::from_lookup(|name| match name {
        "SLSKD_URL" => Some("http://127.0.0.1:5030".to_owned()),
        "SLSKD_DOWNLOADS_PATH" => Some("/srv/slskd/downloads".to_owned()),
        _ => None,
    });
    assert_eq!(
        partial.err(),
        Some(ConfigError::PartialSoulseekConfig(
            "SLSKD_API_KEY".to_owned()
        ))
    );
}

#[test]
fn a_missing_download_directory_fails_startup() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("downloads");
    let soulseek = Config::from_lookup(|name| match name {
        "SLSKD_URL" => Some("http://127.0.0.1:5030".to_owned()),
        "SLSKD_API_KEY" => Some("test-key".to_owned()),
        "SLSKD_DOWNLOADS_PATH" => Some(missing.to_string_lossy().into_owned()),
        _ => None,
    })
    .expect("config")
    .soulseek
    .expect("on");
    let error = config::validate_soulseek_downloads(&soulseek).expect_err("missing");
    assert!(
        matches!(&error, ConfigError::UnreadableSlskdDownloads(path, _) if *path == missing.to_string_lossy()),
        "{error}"
    );
    std::fs::create_dir(&missing).unwrap();
    assert_eq!(config::validate_soulseek_downloads(&soulseek), Ok(()));
}

#[tokio::test(flavor = "multi_thread")]
async fn status_reports_a_logged_in_slskd_that_shares_managed_storage() {
    let slskd = FakeSlskd::start().await;
    let harness = harness(Some(&slskd.url)).await;
    slskd.logged_in();
    slskd.reply(
        Method::GET,
        "/api/v0/shares",
        StatusCode::OK,
        json!({"local": [share("/srv/other", false, 7), share(&format!("{}/", harness.managed.display()), false, 42)]}),
    );
    let (status, body) = send(&harness.router, Method::GET, "/api/v1/soulseek/status").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({
            "slskd": {"reachable": true, "version": "0.24.1"},
            "soulseek": {"connected": true, "loggedIn": true, "username": "earthly-listener"},
            "sharing": {"state": "shared", "sharedFileCount": 42}
        })
    );
    let requests = slskd.requests();
    assert!(!requests.is_empty());
    for request in requests {
        assert_eq!(
            request.api_key.as_deref(),
            Some(API_KEY),
            "{} {}",
            request.method,
            request.uri
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreachable_slskd_reads_as_unreachable_with_unknown_sharing() {
    let harness = harness(Some(&unreachable_slskd())).await;
    let (status, body) = send(&harness.router, Method::GET, "/api/v1/soulseek/status").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({
            "slskd": {"reachable": false},
            "soulseek": {"connected": false, "loggedIn": false},
            "sharing": {"state": "unknown"}
        })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_api_key_reads_as_unreachable() {
    let slskd = FakeSlskd::start().await;
    let harness = harness(Some(&slskd.url)).await;
    for path in ["/api/v0/application", "/api/v0/shares"] {
        slskd.reply(Method::GET, path, StatusCode::UNAUTHORIZED, Value::Null);
    }
    let (status, body) = send(&harness.router, Method::GET, "/api/v1/soulseek/status").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["slskd"], json!({"reachable": false}));
    assert_eq!(body["sharing"], json!({"state": "unknown"}));
}

#[tokio::test(flavor = "multi_thread")]
async fn storage_outside_slskd_shares_reads_as_not_shared() {
    let slskd = FakeSlskd::start().await;
    let harness = harness(Some(&slskd.url)).await;
    slskd.reply(
        Method::GET,
        "/api/v0/application",
        StatusCode::OK,
        application(true, false),
    );
    let managed = harness.managed.display().to_string();
    let parent = harness.managed.parent().unwrap().display().to_string();
    let cases = [
        json!({}),
        json!({"local": [share("/srv/other", false, 7), share(&format!("{managed}-old"), false, 3)]}),
        // Only a directory inside Managed Storage.
        json!({"local": [share(&format!("{managed}/Artist"), false, 5)]}),
        // A share of a parent directory with Managed Storage excluded.
        json!({"local": [share(&parent, false, 9), share(&managed, true, 0)]}),
    ];
    for shares in cases {
        slskd.reply(
            Method::GET,
            "/api/v0/shares",
            StatusCode::OK,
            shares.clone(),
        );
        let (status, body) = send(&harness.router, Method::GET, "/api/v1/soulseek/status").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["soulseek"],
            json!({"connected": true, "loggedIn": false}),
            "{shares}"
        );
        assert_eq!(body["sharing"], json!({"state": "not_shared"}), "{shares}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_shared_parent_directory_shares_managed_storage() {
    let slskd = FakeSlskd::start().await;
    let harness = harness(Some(&slskd.url)).await;
    slskd.logged_in();
    let parent = harness.managed.parent().unwrap().display().to_string();
    slskd.reply(
        Method::GET,
        "/api/v0/shares",
        StatusCode::OK,
        json!({"local": [share(&parent, false, 90)]}),
    );
    let (_, body) = send(&harness.router, Method::GET, "/api/v1/soulseek/status").await;
    // The parent's file count is not Managed Storage's, so it is left out.
    assert_eq!(body["sharing"], json!({"state": "shared"}));
}

#[tokio::test(flavor = "multi_thread")]
async fn slskd_joins_the_dependency_report_as_an_optional_dependency() {
    let slskd = FakeSlskd::start().await;
    slskd.logged_in();
    let downloads = tempfile::tempdir().unwrap();
    assert_eq!(
        soulseek::probe_dependency(&soulseek_config(&slskd.url, downloads.path())).await,
        Dependency {
            name: "slskd".to_owned(),
            required: false,
            available: true,
            version: "0.24.1".to_owned(),
        }
    );
    assert_eq!(
        slskd.requests()[0].api_key.as_deref(),
        Some(API_KEY),
        "the probe authenticates"
    );
    assert_eq!(
        soulseek::probe_dependency(&soulseek_config(&unreachable_slskd(), downloads.path())).await,
        Dependency {
            name: "slskd".to_owned(),
            required: false,
            available: false,
            version: String::new(),
        }
    );
}

/// One file in an slskd search response.
fn file(filename: &str, size: u64, attributes: Value) -> Value {
    let mut file = json!({
        "filename": filename,
        "size": size,
        "extension": "",
        "code": 1,
        "isLocked": false
    });
    file.as_object_mut()
        .unwrap()
        .extend(attributes.as_object().unwrap().clone());
    file
}

/// One user's slskd search response.
fn response(username: &str, free_slot: bool, queue: u64, speed: u64, files: Vec<Value>) -> Value {
    json!({
        "username": username,
        "hasFreeUploadSlot": free_slot,
        "queueLength": queue,
        "uploadSpeed": speed,
        "fileCount": files.len(),
        "lockedFileCount": 0,
        "lockedFiles": [],
        "token": 7,
        "files": files
    })
}

/// slskd's `GET /api/v0/searches/{id}?includeResponses=true`.
fn search(id: &str, text: &str, complete: bool, responses: Vec<Value>) -> Value {
    json!({
        "id": id,
        "searchText": text,
        "state": if complete { "Completed, TimedOut" } else { "InProgress" },
        "isComplete": complete,
        "responseCount": responses.len(),
        "fileCount": 0,
        "lockedFileCount": 0,
        "startedAt": "2026-10-29T10:00:00.0000000",
        "token": 7,
        "responses": responses
    })
}

/// An Album offered as 24-bit FLAC by alice, erin and (incomplete) bob, as
/// FLAC without reported quality by dave, as MP3 320 by carol, and one bob
/// file as 16-bit FLAC.
fn album_responses() -> Vec<Value> {
    let hi_res = json!({"bitDepth": 24, "sampleRate": 48000, "length": 320});
    vec![
        response(
            "carol",
            false,
            2,
            500,
            vec![
                file(
                    r"Share\Daft Punk - Discovery\01 One More Time.mp3",
                    10,
                    json!({"bitRate": 320, "isVariableBitRate": false}),
                ),
                file(
                    r"Share\Daft Punk - Discovery\02 Aerodynamic.mp3",
                    11,
                    json!({"bitRate": 320, "isVariableBitRate": false}),
                ),
                file(r"Share\Daft Punk - Discovery\notes.txt", 1, json!({})),
            ],
        ),
        response(
            "bob",
            true,
            0,
            9000,
            vec![
                file(
                    r"Music\Discovery\01 One More Time.flac",
                    300,
                    hi_res.clone(),
                ),
                file(
                    r"Music\Discovery\02 Aerodynamic.flac",
                    50,
                    json!({"bitDepth": 16, "sampleRate": 44100}),
                ),
            ],
        ),
        response(
            "erin",
            false,
            1,
            100000,
            vec![
                file(r"E\Discovery\01 One More Time.flac", 100, hi_res.clone()),
                file(r"E\Discovery\02 Aerodynamic.flac", 200, hi_res.clone()),
            ],
        ),
        response(
            "dave",
            true,
            0,
            100,
            vec![
                file(r"D\Discovery\01 One More Time.flac", 120, json!({})),
                file(r"D\Discovery\02 Aerodynamic.flac", 120, json!({})),
            ],
        ),
        response(
            "alice",
            true,
            0,
            1000,
            vec![
                file(
                    r"@@alice\Daft Punk\Discovery [24-48]\01 One More Time.flac",
                    100,
                    hi_res.clone(),
                ),
                file(
                    r"@@alice\Daft Punk\Discovery [24-48]\02 Aerodynamic.flac",
                    200,
                    hi_res.clone(),
                ),
                file(
                    r"@@alice\Daft Punk\Discovery [24-48]\cover.jpg",
                    10,
                    json!({}),
                ),
            ],
        ),
    ]
}

async fn start_search(harness: &Harness, slskd: &FakeSlskd, query: &str, scope: &str) -> String {
    slskd.reply(Method::POST, "/api/v0/searches", StatusCode::OK, json!({}));
    let (status, body) = send_json(
        &harness.router,
        Method::POST,
        "/api/v1/soulseek/searches",
        Some(json!({"query": query, "scope": scope})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["searchId"].as_str().expect("searchId").to_owned()
}

/// A version's sources without their opaque ids, which are checked apart.
fn without_source_ids(versions: &Value) -> Value {
    let mut versions = versions.clone();
    for version in versions.as_array_mut().unwrap() {
        for source in version["sources"].as_array_mut().unwrap() {
            let id = source.as_object_mut().unwrap().remove("sourceId");
            assert!(id.and_then(|id| id.as_str().map(str::len)).unwrap_or(0) > 0);
        }
    }
    versions
}

#[tokio::test(flavor = "multi_thread")]
async fn a_search_starts_in_slskd_with_the_trimmed_query() {
    let slskd = FakeSlskd::start().await;
    let harness = harness(Some(&slskd.url)).await;
    let search_id = start_search(&harness, &slskd, "  Daft Punk Discovery\n", "album").await;
    assert!(uuid::Uuid::parse_str(&search_id).is_ok(), "{search_id}");
    let requests = slskd.requests();
    let started = requests
        .iter()
        .find(|request| request.method == Method::POST)
        .expect("slskd search");
    assert_eq!(started.uri, "/api/v0/searches");
    assert_eq!(started.api_key.as_deref(), Some(API_KEY));
    assert_eq!(
        started.body,
        json!({"id": search_id, "searchText": "Daft Punk Discovery"})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn album_sources_group_into_versions_in_display_order() {
    let slskd = FakeSlskd::start().await;
    let harness = harness(Some(&slskd.url)).await;
    let search_id = start_search(&harness, &slskd, "Daft Punk Discovery", "album").await;
    slskd.reply(
        Method::GET,
        &format!("/api/v0/searches/{search_id}"),
        StatusCode::OK,
        search(&search_id, "Daft Punk Discovery", false, album_responses()),
    );
    let uri = format!("/api/v1/soulseek/searches/{search_id}");
    let (status, body) = send(&harness.router, Method::GET, &uri).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["searchId"], search_id.as_str());
    assert_eq!(body["query"], "Daft Punk Discovery");
    assert_eq!(body["scope"], "album");
    assert_eq!(body["state"], "searching");
    let source = |username: &str,
                  folder: &str,
                  files: u64,
                  bytes: u64,
                  complete: bool,
                  free: bool,
                  queue: u64,
                  speed: u64| {
        json!({
            "username": username,
            "folder": folder,
            "fileCount": files,
            "totalBytes": bytes,
            "complete": complete,
            "hasFreeUploadSlot": free,
            "queueLength": queue,
            "uploadSpeedBytesPerSecond": speed
        })
    };
    assert_eq!(
        without_source_ids(&body["versions"]),
        json!([
            {
                "versionKey": "flac-24-48000",
                "quality": {"format": "flac", "lossless": true, "bitDepth": 24, "sampleRateHz": 48000},
                "fileCount": 2,
                "totalBytes": 300,
                "sourceCount": 3,
                "completeSourceCount": 2,
                "sources": [
                    source("alice", r"@@alice\Daft Punk\Discovery [24-48]", 2, 300, true, true, 0, 1000),
                    source("erin", r"E\Discovery", 2, 300, true, false, 1, 100000),
                    source("bob", r"Music\Discovery", 1, 300, false, true, 0, 9000),
                ]
            },
            {
                "versionKey": "flac-16-44100",
                "quality": {"format": "flac", "lossless": true, "bitDepth": 16, "sampleRateHz": 44100},
                "fileCount": 1,
                "totalBytes": 50,
                "sourceCount": 1,
                "completeSourceCount": 1,
                "sources": [source("bob", r"Music\Discovery", 1, 50, true, true, 0, 9000)]
            },
            {
                "versionKey": "flac-x-x",
                "quality": {"format": "flac", "lossless": true},
                "fileCount": 2,
                "totalBytes": 240,
                "sourceCount": 1,
                "completeSourceCount": 1,
                "sources": [source("dave", r"D\Discovery", 2, 240, true, true, 0, 100)]
            },
            {
                "versionKey": "mp3-320-cbr",
                "quality": {"format": "mp3", "lossless": false, "bitrateKbps": 320, "variableBitrate": false},
                "fileCount": 2,
                "totalBytes": 21,
                "sourceCount": 1,
                "completeSourceCount": 1,
                "sources": [source("carol", r"Share\Daft Punk - Discovery", 2, 21, true, false, 2, 500)]
            }
        ])
    );
    let reads = slskd
        .requests()
        .into_iter()
        .filter(|request| request.method == Method::GET)
        .collect::<Vec<_>>();
    assert_eq!(
        reads[0].uri,
        format!("/api/v0/searches/{search_id}?includeResponses=true")
    );
    // Source ids are stable between reads and distinct within a version.
    let (_, again) = send(&harness.router, Method::GET, &uri).await;
    assert_eq!(again, body);
    let ids: Vec<&Value> = body["versions"][0]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|source| &source["sourceId"])
        .collect();
    assert_eq!(ids.len(), 3);
    assert!(ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2]);
}

#[tokio::test(flavor = "multi_thread")]
async fn track_sources_are_single_files_and_a_finished_search_is_complete() {
    let slskd = FakeSlskd::start().await;
    let harness = harness(Some(&slskd.url)).await;
    let search_id = start_search(&harness, &slskd, "Daft Punk One More Time", "track").await;
    slskd.reply(
        Method::GET,
        &format!("/api/v0/searches/{search_id}"),
        StatusCode::OK,
        search(
            &search_id,
            "Daft Punk One More Time",
            true,
            vec![response(
                "alice",
                true,
                0,
                1000,
                vec![
                    file(
                        r"A\Singles\One More Time.mp3",
                        9,
                        json!({"bitRate": 245, "isVariableBitRate": true}),
                    ),
                    file(
                        r"A\Singles\One More Time (Radio Edit).mp3",
                        7,
                        json!({"bitRate": 251, "isVariableBitRate": true}),
                    ),
                ],
            )],
        ),
    );
    let (status, body) = send(
        &harness.router,
        Method::GET,
        &format!("/api/v1/soulseek/searches/{search_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["scope"], "track");
    assert_eq!(body["state"], "complete");
    let versions = body["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 1, "{body}");
    let version = &versions[0];
    // Each file is its own source; both average to LAME's V0 level.
    assert_eq!(version["versionKey"], "mp3-vbr-245");
    assert_eq!(
        version["quality"],
        json!({"format": "mp3", "lossless": false, "bitrateKbps": 245, "variableBitrate": true})
    );
    assert_eq!(version["fileCount"], 1);
    assert_eq!(version["totalBytes"], 9);
    assert_eq!(version["sourceCount"], 2);
    assert_eq!(version["completeSourceCount"], 2);
    for source in version["sources"].as_array().unwrap() {
        assert_eq!(source["folder"], r"A\Singles");
        assert_eq!(source["fileCount"], 1);
        assert_eq!(source["complete"], true);
    }
    assert_ne!(
        version["sources"][0]["sourceId"],
        version["sources"][1]["sourceId"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bad_search_is_refused_without_calling_slskd() {
    let slskd = FakeSlskd::start().await;
    let harness = harness(Some(&slskd.url)).await;
    for body in [
        json!({"query": "", "scope": "album"}),
        json!({"query": " \n\t", "scope": "album"}),
        json!({"query": "x".repeat(201), "scope": "album"}),
        json!({"query": "Discovery", "scope": "single"}),
        json!({"query": "Discovery"}),
        json!("Discovery"),
    ] {
        let (status, answer) = send_json(
            &harness.router,
            Method::POST,
            "/api/v1/soulseek/searches",
            Some(body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(answer["code"], "bad_request", "{body}");
    }
    assert!(slskd.requests().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_search_answers_404() {
    let slskd = FakeSlskd::start().await;
    let harness = harness(Some(&slskd.url)).await;
    // slskd no longer knows a search the Music Server started.
    let forgotten = start_search(&harness, &slskd, "Discovery", "album").await;
    for search_id in [
        forgotten.as_str(),
        "s1",
        // A search the Music Server did not start.
        "0b5d5d5e-8a3f-4c11-9d3e-5b0c3f2a1e77",
    ] {
        let (status, body) = send(
            &harness.router,
            Method::GET,
            &format!("/api/v1/soulseek/searches/{search_id}"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{search_id}");
        assert_eq!(body["code"], "search_not_found", "{search_id}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_search_without_slskd_or_its_login_answers_503() {
    let slskd = FakeSlskd::start().await;
    let logged_out = harness(Some(&slskd.url)).await;
    slskd.reply(
        Method::POST,
        "/api/v0/searches",
        StatusCode::CONFLICT,
        json!("The server connection must be connected and logged in to perform a search (currently: Disconnected)"),
    );
    let create = Some(json!({"query": "Discovery", "scope": "album"}));
    let (status, body) = send_json(
        &logged_out.router,
        Method::POST,
        "/api/v1/soulseek/searches",
        create.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "soulseek_not_logged_in");

    let unreachable = harness(Some(&unreachable_slskd())).await;
    let (status, body) = send_json(
        &unreachable.router,
        Method::POST,
        "/api/v1/soulseek/searches",
        create,
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "slskd_unreachable");
    assert!(
        body["message"].as_str().unwrap().contains("slskd"),
        "{body}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_version_resolves_to_its_ordered_sources_and_their_files() {
    let slskd = FakeSlskd::start().await;
    let harness = harness(Some(&slskd.url)).await;
    let search_id = start_search(&harness, &slskd, "Daft Punk Discovery", "album").await;
    slskd.reply(
        Method::GET,
        &format!("/api/v0/searches/{search_id}"),
        StatusCode::OK,
        search(&search_id, "Daft Punk Discovery", true, album_responses()),
    );
    let soulseek = &harness.state.soulseek;
    let resolved = soulseek
        .resolve_version(&search_id, "flac-24-48000", None)
        .await
        .expect("version");
    assert_eq!(resolved.query, "Daft Punk Discovery");
    assert_eq!(resolved.scope, soulseek::Scope::Album);
    let users: Vec<&str> = resolved
        .version
        .sources
        .iter()
        .map(|source| source.username.as_str())
        .collect();
    assert_eq!(users, ["alice", "erin", "bob"]);
    let alice = &resolved.version.sources[0];
    assert_eq!(
        alice
            .files
            .iter()
            .map(|file| (file.filename.as_str(), file.size))
            .collect::<Vec<_>>(),
        [
            (
                r"@@alice\Daft Punk\Discovery [24-48]\01 One More Time.flac",
                100
            ),
            (
                r"@@alice\Daft Punk\Discovery [24-48]\02 Aerodynamic.flac",
                200
            ),
        ]
    );
    assert_eq!(
        alice
            .artwork
            .iter()
            .map(|file| file.filename.as_str())
            .collect::<Vec<_>>(),
        [r"@@alice\Daft Punk\Discovery [24-48]\cover.jpg"]
    );

    // A chosen source goes first; the rest keep their order.
    let bob = resolved.version.sources[2].id.clone();
    let chosen = soulseek
        .resolve_version(&search_id, "flac-24-48000", Some(&bob))
        .await
        .expect("version");
    let users: Vec<&str> = chosen
        .version
        .sources
        .iter()
        .map(|source| source.username.as_str())
        .collect();
    assert_eq!(users, ["bob", "alice", "erin"]);

    for (key, source) in [("flac-8-8000", None), ("flac-24-48000", Some("nope"))] {
        assert_eq!(
            soulseek
                .resolve_version(&search_id, key, source)
                .await
                .err(),
            Some(soulseek::SoulseekError::VersionNotFound),
            "{key} {source:?}"
        );
    }
}

const ALICE_ONE: &str = r"@@alice\Daft Punk\Discovery [24-48]\01 One More Time.flac";
const ALICE_TWO: &str = r"@@alice\Daft Punk\Discovery [24-48]\02 Aerodynamic.flac";
const BOB_ONE: &str = r"Music\Discovery\01 One More Time.flac";
/// Long enough that no source stalls unless a test wants it to.
const NO_STALL: Duration = Duration::from_secs(60);

/// A configured app with its worker running and a finished search for the
/// Album; returns the search id.
async fn acquiring(slskd: &FakeSlskd, stall: Duration) -> (Harness, String) {
    let mut harness = harness(Some(&slskd.url)).await;
    slskd.serve_downloads(&harness.downloads);
    harness.start_worker(stall).await;
    let search_id = start_search(&harness, slskd, "Daft Punk Discovery", "album").await;
    slskd.knows_album_search(&search_id);
    (harness, search_id)
}

async fn start_acquisition(harness: &Harness, body: Value) -> (StatusCode, Value) {
    send_json(
        &harness.router,
        Method::POST,
        "/api/v1/soulseek/acquisitions",
        Some(body),
    )
    .await
}

/// Starts acquiring the 24-bit FLAC version; returns its id.
async fn acquire_hi_res(harness: &Harness, search_id: &str) -> String {
    let (status, body) = start_acquisition(
        harness,
        json!({"searchId": search_id, "versionKey": "flac-24-48000"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["acquisitionId"].as_str().unwrap().to_owned()
}

async fn acquisition(harness: &Harness, id: &str) -> Value {
    let (status, body) = send(
        &harness.router,
        Method::GET,
        &format!("/api/v1/soulseek/acquisitions/{id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

/// Reads the acquisition until `done` holds, for at most 15 seconds.
async fn wait_for(harness: &Harness, id: &str, what: &str, done: impl Fn(&Value) -> bool) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let body = acquisition(harness, id).await;
        if done(&body) {
            return body;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "waiting for {what}: {body:#}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn file_states(body: &Value) -> Vec<(String, String)> {
    body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| {
            (
                file["filename"].as_str().unwrap().to_owned(),
                file["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn states(body: &Value) -> Vec<String> {
    file_states(body)
        .into_iter()
        .map(|(_, state)| state)
        .collect()
}

fn files_in(directory: &Path) -> Vec<String> {
    fn walk(path: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), root, out);
            } else {
                out.push(
                    entry
                        .path()
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    let mut files = Vec::new();
    walk(directory, directory, &mut files);
    files.sort();
    files
}

#[tokio::test(flavor = "multi_thread")]
async fn an_acquisition_starts_with_the_first_source_and_downloads_its_files() {
    let slskd = FakeSlskd::start().await;
    slskd.file_behaves(ALICE_TWO, Behavior::Hold);
    let (harness, search_id) = acquiring(&slskd, NO_STALL).await;
    let (status, started) = start_acquisition(
        &harness,
        json!({"searchId": search_id, "versionKey": "flac-24-48000"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let id = started["acquisitionId"].as_str().unwrap();
    assert!(uuid::Uuid::parse_str(id).is_ok());
    assert_eq!(started["query"], "Daft Punk Discovery");
    assert_eq!(started["scope"], "album");
    assert_eq!(started["versionKey"], "flac-24-48000");
    assert_eq!(
        started["quality"],
        json!({"format": "flac", "lossless": true, "bitDepth": 24, "sampleRateHz": 48000})
    );
    assert_eq!(started["state"], "downloading");
    assert_eq!(started["sourceCount"], 3);
    assert_eq!(started["sourcesTried"], 1);
    assert_eq!(started["currentSource"]["username"], "alice");
    assert_eq!(
        started["currentSource"]["folder"],
        r"@@alice\Daft Punk\Discovery [24-48]"
    );
    assert_eq!(
        started["files"],
        json!([
            {"filename": "01 One More Time.flac", "state": "queued", "bytesTransferred": 0, "totalBytes": 100},
            {"filename": "02 Aerodynamic.flac", "state": "queued", "bytesTransferred": 0, "totalBytes": 200}
        ])
    );
    assert!(started.get("batchId").is_none(), "{started}");
    assert!(started["createdAt"].as_str().unwrap().ends_with('Z'));

    let progress = wait_for(&harness, id, "per-file progress", |body| {
        states(body) == ["staged", "downloading"]
    })
    .await;
    assert_eq!(progress["files"][1]["bytesTransferred"], 100);
    let queued = slskd.requests_to(Method::POST, "/api/v0/transfers/downloads/alice");
    assert_eq!(queued.len(), 1, "one request queues the source's files");
    assert_eq!(
        queued[0].body,
        json!([
            {"filename": ALICE_ONE, "size": 100},
            {"filename": ALICE_TWO, "size": 200},
            {"filename": r"@@alice\Daft Punk\Discovery [24-48]\cover.jpg", "size": 10}
        ])
    );
    assert_eq!(queued[0].api_key.as_deref(), Some(API_KEY));

    let (status, list) = send(
        &harness.router,
        Method::GET,
        "/api/v1/soulseek/acquisitions",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["acquisitionId"], id);
}

#[tokio::test(flavor = "multi_thread")]
async fn starting_the_same_version_again_returns_the_active_acquisition() {
    let slskd = FakeSlskd::start().await;
    slskd.user_behaves("alice", Behavior::Hold);
    let (harness, search_id) = acquiring(&slskd, NO_STALL).await;
    let id = acquire_hi_res(&harness, &search_id).await;
    // Another search for the same query reopens it too.
    let again_id = start_search(&harness, &slskd, "Daft Punk Discovery", "album").await;
    slskd.knows_album_search(&again_id);
    let (status, again) = start_acquisition(
        &harness,
        json!({"searchId": again_id, "versionKey": "flac-24-48000"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["acquisitionId"], id.as_str());
    // Another version is another acquisition.
    let (status, other) = start_acquisition(
        &harness,
        json!({"searchId": search_id, "versionKey": "flac-x-x"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{other}");
    assert_ne!(other["acquisitionId"], id.as_str());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bad_acquisition_request_is_refused() {
    let slskd = FakeSlskd::start().await;
    let (harness, search_id) = acquiring(&slskd, NO_STALL).await;
    for (body, status, code) in [
        (
            json!({"versionKey": "flac-24-48000"}),
            StatusCode::BAD_REQUEST,
            "bad_request",
        ),
        (
            json!({"searchId": search_id}),
            StatusCode::BAD_REQUEST,
            "bad_request",
        ),
        (
            json!({"searchId": search_id, "versionKey": "flac-8-8000"}),
            StatusCode::NOT_FOUND,
            "version_not_found",
        ),
        (
            json!({"searchId": search_id, "versionKey": "flac-24-48000", "sourceId": "nope"}),
            StatusCode::NOT_FOUND,
            "version_not_found",
        ),
        (
            json!({"searchId": "0b5d5d5e-8a3f-4c11-9d3e-5b0c3f2a1e77", "versionKey": "flac-24-48000"}),
            StatusCode::NOT_FOUND,
            "search_not_found",
        ),
    ] {
        let (answered, answer) = start_acquisition(&harness, body.clone()).await;
        assert_eq!(answered, status, "{body}");
        assert_eq!(answer["code"], code, "{body}");
    }
    for method in [Method::GET, Method::DELETE] {
        let (status, body) = send(
            &harness.router,
            method.clone(),
            "/api/v1/soulseek/acquisitions/0b5d5d5e-8a3f-4c11-9d3e-5b0c3f2a1e77",
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method}");
        assert_eq!(body["code"], "acquisition_not_found", "{method}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chosen_source_is_tried_first() {
    let slskd = FakeSlskd::start().await;
    slskd.user_behaves("erin", Behavior::Hold);
    let (harness, search_id) = acquiring(&slskd, NO_STALL).await;
    let (_, body) = send(
        &harness.router,
        Method::GET,
        &format!("/api/v1/soulseek/searches/{search_id}"),
    )
    .await;
    let erin = body["versions"][0]["sources"][1]["sourceId"].clone();
    let (status, started) = start_acquisition(
        &harness,
        json!({"searchId": search_id, "versionKey": "flac-24-48000", "sourceId": erin}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    assert_eq!(started["currentSource"]["username"], "erin");
    assert_eq!(started["currentSource"]["sourceId"], erin);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_source_falls_over_to_the_next_source_of_the_version() {
    let slskd = FakeSlskd::start().await;
    slskd.file_behaves(ALICE_TWO, Behavior::Fail);
    slskd.user_behaves("erin", Behavior::Hold);
    let (harness, search_id) = acquiring(&slskd, NO_STALL).await;
    let id = acquire_hi_res(&harness, &search_id).await;
    let body = wait_for(&harness, &id, "erin's download", |body| {
        body["currentSource"]["username"] == "erin" && body["files"][1]["state"] == "downloading"
    })
    .await;
    assert_eq!(body["state"], "downloading");
    assert_eq!(body["sourcesTried"], 2);
    // alice's file that arrived is kept; only the missing one comes from erin.
    assert_eq!(body["files"][0]["state"], "staged");
    let queued = slskd.requests_to(Method::POST, "/api/v0/transfers/downloads/erin");
    assert_eq!(queued.len(), 1);
    assert_eq!(
        queued[0].body,
        json!([{"filename": r"E\Discovery\02 Aerodynamic.flac", "size": 200}])
    );
    // alice's failed transfer is gone from slskd.
    assert!(
        slskd
            .transfers()
            .iter()
            .all(|transfer| transfer.filename != ALICE_TWO),
        "{:?}",
        slskd.transfers()
    );
    slskd.finish_held();
    let done = wait_for(&harness, &id, "review", |body| body["state"] == "review").await;
    assert_eq!(states(&done), ["staged", "staged"], "{done}");
    // Managed Import has both files; slskd's copies and transfers are gone.
    assert!(files_in(&harness.downloads).is_empty());
    assert!(slskd.transfers().is_empty(), "{:?}", slskd.transfers());
}

#[tokio::test(flavor = "multi_thread")]
async fn exhausted_sources_fail_the_acquisition_with_the_files_that_arrived() {
    let slskd = FakeSlskd::start().await;
    slskd.file_behaves(ALICE_ONE, Behavior::Fail);
    slskd.file_behaves(ALICE_TWO, Behavior::Fail);
    slskd.user_behaves("erin", Behavior::Offline);
    // bob has only the first file.
    let (harness, search_id) = acquiring(&slskd, NO_STALL).await;
    let id = acquire_hi_res(&harness, &search_id).await;
    let body = wait_for(&harness, &id, "failure", |body| body["state"] == "failed").await;
    assert_eq!(body["sourcesTried"], 3);
    assert!(body.get("currentSource").is_none(), "{body}");
    assert_eq!(states(&body)[1], "failed", "{body}");
    let reason = body["failureReason"].as_str().unwrap();
    assert!(reason.contains("1 of 2 files"), "{reason}");
    assert_eq!(
        slskd.requests_to(Method::POST, "/api/v0/transfers/downloads/bob")[0].body,
        json!([{"filename": BOB_ONE, "size": 300}])
    );
    // The file that arrived is in the Import Preview for review.
    let batch_uri = format!(
        "/api/v1/import-batches/{}",
        body["batchId"].as_str().expect("batchId")
    );
    let (status, batch) = send(&harness.router, Method::GET, &batch_uri).await;
    assert_eq!(status, StatusCode::OK, "{batch}");
    assert_eq!(
        batch["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["originalFilename"].clone())
            .collect::<Vec<_>>(),
        ["01 One More Time.flac"]
    );
    // Dismissing the failed acquisition discards its batch.
    let (status, _) = send(
        &harness.router,
        Method::DELETE,
        &format!("/api/v1/soulseek/acquisitions/{id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(&harness.router, Method::GET, &batch_uri).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_failed_acquisition_dismisses_it() {
    let slskd = FakeSlskd::start().await;
    for user in ["alice", "erin", "bob"] {
        slskd.user_behaves(user, Behavior::Fail);
    }
    let (harness, search_id) = acquiring(&slskd, NO_STALL).await;
    let id = acquire_hi_res(&harness, &search_id).await;
    wait_for(&harness, &id, "failure", |body| body["state"] == "failed").await;
    let uri = format!("/api/v1/soulseek/acquisitions/{id}");
    let (status, _) = send(&harness.router, Method::DELETE, &uri).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(&harness.router, Method::GET, &uri).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, list) = send(
        &harness.router,
        Method::GET,
        "/api/v1/soulseek/acquisitions",
    )
    .await;
    assert_eq!(list["items"], json!([]));
    let (status, _) = send(&harness.router, Method::DELETE, &uri).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stalled_source_falls_over() {
    let slskd = FakeSlskd::start().await;
    // alice's transfers start and then move no bytes.
    slskd.user_behaves("alice", Behavior::Hold);
    slskd.user_behaves("erin", Behavior::Hold);
    let (harness, search_id) = acquiring(&slskd, Duration::from_millis(400)).await;
    let id = acquire_hi_res(&harness, &search_id).await;
    let body = wait_for(&harness, &id, "erin", |body| {
        body["currentSource"]["username"] == "erin"
    })
    .await;
    assert_eq!(body["sourcesTried"], 2);
    assert!(
        slskd
            .transfers()
            .iter()
            .all(|transfer| transfer.username != "alice"),
        "the stalled source's transfers are removed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn waiting_in_a_remote_queue_is_not_a_stall() {
    let slskd = FakeSlskd::start().await;
    slskd.user_behaves("alice", Behavior::Queue);
    let stall = Duration::from_millis(300);
    let (harness, search_id) = acquiring(&slskd, stall).await;
    let id = acquire_hi_res(&harness, &search_id).await;
    wait_for(&harness, &id, "alice's queued transfers", |_| {
        slskd
            .transfers()
            .iter()
            .filter(|transfer| transfer.username == "alice")
            .count()
            >= 2
    })
    .await;
    // Several stall intervals pass while alice's upload queue holds the files.
    tokio::time::sleep(stall * 5).await;
    let body = acquisition(&harness, &id).await;
    assert_eq!(body["state"], "downloading", "{body}");
    assert_eq!(body["currentSource"]["username"], "alice", "{body}");
    assert_eq!(body["sourcesTried"], 1, "{body}");
    assert!(
        slskd
            .requests_to(Method::POST, "/api/v0/transfers/downloads/erin")
            .is_empty(),
        "no other source is tried"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_removes_transfers_and_downloaded_copies() {
    let slskd = FakeSlskd::start().await;
    slskd.content("01 One More Time.flac", FLAC);
    slskd.file_behaves(ALICE_TWO, Behavior::Hold);
    let (harness, search_id) = acquiring(&slskd, NO_STALL).await;
    let id = acquire_hi_res(&harness, &search_id).await;
    let body = wait_for(&harness, &id, "first file", |body| {
        states(body) == ["staged", "downloading"]
    })
    .await;
    let batch_id = body["batchId"].as_str().expect("batchId").to_owned();
    assert!(
        !files_in(&harness.downloads).is_empty(),
        "the cover waits for the audio"
    );
    assert_eq!(files_in(&harness.managed.join(".staging")).len(), 1);
    let uri = format!("/api/v1/soulseek/acquisitions/{id}");
    let (status, _) = send(&harness.router, Method::DELETE, &uri).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(acquisition(&harness, &id).await["state"], "cancelled");
    assert!(slskd.transfers().is_empty(), "{:?}", slskd.transfers());
    assert!(files_in(&harness.downloads).is_empty());
    assert!(files_in(&harness.managed.join(".staging")).is_empty());
    let (status, _) = send(
        &harness.router,
        Method::GET,
        &format!("/api/v1/import-batches/{batch_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the batch is cancelled");
    // Deleting a cancelled acquisition dismisses it.
    let (status, _) = send(&harness.router, Method::DELETE, &uri).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = send(&harness.router, Method::GET, &uri).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "acquisition_not_found");
    // A cancelled acquisition does not block a new one.
    let (status, _) = start_acquisition(
        &harness,
        json!({"searchId": search_id, "versionKey": "flac-24-48000"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_acquisition_continues_after_a_restart_from_slskd_state() {
    let slskd = FakeSlskd::start().await;
    slskd.user_behaves("alice", Behavior::Hold);
    let (harness, search_id) = acquiring(&slskd, NO_STALL).await;
    let id = acquire_hi_res(&harness, &search_id).await;
    wait_for(&harness, &id, "downloads in progress", |body| {
        states(body) == ["downloading", "downloading"]
    })
    .await;
    let mut restarted = harness.restart().await;
    // slskd finishes while the Music Server is down.
    slskd.finish_held();
    restarted.start_worker(NO_STALL).await;
    let body = wait_for(&restarted, &id, "review", |body| body["state"] == "review").await;
    assert_eq!(states(&body), ["staged", "staged"], "{body}");
    // An acquisition awaiting review survives another restart, staging
    // included: its batch is server-owned.
    let batch_id = body["batchId"].as_str().unwrap().to_owned();
    let mut again = restarted.restart().await;
    again.start_worker(NO_STALL).await;
    let (status, batch) = send(
        &again.router,
        Method::GET,
        &format!("/api/v1/import-batches/{batch_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{batch}");
    assert_eq!(batch["files"].as_array().unwrap().len(), 2);
    assert_eq!(acquisition(&again, &id).await["state"], "review");
    assert_eq!(
        slskd
            .requests_to(Method::POST, "/api/v0/transfers/downloads/alice")
            .len(),
        1,
        "nothing is queued twice"
    );
}

/// frank's folder of the fixture Album: one well-tagged FLAC, one file
/// that is not audio, and two images of which `cover.png` is the cover.
fn strict_album_response(username: &str, folder: &str) -> Value {
    response(
        username,
        true,
        0,
        1000,
        vec![
            file(
                &format!(r"{folder}\03 Inspection Fixture.flac"),
                FLAC.len() as u64,
                json!({}),
            ),
            file(
                &format!(r"{folder}\04 Broken.flac"),
                NOT_AUDIO.len() as u64,
                json!({}),
            ),
            file(&format!(r"{folder}\back.png"), 5, json!({})),
            file(
                &format!(r"{folder}\cover.png"),
                COVER.len() as u64,
                json!({}),
            ),
        ],
    )
}

/// Acquires the fixture Album from frank and waits for its review.
async fn reviewed_strict_album(slskd: &FakeSlskd) -> (Harness, String, Value) {
    slskd.content("03 Inspection Fixture.flac", FLAC);
    slskd.content("04 Broken.flac", NOT_AUDIO);
    slskd.content("cover.png", COVER);
    let mut harness = harness(Some(&slskd.url)).await;
    slskd.serve_downloads(&harness.downloads);
    harness.start_worker(NO_STALL).await;
    let search_id = start_search(&harness, slskd, "Strict Import Tests", "album").await;
    slskd.reply(
        Method::GET,
        &format!("/api/v0/searches/{search_id}"),
        StatusCode::OK,
        search(
            &search_id,
            "Strict Import Tests",
            true,
            vec![strict_album_response("frank", r"F\Strict Import Tests")],
        ),
    );
    let (status, started) = start_acquisition(
        &harness,
        json!({"searchId": search_id, "versionKey": "flac-x-x"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let id = started["acquisitionId"].as_str().unwrap().to_owned();
    let review = wait_for(&harness, &id, "review", |body| body["state"] == "review").await;
    (harness, id, review)
}

async fn get_raw(router: &axum::Router, uri: &str) -> (StatusCode, Bytes) {
    let response = router
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .expect("response");
    let status = response.status();
    (
        status,
        response.into_body().collect().await.unwrap().to_bytes(),
    )
}

async fn library_track_count(harness: &Harness) -> u64 {
    let (status, page) = send(&harness.router, Method::GET, "/api/v1/library/tracks").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    page["items"].as_array().unwrap().len() as u64
}

#[tokio::test(flavor = "multi_thread")]
async fn an_acquisition_reaches_review_and_its_confirmed_files_enter_the_library() {
    let slskd = FakeSlskd::start().await;
    let (harness, id, review) = reviewed_strict_album(&slskd).await;
    assert_eq!(states(&review), ["staged", "staged"], "{review}");
    let batch_id = review["batchId"].as_str().expect("batchId").to_owned();
    for file in review["files"].as_array().unwrap() {
        assert!(file["jobId"].as_str().is_some(), "{file}");
    }
    // Only the audio and the cover image were downloaded.
    let queued = slskd.requests_to(Method::POST, "/api/v0/transfers/downloads/frank");
    let queued: Vec<String> = queued
        .iter()
        .flat_map(|request| request.body.as_array().unwrap().clone())
        .map(|file| file["filename"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        queued,
        [
            r"F\Strict Import Tests\03 Inspection Fixture.flac",
            r"F\Strict Import Tests\04 Broken.flac",
            r"F\Strict Import Tests\cover.png",
        ]
    );
    // The copies are Managed Import's now.
    assert!(files_in(&harness.downloads).is_empty());
    assert!(slskd.transfers().is_empty(), "{:?}", slskd.transfers());

    // The batch reads like any Import Batch: one ready file, one rejected
    // by the Strict Import Profile, and the folder's cover offered.
    let batch_uri = format!("/api/v1/import-batches/{batch_id}");
    let (status, batch) = send(&harness.router, Method::GET, &batch_uri).await;
    assert_eq!(status, StatusCode::OK, "{batch}");
    assert_eq!(batch["status"], "uploading");
    let files = batch["files"].as_array().unwrap();
    assert_eq!(files.len(), 2, "{batch}");
    let ready = files
        .iter()
        .find(|file| file["state"] == "accepted")
        .expect("ready file");
    assert_eq!(ready["originalFilename"], "03 Inspection Fixture.flac");
    let rejected = files
        .iter()
        .find(|file| file["state"] == "rejected")
        .expect("rejected file");
    assert_eq!(rejected["originalFilename"], "04 Broken.flac");
    assert!(
        !rejected["errorCode"]
            .as_str()
            .unwrap_or_default()
            .is_empty()
    );
    let albums = batch["albums"].as_array().unwrap();
    assert_eq!(albums.len(), 1, "{batch}");
    let artwork = albums[0]["artworks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artwork| artwork["id"].as_str().unwrap().starts_with("upload-"))
        .expect("the source folder's cover");
    assert_eq!(artwork["mediaType"], "image/png");
    let (status, image) = get_raw(
        &harness.router,
        &format!(
            "/api/v1/import-batches/{batch_id}/artwork/{}",
            artwork["id"].as_str().unwrap()
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&image[..], COVER);

    // No client heartbeats the batch, and it does not expire.
    let tomorrow = std::time::SystemTime::now() + Duration::from_secs(24 * 60 * 60);
    harness
        .state
        .managed_import
        .service
        .cleanup_inactive(tomorrow)
        .await
        .unwrap();
    let (status, _) = send(&harness.router, Method::GET, &batch_uri).await;
    assert_eq!(status, StatusCode::OK, "a server-owned batch survives");
    assert_eq!(acquisition(&harness, &id).await["state"], "review");

    // Confirming through the existing route completes the acquisition.
    let (status, confirmed) = send_json(
        &harness.router,
        Method::POST,
        &format!("{batch_uri}/confirm"),
        Some(json!({
            "revision": batch["revision"],
            "selectedFileIds": [ready["jobId"]],
            "albumDecisions": [{
                "albumKey": albums[0]["key"],
                "artworkMode": "selected",
                "artworkId": artwork["id"]
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    assert_eq!(acquisition(&harness, &id).await["state"], "completed");
    assert_eq!(library_track_count(&harness).await, 1);
    wait_for(&harness, &id, "the worker to record it", |body| {
        body["state"] == "completed"
    })
    .await;

    // Dismissing a completed acquisition leaves its Tracks alone.
    let uri = format!("/api/v1/soulseek/acquisitions/{id}");
    let (status, _) = send(&harness.router, Method::DELETE, &uri).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(&harness.router, Method::GET, &uri).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(library_track_count(&harness).await, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_acquisition_in_review_cannot_be_cancelled_while_committing() {
    let slskd = FakeSlskd::start().await;
    let (harness, id, review) = reviewed_strict_album(&slskd).await;
    let batch_id = review["batchId"].as_str().unwrap().to_owned();
    let set_status = |status: &'static str| {
        let pool = harness.state.db.clone();
        let batch_id = batch_id.clone();
        async move {
            sqlx::query("UPDATE managed_import_batches SET status = ? WHERE id = ?")
                .bind(status)
                .bind(batch_id)
                .execute(&pool)
                .await
                .unwrap();
        }
    };
    let uri = format!("/api/v1/soulseek/acquisitions/{id}");
    set_status("confirming").await;
    let (status, body) = send(&harness.router, Method::DELETE, &uri).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "acquisition_state_conflict");
    set_status("uploading").await;

    // In review, cancelling removes the batch and its staging.
    assert!(!files_in(&harness.managed.join(".staging")).is_empty());
    let (status, _) = send(&harness.router, Method::DELETE, &uri).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(acquisition(&harness, &id).await["state"], "cancelled");
    let (status, _) = send(
        &harness.router,
        Method::GET,
        &format!("/api/v1/import-batches/{batch_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(files_in(&harness.managed.join(".staging")).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_the_batch_through_its_own_route_cancels_the_acquisition() {
    let slskd = FakeSlskd::start().await;
    let (harness, id, review) = reviewed_strict_album(&slskd).await;
    let batch_id = review["batchId"].as_str().unwrap();
    let (status, _) = send(
        &harness.router,
        Method::DELETE,
        &format!("/api/v1/import-batches/{batch_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(acquisition(&harness, &id).await["state"], "cancelled");
}
