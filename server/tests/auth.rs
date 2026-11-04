//! Username and password sign-in (ADR 0042): what passes with sign-in on,
//! and that the loopback listener behaves as before with it off.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use base64::Engine;
use earthly_music_server::app::{self, AppState};
use earthly_music_server::config::{Config, ConfigError};
use earthly_music_server::modules::auth::hash_password;
use earthly_music_server::{db, dependencies};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::LazyLock;
use tower::ServiceExt;

const USERNAME: &str = "owner";
const PASSWORD: &str = "correct horse battery staple";
const PUBLIC_HOST: &str = "music.example.com";
const LOCAL_HOST: &str = "127.0.0.1:8090";
const PEER: &str = "203.0.113.5:40000";
const PROXY: &str = "172.18.0.2:40000";

static PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| hash_password(PASSWORD).unwrap());

struct Harness {
    router: axum::Router,
    state: AppState,
    directory: tempfile::TempDir,
}

fn lookup(pairs: Vec<(&'static str, String)>) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<&str, String> = pairs.into_iter().collect();
    move |key| map.get(key).cloned()
}

async fn harness_in(directory: tempfile::TempDir, extra: &[(&'static str, &str)]) -> Harness {
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let pool = db::open_and_migrate(
        directory.path().join("app.db").to_str().unwrap(),
        &migrations,
    )
    .await
    .expect("database");
    let mut pairs = vec![
        (
            "MANAGED_STORAGE_PATH",
            directory
                .path()
                .join("managed")
                .to_string_lossy()
                .into_owned(),
        ),
        ("ARTIST_INFO_ENABLED", "false".to_owned()),
        ("LYRICS_LOOKUP_ENABLED", "false".to_owned()),
    ];
    pairs.extend(extra.iter().map(|(key, value)| (*key, (*value).to_owned())));
    let config = Config::from_lookup(lookup(pairs)).expect("config");
    let state = AppState::new(config, dependencies::probe_system().await, pool);
    if let Some(auth) = &state.auth {
        auth.start().await.expect("sign-in setup");
    }
    Harness {
        router: app::router(state.clone()),
        state,
        directory,
    }
}

async fn signed_in_harness(extra: &[(&'static str, &str)]) -> Harness {
    let mut settings = vec![
        ("AUTH_USERNAME", USERNAME),
        ("AUTH_PASSWORD_HASH", PASSWORD_HASH.as_str()),
    ];
    settings.extend_from_slice(extra);
    harness_in(tempfile::tempdir().unwrap(), &settings).await
}

struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Answer {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or_default()
    }

    fn cookie(&self) -> String {
        self.headers
            .get(header::SET_COOKIE)
            .map(|value| value.to_str().unwrap().to_owned())
            .unwrap_or_default()
    }

    /// `name=value` from Set-Cookie, ready for a Cookie header.
    fn cookie_pair(&self) -> String {
        self.cookie().split(';').next().unwrap().to_owned()
    }
}

fn get(path: &str, host: &str) -> axum::http::request::Builder {
    Request::get(path)
        .header(header::HOST, host)
        .extension(ConnectInfo(PEER.parse::<SocketAddr>().unwrap()))
}

fn login_request(host: &str, peer: &str, username: &str, password: &str) -> Request<Body> {
    Request::post("/api/v1/auth/login")
        .header(header::HOST, host)
        .header(header::CONTENT_TYPE, "application/json")
        .extension(ConnectInfo(peer.parse::<SocketAddr>().unwrap()))
        .body(Body::from(
            json!({ "username": username, "password": password }).to_string(),
        ))
        .unwrap()
}

fn basic(username: &str, password: &str) -> String {
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
    format!("Basic {encoded}")
}

async fn send(harness: &Harness, request: Request<Body>) -> Answer {
    let response = harness.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    Answer {
        status,
        headers,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

async fn login(harness: &Harness, host: &str, password: &str) -> Answer {
    send(harness, login_request(host, PEER, USERNAME, password)).await
}

#[tokio::test(flavor = "multi_thread")]
async fn with_sign_in_off_the_loopback_listener_works_as_before() {
    let harness = harness_in(tempfile::tempdir().unwrap(), &[]).await;
    let me = send(
        &harness,
        get("/api/v1/me", LOCAL_HOST).body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["username"], "admin");
    let foreign = send(
        &harness,
        get("/api/v1/me", PUBLIC_HOST).body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(foreign.status, StatusCode::FORBIDDEN);
    assert_eq!(foreign.code(), "forbidden_host");
    let health = send(
        &harness,
        get("/api/v1/health", LOCAL_HOST)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(
        !health.body["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("auth.session.v1"))
    );
    let attempt = login(&harness, LOCAL_HOST, PASSWORD).await;
    assert_eq!(attempt.status, StatusCode::NOT_FOUND);
    assert_eq!(attempt.code(), "auth_disabled");
    let logout = Request::post("/api/v1/auth/logout")
        .header(header::HOST, LOCAL_HOST)
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&harness, logout).await.code(), "auth_disabled");
}

#[tokio::test(flavor = "multi_thread")]
async fn with_sign_in_on_the_api_needs_credentials_on_every_host() {
    let harness = signed_in_harness(&[]).await;
    // A loopback Host gets no free pass.
    for host in [LOCAL_HOST, "localhost:8090", PUBLIC_HOST, "harmony:8090"] {
        let me = send(
            &harness,
            get("/api/v1/me", host).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(me.status, StatusCode::UNAUTHORIZED, "{host}");
        assert_eq!(me.code(), "unauthenticated");
        assert!(!me.headers.contains_key(header::WWW_AUTHENTICATE));
        assert_eq!(me.headers["x-frame-options"], "DENY");
        assert_eq!(me.headers[header::CACHE_CONTROL], "no-store");
    }
    let without_host = Request::get("/api/v1/library/tracks")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        send(&harness, without_host).await.status,
        StatusCode::UNAUTHORIZED
    );
    let health = send(
        &harness,
        get("/api/v1/health", PUBLIC_HOST)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(health.status, StatusCode::OK);
    assert!(
        health.body["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("auth.session.v1"))
    );
    // The Web Client's files and the docs are no API, and the API
    // reference is public like them.
    for path in [
        "/",
        "/library/albums",
        "/docs",
        "/api/docs",
        "/api/openapi.yaml",
        "/api/docs/api-reference.js",
    ] {
        let page = send(
            &harness,
            get(path, PUBLIC_HOST).body(Body::empty()).unwrap(),
        )
        .await;
        assert_ne!(page.status, StatusCode::UNAUTHORIZED, "{path}");
        assert!(page.headers.contains_key("content-security-policy"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_cookie_signs_in_until_logout() {
    let harness = signed_in_harness(&[]).await;
    let signed_in = login(&harness, PUBLIC_HOST, PASSWORD).await;
    assert_eq!(signed_in.status, StatusCode::NO_CONTENT);
    let cookie = signed_in.cookie();
    for attribute in [
        "HttpOnly",
        "SameSite=Lax",
        "Path=/",
        "Max-Age=2592000",
        "Secure",
    ] {
        assert!(cookie.contains(attribute), "{attribute} in {cookie}");
    }
    let token = cookie.split(';').next().unwrap();
    assert!(token.starts_with("harmony_session=") && token.len() > 40);
    let me = send(
        &harness,
        get("/api/v1/me", PUBLIC_HOST)
            .header(header::COOKIE, signed_in.cookie_pair())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["username"], USERNAME);
    // A fresh session is not renewed on every request.
    assert!(!me.headers.contains_key(header::SET_COOKIE));

    let logout = Request::post("/api/v1/auth/logout")
        .header(header::HOST, PUBLIC_HOST)
        .header(header::COOKIE, signed_in.cookie_pair())
        .body(Body::empty())
        .unwrap();
    let logout = send(&harness, logout).await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT);
    assert!(logout.cookie().contains("Max-Age=0"), "{}", logout.cookie());
    let after = send(
        &harness,
        get("/api/v1/me", PUBLIC_HOST)
            .header(header::COOKIE, signed_in.cookie_pair())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
    // Without a session, logout still answers 204.
    let again = Request::post("/api/v1/auth/logout")
        .header(header::HOST, PUBLIC_HOST)
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&harness, again).await.status, StatusCode::NO_CONTENT);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_loopback_host_gets_a_cookie_without_secure() {
    let harness = signed_in_harness(&[]).await;
    let signed_in = login(&harness, LOCAL_HOST, PASSWORD).await;
    assert_eq!(signed_in.status, StatusCode::NO_CONTENT);
    assert!(
        !signed_in.cookie().contains("Secure"),
        "{}",
        signed_in.cookie()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_sessions_are_refused_and_a_new_password_ends_sessions() {
    let directory = tempfile::tempdir().unwrap();
    let settings = [
        ("AUTH_USERNAME", USERNAME),
        ("AUTH_PASSWORD_HASH", PASSWORD_HASH.as_str()),
    ];
    let harness = harness_in(directory, &settings).await;
    let me = |cookie: String| {
        get("/api/v1/me", PUBLIC_HOST)
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap()
    };
    let first = login(&harness, PUBLIC_HOST, PASSWORD).await.cookie_pair();
    sqlx::query("UPDATE sessions SET expires_at = 0")
        .execute(&*harness.state.db)
        .await
        .unwrap();
    assert_eq!(
        send(&harness, me(first)).await.status,
        StatusCode::UNAUTHORIZED
    );
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
        .fetch_one(&*harness.state.db)
        .await
        .unwrap();
    assert_eq!(left, 0);

    // A session due for renewal gets a new cookie.
    let second = login(&harness, PUBLIC_HOST, PASSWORD).await.cookie_pair();
    sqlx::query("UPDATE sessions SET last_seen_at = last_seen_at - 2 * 86400")
        .execute(&*harness.state.db)
        .await
        .unwrap();
    let renewed = send(&harness, me(second.clone())).await;
    assert_eq!(renewed.status, StatusCode::OK);
    assert!(renewed.cookie().contains("Max-Age=2592000"));
    assert_eq!(renewed.cookie_pair(), second);

    // Restarting with another password hash ends the user's sessions.
    let other_hash = hash_password("another password").unwrap();
    drop(harness.router);
    let restarted = harness_in(
        harness.directory,
        &[
            ("AUTH_USERNAME", USERNAME),
            ("AUTH_PASSWORD_HASH", other_hash.as_str()),
        ],
    )
    .await;
    assert_eq!(
        send(&restarted, me(second)).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plain_password_keeps_sessions_across_restarts_until_it_changes() {
    let me = |cookie: &str| {
        get("/api/v1/me", PUBLIC_HOST)
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap()
    };
    // Starts from a hash of the same password: switching to AUTH_PASSWORD keeps it.
    let harness = signed_in_harness(&[]).await;
    let cookie = login(&harness, PUBLIC_HOST, PASSWORD).await.cookie_pair();
    let mut directory = harness.directory;
    for _ in 0..2 {
        let restarted = harness_in(
            directory,
            &[("AUTH_USERNAME", USERNAME), ("AUTH_PASSWORD", PASSWORD)],
        )
        .await;
        assert_eq!(send(&restarted, me(&cookie)).await.status, StatusCode::OK);
        directory = restarted.directory;
    }

    let changed = harness_in(
        directory,
        &[
            ("AUTH_USERNAME", USERNAME),
            ("AUTH_PASSWORD", "another password"),
        ],
    )
    .await;
    assert_eq!(
        send(&changed, me(&cookie)).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&changed, PUBLIC_HOST, "another password")
            .await
            .status,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn basic_credentials_work_on_any_request() {
    let harness = signed_in_harness(&[]).await;
    for _ in 0..2 {
        let me = send(
            &harness,
            get("/api/v1/me", PUBLIC_HOST)
                .header(header::AUTHORIZATION, basic(USERNAME, PASSWORD))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(me.status, StatusCode::OK);
        assert_eq!(me.body["username"], USERNAME);
    }
    let wrong = send(
        &harness,
        get("/api/v1/me", PUBLIC_HOST)
            .header(header::AUTHORIZATION, basic(USERNAME, "wrong"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.code(), "invalid_credentials");
    assert!(!wrong.headers.contains_key(header::WWW_AUTHENTICATE));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_password_or_username_is_refused_alike() {
    let harness = signed_in_harness(&[]).await;
    let wrong = login(&harness, PUBLIC_HOST, "wrong").await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.code(), "invalid_credentials");
    assert!(wrong.cookie().is_empty());
    let unknown = send(
        &harness,
        login_request(PUBLIC_HOST, PEER, "nobody", PASSWORD),
    )
    .await;
    assert_eq!(unknown.status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown.code(), "invalid_credentials");
    let malformed = Request::post("/api/v1/auth/login")
        .header(header::HOST, PUBLIC_HOST)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"username":"owner"}"#))
        .unwrap();
    assert_eq!(
        send(&harness, malformed).await.status,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn five_failures_then_attempts_wait_without_checking_the_password() {
    let harness = signed_in_harness(&[]).await;
    for _ in 0..5 {
        assert_eq!(
            login(&harness, PUBLIC_HOST, "wrong").await.status,
            StatusCode::UNAUTHORIZED
        );
    }
    // Even the right password waits.
    let limited = login(&harness, PUBLIC_HOST, PASSWORD).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.code(), "too_many_attempts");
    let retry_after: u64 = limited.headers[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=900).contains(&retry_after), "{retry_after}");
    assert!(limited.cookie().is_empty());
    // Basic counts against the same limits.
    let basic_limited = send(
        &harness,
        get("/api/v1/me", PUBLIC_HOST)
            .header(header::AUTHORIZATION, basic(USERNAME, PASSWORD))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(basic_limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(basic_limited.headers.contains_key(header::RETRY_AFTER));
}

#[tokio::test(flavor = "multi_thread")]
async fn failures_from_one_address_do_not_keep_the_owner_out_elsewhere() {
    let harness = signed_in_harness(&[]).await;
    for _ in 0..8 {
        send(
            &harness,
            login_request(PUBLIC_HOST, "203.0.113.66:4000", USERNAME, "wrong"),
        )
        .await;
    }
    let owner = send(
        &harness,
        login_request(PUBLIC_HOST, "198.51.100.9:4000", USERNAME, PASSWORD),
    )
    .await;
    assert_eq!(owner.status, StatusCode::NO_CONTENT);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_wrong_basic_header_is_not_a_new_attempt() {
    let harness = signed_in_harness(&[]).await;
    // A Desktop Client with a stale password keeps polling.
    for _ in 0..10 {
        let stale = send(
            &harness,
            get("/api/v1/me", PUBLIC_HOST)
                .header(header::AUTHORIZATION, basic(USERNAME, "old password"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(stale.code(), "invalid_credentials");
    }
    // The same address can still sign in.
    assert_eq!(
        login(&harness, PUBLIC_HOST, PASSWORD).await.status,
        StatusCode::NO_CONTENT
    );
}

/// Five failures from one client address, each with its own username so
/// only the address's counter fills.
async fn fail_five_times(harness: &Harness, peer: &str, forwarded: &str) {
    for attempt in 0..5 {
        let mut request = login_request(PUBLIC_HOST, peer, &format!("user{attempt}"), "wrong");
        request
            .headers_mut()
            .insert("x-forwarded-for", forwarded.parse().unwrap());
        assert_eq!(
            send(harness, request).await.status,
            StatusCode::UNAUTHORIZED
        );
    }
}

async fn attempt_from(harness: &Harness, peer: &str, forwarded: &str) -> StatusCode {
    let mut request = login_request(PUBLIC_HOST, peer, "fresh-user", "wrong");
    request
        .headers_mut()
        .insert("x-forwarded-for", forwarded.parse().unwrap());
    send(harness, request).await.status
}

#[tokio::test(flavor = "multi_thread")]
async fn forwarded_addresses_are_ignored_from_an_untrusted_peer() {
    let harness = signed_in_harness(&[("TRUSTED_PROXIES", "172.18.0.0/16")]).await;
    fail_five_times(&harness, PEER, "198.51.100.1").await;
    // Claiming another address does not escape the peer's wait.
    assert_eq!(
        attempt_from(&harness, PEER, "198.51.100.2").await,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn forwarded_addresses_name_the_client_behind_a_trusted_proxy() {
    let harness = signed_in_harness(&[("TRUSTED_PROXIES", "172.18.0.0/16, 10.0.0.1")]).await;
    fail_five_times(&harness, PROXY, "198.51.100.1").await;
    assert_eq!(
        attempt_from(&harness, PROXY, "198.51.100.1").await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // Another client behind the same proxy is not slowed down.
    assert_eq!(
        attempt_from(&harness, PROXY, "198.51.100.2").await,
        StatusCode::UNAUTHORIZED
    );
    // What the client wrote left of the proxy's entry does not count.
    assert_eq!(
        attempt_from(&harness, PROXY, "198.51.100.2, 198.51.100.1").await,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_on_the_request_host_may_write_and_others_may_not() {
    let harness = signed_in_harness(&[]).await;
    let mut same = login_request(PUBLIC_HOST, PEER, USERNAME, PASSWORD);
    same.headers_mut().insert(
        header::ORIGIN,
        format!("https://{PUBLIC_HOST}").parse().unwrap(),
    );
    assert_eq!(send(&harness, same).await.status, StatusCode::NO_CONTENT);
    let mut foreign = login_request(PUBLIC_HOST, PEER, USERNAME, PASSWORD);
    foreign
        .headers_mut()
        .insert(header::ORIGIN, "https://evil.example".parse().unwrap());
    let foreign = send(&harness, foreign).await;
    assert_eq!(foreign.status, StatusCode::FORBIDDEN);
    assert_eq!(foreign.code(), "forbidden_origin");
    // A preflight is answered before the gate, as before.
    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .uri("/api/v1/me")
        .header(header::HOST, PUBLIC_HOST)
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .body(Body::empty())
        .unwrap();
    assert_ne!(
        send(&harness, preflight).await.status,
        StatusCode::UNAUTHORIZED
    );
}

fn config(pairs: &[(&'static str, &str)]) -> Result<Config, ConfigError> {
    Config::from_lookup(lookup(
        pairs
            .iter()
            .map(|(key, value)| (*key, (*value).to_owned()))
            .collect(),
    ))
}

#[test]
fn sign_in_settings_come_together_and_alone() {
    let hash = PASSWORD_HASH.as_str();
    assert!(config(&[]).unwrap().auth.is_none());
    let on = config(&[("AUTH_USERNAME", USERNAME), ("AUTH_PASSWORD_HASH", hash)]).unwrap();
    assert_eq!(on.auth.unwrap().username, USERNAME);
    assert_eq!(
        config(&[("AUTH_USERNAME", USERNAME)]).err(),
        Some(ConfigError::IncompleteAuth(
            "AUTH_PASSWORD or AUTH_PASSWORD_HASH"
        ))
    );
    let plain = config(&[("AUTH_USERNAME", USERNAME), ("AUTH_PASSWORD", PASSWORD)]).unwrap();
    assert_eq!(plain.auth.unwrap().username, USERNAME);
    assert_eq!(
        config(&[
            ("AUTH_USERNAME", USERNAME),
            ("AUTH_PASSWORD", PASSWORD),
            ("AUTH_PASSWORD_HASH", hash),
        ])
        .err(),
        Some(ConfigError::TwoAuthPasswords)
    );
    assert_eq!(
        config(&[("AUTH_PASSWORD_HASH", hash)]).err(),
        Some(ConfigError::IncompleteAuth("AUTH_USERNAME"))
    );
    assert_eq!(
        config(&[
            ("AUTH_USERNAME", USERNAME),
            ("AUTH_PASSWORD_HASH", hash),
            ("OWNER_JWT_ISSUER", "https://proxy.example.com"),
        ])
        .err(),
        Some(ConfigError::AuthWithOwnerAccess("OWNER_JWT_ISSUER"))
    );
    for invalid in [
        "plain-text",
        "$argon2i$v=19$m=16,t=2,p=1$c29tZXNhbHQ$Kq0QsaQ3cR2DFg",
        "$2b$12$abcdefghijklmnopqrstuuJ7bWlsY3XrFcb5o/0HfEx3Rj1sF1Q6",
    ] {
        let error = config(&[("AUTH_USERNAME", USERNAME), ("AUTH_PASSWORD_HASH", invalid)])
            .err()
            .unwrap();
        assert_eq!(error, ConfigError::InvalidAuthPasswordHash);
        assert!(!error.to_string().contains(invalid));
    }
    assert!(matches!(
        config(&[("AUTH_USERNAME", "own:er"), ("AUTH_PASSWORD_HASH", hash)]),
        Err(ConfigError::InvalidAuthUsername(_))
    ));
}
