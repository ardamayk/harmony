//! The Owner's listener behind Cloudflare Access (ADR 0038): which requests
//! pass with `OWNER_PUBLIC_URL` as their Host. The key signs only these tests.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use earthly_music_server::app::{self, AppState};
use earthly_music_server::config::Config;
use earthly_music_server::owner_access::OwnerAccess;
use earthly_music_server::{db, dependencies};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

const HOST: &str = "harmony.example.com";
const ORIGIN: &str = "https://harmony.example.com";
const TEAM: &str = "zenima.cloudflareaccess.com";
const AUDIENCE: &str = "app-audience";
const KEY_ID: &str = "test-key";
const PRIVATE_KEY: &[u8] = include_bytes!("../testdata/owner-access-test-key.pem");
const MODULUS: &str = "u3tD2_VCGWbAf-ivHhPJno6jj13cVcAOAw-zQByNQwAdTCDa7nEV0An4xDSNnyqICTYOldfdM1rspv5R7uUaE--_aozU_x8VwkPdXKKIg0voV4Ld65bxLiXPIw0jrVDTVh7-9d-RCoutni-GVFbeNBIOf6-ItB0w3k_JWSyVIXsiq6q9gkEOeD6wmRJtpuGlNozsBW8Dw4RsAWSetA3OFT6lUw8RigM3FEPLj8fNxsSMJiyl8mac7x023sb7tD2hjWBpApR0LH0EeycH4In9WnYWsxU-wyL3sH_84XgIWaOz94kHkAw5vbCfbxohfwvxFLAzQQvUpCuJY3x_j4DvUw";
const HEALTH: &str = "/api/v1/health";

struct Harness {
    router: Router,
    cert_requests: Arc<AtomicUsize>,
    _directory: tempfile::TempDir,
}

async fn certs_server() -> (String, Arc<AtomicUsize>) {
    let requests = Arc::new(AtomicUsize::new(0));
    let counter = requests.clone();
    let router = Router::new().route(
        "/certs",
        get(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            async {
                axum::Json(json!({ "keys": [
                    { "kid": KEY_ID, "kty": "RSA", "alg": "RS256", "use": "sig", "n": MODULUS, "e": "AQAB" }
                ]}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/certs", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await });
    (url, requests)
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
    let config = Config::from_lookup(|name| match name {
        "MANAGED_STORAGE_PATH" => Some(managed.to_string_lossy().into_owned()),
        "ARTIST_INFO_ENABLED" | "LYRICS_LOOKUP_ENABLED" => Some("false".to_owned()),
        "OWNER_PUBLIC_URL" => Some(ORIGIN.to_owned()),
        "CF_ACCESS_TEAM_DOMAIN" => Some(TEAM.to_owned()),
        "CF_ACCESS_AUD" => Some(AUDIENCE.to_owned()),
        _ => None,
    })
    .expect("config");
    let settings = config.owner_access.clone().expect("on");
    let (certs_url, cert_requests) = certs_server().await;
    let mut state = AppState::new(config, dependencies::probe_system().await, pool);
    state.owner_access = Some(Arc::new(OwnerAccess::with_certs_url(settings, certs_url)));
    Harness {
        router: app::router(state),
        cert_requests,
        _directory: directory,
    }
}

fn token_with(key_id: &str, audience: &str, issuer: &str, expires_in: i64) -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(key_id.to_owned());
    encode(
        &header,
        &json!({
            "aud": [audience],
            "iss": issuer,
            "exp": now.as_secs() as i64 + expires_in,
        }),
        &EncodingKey::from_rsa_pem(PRIVATE_KEY).unwrap(),
    )
    .unwrap()
}

fn valid_token() -> String {
    token_with(KEY_ID, AUDIENCE, &format!("https://{TEAM}"), 300)
}

fn request(host: &str, token: Option<&str>) -> axum::http::request::Builder {
    let builder = Request::get(HEALTH).header(header::HOST, host);
    match token {
        Some(token) => builder.header("cf-access-jwt-assertion", token),
        None => builder,
    }
}

async fn status(harness: &Harness, request: Request<Body>) -> StatusCode {
    harness
        .router
        .clone()
        .oneshot(request)
        .await
        .unwrap()
        .status()
}

async fn status_with(harness: &Harness, token: Option<&str>) -> StatusCode {
    status(harness, request(HOST, token).body(Body::empty()).unwrap()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_token_signed_for_this_application_passes() {
    let harness = harness().await;
    let response = harness
        .router
        .clone()
        .oneshot(
            request(HOST, Some(&valid_token()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    let csp = response.headers()["content-security-policy"]
        .to_str()
        .unwrap();
    assert!(csp.contains("script-src 'self';"), "{csp}");
}

#[tokio::test(flavor = "multi_thread")]
async fn every_other_token_on_the_public_host_is_refused() {
    let harness = harness().await;
    let issuer = format!("https://{TEAM}");
    assert_eq!(status_with(&harness, None).await, StatusCode::FORBIDDEN);
    for token in [
        "not-a-token".to_owned(),
        token_with(KEY_ID, "another-application", &issuer, 300),
        token_with(KEY_ID, AUDIENCE, "https://other.cloudflareaccess.com", 300),
        token_with(KEY_ID, AUDIENCE, &issuer, -300),
        token_with("unknown-key", AUDIENCE, &issuer, 300),
    ] {
        assert_eq!(
            status_with(&harness, Some(&token)).await,
            StatusCode::FORBIDDEN,
            "{token}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_token_signed_with_the_public_key_as_a_secret_is_refused() {
    let harness = harness().await;
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(KEY_ID.to_owned());
    let token = encode(
        &header,
        &json!({ "aud": [AUDIENCE], "iss": format!("https://{TEAM}"), "exp": u32::MAX }),
        &EncodingKey::from_secret(MODULUS.as_bytes()),
    )
    .unwrap();
    assert_eq!(
        status_with(&harness, Some(&token)).await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn loopback_needs_no_token_and_other_hosts_are_refused_even_with_one() {
    let harness = harness().await;
    let local = request("127.0.0.1:8090", None).body(Body::empty()).unwrap();
    assert_eq!(status(&harness, local).await, StatusCode::OK);
    let other = request("other.example.com", Some(&valid_token()))
        .body(Body::empty())
        .unwrap();
    assert_eq!(status(&harness, other).await, StatusCode::FORBIDDEN);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_write_from_the_public_origin_passes_and_one_from_elsewhere_does_not() {
    let harness = harness().await;
    let write = |origin: &str| {
        Request::post("/api/v1/listen-together/room")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, origin)
            .header(header::CONTENT_TYPE, "application/json")
            .header("cf-access-jwt-assertion", valid_token())
            .body(Body::from("{}"))
            .unwrap()
    };
    assert_ne!(status(&harness, write(ORIGIN)).await, StatusCode::FORBIDDEN);
    assert_eq!(
        status(&harness, write("https://evil.example.com")).await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_key_cannot_keep_asking_for_the_certs() {
    let harness = harness().await;
    let token = token_with("unknown-key", AUDIENCE, &format!("https://{TEAM}"), 300);
    for _ in 0..5 {
        status_with(&harness, Some(&token)).await;
    }
    assert_eq!(harness.cert_requests.load(Ordering::SeqCst), 1);
}
