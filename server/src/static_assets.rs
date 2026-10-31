//! The embedded Web Client, product docs and Scalar's API reference bundle.
//! `mise run server:build` builds the first two and copies all three into
//! server/static (scripts/sync-static.sh) before the server compiles; only
//! the empty directories are committed.

use crate::respond::ApiError;
use axum::body::Body;
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "static/web"]
struct Web;

#[derive(RustEmbed)]
#[folder = "static/docs"]
struct Docs;

/// Scalar's standalone browser bundle from `@scalar/api-reference`.
#[derive(RustEmbed)]
#[folder = "static/api-reference"]
struct ApiReference;

/// Vite names everything under `assets/` by its content, so a browser can keep
/// it for good; anything else must be asked about, or a new build stays unseen.
fn cache_control(path: &str) -> HeaderValue {
    HeaderValue::from_static(if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    })
}

/// Serves a Web Client asset, or index.html for any other path so the
/// client's history routes load on reload. An API path no route took gets
/// the API's own 404, not a page, and only reads reach the client.
pub async fn web(method: Method, uri: Uri) -> Response {
    if uri.path() == "/api" || uri.path().starts_with("/api/") {
        return ApiError::not_found("not found").into_response();
    }
    if method != Method::GET && method != Method::HEAD {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(header::ALLOW, "GET, HEAD")],
        )
            .into_response();
    }
    let path = uri.path().trim_start_matches('/');
    if !path.is_empty()
        && let Some(mut response) = asset::<Web>(path)
    {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, cache_control(path));
        return response;
    }
    match Web::get("index.html") {
        Some(file) => (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            Body::from(file.data.into_owned()),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Serves `/docs/*`; a directory path serves its index.html.
pub async fn docs(uri: Uri) -> Response {
    let path = uri
        .path()
        .trim_start_matches("/docs")
        .trim_start_matches('/');
    let path = if path.is_empty() || path.ends_with('/') {
        format!("{path}index.html")
    } else {
        path.to_owned()
    };
    asset::<Docs>(&path).unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

/// Serves `/api/docs/scalar/*`, the files of Scalar's browser bundle.
pub async fn api_reference(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches("/api/docs/scalar/");
    asset::<ApiReference>(path).unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

fn asset<E: RustEmbed>(path: &str) -> Option<Response> {
    let file = E::get(path)?;
    let mime = file.metadata.mimetype().to_owned();
    Some(
        (
            [(header::CONTENT_TYPE, mime)],
            Body::from(file.data.into_owned()),
        )
            .into_response(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_content_named_files_are_kept_for_good() {
        assert!(
            cache_control("assets/index-abc123.js")
                .to_str()
                .unwrap()
                .contains("immutable")
        );
        assert_eq!(cache_control("favicon.ico"), "no-cache");
    }
}
