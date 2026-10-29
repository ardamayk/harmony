//! API and product documentation (server/internal/modules/docs).

use crate::static_assets;
use axum::Router;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;

const OPENAPI_SPEC: &str = include_str!("../../../packages/contracts/openapi.yaml");
const API_REFERENCE_HTML: &str = include_str!("../../api-reference.html");
const API_REFERENCE_SCRIPT: &str = include_str!("../../api-reference.js");

pub fn routes<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new()
        .route(
            "/api/openapi.yaml",
            get(|| async { ([(header::CONTENT_TYPE, "application/yaml")], OPENAPI_SPEC) }),
        )
        .route("/api/docs", get(api_reference))
        .route(
            "/api/docs/api-reference.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    API_REFERENCE_SCRIPT,
                )
            }),
        )
        .route(
            "/api/docs/scalar/{*path}",
            get(static_assets::api_reference),
        )
        // 301 with a short HTML body, as Go's http.Redirect answers a GET.
        .route(
            "/docs",
            get(|| async {
                (
                    StatusCode::MOVED_PERMANENTLY,
                    [
                        (header::LOCATION, "/docs/"),
                        (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                    ],
                    "<a href=\"/docs/\">Moved Permanently</a>.\n\n",
                )
            }),
        )
        .route("/docs/", get(static_assets::docs))
        .route("/docs/{*path}", get(static_assets::docs))
}

/// Scalar's API reference (ADR 0035). Its scripts, styles and the OpenAPI
/// document all come from this server; the policy allows no other origin,
/// so the page cannot load fonts, scripts or telemetry from anywhere else.
/// Styles also allow 'unsafe-inline': Scalar sets `style` attributes and
/// injects `<style>` elements at runtime, which a nonce cannot cover.
const API_REFERENCE_POLICY: &str = "default-src 'none'; script-src 'self'; \
    style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; \
    font-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

async fn api_reference() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CONTENT_SECURITY_POLICY, API_REFERENCE_POLICY),
        ],
        API_REFERENCE_HTML,
    )
}
