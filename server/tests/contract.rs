//! The assembled router against packages/contracts/openapi.yaml (the Go
//! server's contract tests): every documented operation is mounted, the
//! health response advertises only documented capabilities, and CORS
//! preflight allows every documented request header.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use earthly_music_server::app::{self, AppState};
use earthly_music_server::config::Config;
use earthly_music_server::{db, dependencies};
use http_body_util::BodyExt;
use serde_json::Value;
use std::path::Path;
use tower::ServiceExt;

const ORIGIN: &str = "https://app.example";

fn contract() -> Value {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../packages/contracts/openapi.yaml"),
    )
    .expect("read contract");
    serde_yaml_ng::from_str(&text).expect("parse contract")
}

async fn router(directory: &Path) -> axum::Router {
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let pool = db::open_and_migrate(directory.join("app.db").to_str().unwrap(), &migrations)
        .await
        .expect("database");
    let managed = directory.join("managed");
    let config = Config::from_lookup(|name| match name {
        "MANAGED_STORAGE_PATH" => Some(managed.to_string_lossy().into_owned()),
        "APP_VERSION" => Some("contract-test".to_owned()),
        // Nothing here may reach the real Radio Browser catalog.
        "RADIO_BROWSER_BASE_URL" => Some("http://127.0.0.1:9".to_owned()),
        _ => None,
    })
    .expect("config");
    let mut config = config;
    config.cors_origins = vec![ORIGIN.to_owned()];
    app::router(AppState::new(
        config,
        dependencies::probe_system().await,
        pool,
    ))
}

async fn send(
    router: &axum::Router,
    request: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let response = router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    // Streams such as the Queue event feed never end; a status and the
    // first second of body are enough here.
    let body = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        response.into_body().collect(),
    )
    .await
    .map(|collected| collected.expect("body").to_bytes().to_vec())
    .unwrap_or_default();
    (status, headers, body)
}

const METHODS: [&str; 5] = ["get", "post", "put", "patch", "delete"];

#[tokio::test(flavor = "multi_thread")]
async fn mounts_every_documented_operation() {
    let directory = tempfile::tempdir().unwrap();
    let router = router(directory.path()).await;
    let contract = contract();
    let mut missing = Vec::new();
    let mut checked = 0;
    let parameter = regex::Regex::new(r"\{[^}]+\}").unwrap();
    for (path, item) in contract["paths"].as_object().expect("paths") {
        for method in METHODS {
            let Some(operation) = item.get(method) else {
                continue;
            };
            checked += 1;
            let uri = parameter.replace_all(path, "contract-probe");
            let request = Request::builder()
                .method(method.to_uppercase().as_str())
                .uri(uri.as_ref())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(if method == "get" { "" } else { "{}" }))
                .unwrap();
            let (status, headers, body) = send(&router, request).await;
            let is_spa = headers
                .get(header::CONTENT_TYPE)
                .is_some_and(|value| value.to_str().unwrap_or_default().starts_with("text/html"))
                && path.starts_with("/api/");
            if status == StatusCode::METHOD_NOT_ALLOWED
                || is_spa
                || (status == StatusCode::NOT_FOUND && body.is_empty() && !path.contains("/stream"))
            {
                missing.push(format!(
                    "{} {path} ({})",
                    method.to_uppercase(),
                    operation["operationId"]
                ));
            }
        }
    }
    assert!(checked > 50, "only {checked} operations documented");
    let mut ids = std::collections::HashSet::new();
    for item in contract["paths"].as_object().unwrap().values() {
        for method in METHODS {
            if let Some(id) = item
                .get(method)
                .and_then(|operation| operation["operationId"].as_str())
            {
                assert!(ids.insert(id.to_owned()), "operationId {id} is used twice");
            }
        }
    }
    assert!(
        missing.is_empty(),
        "documented but not mounted: {missing:#?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn advertises_only_documented_capabilities() {
    let directory = tempfile::tempdir().unwrap();
    let router = router(directory.path()).await;
    let request = Request::get("/api/v1/health").body(Body::empty()).unwrap();
    let (status, _, body) = send(&router, request).await;
    assert_eq!(status, StatusCode::OK);
    let health: Value = serde_json::from_slice(&body).unwrap();
    let advertised: Vec<&str> = health["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    let contract = contract();
    let documentation = contract["components"]["schemas"]["HealthResponse"]["properties"]["capabilities"]["description"]
        .as_str()
        .expect("capabilities description");
    for capability in &advertised {
        assert!(
            documentation.contains(capability),
            "capability {capability:?} is not documented"
        );
    }
    for required in [
        "managed-import.v1",
        "managed-import-batches.v1",
        "managed-track-deletion.v1",
        "managed-track-replacement.v1",
        "managed-album-deletion.v1",
    ] {
        assert!(
            advertised.contains(&required),
            "capability {required:?} is missing"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn cors_allows_every_documented_request_header() {
    let directory = tempfile::tempdir().unwrap();
    let router = router(directory.path()).await;
    let contract = contract();
    let mut names = std::collections::BTreeSet::new();
    for item in contract["paths"].as_object().unwrap().values() {
        let operations = METHODS.iter().filter_map(|method| item.get(*method));
        for parameters in std::iter::once(item)
            .chain(operations)
            .filter_map(|value| value["parameters"].as_array())
        {
            for parameter in parameters {
                let parameter = match parameter.get("$ref").and_then(Value::as_str) {
                    Some(reference) => {
                        let name = reference.rsplit('/').next().unwrap();
                        &contract["components"]["parameters"][name]
                    }
                    None => parameter,
                };
                if parameter["in"] == "header" {
                    names.insert(parameter["name"].as_str().unwrap().to_owned());
                }
            }
        }
    }
    assert!(
        !names.is_empty(),
        "the contract documents no request headers"
    );
    for name in names {
        let request = Request::builder()
            .method(Method::OPTIONS)
            .uri("/api/v1/contract")
            .header(header::ORIGIN, ORIGIN)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, &name)
            .body(Body::empty())
            .unwrap();
        let (status, headers, _) = send(&router, request).await;
        let allowed = headers
            .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
            .map(|value| value.to_str().unwrap().to_lowercase())
            .unwrap_or_default();
        assert!(
            status.is_success() && allowed.contains(&name.to_lowercase()),
            "documented header {name} is not allowed by CORS preflight (status {status}, allow {allowed:?})"
        );
    }
}

/// Every `/api/v1` route the server source registers is in the contract.
/// Routes are always registered as `.route("<path>", …)`, so the paths can
/// be read from the source even when a call spans lines.
#[test]
fn documents_every_mounted_api_path() {
    let parameter = regex::Regex::new(r"\{[^}]+\}").unwrap();
    let normalize = |path: &str| parameter.replace_all(path, "{}").into_owned();
    let contract = contract();
    let documented: std::collections::HashSet<String> = contract["paths"]
        .as_object()
        .unwrap()
        .keys()
        .map(|path| normalize(path))
        .collect();
    let route = regex::Regex::new(r#"\.route\(\s*"(/api/v1/[^"]*)""#).unwrap();
    let mut mounted = std::collections::BTreeSet::new();
    let mut stack = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs")
                && !path.to_string_lossy().ends_with("_test.rs")
            {
                let text = std::fs::read_to_string(&path).unwrap();
                mounted.extend(route.captures_iter(&text).map(|found| normalize(&found[1])));
            }
        }
    }
    assert!(
        mounted.len() > 40,
        "found only {} mounted paths",
        mounted.len()
    );
    let undocumented: Vec<_> = mounted
        .iter()
        .filter(|path| !documented.contains(*path))
        .collect();
    assert!(
        undocumented.is_empty(),
        "mounted but not documented: {undocumented:#?}"
    );
}
