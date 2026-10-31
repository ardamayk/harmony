//! The complete HTTP surface: every module on one router, with the Go
//! server's middleware (server/cmd/server/router.go), and the Guest
//! Listener's far smaller one (ADR 0037).

use crate::config::{Config, ListenTogetherConfig};
use crate::db::Db;
use crate::dependencies::Report;
use crate::owner_access::OwnerAccess;
use crate::respond::ApiError;
use crate::{api, modules, observability, static_assets};
use axum::Router;
use axum::extract::{Request, State};
use axum::http::{
    Extensions, HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Version, header,
};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use std::sync::Arc;
use std::time::Duration;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Below this, compressing saves less than it costs.
const MIN_COMPRESSED_BYTES: u64 = 1024;
const COMPRESSED_TYPES: &[&str] = &[
    "text/html",
    "text/css",
    "text/javascript",
    "application/javascript",
    "application/json",
    "image/svg+xml",
];
/// What a browser may load on the Guest Listener: the Web Client's own files.
/// Styles stay inline
/// because components set them at runtime.
const GUEST_CSP: &str = "default-src 'self'; script-src 'self'; \
    style-src 'self' 'unsafe-inline'; font-src 'self' data:; img-src 'self' data: blob:; \
    media-src 'self' blob:; object-src 'none'; base-uri 'none'; form-action 'self'; \
    frame-ancestors 'none'";
/// The Owner's Web Client on its public address. Its covers and previews
/// can come from outside services, so images and media reach further.
const OWNER_CSP: &str = "default-src 'self'; script-src 'self'; \
    style-src 'self' 'unsafe-inline'; font-src 'self' data:; img-src 'self' data: blob: https:; \
    media-src 'self' blob: https:; object-src 'none'; base-uri 'none'; form-action 'self'; \
    frame-ancestors 'none'";
/// The docs and Scalar run their own scripts, so only framing is limited.
const OWNER_DOCS_CSP: &str = "object-src 'none'; base-uri 'none'; frame-ancestors 'none'";

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub dependencies: Arc<Report>,
    pub db: Db,
    pub waveforms: Arc<modules::library::WaveformService>,
    pub search: Arc<modules::search::SearchService>,
    pub radio: Arc<modules::radio::Radio>,
    pub artist_info: Arc<modules::artist_info::ArtistInfo>,
    pub lyrics: Arc<modules::lyrics::Lyrics>,
    /// Shared with Managed Import, whose writes change queues too.
    pub queue_events: Arc<modules::playback::QueueEventBroker>,
    pub managed_import: Arc<modules::managed_import::ManagedImport>,
    pub external_source: Arc<modules::external_source::ExternalSource>,
    pub listen_together: Arc<modules::listen_together::ListenTogether>,
    pub owner_access: Option<Arc<OwnerAccess>>,
}

impl AppState {
    pub fn new(config: Config, dependencies: Report, db: Db) -> Self {
        let radio = modules::radio::Radio::new(db.clone(), &config.radio_browser_base_url);
        let waveforms = modules::library::WaveformService::new(
            db.clone(),
            dependencies.has(crate::dependencies::FFMPEG),
        );
        let artist_info = modules::artist_info::ArtistInfo::new(
            db.clone(),
            modules::artist_info::Sources::public(),
            config.artist_info_enabled,
            &config.artist_info_language,
            &config.version,
            config.lastfm_api_key.clone(),
        );
        let lyrics = modules::lyrics::Lyrics::new(
            db.clone(),
            "https://lrclib.net",
            config.lyrics_lookup_enabled,
            &config.version,
        );
        let queue_events: Arc<modules::playback::QueueEventBroker> = Arc::default();
        let managed_import =
            modules::managed_import::ManagedImport::new(db.clone(), &config, queue_events.clone());
        let external_source =
            modules::external_source::ExternalSource::new(config.external_source.as_ref());
        let listen_together = modules::listen_together::ListenTogether::new(
            &config,
            modules::library::LibraryService::new(db.clone()),
        );
        let owner_access = config
            .owner_access
            .clone()
            .map(|settings| Arc::new(OwnerAccess::new(settings)));
        Self {
            config: Arc::new(config),
            dependencies: Arc::new(dependencies),
            db: db.clone(),
            waveforms: Arc::new(waveforms),
            search: Arc::new(modules::search::SearchService::new(db.clone())),
            radio: Arc::new(radio),
            artist_info: Arc::new(artist_info),
            lyrics: Arc::new(lyrics),
            queue_events,
            managed_import: Arc::new(managed_import),
            external_source: Arc::new(external_source),
            listen_together: Arc::new(listen_together),
            owner_access,
        }
    }
}

pub fn router(state: AppState) -> Router {
    let cors = cors(&state.config.cors_origins);
    let mut origins = state.config.cors_origins.clone();
    origins.extend(
        state
            .config
            .owner_access
            .iter()
            .map(|access| access.public_origin.clone()),
    );
    let origins = Arc::new(origins);
    Router::new()
        .route("/api/v1/health", get(api::get_health))
        .route("/api/v1/me", get(api::get_me))
        .merge(modules::docs::routes())
        .merge(modules::preferences::routes())
        .merge(modules::client_metrics::routes())
        .merge(modules::library::routes())
        .merge(modules::artist_info::routes())
        .merge(modules::lyrics::routes())
        .merge(modules::playlists::routes())
        .merge(modules::search::routes())
        .merge(modules::radio::routes())
        .merge(modules::playback::routes())
        .merge(modules::managed_import::routes())
        .merge(modules::external_source::routes())
        .merge(modules::listen_together::owner_routes())
        .fallback(static_assets::web)
        .layer(middleware::from_fn(stream_aware_timeout))
        .layer(middleware::from_fn(no_simple_writes))
        .layer(middleware::from_fn(move |request: Request, next: Next| {
            let origins = origins.clone();
            async move { trusted_origin(&origins, request, next).await }
        }))
        .layer(middleware::from_fn_with_state(state.clone(), loopback_host))
        .layer(cors)
        .layer(CatchPanicLayer::new())
        .layer(compression())
        // Outermost, so the request's transaction covers the other layers
        // and a caught panic is reported within it. A no-op without Sentry.
        .layer(middleware::from_fn(observability::http_transaction))
        .with_state(state)
}

/// What the Guest Listener serves, or `None` when Listen Together is not
/// configured. Only modules with a route a Guest may use are mounted, and
/// `guest_gate` still refuses all but the allowlist. There is no CORS layer:
/// the Web Client and the API share `PUBLIC_URL`'s origin.
pub fn guest_router(state: AppState) -> Option<Router> {
    let settings = Arc::new(state.config.listen_together.clone()?);
    let host_settings = settings.clone();
    Some(
        Router::new()
            .route("/api/v1/health", get(api::get_guest_health))
            .route("/api/v1/me", get(api::get_me))
            .merge(modules::preferences::routes())
            .merge(modules::library::routes())
            .merge(modules::artist_info::routes())
            .merge(modules::lyrics::routes())
            .merge(modules::search::routes())
            .merge(modules::playback::routes())
            .merge(modules::listen_together::guest_routes())
            .fallback(static_assets::web)
            .layer(middleware::from_fn(stream_aware_timeout))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                modules::listen_together::guest_gate,
            ))
            .layer(middleware::from_fn(no_simple_writes))
            .layer(middleware::from_fn(move |request: Request, next: Next| {
                let settings = settings.clone();
                async move { public_origin(&settings, request, next).await }
            }))
            .layer(middleware::from_fn(move |request: Request, next: Next| {
                let settings = host_settings.clone();
                async move { public_host(&settings, request, next).await }
            }))
            .layer(CatchPanicLayer::new())
            .layer(compression())
            .layer(middleware::from_fn(guest_headers))
            .layer(middleware::from_fn(observability::http_transaction))
            .with_state(state),
    )
}

/// The Guest Listener faces the internet, so every answer, a refusal included,
/// tells the browser to keep it out of frames and off plain HTTP.
async fn guest_headers(request: Request, next: Next) -> Response {
    browser_headers(next.run(request).await, GUEST_CSP)
}

fn browser_headers(mut response: Response, csp: &'static str) -> Response {
    let headers = response.headers_mut();
    for (name, value) in [
        ("content-security-policy", csp),
        ("cross-origin-opener-policy", "same-origin"),
        ("cross-origin-resource-policy", "same-origin"),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=(), payment=(), usb=()",
        ),
        ("referrer-policy", "no-referrer"),
        (
            "strict-transport-security",
            "max-age=31536000; includeSubDomains",
        ),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("x-robots-tag", "noindex, nofollow"),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    response
}

/// Text worth compressing. Named rather than excluded: a stream the server
/// merely passes on, such as a radio station, must keep arriving as it comes.
fn is_compressible(_: StatusCode, _: Version, headers: &HeaderMap, _: &Extensions) -> bool {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let is_small = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|length| length < MIN_COMPRESSED_BYTES);
    !is_small
        && COMPRESSED_TYPES
            .iter()
            .any(|kind| content_type.starts_with(kind))
}

type CompressWhen = fn(StatusCode, Version, &HeaderMap, &Extensions) -> bool;

fn compression() -> CompressionLayer<CompressWhen> {
    CompressionLayer::new().compress_when(is_compressible as _)
}

fn cors(origins: &[String]) -> CorsLayer {
    let origins: Vec<_> = origins
        .iter()
        .filter_map(|origin| origin.parse().ok())
        .collect();
    let headers = [
        "accept",
        "authorization",
        "content-type",
        "if-match",
        "last-event-id",
        "range",
        "x-import-filename",
        "x-import-filename-encoding",
        "x-permanent-delete",
        "x-track-replacement",
    ]
    .map(HeaderName::from_static);
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(headers)
        // Lets the clients read External Source timings.
        .expose_headers([HeaderName::from_static("server-timing")])
        .allow_credentials(true)
        .max_age(Duration::from_secs(300))
}

/// The server has no authentication and trusts being reachable only on
/// loopback, but any website can make the user's browser call it. This
/// refuses a Host that is not a loopback name, so a page that rebinds its own
/// domain to 127.0.0.1 (DNS rebinding) cannot use the API as its own origin.
/// A request without a Host does not come from a browser. The one other Host
/// is `OWNER_PUBLIC_URL`'s, and it needs a token its proxy signed.
async fn loopback_host(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or_else(|| {
            request
                .uri()
                .authority()
                .map(|authority| authority.to_string())
        });
    match host {
        Some(host) if !is_loopback_host(&host) => match &state.owner_access {
            Some(access) if access.accepts_host(&host) => {
                through_access(access, request, next).await
            }
            _ => ApiError::new(
                StatusCode::FORBIDDEN,
                "forbidden_host",
                "the Music Server only answers requests addressed to a loopback host",
            )
            .into_response(),
        },
        _ => next.run(request).await,
    }
}

async fn through_access(access: &OwnerAccess, request: Request, next: Next) -> Response {
    if !access.allows(request.headers()).await {
        return browser_headers(
            ApiError::new(
                StatusCode::FORBIDDEN,
                "forbidden_access",
                "sign in through the proxy in front of this server",
            )
            .into_response(),
            OWNER_DOCS_CSP,
        );
    }
    let path = request.uri().path();
    let docs = path.starts_with("/docs") || path.starts_with("/api/docs");
    let api = path.starts_with("/api/") && !docs;
    let csp = if docs { OWNER_DOCS_CSP } else { OWNER_CSP };
    let response = browser_headers(next.run(request).await, csp);
    if api { no_store(response) } else { response }
}

/// Keeps an API answer out of caches unless it says otherwise. A Track's audio
/// carries an ETag, so a replay revalidates instead of downloading it again.
fn no_store(mut response: Response) -> Response {
    if !response.headers().contains_key(header::ETAG) {
        response
            .headers_mut()
            .entry(header::CACHE_CONTROL)
            .or_insert(HeaderValue::from_static("no-store"));
    }
    response
}

fn is_loopback_host(host: &str) -> bool {
    let name = match host.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or_default(),
        None => host.rsplit_once(':').map_or(host, |(name, _)| name),
    };
    name.eq_ignore_ascii_case("localhost")
        || name
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

/// The Guest Listener's `loopback_host`: a proxied request comes from
/// loopback too, so only `PUBLIC_URL`'s host is accepted. Unlike the Owner's
/// listener, a request without one is refused.
async fn public_host(settings: &ListenTogetherConfig, request: Request, next: Next) -> Response {
    if is_public_host(settings, request.headers()) {
        return next.run(request).await;
    }
    ApiError::new(
        StatusCode::FORBIDDEN,
        "forbidden_host",
        "the Guest Listener only answers requests addressed to PUBLIC_URL",
    )
    .into_response()
}

fn is_public_host(settings: &ListenTogetherConfig, headers: &HeaderMap) -> bool {
    let mut hosts = headers.get_all(header::HOST).iter();
    match (hosts.next(), hosts.next()) {
        (Some(host), None) => host.to_str().is_ok_and(|host| settings.accepts_host(host)),
        _ => false,
    }
}

/// Refuses any request a page on another origin sent, reads included: the
/// Guest Listener has no reason to answer another site. A request without
/// Origin does not come from another site's page and passes.
async fn public_origin(settings: &ListenTogetherConfig, request: Request, next: Next) -> Response {
    let foreign = request
        .headers()
        .get_all(header::ORIGIN)
        .iter()
        .any(|origin| origin.as_bytes() != settings.public_origin.as_bytes());
    if foreign {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden_origin",
            "the Guest Listener only accepts requests from PUBLIC_URL's pages",
        )
        .into_response();
    }
    next.run(request).await
}

/// Refuses a state-changing request from a page on another origin. Browsers
/// send Origin on every cross-origin write, even a body-less `no-cors` POST
/// that needs no preflight, so only this server's own pages and the CORS
/// allowlist may change state. Requests without Origin (the desktop core,
/// scripts) are not from a browser page and pass. `origins` holds
/// `OWNER_PUBLIC_URL`'s origin too, so that host's pages match scheme and all.
async fn trusted_origin(origins: &[String], request: Request, next: Next) -> Response {
    if is_untrusted_write(origins, request.method(), request.headers()) {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden_origin",
            "the Music Server only accepts changes from its own pages",
        )
        .into_response();
    }
    next.run(request).await
}

fn is_untrusted_write(origins: &[String], method: &Method, headers: &HeaderMap) -> bool {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return false;
    }
    let Some(origin) = headers.get(header::ORIGIN) else {
        return false;
    };
    let Ok(origin) = origin.to_str() else {
        return true;
    };
    if origins
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(origin))
    {
        return false;
    }
    let same_origin = origin
        .split_once("://")
        .zip(
            headers
                .get(header::HOST)
                .and_then(|host| host.to_str().ok()),
        )
        .is_some_and(|((_, authority), host)| {
            is_loopback_host(host) && authority.eq_ignore_ascii_case(host)
        });
    !same_origin
}

/// Refuses a state-changing request whose body a cross-site page can send
/// without a CORS preflight: a form content type, or a body without one.
/// The clients send JSON or audio with its own media type, which a browser
/// only sends cross-origin after the CORS allowlist approves it.
async fn no_simple_writes(request: Request, next: Next) -> Response {
    if is_simple_write(request.method(), request.headers()) {
        return ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "send the request body as application/json or with its media type",
        )
        .into_response();
    }
    next.run(request).await
}

fn is_simple_write(method: &Method, headers: &HeaderMap) -> bool {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return false;
    }
    match headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    {
        Some(value) => matches!(
            value
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
                .as_str(),
            "text/plain" | "application/x-www-form-urlencoded" | "multipart/form-data"
        ),
        None => {
            headers.contains_key(header::TRANSFER_ENCODING)
                || headers
                    .get(header::CONTENT_LENGTH)
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|length| length.trim() != "0")
        }
    }
}

/// Bounds every request to 60s (504 when it runs out, like chi's Timeout),
/// except streams and the long synchronous import and deletion requests.
async fn stream_aware_timeout(request: Request, next: Next) -> Response {
    if is_unbounded_request(&request) {
        return next.run(request).await;
    }
    match tokio::time::timeout(REQUEST_TIMEOUT, next.run(request)).await {
        Ok(response) => response,
        Err(_) => StatusCode::GATEWAY_TIMEOUT.into_response(),
    }
}

fn is_unbounded_request(request: &Request) -> bool {
    let path = request.uri().path();
    let method = request.method();
    let header_is = |name: &str| {
        request
            .headers()
            .get(name)
            .is_some_and(|value| value == "1")
    };
    if is_stream_path(path) {
        return true;
    }
    // Batch confirmation rechecks and commits all selected files synchronously.
    if method == Method::POST
        && path.starts_with("/api/v1/import-batches/")
        && path.ends_with("/confirm")
    {
        return true;
    }
    if method == Method::DELETE
        && header_is("x-permanent-delete")
        && path.starts_with("/api/v1/library/tracks/")
    {
        return true;
    }
    method == Method::POST
        && header_is("x-track-replacement")
        && path.starts_with("/api/v1/imports/")
        && path.ends_with("/replacement")
}

fn is_stream_path(path: &str) -> bool {
    if path == "/api/v1/playback/queue/events" || path == "/api/v1/listen-together/room/events" {
        return true;
    }
    if path.starts_with("/api/v1/imports/") && path.ends_with("/file") {
        return true;
    }
    // External Track streams, playlists and segments, and resolving one,
    // which yt-dlp bounds itself. Artwork is one small image, so it keeps
    // the normal bound.
    if path.starts_with("/api/v1/external/tracks/") && !path.ends_with("/artwork") {
        return true;
    }
    path.ends_with("/stream")
        && (path.starts_with("/api/v1/tracks/")
            || path.starts_with("/api/v1/radio/stations/")
            || path.starts_with("/api/v1/radio/preview/"))
}

#[cfg(test)]
mod tests {
    use super::{
        Extensions, StatusCode, Version, header, is_compressible, is_stream_path, no_store,
    };
    use axum::http::{HeaderMap, HeaderValue};
    use axum::response::{IntoResponse, Response};

    #[test]
    fn compresses_text_but_not_audio_or_event_streams() {
        let compresses = |content_type: &'static str, length: &'static str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
            headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static(length));
            is_compressible(
                StatusCode::OK,
                Version::HTTP_11,
                &headers,
                &Extensions::new(),
            )
        };
        assert!(compresses("text/javascript", "544433"));
        assert!(compresses("application/json", "2048"));
        assert!(!compresses("application/json", "12"));
        assert!(!compresses("audio/flac", "544433"));
        assert!(!compresses("text/event-stream", "544433"));
        assert!(!compresses("application/octet-stream", "544433"));
    }

    #[test]
    fn no_store_leaves_an_answers_own_caching_alone() {
        let with = |name, value| {
            let mut response: Response = ().into_response();
            response
                .headers_mut()
                .insert(name, HeaderValue::from_static(value));
            no_store(response)
        };
        let cache_control =
            |response: Response| response.headers().get(header::CACHE_CONTROL).cloned();
        assert_eq!(
            cache_control(no_store(().into_response())).unwrap(),
            "no-store"
        );
        let cover = with(header::CACHE_CONTROL, "public, max-age=86400");
        assert_eq!(cache_control(cover).unwrap(), "public, max-age=86400");
        assert_eq!(cache_control(with(header::ETAG, "\"a\"")), None);
    }

    #[test]
    fn room_events_are_unbounded() {
        assert!(is_stream_path("/api/v1/listen-together/room/events"));
        assert!(!is_stream_path("/api/v1/listen-together/room"));
    }

    #[test]
    fn external_track_audio_is_unbounded_but_its_artwork_is_not() {
        for path in [
            "/api/v1/external/tracks/a.b/stream",
            "/api/v1/external/tracks/a.b/playback",
            "/api/v1/external/tracks/a.b/playlist.m3u8",
            "/api/v1/external/tracks/a.b/resources/c.d",
        ] {
            assert!(is_stream_path(path), "{path}");
        }
        assert!(!is_stream_path("/api/v1/external/tracks/a.b/artwork"));
    }
}
