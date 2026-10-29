//! Soulseek Acquisition through the assembled router (ADR 0036), with a
//! fake slskd: a small HTTP server on loopback that answers scripted JSON
//! and records every request. Nothing here reaches the Soulseek network.

use axum::body::Body;
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
use tower::ServiceExt;

const CAPABILITY: &str = "soulseek-acquisition.v1";
const API_KEY: &str = "test-key";

/// One request the fake slskd received.
#[derive(Clone, Debug)]
struct Recorded {
    method: Method,
    /// Path and query, such as `/api/v0/searches/<id>?includeResponses=true`.
    uri: String,
    api_key: Option<String>,
}

#[derive(Default)]
struct FakeState {
    /// Replies by `"<METHOD> <path>"`; anything else answers 404.
    replies: HashMap<String, (StatusCode, Value)>,
    requests: Vec<Recorded>,
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
) -> Response {
    let mut state = state.lock().unwrap();
    state.requests.push(Recorded {
        method: method.clone(),
        uri: uri.to_string(),
        api_key: headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
    });
    match state.replies.get(&format!("{method} {}", uri.path())) {
        Some((status, body)) => (*status, axum::Json(body.clone())).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
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
    managed: PathBuf,
    _directory: tempfile::TempDir,
    _downloads: tempfile::TempDir,
}

/// The router, with Soulseek Acquisition configured against `slskd` when
/// given.
async fn harness(slskd: Option<&url::Url>) -> Harness {
    let directory = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let pool = db::open_and_migrate(
        directory.path().join("app.db").to_str().unwrap(),
        &migrations,
    )
    .await
    .expect("database");
    let managed = directory.path().join("managed");
    std::fs::create_dir(&managed).unwrap();
    let mut config = Config::from_lookup(|name| match name {
        "MANAGED_STORAGE_PATH" => Some(managed.to_string_lossy().into_owned()),
        "RADIO_BROWSER_BASE_URL" => Some("http://127.0.0.1:9".to_owned()),
        "ARTIST_INFO_ENABLED" | "LYRICS_LOOKUP_ENABLED" => Some("false".to_owned()),
        _ => None,
    })
    .expect("config");
    config.soulseek = slskd.map(|url| soulseek_config(url, downloads.path()));
    Harness {
        router: app::router(AppState::new(config, Report::default(), pool)),
        managed,
        _directory: directory,
        _downloads: downloads,
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
