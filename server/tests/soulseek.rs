//! Soulseek Acquisition through the assembled router (ADR 0036). Nothing
//! here reaches slskd or the Soulseek network.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use earthly_music_server::app::{self, AppState};
use earthly_music_server::config::{self, Config, ConfigError, SoulseekConfig};
use earthly_music_server::db;
use earthly_music_server::dependencies::Report;
use http_body_util::BodyExt;
use serde_json::Value;
use std::path::Path;
use tower::ServiceExt;

const CAPABILITY: &str = "soulseek-acquisition.v1";

/// The router, with Soulseek Acquisition configured when `downloads` names
/// slskd's download directory.
async fn router(directory: &Path, downloads: Option<&Path>) -> axum::Router {
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let pool = db::open_and_migrate(directory.join("app.db").to_str().unwrap(), &migrations)
        .await
        .expect("database");
    let managed = directory.join("managed");
    let mut config = Config::from_lookup(|name| match name {
        "MANAGED_STORAGE_PATH" => Some(managed.to_string_lossy().into_owned()),
        "RADIO_BROWSER_BASE_URL" => Some("http://127.0.0.1:9".to_owned()),
        "ARTIST_INFO_ENABLED" | "LYRICS_LOOKUP_ENABLED" => Some("false".to_owned()),
        _ => None,
    })
    .expect("config");
    config.soulseek = downloads.map(|downloads| SoulseekConfig {
        // Nothing listens on the discard port.
        slskd_url: url::Url::parse("http://127.0.0.1:9").unwrap(),
        api_key: "test-key".to_owned(),
        downloads_path: downloads.to_owned(),
    });
    app::router(AppState::new(config, Report::default(), pool))
}

async fn send(router: &axum::Router, method: Method, uri: &str) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method.clone()).uri(uri);
    let body = if method == Method::POST {
        request = request.header(header::CONTENT_TYPE, "application/json");
        Body::from("{}")
    } else {
        Body::empty()
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
    let directory = tempfile::tempdir().unwrap();
    let router = router(directory.path(), None).await;
    for (method, uri) in ROUTES {
        let (status, body) = send(&router, method.clone(), uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
        assert_eq!(
            body["code"], "soulseek_acquisition_disabled",
            "{method} {uri}"
        );
    }
    assert!(
        !capabilities(&router)
            .await
            .contains(&Value::from(CAPABILITY))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_configured_feature_is_advertised_and_its_routes_answer() {
    let directory = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let router = router(directory.path(), Some(downloads.path())).await;
    assert!(
        capabilities(&router)
            .await
            .contains(&Value::from(CAPABILITY))
    );
    for (method, uri) in ROUTES {
        let (_, body) = send(&router, method.clone(), uri).await;
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
