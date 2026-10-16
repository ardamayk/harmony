//! The complete HTTP surface: every module on one router, with the Go
//! server's middleware (server/cmd/server/router.go).

use crate::config::Config;
use crate::dependencies::Report;
use crate::{api, modules, static_assets};
use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderName, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::Duration;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub dependencies: Arc<Report>,
    pub db: SqlitePool,
    pub waveforms: Arc<modules::library::WaveformService>,
    pub search: Arc<modules::search::SearchService>,
}

impl AppState {
    pub fn new(config: Config, dependencies: Report, db: SqlitePool) -> Self {
        let waveforms = modules::library::WaveformService::new(
            modules::library::Repository::new(db.clone()),
            dependencies.has(crate::dependencies::FFMPEG),
        );
        Self {
            config: Arc::new(config),
            dependencies: Arc::new(dependencies),
            db: db.clone(),
            waveforms: Arc::new(waveforms),
            search: Arc::new(modules::search::SearchService::new(db.clone())),
        }
    }
}

pub fn router(state: AppState) -> Router {
    let cors = cors(&state.config.cors_origins);
    Router::new()
        .route("/api/v1/health", get(api::get_health))
        .route("/api/v1/me", get(api::get_me))
        .merge(modules::docs::routes())
        .merge(modules::preferences::routes())
        .merge(modules::library::routes())
        .merge(modules::playlists::routes())
        .merge(modules::search::routes())
        .fallback(static_assets::web)
        .layer(middleware::from_fn(stream_aware_timeout))
        .layer(cors)
        .layer(CatchPanicLayer::new())
        .with_state(state)
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
        .allow_credentials(true)
        .max_age(Duration::from_secs(300))
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
    if path == "/api/v1/playback/queue/events" {
        return true;
    }
    if path.starts_with("/api/v1/imports/") && path.ends_with("/file") {
        return true;
    }
    path.ends_with("/stream")
        && (path.starts_with("/api/v1/tracks/")
            || path.starts_with("/api/v1/radio/stations/")
            || path.starts_with("/api/v1/radio/preview/"))
}
