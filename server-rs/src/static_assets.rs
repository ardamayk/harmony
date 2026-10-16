//! The embedded Web Client and product docs (server/internal/staticassets).
//! Both are built into server/internal/staticassets by `mise run
//! server:build` before the server compiles.

use axum::body::Body;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../server/internal/staticassets/web"]
struct Web;

#[derive(RustEmbed)]
#[folder = "../server/internal/staticassets/docs"]
struct Docs;

/// Serves a Web Client asset, or index.html for any other path so the
/// client's history routes load on reload.
pub async fn web(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if !path.is_empty()
        && let Some(response) = asset::<Web>(path)
    {
        return response;
    }
    match Web::get("index.html") {
        Some(file) => (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
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
