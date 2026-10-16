//! API and product documentation (server/internal/modules/docs).

use crate::static_assets;
use axum::Router;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;

const OPENAPI_SPEC: &str = include_str!("../../../packages/contracts/openapi.yaml");
const SWAGGER_HTML: &str = include_str!("../../../server/internal/modules/docs/swagger.html");

pub fn routes<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new()
        .route(
            "/api/openapi.yaml",
            get(|| async { ([(header::CONTENT_TYPE, "application/yaml")], OPENAPI_SPEC) }),
        )
        .route(
            "/api/docs",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    SWAGGER_HTML,
                )
                    .into_response()
            }),
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
