//! HTTP for the External Source: parse the request, call the service,
//! respond. Every route answers 404 `external_source_disabled` while the
//! source is not configured.

use super::service::{
    Artwork, DEFAULT_SEARCH_LIMIT, ExternalError, Playback, Resource, SearchResults,
};
use crate::app::AppState;
use crate::modules::radio::{HLS_CONTENT_TYPE, UpstreamResponse, copy_stream_headers};
use crate::query::GoQuery;
use crate::respond::{ApiError, ApiResult};
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;

pub fn routes() -> Router<AppState> {
    // `get` also answers HEAD, without the body.
    Router::new()
        .route("/api/v1/external/search", get(search))
        .route(
            "/api/v1/external/tracks/{external_track_id}/playback",
            get(playback),
        )
        .route(
            "/api/v1/external/tracks/{external_track_id}/stream",
            get(stream),
        )
        .route(
            "/api/v1/external/tracks/{external_track_id}/playlist.m3u8",
            get(playlist),
        )
        .route(
            "/api/v1/external/tracks/{external_track_id}/resources/{resource}",
            get(resource),
        )
        .route(
            "/api/v1/external/tracks/{external_track_id}/artwork",
            get(artwork),
        )
}

impl From<ExternalError> for ApiError {
    fn from(error: ExternalError) -> Self {
        let (status, code) = match &error {
            ExternalError::Disabled => (StatusCode::NOT_FOUND, "external_source_disabled"),
            ExternalError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            ExternalError::ArtworkMissing => {
                (StatusCode::NOT_FOUND, "external_track_artwork_missing")
            }
            ExternalError::Unplayable(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "external_track_unplayable",
            ),
            ExternalError::Failed(_) => (StatusCode::BAD_GATEWAY, "external_source_failed"),
            ExternalError::Unavailable(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "external_source_unavailable",
            ),
            ExternalError::Timeout(_) => (StatusCode::GATEWAY_TIMEOUT, "external_source_timeout"),
        };
        ApiError::new(status, code, error.to_string())
    }
}

async fn search(State(state): State<AppState>, query: GoQuery) -> ApiResult<Json<SearchResults>> {
    let source = &state.external_source;
    source.ensure_enabled()?;
    let limit = match query.get("limit") {
        "" => DEFAULT_SEARCH_LIMIT,
        raw => raw.parse().map_err(|_| {
            ApiError::bad_request("limit must be between 1 and 20").with_field("limit")
        })?,
    };
    Ok(Json(source.search(query.get("q"), limit).await?))
}

async fn playback(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Playback>> {
    Ok(Json(state.external_source.playback(&id).await?))
}

async fn stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let range = headers.get(header::RANGE).cloned();
    let head = method == Method::HEAD;
    let response = state.external_source.open_stream(&id, range, head).await?;
    Ok(media(response))
}

async fn playlist(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Response> {
    Ok(hls(state.external_source.playlist(&id).await?))
}

async fn resource(
    State(state): State<AppState>,
    Path((id, resource)): Path<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let range = headers.get(header::RANGE).cloned();
    Ok(
        match state
            .external_source
            .resource(&id, &resource, range)
            .await?
        {
            Resource::Playlist(playlist) => hls(playlist),
            Resource::Media(response) => media(response),
        },
    )
}

async fn artwork(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Response> {
    Ok(image(state.external_source.artwork(&id).await?))
}

/// The cover as the image type it was checked to be, kept private to the
/// listener's browser for an hour (its id dies with the process anyway).
fn image(artwork: Artwork) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, artwork.media_type),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::CACHE_CONTROL, "private, max-age=3600"),
        ],
        artwork.data,
    )
        .into_response()
}

/// The upstream audio with its status (200, 206 or 416), length and range,
/// served as a media type only (never a page on this origin).
fn media(response: UpstreamResponse) -> Response {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
    let headers = copy_stream_headers(&response.headers, true);
    (status, headers, Body::from_stream(response.body)).into_response()
}

fn hls(playlist: String) -> Response {
    let mut headers = copy_stream_headers(&HeaderMap::new(), false);
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(HLS_CONTENT_TYPE),
    );
    // Its resource links expire, so it is fetched fresh each time.
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    (StatusCode::OK, headers, playlist).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures_util::StreamExt;

    fn upstream(status: u16, headers: &[(&'static str, &'static str)]) -> UpstreamResponse {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(*name, HeaderValue::from_static(value));
        }
        UpstreamResponse {
            status,
            headers: map,
            url: url::Url::parse("https://media.example/a").expect("url"),
            body: futures_util::stream::iter([Ok(Bytes::from_static(b"ab"))]).boxed(),
        }
    }

    #[test]
    fn media_keeps_ranges_and_lengths_but_never_serves_a_page() {
        let response = media(upstream(
            206,
            &[
                ("content-type", "audio/mp4"),
                ("content-range", "bytes 0-1/10"),
                ("content-length", "2"),
                ("accept-ranges", "bytes"),
                ("set-cookie", "a=b"),
            ],
        ));
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        let headers = response.headers();
        assert_eq!(headers[header::CONTENT_TYPE], "audio/mp4");
        assert_eq!(headers[header::CONTENT_RANGE], "bytes 0-1/10");
        assert_eq!(headers[header::CONTENT_LENGTH], "2");
        assert_eq!(headers[header::ACCEPT_RANGES], "bytes");
        assert!(headers.get(header::SET_COOKIE).is_none());
        let response = media(upstream(200, &[("content-type", "text/html")]));
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/octet-stream"
        );
        assert_eq!(
            response.headers()[header::CONTENT_SECURITY_POLICY],
            "sandbox"
        );
    }

    #[test]
    fn playlists_are_hls_and_not_cached() {
        let response = hls("#EXTM3U\n".to_owned());
        let headers = response.headers();
        assert_eq!(headers[header::CONTENT_TYPE], HLS_CONTENT_TYPE);
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    }

    #[test]
    fn artwork_is_served_as_its_image_type_and_cached_privately() {
        let response = image(Artwork {
            media_type: "image/webp",
            data: Bytes::from_static(b"RIFF"),
        });
        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers();
        assert_eq!(headers[header::CONTENT_TYPE], "image/webp");
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(headers[header::CACHE_CONTROL], "private, max-age=3600");
    }

    #[test]
    fn errors_map_to_their_codes() {
        for (error, status, code) in [
            (ExternalError::Disabled, 404, "external_source_disabled"),
            (
                ExternalError::ArtworkMissing,
                404,
                "external_track_artwork_missing",
            ),
            (ExternalError::BadRequest("x".into()), 400, "bad_request"),
            (
                ExternalError::Unplayable("x".into()),
                422,
                "external_track_unplayable",
            ),
            (
                ExternalError::Failed("x".into()),
                502,
                "external_source_failed",
            ),
            (
                ExternalError::Unavailable("x".into()),
                503,
                "external_source_unavailable",
            ),
            (
                ExternalError::Timeout("x".into()),
                504,
                "external_source_timeout",
            ),
        ] {
            let error = ApiError::from(error);
            assert_eq!(
                (error.status.as_u16(), error.body.code.as_str()),
                (status, code)
            );
        }
    }
}
