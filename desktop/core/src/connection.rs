use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, RwLock};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use url::{Host, Url};

const HEALTH_PATH: &str = "/api/v1/health";
const QUEUE_EVENTS_PATH: &str = "/api/v1/playback/queue/events";
const REQUIRED_SERVER_CAPABILITIES: &[&str] = &[
    "api.v1",
    "playback.queue-events.v1",
    "managed-import-batches.v1",
    "managed-track-deletion.v1",
    "managed-track-replacement.v1",
];
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
/// Deadline for an ordinary renderer API request.
const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Deadline for External Source search and playback resolution. The Music
/// Server gives yt-dlp up to 20 s to search and 30 s to resolve, after waiting
/// for one of its two yt-dlp permits, so these outlast the ordinary deadline.
const EXTERNAL_SOURCE_REQUEST_TIMEOUT: Duration = Duration::from_secs(75);
const ALLOWED_REQUEST_HEADERS: &[&str] = &[
    "accept",
    "authorization",
    "content-type",
    // Queue mutations carry their Queue Revision as a precondition.
    "if-match",
    "range",
    // Ties a media stream to the Music Server's side of it (ADR 0033).
    "sentry-trace",
    "x-permanent-delete",
    "x-track-replacement",
];

/// Headers reqwest sets itself, or that would change what a request means.
const RESERVED_SERVER_HEADERS: &[&str] = &[
    "connection",
    "content-length",
    "host",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];
const MAX_SERVER_HEADERS: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerOrigin {
    url: Url,
}

impl ServerOrigin {
    pub fn parse(value: &str) -> Result<Self, ConnectionError> {
        let mut url = Url::parse(value).map_err(|_| {
            ConnectionError::new(
                ConnectionErrorCode::InvalidOrigin,
                "Enter a complete Music Server URL such as http://127.0.0.1:8090 or https://music.example.com.",
            )
        })?;
        let is_loopback = match url.host() {
            Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
            Some(Host::Ipv4(address)) => address.is_loopback(),
            Some(Host::Ipv6(address)) => address.is_loopback(),
            None => false,
        };
        let has_origin_only = url.username().is_empty()
            && url.password().is_none()
            && matches!(url.path(), "" | "/")
            && url.query().is_none()
            && url.fragment().is_none();
        let is_safe_scheme = match url.scheme() {
            "https" => url.host().is_some(),
            "http" => is_loopback,
            _ => false,
        };
        if !is_safe_scheme || !has_origin_only {
            return Err(ConnectionError::new(
                ConnectionErrorCode::InvalidOrigin,
                "Music Server URL must be an https origin, or http on loopback, without credentials, path, query, or fragment.",
            ));
        }

        if matches!(url.host(), Some(Host::Domain(host)) if host.eq_ignore_ascii_case("localhost"))
        {
            url.set_host(Some("127.0.0.1")).map_err(|_| {
                ConnectionError::new(
                    ConnectionErrorCode::InvalidOrigin,
                    "Music Server URL could not be normalized.",
                )
            })?;
        }

        let canonical = Url::parse(&url.origin().ascii_serialization()).map_err(|_| {
            ConnectionError::new(
                ConnectionErrorCode::InvalidOrigin,
                "Music Server URL could not be normalized.",
            )
        })?;
        Ok(Self { url: canonical })
    }

    pub fn as_str(&self) -> &str {
        self.url.as_str().trim_end_matches('/')
    }

    pub(crate) fn endpoint(&self, path: &str) -> Result<Url, ConnectionError> {
        self.url.join(path).map_err(|_| {
            ConnectionError::new(
                ConnectionErrorCode::InvalidOrigin,
                "Music Server request URL is invalid.",
            )
        })
    }
}

/// Headers the operator's proxy needs on every request to one Music Server.
/// Their values never leave the core: not to the renderer, and not to a log.
#[derive(Clone, Debug, Default)]
pub struct ServerHeaders(HeaderMap);

impl ServerHeaders {
    pub fn parse(headers: &BTreeMap<String, String>) -> Result<Self, ConnectionError> {
        let invalid =
            |message: String| ConnectionError::new(ConnectionErrorCode::InvalidRequest, message);
        if headers.len() > MAX_SERVER_HEADERS {
            return Err(invalid(format!(
                "A Music Server connection allows at most {MAX_SERVER_HEADERS} custom headers."
            )));
        }
        let mut parsed = HeaderMap::with_capacity(headers.len());
        for (name, value) in headers {
            let header = HeaderName::try_from(name.trim())
                .ok()
                .filter(|header| !RESERVED_SERVER_HEADERS.contains(&header.as_str()))
                .ok_or_else(|| invalid(format!("Custom header name {name:?} is not allowed.")))?;
            let mut value = HeaderValue::try_from(value.trim())
                .map_err(|_| invalid(format!("Custom header {name} has an invalid value.")))?;
            value.set_sensitive(true);
            parsed.insert(header, value);
        }
        Ok(Self(parsed))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionErrorCode {
    InvalidOrigin,
    InvalidRequest,
    Unreachable,
    AccessDenied,
    InvalidResponse,
    ResponseTooLarge,
    ProxyUnavailable,
    CapabilityMismatch,
    Storage,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionError {
    pub code: ConnectionErrorCode,
    pub message: String,
}

impl ConnectionError {
    pub(crate) fn new(code: ConnectionErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ConnectionError {}

#[derive(Clone, Debug, Deserialize)]
struct HealthResponse {
    status: String,
    version: String,
    capabilities: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionCheck {
    pub origin: String,
    pub version: String,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub body: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

pub struct HttpBridge {
    client: reqwest::Client,
    streaming_client: reqwest::Client,
    max_response_bytes: usize,
    /// Kept with its origin, so another server never receives them.
    saved_headers: RwLock<Option<(ServerOrigin, ServerHeaders)>>,
}

impl HttpBridge {
    pub fn new() -> Result<Self, ConnectionError> {
        Self::build(MAX_RESPONSE_BYTES)
    }

    #[cfg(test)]
    fn with_max_response_bytes(max_response_bytes: usize) -> Result<Self, ConnectionError> {
        Self::build(max_response_bytes)
    }

    fn build(max_response_bytes: usize) -> Result<Self, ConnectionError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(API_REQUEST_TIMEOUT)
            .redirect(Policy::none())
            .build()
            .map_err(|_| {
                ConnectionError::new(
                    ConnectionErrorCode::InvalidResponse,
                    "Desktop HTTP transport could not be initialized.",
                )
            })?;
        let streaming_client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .redirect(Policy::none())
            .build()
            .map_err(|_| {
                ConnectionError::new(
                    ConnectionErrorCode::InvalidResponse,
                    "Desktop streaming transport could not be initialized.",
                )
            })?;
        Ok(Self {
            client,
            streaming_client,
            max_response_bytes,
            saved_headers: RwLock::default(),
        })
    }

    /// Replaces the headers saved for `origin`.
    pub fn save_headers(&self, origin: &ServerOrigin, headers: ServerHeaders) {
        *self.saved_headers.write().expect("server headers lock") = Some((origin.clone(), headers));
    }

    /// Forgets saved headers that belong to another origin than `origin`.
    pub fn keep_headers_of(&self, origin: &ServerOrigin) {
        let mut saved = self.saved_headers.write().expect("server headers lock");
        if saved.as_ref().is_some_and(|(owner, _)| owner != origin) {
            *saved = None;
        }
    }

    /// The saved headers' names, for showing; their values stay here.
    pub fn header_names(&self, origin: &ServerOrigin) -> Vec<String> {
        self.headers_for(origin)
            .keys()
            .map(|name| name.as_str().to_owned())
            .collect()
    }

    fn headers_for(&self, origin: &ServerOrigin) -> HeaderMap {
        self.saved_headers
            .read()
            .expect("server headers lock")
            .as_ref()
            .filter(|(owner, _)| owner == origin)
            .map(|(_, headers)| headers.0.clone())
            .unwrap_or_default()
    }

    pub async fn test_server(
        &self,
        origin: &ServerOrigin,
    ) -> Result<ConnectionCheck, ConnectionError> {
        self.test_server_with(origin, self.headers_for(origin))
            .await
    }

    /// Tests `origin` with headers that are not saved yet.
    pub async fn test_server_with_headers(
        &self,
        origin: &ServerOrigin,
        headers: &ServerHeaders,
    ) -> Result<ConnectionCheck, ConnectionError> {
        self.test_server_with(origin, headers.0.clone()).await
    }

    async fn test_server_with(
        &self,
        origin: &ServerOrigin,
        headers: HeaderMap,
    ) -> Result<ConnectionCheck, ConnectionError> {
        let response = self
            .client
            .get(origin.endpoint(HEALTH_PATH)?)
            .headers(headers)
            .send()
            .await
            .map_err(|_| {
                ConnectionError::new(
                    ConnectionErrorCode::Unreachable,
                    "Could not reach Music Server. Check that it is running and the URL is correct.",
                )
            })?;
        let status = response.status();
        if status.is_redirection()
            || status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ConnectionError::new(
                ConnectionErrorCode::AccessDenied,
                "The server or a proxy in front of it refused this client. Check the custom headers and whether they have expired.",
            ));
        }
        if !response.status().is_success() {
            return Err(ConnectionError::new(
                ConnectionErrorCode::InvalidResponse,
                format!(
                    "Music Server health check returned HTTP {}.",
                    response.status().as_u16()
                ),
            ));
        }
        let health = response.json::<HealthResponse>().await.map_err(|_| {
            ConnectionError::new(
                ConnectionErrorCode::InvalidResponse,
                "Music Server health response is not valid JSON.",
            )
        })?;
        if health.status != "ok" {
            return Err(ConnectionError::new(
                ConnectionErrorCode::InvalidResponse,
                "Music Server health response did not report an ok status.",
            ));
        }
        let missing = REQUIRED_SERVER_CAPABILITIES
            .iter()
            .filter(|required| {
                !health
                    .capabilities
                    .iter()
                    .any(|available| available == **required)
            })
            .copied()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(ConnectionError::new(
                ConnectionErrorCode::CapabilityMismatch,
                format!(
                    "Music Server is missing required capabilities: {}. Update the Music Server before connecting.",
                    missing.join(", ")
                ),
            ));
        }

        Ok(ConnectionCheck {
            origin: origin.as_str().to_owned(),
            version: health.version,
            capabilities: health.capabilities,
        })
    }

    pub async fn send(
        &self,
        origin: &ServerOrigin,
        request: HttpRequest,
    ) -> Result<HttpResponse, ConnectionError> {
        let url = self.restricted_url(origin, &request.url)?;
        let method = reqwest::Method::from_bytes(request.method.as_bytes()).map_err(|_| {
            ConnectionError::new(
                ConnectionErrorCode::InvalidRequest,
                "Desktop HTTP request method is invalid.",
            )
        })?;
        if !matches!(
            method,
            reqwest::Method::GET
                | reqwest::Method::POST
                | reqwest::Method::PUT
                | reqwest::Method::PATCH
                | reqwest::Method::DELETE
                | reqwest::Method::HEAD
        ) {
            return Err(ConnectionError::new(
                ConnectionErrorCode::InvalidRequest,
                "Desktop HTTP request method is not allowed.",
            ));
        }

        let timeout = api_request_timeout(&method, url.path());
        let mut outgoing = match timeout {
            Some(timeout) => self.client.request(method, url).timeout(timeout),
            None => self.streaming_client.request(method, url),
        };
        for (name, value) in request.headers {
            if !ALLOWED_REQUEST_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
                return Err(ConnectionError::new(
                    ConnectionErrorCode::InvalidRequest,
                    format!("Desktop HTTP request header {name} is not allowed."),
                ));
            }
            outgoing = outgoing.header(name, value);
        }
        outgoing = outgoing.headers(self.headers_for(origin));
        if let Some(body) = request.body {
            outgoing = outgoing.body(body);
        }

        let response = outgoing.send().await.map_err(|_| {
            ConnectionError::new(
                ConnectionErrorCode::Unreachable,
                "Music Server request failed. Check that the server is still running.",
            )
        })?;
        self.collect_response(response).await
    }

    /// Sends like [`Self::send`] until `cancellation` fires, then drops the
    /// request: its connection closes, so the Music Server stops the handler.
    pub async fn send_cancellable(
        &self,
        origin: &ServerOrigin,
        request: HttpRequest,
        cancellation: &CancellationToken,
    ) -> Result<HttpResponse, ConnectionError> {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(ConnectionError::new(
                ConnectionErrorCode::Cancelled,
                "Desktop HTTP request was cancelled.",
            )),
            response = self.send(origin, request) => response,
        }
    }

    pub async fn send_import(
        &self,
        origin: &ServerOrigin,
        path: &str,
        filename: &str,
        content_type: &str,
        content_length: u64,
        body: reqwest::Body,
    ) -> Result<HttpResponse, ConnectionError> {
        let encoded_filename =
            url::form_urlencoded::byte_serialize(filename.as_bytes()).collect::<String>();
        let response = self
            .streaming_client
            .put(origin.endpoint(path)?)
            .headers(self.headers_for(origin))
            .header("accept", "application/json")
            .header("content-type", content_type)
            .header("content-length", content_length)
            .header("x-import-filename", encoded_filename)
            .header("x-import-filename-encoding", "url")
            .body(body)
            .send()
            .await
            .map_err(|_| {
                ConnectionError::new(
                    ConnectionErrorCode::Unreachable,
                    "Managed Import upload could not reach the Music Server.",
                )
            })?;
        if response.status().is_redirection() {
            return Err(ConnectionError::new(
                ConnectionErrorCode::InvalidResponse,
                "Managed Import redirects are disabled to preserve the configured exact origin.",
            ));
        }
        self.collect_response(response).await
    }

    async fn collect_response(
        &self,
        response: reqwest::Response,
    ) -> Result<HttpResponse, ConnectionError> {
        if response
            .content_length()
            .is_some_and(|size| size > self.max_response_bytes as u64)
        {
            return Err(response_too_large_error());
        }
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_owned(), value.to_owned()))
            })
            .collect();
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| {
                ConnectionError::new(
                    ConnectionErrorCode::InvalidResponse,
                    "Music Server response body could not be read.",
                )
            })?;
            if chunk.len() > self.max_response_bytes.saturating_sub(body.len()) {
                return Err(response_too_large_error());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }

    pub async fn send_media(
        &self,
        origin: &ServerOrigin,
        request: HttpRequest,
    ) -> Result<reqwest::Response, ConnectionError> {
        let url = self.restricted_url(origin, &request.url)?;
        let method = reqwest::Method::from_bytes(request.method.as_bytes()).map_err(|_| {
            ConnectionError::new(
                ConnectionErrorCode::InvalidRequest,
                "Desktop media request method is invalid.",
            )
        })?;
        if !matches!(method, reqwest::Method::GET | reqwest::Method::HEAD) {
            return Err(ConnectionError::new(
                ConnectionErrorCode::InvalidRequest,
                "Desktop media requests allow only GET and HEAD.",
            ));
        }
        if request.body.is_some() {
            return Err(ConnectionError::new(
                ConnectionErrorCode::InvalidRequest,
                "Desktop media requests cannot include a body.",
            ));
        }

        let mut outgoing = self.streaming_client.request(method, url);
        for (name, value) in request.headers {
            if !ALLOWED_REQUEST_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
                return Err(ConnectionError::new(
                    ConnectionErrorCode::InvalidRequest,
                    format!("Desktop media request header {name} is not allowed."),
                ));
            }
            outgoing = outgoing.header(name, value);
        }
        let response = outgoing
            .headers(self.headers_for(origin))
            .send()
            .await
            .map_err(|_| {
                ConnectionError::new(
                    ConnectionErrorCode::Unreachable,
                    "Music Server media request failed. Check that the server is still running.",
                )
            })?;
        if response.status().is_redirection() {
            return Err(ConnectionError::new(
                ConnectionErrorCode::InvalidResponse,
                "Music Server media redirects are disabled to preserve the configured exact origin.",
            ));
        }
        Ok(response)
    }

    pub async fn open_queue_event_stream(
        &self,
        origin: &ServerOrigin,
        last_event_id: Option<&str>,
    ) -> Result<reqwest::Response, ConnectionError> {
        let url = self.restricted_url(origin, QUEUE_EVENTS_PATH)?;
        let mut request = self
            .streaming_client
            .get(url)
            .headers(self.headers_for(origin))
            .header(reqwest::header::ACCEPT, "text/event-stream");
        if let Some(last_event_id) = last_event_id {
            request = request.header("Last-Event-ID", last_event_id);
        }
        let response = request.send().await.map_err(|_| {
            ConnectionError::new(
                ConnectionErrorCode::Unreachable,
                "Music Server Queue event stream is unreachable.",
            )
        })?;
        let is_event_stream = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"));
        if !response.status().is_success() || !is_event_stream {
            return Err(ConnectionError::new(
                ConnectionErrorCode::InvalidResponse,
                "Music Server Queue event stream returned an invalid response.",
            ));
        }
        Ok(response)
    }

    fn restricted_url(
        &self,
        origin: &ServerOrigin,
        requested: &str,
    ) -> Result<Url, ConnectionError> {
        let url = if requested.starts_with('/') {
            origin.endpoint(requested)?
        } else {
            Url::parse(requested).map_err(|_| {
                ConnectionError::new(
                    ConnectionErrorCode::InvalidRequest,
                    "Desktop HTTP request URL is invalid.",
                )
            })?
        };
        let has_credentials = !url.username().is_empty() || url.password().is_some();
        let is_exact_origin = url.origin() == origin.url.origin();
        if has_credentials || url.fragment().is_some() || !is_exact_origin {
            return Err(ConnectionError::new(
                ConnectionErrorCode::InvalidOrigin,
                "Desktop HTTP requests are restricted to the configured Music Server origin.",
            ));
        }
        Ok(url)
    }
}

/// The deadline for one renderer API request sent through
/// [`HttpBridge::send`], or `None` when it waits for the Music Server's answer
/// however long it takes. Media goes through the media proxy instead, which
/// has no overall deadline.
fn api_request_timeout(method: &reqwest::Method, path: &str) -> Option<Duration> {
    // Batch confirmation rechecks and commits every selected file before
    // responding, so it can outlast the ordinary API request deadline.
    if method == reqwest::Method::POST
        && path.starts_with("/api/v1/import-batches/")
        && path.ends_with("/confirm")
    {
        return None;
    }
    let segments = path.split('/').collect::<Vec<_>>();
    let is_external_source_lookup = match segments.as_slice() {
        ["", "api", "v1", "external", "search"] => true,
        ["", "api", "v1", "external", "tracks", track_id, "playback"] => !track_id.is_empty(),
        _ => false,
    };
    if is_external_source_lookup {
        Some(EXTERNAL_SOURCE_REQUEST_TIMEOUT)
    } else {
        Some(API_REQUEST_TIMEOUT)
    }
}

/// Cancellation of in-flight renderer HTTP requests, by the renderer's
/// request ID. A cancel for an ID that is not in flight does nothing, so the
/// shell registers a request before it reads the next command (see `run`).
#[derive(Default)]
pub struct HttpRequestCancellations {
    tokens: Mutex<HashMap<String, CancellationToken>>,
}

impl HttpRequestCancellations {
    /// The request's token, created on first use.
    pub fn begin(&self, request_id: &str) -> CancellationToken {
        self.lock()
            .entry(request_id.to_owned())
            .or_default()
            .clone()
    }

    pub fn cancel(&self, request_id: &str) {
        if let Some(token) = self.lock().get(request_id) {
            token.cancel();
        }
    }

    pub fn finish(&self, request_id: &str) {
        self.lock().remove(request_id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, CancellationToken>> {
        // The map stays consistent even if a holder panicked.
        self.tokens
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn response_too_large_error() -> ConnectionError {
    ConnectionError::new(
        ConnectionErrorCode::ResponseTooLarge,
        "Music Server response exceeded the Desktop HTTP bridge safety limit. Request a smaller media response.",
    )
}

#[derive(Deserialize, Serialize)]
struct PersistedConnection {
    origin: String,
}

pub struct ConnectionStore {
    path: PathBuf,
}

impl ConnectionStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<Option<ServerOrigin>, ConnectionError> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Err(ConnectionError::new(
                    ConnectionErrorCode::Storage,
                    "Saved Music Server connection could not be read.",
                ));
            }
        };
        let saved = serde_json::from_str::<PersistedConnection>(&contents).map_err(|_| {
            ConnectionError::new(
                ConnectionErrorCode::Storage,
                "Saved Music Server connection is invalid. Reconfigure the connection.",
            )
        })?;
        ServerOrigin::parse(&saved.origin).map(Some)
    }

    pub fn save(&self, origin: &ServerOrigin) -> Result<(), ConnectionError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|_| {
                ConnectionError::new(
                    ConnectionErrorCode::Storage,
                    "Music Server connection directory could not be created.",
                )
            })?;
        }
        let contents = serde_json::to_vec_pretty(&PersistedConnection {
            origin: origin.as_str().to_owned(),
        })
        .map_err(|_| {
            ConnectionError::new(
                ConnectionErrorCode::Storage,
                "Music Server connection could not be encoded.",
            )
        })?;
        let temporary_path = self.path.with_extension("tmp");
        fs::write(&temporary_path, contents).map_err(|_| {
            ConnectionError::new(
                ConnectionErrorCode::Storage,
                "Music Server connection could not be saved.",
            )
        })?;
        fs::rename(&temporary_path, &self.path).map_err(|_| {
            ConnectionError::new(
                ConnectionErrorCode::Storage,
                "Music Server connection could not be finalized.",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        API_REQUEST_TIMEOUT, ConnectionErrorCode, ConnectionStore, EXTERNAL_SOURCE_REQUEST_TIMEOUT,
        HttpBridge, HttpRequest, HttpRequestCancellations, MAX_SERVER_HEADERS, ServerHeaders,
        ServerOrigin, api_request_timeout,
    };
    use std::collections::BTreeMap;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::mpsc::{self, Sender};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_util::io::ReaderStream;

    fn serve_health(capabilities: &[&str]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let body = serde_json::json!({
            "status": "ok",
            "version": "0.1.0-test",
            "capabilities": capabilities,
        })
        .to_string();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("read request");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body,
            )
            .expect("write response");
        });
        format!("http://{address}")
    }

    fn serve_chunked_body_without_eof(body: &'static [u8]) -> (String, Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let (release_sender, release_receiver) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("read request");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",
                body.len(),
            )
            .expect("write response headers");
            stream.write_all(body).expect("write response body");
            stream.write_all(b"\r\n").expect("finish oversized chunk");
            stream.flush().expect("flush oversized chunk");
            release_receiver.recv().expect("release test server");
            let _ = stream.write_all(b"0\r\n\r\n");
        });
        (format!("http://{address}"), release_sender)
    }

    fn temporary_store_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("earthly-audio-connection-{unique}.json"))
    }

    #[tokio::test]
    async fn valid_connection_reports_server_capabilities() {
        let origin = serve_health(&[
            "api.v1",
            "playback.queue-events.v1",
            "managed-import-batches.v1",
            "managed-track-deletion.v1",
            "managed-track-replacement.v1",
            "optional.future",
        ]);
        let check = HttpBridge::new()
            .expect("create bridge")
            .test_server(&ServerOrigin::parse(&origin).expect("valid origin"))
            .await
            .expect("connection succeeds");

        assert_eq!(check.version, "0.1.0-test");
        assert_eq!(
            check.capabilities,
            [
                "api.v1",
                "playback.queue-events.v1",
                "managed-import-batches.v1",
                "managed-track-deletion.v1",
                "managed-track-replacement.v1",
                "optional.future"
            ]
        );
    }

    #[tokio::test]
    async fn unreachable_server_returns_actionable_error() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("reserve port");
        let origin = format!("http://{}", listener.local_addr().expect("address"));
        drop(listener);

        let error = HttpBridge::new()
            .expect("create bridge")
            .test_server(&ServerOrigin::parse(&origin).expect("valid origin"))
            .await
            .expect_err("connection should fail");

        assert_eq!(error.code, ConnectionErrorCode::Unreachable);
        assert!(error.message.contains("Could not reach Music Server"));
    }

    #[test]
    fn invalid_origin_rejects_non_loopback_and_url_paths() {
        for value in [
            "http://192.168.1.10:8090",
            "http://127.0.0.1:8090/api",
            "http://user:secret@127.0.0.1:8090",
            "file:///tmp/server",
            "http://127.0.0.1.attacker.example:8090",
            "http://localhost.attacker.example:8090",
            "http://127.0.0.1:8090?redirect=1",
            "http://127.0.0.1:8090#fragment",
            "ws://127.0.0.1:8090",
            "http://attacker.example@127.0.0.1:8090",
            "http://music.example.com",
            "https://music.example.com/path",
            "https://user:secret@music.example.com",
            "https://music.example.com?x=1",
        ] {
            let error = ServerOrigin::parse(value).expect_err("origin should fail");
            assert_eq!(error.code, ConnectionErrorCode::InvalidOrigin, "{value}");
        }
    }

    #[test]
    fn an_https_origin_may_be_anywhere() {
        let origin = ServerOrigin::parse("https://music.example.com/").expect("valid origin");

        assert_eq!(origin.as_str(), "https://music.example.com");
    }

    #[test]
    fn custom_headers_refuse_reserved_and_malformed_names() {
        let parse = |name: &str, value: &str| {
            ServerHeaders::parse(&BTreeMap::from([(name.to_owned(), value.to_owned())]))
        };
        assert!(parse("X-Proxy-Key", "secret").is_ok());
        for name in ["Host", "content-length", "Transfer-Encoding", "not a name"] {
            let error = parse(name, "x").expect_err("name should fail");
            assert_eq!(error.code, ConnectionErrorCode::InvalidRequest, "{name}");
        }
        let error = parse("X-Proxy-Key", "secret\nvalue").expect_err("value should fail");
        assert!(!error.message.contains("secret"), "{}", error.message);
        let too_many = (0..=MAX_SERVER_HEADERS)
            .map(|index| (format!("x-{index}"), "v".to_owned()))
            .collect();
        assert!(ServerHeaders::parse(&too_many).is_err());
    }

    /// A Music Server that records each request's custom header and answers
    /// `status` to all of them.
    async fn recording_server(status: &'static str) -> (String, Arc<Mutex<Vec<Option<String>>>>) {
        let seen: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
        let recorded = seen.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let origin = format!("http://{}", listener.local_addr().expect("address"));
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.expect("accept");
                let mut buffer = vec![0; 4096];
                let read = socket.read(&mut buffer).await.expect("read");
                let head = String::from_utf8_lossy(&buffer[..read]).to_ascii_lowercase();
                recorded.lock().expect("seen").push(
                    head.lines()
                        .find_map(|line| line.strip_prefix("x-proxy-key: "))
                        .map(|value| value.trim().to_owned()),
                );
                let body = r#"{"status":"ok","version":"1","capabilities":["api.v1","playback.queue-events.v1","managed-import-batches.v1","managed-track-deletion.v1","managed-track-replacement.v1"]}"#;
                let reply = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(reply.as_bytes()).await.expect("write");
            }
        });
        (origin, seen)
    }

    fn proxy_key() -> ServerHeaders {
        ServerHeaders::parse(&BTreeMap::from([(
            "X-Proxy-Key".to_owned(),
            "secret".to_owned(),
        )]))
        .expect("headers")
    }

    #[tokio::test]
    async fn saved_headers_reach_their_own_origin_and_no_other() {
        let (saved, saved_seen) = recording_server("200 OK").await;
        let (other, other_seen) = recording_server("200 OK").await;
        let saved = ServerOrigin::parse(&saved).expect("origin");
        let other = ServerOrigin::parse(&other).expect("origin");
        let bridge = HttpBridge::new().expect("bridge");
        bridge.save_headers(&saved, proxy_key());

        bridge.test_server(&saved).await.expect("saved origin");
        bridge
            .send(
                &saved,
                HttpRequest {
                    method: "GET".to_owned(),
                    url: "/api/v1/me".to_owned(),
                    headers: BTreeMap::new(),
                    body: None,
                },
            )
            .await
            .expect("api request");
        bridge
            .send_media(
                &saved,
                HttpRequest {
                    method: "GET".to_owned(),
                    url: "/api/v1/tracks/a/stream".to_owned(),
                    headers: BTreeMap::new(),
                    body: None,
                },
            )
            .await
            .expect("media request");
        bridge.test_server(&other).await.expect("other origin");
        assert_eq!(bridge.header_names(&saved), ["x-proxy-key"]);
        assert!(bridge.header_names(&other).is_empty());

        let key = Some("secret".to_owned());
        assert_eq!(
            *saved_seen.lock().expect("seen"),
            vec![key.clone(), key.clone(), key]
        );
        assert_eq!(*other_seen.lock().expect("seen"), vec![None]);
    }

    #[tokio::test]
    async fn changing_the_origin_forgets_the_old_servers_headers() {
        let (first, _) = recording_server("200 OK").await;
        let (second, seen) = recording_server("200 OK").await;
        let first = ServerOrigin::parse(&first).expect("origin");
        let second = ServerOrigin::parse(&second).expect("origin");
        let bridge = HttpBridge::new().expect("bridge");
        bridge.save_headers(&first, proxy_key());

        bridge.keep_headers_of(&first);
        assert!(!bridge.headers_for(&first).is_empty());
        bridge.keep_headers_of(&second);
        bridge.test_server(&second).await.expect("second origin");

        assert!(bridge.headers_for(&first).is_empty());
        assert_eq!(*seen.lock().expect("seen"), vec![None]);
    }

    #[tokio::test]
    async fn a_refusing_proxy_is_not_reported_as_an_unreachable_server() {
        for status in ["302 Found", "401 Unauthorized", "403 Forbidden"] {
            let (origin, _) = recording_server(status).await;
            let origin = ServerOrigin::parse(&origin).expect("origin");
            let error = HttpBridge::new()
                .expect("bridge")
                .test_server(&origin)
                .await
                .expect_err("refused");
            assert_eq!(error.code, ConnectionErrorCode::AccessDenied, "{status}");
        }
    }

    #[test]
    fn localhost_origin_is_canonicalized_to_literal_loopback() {
        let origin = ServerOrigin::parse("http://localhost:8090").expect("valid origin");

        assert_eq!(origin.as_str(), "http://127.0.0.1:8090");
    }

    #[tokio::test]
    async fn capability_mismatch_names_missing_capability() {
        let origin = serve_health(&["optional.future"]);
        let error = HttpBridge::new()
            .expect("create bridge")
            .test_server(&ServerOrigin::parse(&origin).expect("valid origin"))
            .await
            .expect_err("capability check should fail");

        assert_eq!(error.code, ConnectionErrorCode::CapabilityMismatch);
        assert!(error.message.contains("api.v1"));
    }

    #[tokio::test]
    async fn queue_events_capability_is_required() {
        let origin = serve_health(&["api.v1"]);
        let error = HttpBridge::new()
            .expect("create bridge")
            .test_server(&ServerOrigin::parse(&origin).expect("valid origin"))
            .await
            .expect_err("Queue events capability should be required");

        assert_eq!(error.code, ConnectionErrorCode::CapabilityMismatch);
        assert!(error.message.contains("playback.queue-events.v1"));
    }

    #[tokio::test]
    async fn managed_import_batches_capability_is_required() {
        let origin = serve_health(&["api.v1", "playback.queue-events.v1"]);
        let error = HttpBridge::new()
            .expect("create bridge")
            .test_server(&ServerOrigin::parse(&origin).expect("valid origin"))
            .await
            .expect_err("Managed Import Batches capability should be required");

        assert_eq!(error.code, ConnectionErrorCode::CapabilityMismatch);
        assert!(error.message.contains("managed-import-batches.v1"));
    }

    #[tokio::test]
    async fn managed_track_replacement_capability_is_required() {
        let origin = serve_health(&[
            "api.v1",
            "playback.queue-events.v1",
            "managed-import-batches.v1",
            "managed-track-deletion.v1",
        ]);
        let error = HttpBridge::new()
            .expect("create bridge")
            .test_server(&ServerOrigin::parse(&origin).expect("valid origin"))
            .await
            .expect_err("Track Replacement capability should be required");

        assert_eq!(error.code, ConnectionErrorCode::CapabilityMismatch);
        assert!(error.message.contains("managed-track-replacement.v1"));
    }

    #[tokio::test]
    async fn http_bridge_forwards_track_replacement_confirmation_header() {
        let origin = serve_health(&["api.v1"]);
        let configured = ServerOrigin::parse(&origin).expect("valid origin");
        let request = HttpRequest {
            method: "POST".to_owned(),
            url: "/api/v1/imports/import-1/replacement".to_owned(),
            headers: BTreeMap::from([
                ("content-type".to_owned(), "application/json".to_owned()),
                ("x-track-replacement".to_owned(), "1".to_owned()),
            ]),
            body: Some(br#"{"revision":2,"confirmationToken":"token"}"#.to_vec()),
        };

        let response = HttpBridge::new()
            .expect("create bridge")
            .send(&configured, request)
            .await
            .expect("Track Replacement request succeeds");

        assert_eq!(response.status, 200);
    }

    #[tokio::test]
    async fn managed_track_deletion_capability_is_required() {
        let origin = serve_health(&[
            "api.v1",
            "playback.queue-events.v1",
            "managed-import-batches.v1",
        ]);
        let error = HttpBridge::new()
            .expect("create bridge")
            .test_server(&ServerOrigin::parse(&origin).expect("valid origin"))
            .await
            .expect_err("Permanent Track Deletion capability should be required");

        assert_eq!(error.code, ConnectionErrorCode::CapabilityMismatch);
        assert!(error.message.contains("managed-track-deletion.v1"));
    }

    #[test]
    fn connection_store_round_trips_origin() {
        let path = temporary_store_path();
        let store = ConnectionStore::new(path.clone());
        let origin = ServerOrigin::parse("http://127.0.0.1:8090").expect("valid origin");

        store.save(&origin).expect("save origin");
        assert_eq!(store.load().expect("load origin"), Some(origin));

        std::fs::remove_file(path).expect("remove test store");
    }

    #[tokio::test]
    async fn http_bridge_rejects_request_to_different_origin() {
        let configured = ServerOrigin::parse("http://127.0.0.1:8090").expect("valid origin");
        let request = HttpRequest {
            method: "GET".to_owned(),
            url: "http://127.0.0.1:8091/api/v1/health".to_owned(),
            headers: Default::default(),
            body: None,
        };

        let error = HttpBridge::new()
            .expect("create bridge")
            .send(&configured, request)
            .await
            .expect_err("foreign origin should fail");

        assert_eq!(error.code, ConnectionErrorCode::InvalidOrigin);
    }

    #[tokio::test]
    async fn http_bridge_rejects_lookalike_destinations_for_exact_origin() {
        let configured = ServerOrigin::parse("http://127.0.0.1:8090").expect("valid origin");
        for url in [
            "https://127.0.0.1:8090/api/v1/health",
            "http://localhost:8090/api/v1/health",
            "http://127.0.0.1.attacker.example:8090/api/v1/health",
            "//attacker.example/api/v1/health",
            "http://127.0.0.1:8090/api/v1/health#fragment",
            "http://user:secret@127.0.0.1:8090/api/v1/health",
            "file:///etc/passwd",
        ] {
            let request = HttpRequest {
                method: "GET".to_owned(),
                url: url.to_owned(),
                headers: Default::default(),
                body: None,
            };
            let error = HttpBridge::new()
                .expect("create bridge")
                .send(&configured, request)
                .await
                .expect_err(url);
            assert_eq!(error.code, ConnectionErrorCode::InvalidOrigin, "{url}");
        }

        // Dot segments normalize inside the configured origin instead of
        // escaping it.
        let normalized = HttpBridge::new()
            .expect("create bridge")
            .restricted_url(&configured, "/api/v1/../../attacker")
            .expect("normalized path stays on origin");
        assert_eq!(normalized.as_str(), "http://127.0.0.1:8090/attacker");
    }

    #[tokio::test]
    async fn http_bridge_rejects_headers_that_could_redirect_uploads() {
        let configured = ServerOrigin::parse("http://127.0.0.1:8090").expect("valid origin");
        for (name, value) in [
            ("host", "attacker.example"),
            ("cookie", "session=stolen"),
            ("x-forwarded-host", "attacker.example"),
            ("proxy-authorization", "Basic xyz"),
        ] {
            let request = HttpRequest {
                method: "GET".to_owned(),
                url: "/api/v1/health".to_owned(),
                headers: BTreeMap::from([(name.to_owned(), value.to_owned())]),
                body: None,
            };
            let error = HttpBridge::new()
                .expect("create bridge")
                .send(&configured, request)
                .await
                .expect_err(name);
            assert_eq!(error.code, ConnectionErrorCode::InvalidRequest, "{name}");
        }
    }

    #[tokio::test]
    async fn import_bridge_refuses_to_follow_redirects_off_the_configured_origin() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let (accepted_sender, accepted_receiver) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("read request");
            write!(
                stream,
                "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://attacker.example/collect\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .expect("write redirect");
            accepted_sender.send(1_u8).expect("count request");
            // A second connection would mean the client followed the redirect
            // to a different socket on this host; none is expected.
            if listener.accept().is_ok() {
                accepted_sender.send(2_u8).expect("count request");
            }
        });
        let origin = ServerOrigin::parse(&format!("http://{address}")).expect("origin");

        let error = HttpBridge::new()
            .expect("create bridge")
            .send_import(
                &origin,
                "/api/v1/imports/job-1/file",
                "song.flac",
                "audio/flac",
                5,
                reqwest::Body::from("audio"),
            )
            .await
            .expect_err("redirect must be refused");

        assert_eq!(error.code, ConnectionErrorCode::InvalidResponse);
        assert!(error.message.contains("redirect"), "{}", error.message);
        assert_eq!(accepted_receiver.recv().expect("first request"), 1);
        assert!(
            accepted_receiver
                .recv_timeout(Duration::from_millis(200))
                .is_err(),
            "the bridge must not follow the redirect"
        );
    }

    #[tokio::test]
    async fn media_bridge_refuses_redirects_and_non_get_methods() {
        let configured = ServerOrigin::parse("http://127.0.0.1:8090").expect("valid origin");
        let request = HttpRequest {
            method: "POST".to_owned(),
            url: "/api/v1/tracks/track-1/stream".to_owned(),
            headers: Default::default(),
            body: None,
        };
        let error = HttpBridge::new()
            .expect("create bridge")
            .send_media(&configured, request)
            .await
            .expect_err("POST media request must fail");
        assert_eq!(error.code, ConnectionErrorCode::InvalidRequest);

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("read request");
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: http://attacker.example/track.flac\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .expect("write redirect");
        });
        let origin = ServerOrigin::parse(&format!("http://{address}")).expect("origin");
        let request = HttpRequest {
            method: "GET".to_owned(),
            url: "/api/v1/tracks/track-1/stream".to_owned(),
            headers: Default::default(),
            body: None,
        };
        let error = HttpBridge::new()
            .expect("create bridge")
            .send_media(&origin, request)
            .await
            .expect_err("media redirect must be refused");
        assert_eq!(error.code, ConnectionErrorCode::InvalidResponse);
    }

    #[tokio::test]
    async fn http_bridge_forwards_request_to_configured_origin() {
        let origin = serve_health(&["api.v1"]);
        let configured = ServerOrigin::parse(&origin).expect("valid origin");
        let request = HttpRequest {
            method: "GET".to_owned(),
            url: "/api/v1/health".to_owned(),
            headers: BTreeMap::from([("accept".to_owned(), "application/json".to_owned())]),
            body: None,
        };

        let response = HttpBridge::new()
            .expect("create bridge")
            .send(&configured, request)
            .await
            .expect("configured request succeeds");

        assert_eq!(response.status, 200);
        assert!(
            String::from_utf8(response.body)
                .expect("UTF-8 body")
                .contains("api.v1")
        );
    }

    #[tokio::test]
    async fn import_bridge_streams_to_configured_import_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let (request_sender, request_receiver) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .expect("set timeout");
            let mut request = Vec::new();
            let mut chunk = [0_u8; 2048];
            while !request
                .windows(b"audio bytes".len())
                .any(|part| part == b"audio bytes")
            {
                let read = stream.read(&mut chunk).expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
            }
            request_sender.send(request).expect("capture request");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            )
            .expect("write response");
        });
        let fixture = temporary_store_path().with_extension("flac");
        fs::write(&fixture, b"audio bytes").expect("write fixture");
        let file = tokio::fs::File::open(&fixture).await.expect("open fixture");
        let origin = ServerOrigin::parse(&format!("http://{address}")).expect("origin");

        let response = HttpBridge::new()
            .expect("create bridge")
            .send_import(
                &origin,
                "/api/v1/imports/job-1/file",
                "Beyoncé.flac",
                "audio/flac",
                11,
                reqwest::Body::wrap_stream(ReaderStream::new(file)),
            )
            .await
            .expect("upload succeeds");

        let request_bytes = request_receiver.recv().expect("request");
        let request = String::from_utf8_lossy(&request_bytes);
        assert!(request.starts_with("PUT /api/v1/imports/job-1/file HTTP/1.1"));
        assert!(!request.to_ascii_lowercase().contains("transfer-encoding"));
        assert!(request.to_ascii_lowercase().contains("content-length: 11"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("x-import-filename: beyonc%c3%a9.flac")
        );
        assert!(
            request
                .to_ascii_lowercase()
                .contains("x-import-filename-encoding: url")
        );
        assert_eq!(response.status, 200);
        fs::remove_file(fixture).expect("remove fixture");
    }

    #[tokio::test]
    async fn http_bridge_waits_for_slow_import_confirmation() {
        const CONFIRMATION_DELAY: Duration = Duration::from_secs(11);
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept confirmation");
            let mut request = [0_u8; 4096];
            let bytes_read = stream.read(&mut request).expect("read confirmation");
            assert!(bytes_read > 0, "confirmation request must not be empty");
            thread::sleep(CONFIRMATION_DELAY);
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: 22\r\nConnection: close\r\n\r\n{{\"status\":\"completed\"}}")
                .expect("write confirmation result");
        });
        let origin = ServerOrigin::parse(&format!("http://{address}")).expect("origin");
        let response = HttpBridge::new()
            .expect("create bridge")
            .send(
                &origin,
                HttpRequest {
                    method: "POST".to_owned(),
                    url: "/api/v1/import-batches/batch-1/confirm".to_owned(),
                    headers: Default::default(),
                    body: Some(br#"{"revision":45,"selectedFileIds":[]}"#.to_vec()),
                },
            )
            .await
            .expect("a running server must not be reported as unreachable during confirmation");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, br#"{"status":"completed"}"#);
    }

    #[tokio::test]
    async fn http_bridge_forwards_permanent_delete_confirmation_header() {
        let origin = serve_health(&["api.v1"]);
        let configured = ServerOrigin::parse(&origin).expect("valid origin");
        let request = HttpRequest {
            method: "DELETE".to_owned(),
            url: "/api/v1/library/tracks/track-1".to_owned(),
            headers: BTreeMap::from([
                ("content-type".to_owned(), "application/json".to_owned()),
                ("x-permanent-delete".to_owned(), "1".to_owned()),
            ]),
            body: Some(br#"{"confirmationToken":"token"}"#.to_vec()),
        };

        let response = HttpBridge::new()
            .expect("create bridge")
            .send(&configured, request)
            .await
            .expect("Permanent Track Deletion request succeeds");

        assert_eq!(response.status, 200);
    }

    #[tokio::test]
    async fn http_bridge_forwards_queue_revision_preconditions() {
        // Queue Clear and item removal send their Queue Revision as If-Match.
        let origin = serve_health(&["api.v1"]);
        let configured = ServerOrigin::parse(&origin).expect("valid origin");
        let request = HttpRequest {
            method: "DELETE".to_owned(),
            url: "/api/v1/playback/queue?afterItemId=item-1".to_owned(),
            headers: BTreeMap::from([("If-Match".to_owned(), "7".to_owned())]),
            body: None,
        };

        let response = HttpBridge::new()
            .expect("create bridge")
            .send(&configured, request)
            .await
            .expect("Queue Clear request is forwarded");

        assert_eq!(response.status, 200);
    }

    #[tokio::test]
    async fn unknown_length_response_stops_at_hard_limit() {
        let (origin, release_server) = serve_chunked_body_without_eof(b"123456789");
        let configured = ServerOrigin::parse(&origin).expect("valid origin");
        let request = HttpRequest {
            method: "GET".to_owned(),
            url: "/api/v1/library/albums/album-1/cover".to_owned(),
            headers: Default::default(),
            body: None,
        };

        let result = tokio::time::timeout(
            Duration::from_millis(500),
            HttpBridge::with_max_response_bytes(8)
                .expect("create bridge")
                .send(&configured, request),
        )
        .await;
        release_server.send(()).expect("release test server");
        let error = result
            .expect("bridge should reject before end of response")
            .expect_err("response should exceed limit");

        assert_eq!(error.code, ConnectionErrorCode::ResponseTooLarge);
        assert!(error.message.contains("32 MiB") || error.message.contains("limit"));
    }

    #[tokio::test]
    async fn cancelled_request_closes_its_connection_to_the_server() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let (received_sender, received_receiver) = tokio::sync::oneshot::channel();
        let (closed_sender, closed_receiver) = tokio::sync::oneshot::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("read request");
            let _ = received_sender.send(());
            // Never answers, like a slow search; reading 0 bytes means the
            // client closed the connection.
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("set read timeout");
            let closed = matches!(stream.read(&mut request), Ok(0));
            let _ = closed_sender.send(closed);
        });
        let configured = ServerOrigin::parse(&format!("http://{address}")).expect("valid origin");
        let request = HttpRequest {
            method: "GET".to_owned(),
            url: "/api/v1/external/search?q=a".to_owned(),
            headers: Default::default(),
            body: None,
        };
        let cancellations = HttpRequestCancellations::default();
        let cancellation = cancellations.begin("request-1");
        // Kept alive to the end: the cancel alone must close the connection.
        let bridge = HttpBridge::new().expect("create bridge");

        let sending = bridge.send_cancellable(&configured, request, &cancellation);
        tokio::pin!(sending);
        tokio::select! {
            _ = &mut sending => panic!("the server never answers"),
            received = received_receiver => received.expect("server received the request"),
        }
        cancellations.cancel("request-1");
        let error = tokio::time::timeout(Duration::from_secs(1), sending)
            .await
            .expect("cancel ends the request at once")
            .expect_err("a cancelled request fails");

        assert_eq!(error.code, ConnectionErrorCode::Cancelled);
        assert!(
            tokio::time::timeout(Duration::from_secs(5), closed_receiver)
                .await
                .expect("server sees the connection end")
                .expect("server thread reports"),
            "the outgoing request's connection must be closed"
        );
    }

    #[test]
    fn api_request_timeout_is_chosen_by_path() {
        let get = reqwest::Method::GET;
        let post = reqwest::Method::POST;
        let ordinary = Some(API_REQUEST_TIMEOUT);
        let external = Some(EXTERNAL_SOURCE_REQUEST_TIMEOUT);
        assert!(EXTERNAL_SOURCE_REQUEST_TIMEOUT > API_REQUEST_TIMEOUT);

        assert_eq!(
            api_request_timeout(&get, "/api/v1/external/search"),
            external
        );
        assert_eq!(
            api_request_timeout(&get, "/api/v1/external/tracks/track-1/playback"),
            external
        );
        assert_eq!(
            api_request_timeout(&get, "/api/v1/library/tracks"),
            ordinary
        );
        assert_eq!(
            api_request_timeout(&get, "/api/v1/external/tracks//playback"),
            ordinary
        );
        assert_eq!(
            api_request_timeout(&get, "/api/v1/external/tracks/track-1/playback/extra"),
            ordinary
        );
        assert_eq!(
            api_request_timeout(&get, "/api/v1/external/tracks/track-1/artwork"),
            ordinary
        );
        assert_eq!(
            api_request_timeout(&post, "/api/v1/import-batches/batch-1/confirm"),
            None
        );
        assert_eq!(
            api_request_timeout(&get, "/api/v1/import-batches/batch-1/confirm"),
            ordinary
        );
    }

    #[test]
    fn http_cancellation_reaches_only_in_flight_requests() {
        let cancellations = HttpRequestCancellations::default();
        cancellations.cancel("finished");
        assert!(!cancellations.begin("finished").is_cancelled());
        cancellations.finish("finished");

        let first = cancellations.begin("request-1");
        let other = cancellations.begin("request-2");
        assert!(!cancellations.begin("request-1").is_cancelled());
        cancellations.cancel("request-1");

        assert!(first.is_cancelled());
        assert!(!other.is_cancelled());
        cancellations.finish("request-1");
        assert!(!cancellations.begin("request-1").is_cancelled());
    }
}
