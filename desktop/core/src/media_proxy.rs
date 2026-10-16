use crate::connection::{
    ConnectionError, ConnectionErrorCode, HttpBridge, HttpRequest, ServerOrigin,
};
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request, Response, StatusCode};
use axum::routing::any;
use std::collections::BTreeMap;
use std::net::TcpListener as StdTcpListener;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::oneshot;

const PROXY_TOKEN_BYTES: usize = 32;
const MEDIA_PROXY_PORT: u16 = 43129;
#[cfg(not(test))]
const MEDIA_PROXY_PORT_ENV: &str = "EARTHLY_AUDIO_MEDIA_PROXY_PORT";
const MAX_HLS_MANIFEST_BYTES: usize = 1024 * 1024;
const MEDIA_REQUEST_HEADERS: &[&str] = &["accept", "authorization", "content-type", "range"];
const MEDIA_RESPONSE_HEADERS: &[&str] = &[
    "accept-ranges",
    "cache-control",
    "content-disposition",
    "content-length",
    "content-range",
    "content-type",
    "etag",
    "last-modified",
];

#[derive(Clone)]
struct ProxyState {
    bridge: Arc<HttpBridge>,
    origin: Arc<RwLock<Option<ServerOrigin>>>,
    token: String,
}

pub struct MediaProxy {
    base_url: String,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
}

impl MediaProxy {
    pub fn start(
        bridge: Arc<HttpBridge>,
        origin: Arc<RwLock<Option<ServerOrigin>>>,
    ) -> Result<Self, ConnectionError> {
        let listener = StdTcpListener::bind(("127.0.0.1", media_proxy_port()))
            .map_err(|_| proxy_start_error())?;
        listener
            .set_nonblocking(true)
            .map_err(|_| proxy_start_error())?;
        let address = listener.local_addr().map_err(|_| proxy_start_error())?;
        let token = create_proxy_token()?;
        let state = ProxyState {
            bridge,
            origin,
            token: token.clone(),
        };
        let router = Router::new().fallback(any(proxy_request)).with_state(state);
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let (ready_sender, ready_receiver) = std::sync::mpsc::sync_channel(1);
        crate::runtime::spawn(async move {
            let listener = match tokio::net::TcpListener::from_std(listener) {
                Ok(listener) => listener,
                Err(_) => {
                    let _ = ready_sender.send(false);
                    return;
                }
            };
            let _ = ready_sender.send(true);
            if let Err(error) = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_receiver.await;
                })
                .await
            {
                eprintln!("Earthly Audio media proxy failed: {error}");
            }
        });
        if !ready_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or(false)
        {
            return Err(proxy_start_error());
        }

        Ok(Self {
            base_url: format!("http://{address}/{token}"),
            shutdown: Mutex::new(Some(shutdown_sender)),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl Drop for MediaProxy {
    fn drop(&mut self) {
        if let Ok(sender) = self.shutdown.get_mut()
            && let Some(sender) = sender.take()
        {
            let _ = sender.send(());
        }
    }
}

async fn proxy_request(State(state): State<ProxyState>, request: Request<Body>) -> Response<Body> {
    match forward_media_request(&state, request).await {
        Ok(response) => response,
        Err(error) => error_response(error),
    }
}

async fn forward_media_request(
    state: &ProxyState,
    request: Request<Body>,
) -> Result<Response<Body>, ConnectionError> {
    ensure_read_method(request.method())?;
    let relative_url = proxy_relative_url(request.uri(), &state.token)?;
    let headers = request
        .headers()
        .iter()
        .filter(|(name, _)| MEDIA_REQUEST_HEADERS.contains(&name.as_str()))
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_owned(), value.to_owned()))
        })
        .collect::<BTreeMap<_, _>>();
    let media_request = HttpRequest {
        method: request.method().as_str().to_owned(),
        url: relative_url,
        headers,
        body: None,
    };
    let origin = state
        .origin
        .read()
        .map_err(|_| proxy_state_error())?
        .clone()
        .ok_or_else(|| {
            ConnectionError::new(
                ConnectionErrorCode::InvalidOrigin,
                "Configure a Music Server before requesting desktop media.",
            )
        })?;
    let upstream = state.bridge.send_media(&origin, media_request).await?;
    if is_hls_response(&upstream) {
        return hls_response(upstream, &state.token).await;
    }
    let status = upstream.status();
    let headers = upstream.headers().clone();
    let body = Body::from_stream(upstream.bytes_stream());
    let mut response = Response::builder().status(status);
    for (name, value) in headers {
        if let Some(name) = name
            && MEDIA_RESPONSE_HEADERS.contains(&name.as_str())
        {
            response = response.header(name, value);
        }
    }
    response = response.header("access-control-allow-origin", "*").header(
        "access-control-expose-headers",
        "Accept-Ranges, Content-Length, Content-Range, Content-Type",
    );
    response.body(body).map_err(|_| {
        ConnectionError::new(
            ConnectionErrorCode::InvalidResponse,
            "Desktop media proxy could not build the streaming response.",
        )
    })
}

async fn hls_response(
    upstream: reqwest::Response,
    token: &str,
) -> Result<Response<Body>, ConnectionError> {
    let status = upstream.status();
    let headers = upstream.headers().clone();
    let mut manifest = Vec::new();
    let mut stream = upstream.bytes_stream();
    while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
        let chunk = chunk.map_err(|_| invalid_hls_error("could not be read"))?;
        if chunk.len() > MAX_HLS_MANIFEST_BYTES.saturating_sub(manifest.len()) {
            return Err(invalid_hls_error("exceeded the 1 MiB safety limit"));
        }
        manifest.extend_from_slice(&chunk);
    }
    let manifest =
        String::from_utf8(manifest).map_err(|_| invalid_hls_error("was not valid UTF-8"))?;
    let rewritten = rewrite_hls_manifest(&manifest, token)?;
    let mut response = Response::builder()
        .status(status)
        .header("access-control-allow-origin", "*")
        .header(
            "access-control-expose-headers",
            "Accept-Ranges, Content-Length, Content-Range, Content-Type",
        )
        .header("content-length", rewritten.len());
    for (name, value) in headers {
        if let Some(name) = name
            && name != "content-length"
            && MEDIA_RESPONSE_HEADERS.contains(&name.as_str())
        {
            response = response.header(name, value);
        }
    }
    response.body(Body::from(rewritten)).map_err(|_| {
        ConnectionError::new(
            ConnectionErrorCode::InvalidResponse,
            "Desktop media proxy could not build the rewritten HLS response.",
        )
    })
}

fn is_hls_response(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "application/vnd.apple.mpegurl"
                    | "application/x-mpegurl"
                    | "audio/mpegurl"
                    | "audio/x-mpegurl"
            )
        })
}

fn rewrite_hls_manifest(manifest: &str, token: &str) -> Result<String, ConnectionError> {
    if !manifest.trim_start().starts_with("#EXTM3U") {
        return Err(invalid_hls_error("was missing the EXTM3U header"));
    }
    manifest
        .split('\n')
        .map(|line| rewrite_hls_line(line, token))
        .collect::<Result<Vec<_>, _>>()
        .map(|lines| lines.join("\n"))
}

fn rewrite_hls_line(line: &str, token: &str) -> Result<String, ConnectionError> {
    let (content, suffix) = line
        .strip_suffix('\r')
        .map_or((line, ""), |content| (content, "\r"));
    if content.is_empty() {
        return Ok(suffix.to_owned());
    }
    if !content.starts_with('#') {
        return rewrite_hls_uri(content, token).map(|uri| format!("{uri}{suffix}"));
    }

    let mut rewritten = String::with_capacity(content.len());
    let mut remaining = content;
    while let Some(start) = remaining.find("URI=\"") {
        let value_start = start + "URI=\"".len();
        rewritten.push_str(&remaining[..value_start]);
        let value = &remaining[value_start..];
        let end = value
            .find('"')
            .ok_or_else(|| invalid_hls_error("contained an unterminated URI attribute"))?;
        rewritten.push_str(&rewrite_hls_uri(&value[..end], token)?);
        rewritten.push('"');
        remaining = &value[end + 1..];
    }
    rewritten.push_str(remaining);
    rewritten.push_str(suffix);
    Ok(rewritten)
}

fn rewrite_hls_uri(uri: &str, token: &str) -> Result<String, ConnectionError> {
    let parsed = url::Url::parse(&format!("http://desktop.invalid{uri}"))
        .map_err(|_| invalid_hls_error("contained an invalid URI"))?;
    if !uri.starts_with('/') || parsed.fragment().is_some() || !is_media_path(parsed.path()) {
        return Err(invalid_hls_error(
            "contained a URI outside the signed Music Server media API",
        ));
    }
    Ok(format!("/{token}{uri}"))
}

fn invalid_hls_error(reason: &str) -> ConnectionError {
    ConnectionError::new(
        ConnectionErrorCode::InvalidResponse,
        format!("Music Server HLS manifest {reason}."),
    )
}

#[cfg(not(test))]
fn media_proxy_port() -> u16 {
    // Integration tests run the non-test build of this crate and must not
    // collide with a running Desktop Client on the fixed CSP port.
    std::env::var(MEDIA_PROXY_PORT_ENV)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(MEDIA_PROXY_PORT)
}

#[cfg(test)]
fn media_proxy_port() -> u16 {
    0
}

/// Media, covers, and artwork are only ever read; mutations stay behind the
/// renderer's authenticated bridge.
fn ensure_read_method(method: &Method) -> Result<(), ConnectionError> {
    if matches!(*method, Method::GET | Method::HEAD) {
        return Ok(());
    }
    Err(ConnectionError::new(
        ConnectionErrorCode::InvalidRequest,
        "Desktop media proxy only serves GET and HEAD requests.",
    ))
}

fn proxy_relative_url(uri: &axum::http::Uri, token: &str) -> Result<String, ConnectionError> {
    let prefix = format!("/{token}");
    let path = uri.path().strip_prefix(&prefix).ok_or_else(|| {
        ConnectionError::new(
            ConnectionErrorCode::InvalidRequest,
            "Desktop media proxy token is invalid.",
        )
    })?;
    if !is_media_path(path) {
        return Err(ConnectionError::new(
            ConnectionErrorCode::InvalidRequest,
            "Desktop media proxy only serves Music Server media API paths.",
        ));
    }
    Ok(match uri.query() {
        Some(query) => format!("{path}?{query}"),
        None => path.to_owned(),
    })
}

fn is_media_path(path: &str) -> bool {
    (path.starts_with("/api/v1/tracks/") && path.ends_with("/stream"))
        || (path.starts_with("/api/v1/radio/stations/") && path.ends_with("/stream"))
        || (path.starts_with("/api/v1/radio/preview/") && path.ends_with("/stream"))
        || is_image_path(path)
}

/// Album covers and Managed Import artwork. The webview loads them from here
/// because WebKitGTK 2.54 refuses custom-scheme images on an http:// page, and
/// MPRIS clients fetch the cover art URL outside the webview.
fn is_image_path(path: &str) -> bool {
    match path.split('/').collect::<Vec<_>>().as_slice() {
        ["", "api", "v1", "library", "albums", album_id, "cover"] => !album_id.is_empty(),
        [
            "",
            "api",
            "v1",
            "import-batches",
            batch_id,
            "artwork",
            artwork_id,
        ] => !batch_id.is_empty() && !artwork_id.is_empty(),
        _ => false,
    }
}

fn create_proxy_token() -> Result<String, ConnectionError> {
    let mut bytes = [0_u8; PROXY_TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| proxy_start_error())?;
    let mut token = String::with_capacity(PROXY_TOKEN_BYTES * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(token, "{byte:02x}").map_err(|_| proxy_start_error())?;
    }
    Ok(token)
}

fn error_response(error: ConnectionError) -> Response<Body> {
    let status = match error.code {
        ConnectionErrorCode::InvalidOrigin | ConnectionErrorCode::InvalidRequest => {
            StatusCode::BAD_REQUEST
        }
        ConnectionErrorCode::Unreachable => StatusCode::BAD_GATEWAY,
        ConnectionErrorCode::ProxyUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        ConnectionErrorCode::InvalidResponse
        | ConnectionErrorCode::ResponseTooLarge
        | ConnectionErrorCode::CapabilityMismatch
        | ConnectionErrorCode::Storage => StatusCode::INTERNAL_SERVER_ERROR,
    };
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Body::from(error.message))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn proxy_start_error() -> ConnectionError {
    ConnectionError::new(
        ConnectionErrorCode::ProxyUnavailable,
        format!(
            "Desktop media proxy could not bind 127.0.0.1:{MEDIA_PROXY_PORT}. Close another Earthly Audio instance or process using that port, then restart the Desktop Client."
        ),
    )
}

fn proxy_state_error() -> ConnectionError {
    ConnectionError::new(
        ConnectionErrorCode::ProxyUnavailable,
        "Desktop media proxy connection state is unavailable. Restart the Desktop Client.",
    )
}

#[cfg(test)]
mod tests {
    use super::{MediaProxy, ensure_read_method, proxy_relative_url};
    use crate::connection::{HttpBridge, ServerOrigin};
    use axum::Router;
    use axum::body::Body;
    use axum::http::Method;
    use axum::http::{Request, Response, StatusCode};
    use axum::routing::any;
    use futures_util::StreamExt;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Arc, RwLock};
    use std::thread;
    use std::time::Duration;

    const LARGE_CONTENT_LENGTH: usize = 32 * 1024 * 1024 + 1;

    async fn serve_hls_resource(request: Request<Body>) -> Response<Body> {
        let query = request.uri().query().unwrap_or_default();
        match query {
            "resource=media-signed" => Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/vnd.apple.mpegurl")
                .body(Body::from(
                    "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"/api/v1/radio/stations/station-1/stream?resource=key-signed\"\n#EXTINF:6,\n/api/v1/radio/stations/station-1/stream?resource=segment-signed\n",
                ))
                .expect("media response"),
            "resource=segment-signed" => Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header("content-type", "video/mp2t")
                .header("content-range", "bytes 0-6/7")
                .body(Body::from("segment"))
                .expect("segment response"),
            _ => Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/vnd.apple.mpegurl")
                .body(Body::from(
                    "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,URI=\"/api/v1/radio/stations/station-1/stream?resource=audio-signed\"\n#EXT-X-STREAM-INF:BANDWIDTH=128000\n/api/v1/radio/stations/station-1/stream?resource=media-signed\n",
                ))
                .expect("master response"),
        }
    }

    #[test]
    fn proxy_serves_covers_and_import_artwork_but_rejects_other_library_paths() {
        for (allowed, forwarded) in [
            (
                "/token/api/v1/library/albums/album-1/cover",
                "/api/v1/library/albums/album-1/cover",
            ),
            (
                "/token/api/v1/import-batches/batch-1/artwork/cover-1",
                "/api/v1/import-batches/batch-1/artwork/cover-1",
            ),
        ] {
            let uri = allowed.parse().expect("URI");
            assert_eq!(
                proxy_relative_url(&uri, "token").expect("image path allowed"),
                forwarded
            );
        }

        for rejected in [
            "/token/api/v1/library/albums/album-1",
            "/token/api/v1/library/tracks/track-1",
            "/token/api/v1/library/albums",
            "/token/api/v1/import-batches/batch-1",
            "/token/api/v1/import-batches/batch-1/artwork/",
        ] {
            let uri = rejected.parse().expect("URI");
            assert!(
                proxy_relative_url(&uri, "token").is_err(),
                "{rejected} must stay private"
            );
        }
    }

    #[test]
    fn proxy_only_forwards_reads() {
        for allowed in [Method::GET, Method::HEAD] {
            assert!(ensure_read_method(&allowed).is_ok(), "{allowed}");
        }
        for rejected in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
            assert!(ensure_read_method(&rejected).is_err(), "{rejected}");
        }
    }

    fn serve_never_ending_large_stream() -> (String, Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind upstream");
        let address = listener.local_addr().expect("upstream address");
        let (release_sender, release_receiver) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept upstream request");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("read upstream request");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: {LARGE_CONTENT_LENGTH}\r\nConnection: close\r\n\r\nstart",
            )
            .expect("write stream start");
            stream.flush().expect("flush stream start");
            release_receiver.recv().expect("release upstream");
        });
        (format!("http://{address}"), release_sender)
    }

    fn serve_never_ending_chunked_stream() -> (String, Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind upstream");
        let address = listener.local_addr().expect("upstream address");
        let (release_sender, release_receiver) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept upstream request");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("read upstream request");
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nstart\r\n",
                )
                .expect("write stream start");
            stream.flush().expect("flush stream start");
            release_receiver.recv().expect("release upstream");
        });
        (format!("http://{address}"), release_sender)
    }

    fn serve_range_response() -> (String, Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind upstream");
        let address = listener.local_addr().expect("upstream address");
        let (request_sender, request_receiver) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept upstream request");
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).expect("read upstream request");
            request_sender
                .send(String::from_utf8_lossy(&request[..read]).into_owned())
                .expect("capture request");
            stream
                .write_all(
                    b"HTTP/1.1 206 Partial Content\r\nContent-Type: audio/flac\r\nContent-Range: bytes 4-7/12\r\nAccept-Ranges: bytes\r\nContent-Length: 4\r\nConnection: close\r\n\r\n4567",
                )
                .expect("write range response");
        });
        (format!("http://{address}"), request_receiver)
    }

    fn start_proxy(origin: &str) -> MediaProxy {
        MediaProxy::start(
            Arc::new(HttpBridge::new().expect("create bridge")),
            Arc::new(RwLock::new(Some(
                ServerOrigin::parse(origin).expect("valid origin"),
            ))),
        )
        .expect("start media proxy")
    }

    #[tokio::test]
    async fn large_never_ending_track_starts_before_eof() {
        let (origin, release_upstream) = serve_never_ending_large_stream();
        let proxy = start_proxy(&origin);
        let response = reqwest::get(format!("{}/api/v1/tracks/track-1/stream", proxy.base_url()))
            .await
            .expect("request proxy");
        assert_eq!(response.content_length(), Some(LARGE_CONTENT_LENGTH as u64));

        let mut stream = response.bytes_stream();
        let first = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("media should arrive before EOF")
            .expect("media chunk")
            .expect("valid media chunk");

        assert_eq!(first.as_ref(), b"start");
        release_upstream.send(()).expect("release upstream");
    }

    #[tokio::test]
    async fn never_ending_live_radio_starts_before_eof() {
        let (origin, release_upstream) = serve_never_ending_chunked_stream();
        let proxy = start_proxy(&origin);
        let response = reqwest::get(format!(
            "{}/api/v1/radio/stations/station-1/stream",
            proxy.base_url()
        ))
        .await
        .expect("request proxy");

        let mut stream = response.bytes_stream();
        let first = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("live radio should arrive before EOF")
            .expect("radio chunk")
            .expect("valid radio chunk");

        assert_eq!(first.as_ref(), b"start");
        release_upstream.send(()).expect("release upstream");
    }

    #[tokio::test]
    async fn range_status_and_headers_are_preserved() {
        let (origin, captured_request) = serve_range_response();
        let proxy = start_proxy(&origin);
        let response = reqwest::Client::new()
            .get(format!(
                "{}/api/v1/radio/stations/station-1/stream?url=https%3A%2F%2Fmedia.example%2Fsegment.ts",
                proxy.base_url()
            ))
            .header("range", "bytes=4-7")
            .send()
            .await
            .expect("request proxy");

        assert_eq!(response.status(), reqwest::StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response
                .headers()
                .get("content-range")
                .and_then(|value| value.to_str().ok()),
            Some("bytes 4-7/12")
        );
        assert_eq!(
            response
                .headers()
                .get("accept-ranges")
                .and_then(|value| value.to_str().ok()),
            Some("bytes")
        );
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("*")
        );
        assert_eq!(
            response.bytes().await.expect("range body").as_ref(),
            b"4567"
        );
        let request = captured_request
            .recv_timeout(Duration::from_millis(500))
            .expect("captured upstream request")
            .to_ascii_lowercase();
        assert!(request.contains("range: bytes=4-7"));
        assert!(request.contains("?url=https%3a%2f%2fmedia.example%2fsegment.ts"));
    }

    #[tokio::test]
    async fn hls_master_media_and_segment_keep_the_proxy_token_prefix() {
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind HLS upstream");
        let upstream_address = upstream_listener
            .local_addr()
            .expect("HLS upstream address");
        let upstream_task = tokio::spawn(async move {
            axum::serve(
                upstream_listener,
                Router::new().fallback(any(serve_hls_resource)),
            )
            .await
            .expect("serve HLS upstream");
        });
        let proxy = start_proxy(&format!("http://{upstream_address}"));
        let proxy_url = url::Url::parse(proxy.base_url()).expect("proxy URL");
        let proxy_origin = proxy_url.origin().ascii_serialization();
        let proxy_prefix = proxy_url.path();

        let master = reqwest::get(format!(
            "{}/api/v1/radio/stations/station-1/stream",
            proxy.base_url()
        ))
        .await
        .expect("master request");
        assert_eq!(
            master
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok()),
            Some("application/vnd.apple.mpegurl")
        );
        let master_body = master.text().await.expect("master body");
        assert!(master_body.contains(&format!(
            "URI=\"{proxy_prefix}/api/v1/radio/stations/station-1/stream?resource=audio-signed\""
        )));
        let media_path = master_body
            .lines()
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .expect("media path");
        assert!(media_path.starts_with(proxy_prefix));

        let media = reqwest::get(format!("{proxy_origin}{media_path}"))
            .await
            .expect("media request");
        assert_eq!(
            media
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok()),
            Some("application/vnd.apple.mpegurl")
        );
        let media_body = media.text().await.expect("media body");
        assert!(media_body.contains(&format!(
            "URI=\"{proxy_prefix}/api/v1/radio/stations/station-1/stream?resource=key-signed\""
        )));
        let segment_path = media_body
            .lines()
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .expect("segment path");
        assert!(segment_path.starts_with(proxy_prefix));

        let segment = reqwest::get(format!("{proxy_origin}{segment_path}"))
            .await
            .expect("segment request");
        assert_eq!(segment.status(), reqwest::StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            segment
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok()),
            Some("video/mp2t")
        );
        assert_eq!(segment.text().await.expect("segment body"), "segment");
        upstream_task.abort();
    }
}
