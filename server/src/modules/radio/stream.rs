//! The radio stream proxy (radio/stream.go). It only fetches public HTTP(S)
//! hosts: literal private, loopback and link-local addresses are refused
//! before any request, every name is checked again after DNS resolution,
//! and every redirect is validated. HLS playlists are rewritten so each
//! nested URI goes through this proxy with an HMAC-signed, station-bound,
//! one-hour token; DRM-protected playlists are refused. ICY metadata is
//! stripped from audio and reported as now playing.

use super::types::{NowPlaying, Station};
use crate::respond::ApiError;
use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bytes::{Bytes, BytesMut};
use futures_util::future::BoxFuture;
use futures_util::stream::{BoxStream, StreamExt};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::{Host, Url};

const NOW_PLAYING_STALE_AFTER: Duration = Duration::from_secs(60);
const HLS_CONTENT_TYPE: &str = "application/vnd.apple.mpegurl";
const MAX_HLS_PLAYLIST_BYTES: usize = 1 << 20;
const HLS_RESOURCE_TOKEN_TTL_SECONDS: i64 = 3600;
const UNSAFE: &str = "unsafe stream URL";

/// Refuses anything but public HTTP(S) hosts.
pub fn validate_stream_url(raw: &str) -> Result<(), String> {
    let parsed = match Url::parse(raw) {
        Ok(parsed) => parsed,
        // A relative reference has no scheme, as url.Parse would report.
        Err(url::ParseError::RelativeUrlWithoutBase) => {
            return Err(format!("{UNSAFE}: unsupported scheme"));
        }
        Err(error) => return Err(error.to_string()),
    };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(format!("{UNSAFE}: unsupported scheme"));
    }
    match parsed.host() {
        None => Err(format!("{UNSAFE}: blocked host")),
        Some(Host::Domain(domain)) => {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            if domain.is_empty() || domain == "localhost" {
                Err(format!("{UNSAFE}: blocked host"))
            } else {
                Ok(())
            }
        }
        Some(Host::Ipv4(ip)) if is_blocked_ip(IpAddr::V4(ip)) => {
            Err(format!("{UNSAFE}: blocked IP"))
        }
        Some(Host::Ipv6(ip)) if is_blocked_ip(IpAddr::V6(ip)) => {
            Err(format!("{UNSAFE}: blocked IP"))
        }
        Some(_) => Ok(()),
    }
}

/// Go's IsLoopback || IsPrivate || IsLinkLocalUnicast ||
/// IsLinkLocalMulticast || IsUnspecified, with IPv4-mapped IPv6 read as IPv4.
pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_blocked_v4(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(mapped) => is_blocked_v4(mapped),
            None => is_blocked_v6(ip),
        },
    }
}

fn is_blocked_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || (a == 224 && b == 0 && c == 0)
        || ip.is_unspecified()
}

fn is_blocked_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    ip.is_loopback()
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
        || (first & 0xff0f) == 0xff02
        || ip.is_unspecified()
}

/// Latest now-playing per station, marked stale after a minute.
#[derive(Default)]
pub struct NowPlayingCache {
    items: RwLock<HashMap<String, (NowPlaying, Option<SystemTime>)>>,
}

impl NowPlayingCache {
    pub fn set(&self, station_id: &str, now: NowPlaying, updated: Option<SystemTime>) {
        self.items
            .write()
            .expect("now playing lock")
            .insert(station_id.to_owned(), (now, updated));
    }

    pub fn get(&self, station_id: &str) -> Option<NowPlaying> {
        let items = self.items.read().expect("now playing lock");
        let (now, updated) = items.get(station_id)?;
        let mut now = now.clone();
        if updated.is_some_and(|updated| {
            updated
                .elapsed()
                .is_ok_and(|age| age > NOW_PLAYING_STALE_AFTER)
        }) {
            now.stale = true;
        }
        Some(now)
    }
}

/// Parses one ICY metadata block; `None` when it is empty.
pub fn parse_icy_metadata(raw: &str) -> Option<(NowPlaying, Option<SystemTime>)> {
    let raw = raw.trim_matches(|character| character == '\0' || character == ' ');
    if raw.is_empty() {
        return None;
    }
    let title = parse_stream_title(raw);
    if title.is_empty() {
        return Some((
            NowPlaying {
                raw: raw.to_owned(),
                ..NowPlaying::default()
            },
            None,
        ));
    }
    let (artist, track_title) = match title.split_once(" - ") {
        Some((artist, track)) => (artist.trim().to_owned(), track.trim().to_owned()),
        None => (String::new(), title.clone()),
    };
    let now = SystemTime::now();
    Some((
        NowPlaying {
            title: track_title,
            artist,
            raw: title,
            updated_at: Some(crate::gojson::rfc3339_nano(now)),
            stale: false,
        },
        Some(now),
    ))
}

fn parse_stream_title(raw: &str) -> String {
    const PREFIX: &str = "StreamTitle='";
    let Some(start) = raw.find(PREFIX) else {
        return String::new();
    };
    let rest = &raw[start + PREFIX.len()..];
    match rest.find("';") {
        Some(end) => rest[..end].trim().to_owned(),
        None => rest.trim().to_owned(),
    }
}

/// Strips ICY metadata blocks from an audio stream (`metaint` audio bytes,
/// one length byte ×16, the metadata), reporting each parsed block.
pub struct IcyParser {
    metaint: usize,
    remaining: usize,
    pending: BytesMut,
}

impl IcyParser {
    pub fn new(metaint: usize) -> Self {
        Self {
            metaint,
            remaining: metaint,
            pending: BytesMut::new(),
        }
    }

    /// Audio bytes of `chunk`, with any complete metadata handed to `report`.
    pub fn feed(&mut self, chunk: &[u8], mut report: impl FnMut(&str)) -> Bytes {
        self.pending.extend_from_slice(chunk);
        let mut audio = BytesMut::new();
        loop {
            if self.remaining > 0 {
                let take = self.remaining.min(self.pending.len());
                if take == 0 {
                    break;
                }
                audio.extend_from_slice(&self.pending.split_to(take));
                self.remaining -= take;
                continue;
            }
            let Some(&length_byte) = self.pending.first() else {
                break;
            };
            let length = usize::from(length_byte) * 16;
            if self.pending.len() < 1 + length {
                break;
            }
            let block = self.pending.split_to(1 + length);
            if length > 0 {
                report(&String::from_utf8_lossy(&block[1..]));
            }
            self.remaining = self.metaint;
        }
        audio.freeze()
    }
}

#[derive(Serialize, Deserialize)]
struct ResourceClaims {
    s: String,
    u: String,
    e: i64,
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// HMAC-signed tokens for HLS resources, bound to one station and an hour.
pub struct ResourceSigner {
    key: [u8; 32],
    now: fn() -> i64,
}

impl ResourceSigner {
    pub fn new() -> Self {
        let mut key = [0_u8; 32];
        getrandom_fill(&mut key);
        Self { key, now: unix_now }
    }

    fn mac(&self) -> Hmac<Sha256> {
        Hmac::<Sha256>::new_from_slice(&self.key).expect("HMAC accepts any key length")
    }

    pub fn sign(&self, station_id: &str, resource_url: &str) -> String {
        let claims = ResourceClaims {
            s: station_id.to_owned(),
            u: resource_url.to_owned(),
            e: (self.now)() + HLS_RESOURCE_TOKEN_TTL_SECONDS,
        };
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims encode"));
        let mut mac = self.mac();
        mac.update(payload.as_bytes());
        format!(
            "{payload}.{}",
            URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        )
    }

    pub fn verify(&self, token: &str, station_id: &str) -> Result<String, String> {
        const INVALID: &str = "invalid HLS resource token";
        let (payload, signature) = token.split_once('.').ok_or(INVALID)?;
        let signature = URL_SAFE_NO_PAD.decode(signature).map_err(|_| INVALID)?;
        let mut mac = self.mac();
        mac.update(payload.as_bytes());
        mac.verify_slice(&signature).map_err(|_| INVALID)?;
        let payload = URL_SAFE_NO_PAD.decode(payload).map_err(|_| INVALID)?;
        let claims: ResourceClaims = serde_json::from_slice(&payload).map_err(|_| INVALID)?;
        if claims.s != station_id || claims.e <= (self.now)() {
            return Err(INVALID.to_owned());
        }
        validate_stream_url(&claims.u).map_err(|error| format!("{INVALID}: {error}"))?;
        Ok(claims.u)
    }
}

impl Default for ResourceSigner {
    fn default() -> Self {
        Self::new()
    }
}

fn getrandom_fill(buffer: &mut [u8]) {
    // Two v4 UUIDs are 32 bytes of the OS CSPRNG through the uuid crate.
    for chunk in buffer.chunks_mut(16) {
        chunk.copy_from_slice(&uuid::Uuid::new_v4().as_bytes()[..chunk.len()]);
    }
}

#[derive(Debug, PartialEq)]
pub enum RewriteError {
    Drm,
    Invalid(String),
}

impl std::fmt::Display for RewriteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RewriteError::Drm => formatter.write_str("DRM-protected HLS is unsupported"),
            RewriteError::Invalid(message) => formatter.write_str(message),
        }
    }
}

fn has_unsupported_protection(line: &str) -> bool {
    let upper = line.to_uppercase();
    if !upper.starts_with("#EXT-X-KEY:") && !upper.starts_with("#EXT-X-SESSION-KEY:") {
        return false;
    }
    if upper.contains("METHOD=SAMPLE-AES") {
        return true;
    }
    let Some(index) = upper.find("KEYFORMAT=") else {
        return false;
    };
    let key_format = line[index + "KEYFORMAT=".len()..]
        .split(',')
        .next()
        .unwrap_or_default()
        .trim_matches(|character| character == '"' || character == ' ');
    !key_format.eq_ignore_ascii_case("identity")
}

/// Rewrites every URI of a playlist to `proxy_path?resource=<token>`.
pub fn rewrite_playlist(
    playlist: &str,
    base: &Url,
    proxy_path: &str,
    station_id: &str,
    signer: &ResourceSigner,
) -> Result<String, RewriteError> {
    if !playlist.trim().starts_with("#EXTM3U") {
        return Err(RewriteError::Invalid(
            "invalid HLS playlist: missing EXTM3U header".to_owned(),
        ));
    }
    let proxy = |reference: &str| -> Result<String, RewriteError> {
        let resolved = base
            .join(reference)
            .map_err(|error| RewriteError::Invalid(format!("invalid HLS playlist URL: {error}")))?;
        validate_stream_url(resolved.as_str())
            .map_err(|error| RewriteError::Invalid(format!("invalid HLS playlist URL: {error}")))?;
        Ok(format!(
            "{proxy_path}?resource={}",
            signer.sign(station_id, resolved.as_str())
        ))
    };
    let mut lines: Vec<String> = Vec::new();
    for line in playlist.split('\n') {
        if line.starts_with('#') {
            if has_unsupported_protection(line) {
                return Err(RewriteError::Drm);
            }
            lines.push(rewrite_uri_attributes(line, &proxy)?);
        } else if line.trim().is_empty() {
            lines.push(line.to_owned());
        } else {
            lines.push(proxy(line.trim())?);
        }
    }
    Ok(lines.join("\n"))
}

fn rewrite_uri_attributes(
    line: &str,
    proxy: &impl Fn(&str) -> Result<String, RewriteError>,
) -> Result<String, RewriteError> {
    const MARKER: &str = "URI=\"";
    let mut line = line.to_owned();
    let mut offset = 0;
    while let Some(found) = line[offset..].find(MARKER) {
        let start = offset + found + MARKER.len();
        let end = line[start..]
            .find('"')
            .map(|end| start + end)
            .ok_or_else(|| {
                RewriteError::Invalid("invalid HLS playlist: unterminated URI attribute".to_owned())
            })?;
        let rewritten = proxy(&line[start..end])?;
        line = format!("{}{}{}", &line[..start], rewritten, &line[end..]);
        offset = start + rewritten.len() + 1;
    }
    Ok(line)
}

/// One upstream GET and its response, final URL included (after redirects).
pub struct UpstreamResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub url: Url,
    pub body: BoxStream<'static, Result<Bytes, String>>,
}

/// Fetches stream URLs; the production one is [`SafeUpstream`], tests
/// substitute their own.
pub trait Upstream: Send + Sync {
    fn get(
        &self,
        url: Url,
        range: Option<HeaderValue>,
    ) -> BoxFuture<'static, Result<UpstreamResponse, String>>;
}

/// Refuses names that resolve to a blocked address, checking every address
/// before connecting to any.
struct SafeResolver;

impl reqwest::dns::Resolve for SafeResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addresses: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|error| -> Box<dyn std::error::Error + Send + Sync> {
                    format!("resolve stream host: {error}").into()
                })?
                .collect();
            if addresses.iter().any(|address| is_blocked_ip(address.ip())) {
                return Err(format!("{UNSAFE}: resolved to blocked IP").into());
            }
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

pub struct SafeUpstream {
    client: reqwest::Client,
}

impl SafeUpstream {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .no_proxy()
            .dns_resolver(Arc::new(SafeResolver))
            .connect_timeout(Duration::from_secs(30))
            .tcp_keepalive(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 {
                    return attempt.error("too many redirects");
                }
                match validate_stream_url(attempt.url().as_str()) {
                    Ok(()) => attempt.follow(),
                    Err(error) => attempt.error(error),
                }
            }))
            .build()
            .expect("stream client builds");
        Self { client }
    }
}

impl Default for SafeUpstream {
    fn default() -> Self {
        Self::new()
    }
}

impl Upstream for SafeUpstream {
    fn get(
        &self,
        url: Url,
        range: Option<HeaderValue>,
    ) -> BoxFuture<'static, Result<UpstreamResponse, String>> {
        let mut request = self.client.get(url.clone()).header("Icy-MetaData", "1");
        if let Some(range) = range {
            request = request.header(header::RANGE, range);
        }
        Box::pin(async move {
            let response = request
                .send()
                .await
                .map_err(|error| describe_request_error(&url, &error))?;
            Ok(UpstreamResponse {
                status: response.status().as_u16(),
                headers: response.headers().clone(),
                url: response.url().clone(),
                body: response
                    .bytes_stream()
                    .map(|chunk| chunk.map_err(|error| error.to_string()))
                    .boxed(),
            })
        })
    }
}

/// `Get "<url>": <cause>`, the shape of Go's client errors.
fn describe_request_error(url: &Url, error: &reqwest::Error) -> String {
    let mut cause = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(inner) = source {
        cause = inner.to_string();
        source = inner.source();
    }
    format!("Get \"{url}\": {cause}")
}

/// What the proxy reports while an audio stream runs.
pub trait NowPlayingSink: Send + Sync {
    fn now_playing(&self, station_id: &str, now: NowPlaying, updated: Option<SystemTime>);
}

pub struct StreamProxy {
    upstream: Arc<dyn Upstream>,
    signer: ResourceSigner,
    sink: Arc<dyn NowPlayingSink>,
}

/// The parts of the client's request the proxy needs.
pub struct StreamRequest<'a> {
    pub path: &'a str,
    pub query: &'a crate::query::GoQuery,
    pub range: Option<HeaderValue>,
}

/// Upstream media types a stream may keep. Anything else, such as
/// `text/html`, is served as `application/octet-stream`: the proxy answers
/// on this server's origin, so a station must not hand the browser a page.
fn is_stream_content_type(value: &HeaderValue) -> bool {
    let Ok(value) = value.to_str() else {
        return false;
    };
    let essence = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    essence.starts_with("audio/")
        || matches!(
            essence.as_str(),
            "video/mp2t"
                | "application/ogg"
                | "application/aac"
                | "application/octet-stream"
                | HLS_CONTENT_TYPE
                | "application/x-mpegurl"
        )
}

fn copy_stream_headers(from: &HeaderMap) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let content_type = from
        .get(header::CONTENT_TYPE)
        .filter(|value| is_stream_content_type(value))
        .cloned()
        .unwrap_or(HeaderValue::from_static("application/octet-stream"));
    headers.insert(header::CONTENT_TYPE, content_type);
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox"),
    );
    for name in [
        "cache-control",
        "content-range",
        "accept-ranges",
        "icy-name",
        "icy-genre",
        "icy-br",
        "icy-url",
    ] {
        if let Some(value) = from.get(name).filter(|value| !value.is_empty()) {
            headers.insert(name, value.clone());
        }
    }
    headers
}

fn is_hls_playlist(response: &UpstreamResponse) -> bool {
    let content_type = response
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    matches!(
        content_type.as_str(),
        HLS_CONTENT_TYPE | "application/x-mpegurl" | "audio/mpegurl" | "audio/x-mpegurl"
    ) || response.url.path().to_ascii_lowercase().ends_with(".m3u8")
}

impl StreamProxy {
    pub fn new(upstream: Arc<dyn Upstream>, sink: Arc<dyn NowPlayingSink>) -> Self {
        Self {
            upstream,
            signer: ResourceSigner::new(),
            sink,
        }
    }

    pub async fn stream(&self, request: StreamRequest<'_>, station: &Station) -> Response {
        if request.query.values("url").is_some() {
            return ApiError::bad_request("invalid HLS resource token").into_response();
        }
        let is_resource = !request.query.get("resource").is_empty();
        let stream_url = if is_resource {
            match self
                .signer
                .verify(request.query.get("resource"), &station.id)
            {
                Ok(url) => url,
                Err(error) => return ApiError::bad_request(error).into_response(),
            }
        } else {
            station.stream_url.clone()
        };
        if let Err(error) = validate_stream_url(&stream_url) {
            return ApiError::bad_request(error).into_response();
        }
        let url = match Url::parse(&stream_url) {
            Ok(url) => url,
            Err(error) => return ApiError::bad_request(error.to_string()).into_response(),
        };
        let response = match self.upstream.get(url, request.range).await {
            Ok(response) => response,
            Err(error) => {
                return ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", error)
                    .into_response();
            }
        };
        if !(200..300).contains(&response.status) {
            return ApiError::new(
                StatusCode::BAD_GATEWAY,
                "bad_gateway",
                format!("stream status {}", response.status),
            )
            .into_response();
        }
        if is_hls_playlist(&response) {
            return self.playlist(response, request.path, &station.id).await;
        }
        self.audio(response, station.id.clone(), is_resource)
    }

    async fn playlist(
        &self,
        response: UpstreamResponse,
        proxy_path: &str,
        station_id: &str,
    ) -> Response {
        let headers = copy_stream_headers(&response.headers);
        let base = response.url.clone();
        let mut body = response.body;
        let mut playlist = Vec::new();
        while let Some(chunk) = body.next().await {
            let Ok(chunk) = chunk else {
                return ApiError::new(
                    StatusCode::BAD_GATEWAY,
                    "bad_gateway",
                    "failed to read HLS playlist",
                )
                .into_response();
            };
            playlist.extend_from_slice(&chunk);
            if playlist.len() > MAX_HLS_PLAYLIST_BYTES {
                return ApiError::new(
                    StatusCode::BAD_GATEWAY,
                    "bad_gateway",
                    "HLS playlist exceeds size limit",
                )
                .into_response();
            }
        }
        let text = String::from_utf8_lossy(&playlist);
        match rewrite_playlist(&text, &base, proxy_path, station_id, &self.signer) {
            Ok(rewritten) => (StatusCode::OK, headers, rewritten).into_response(),
            Err(RewriteError::Drm) => ApiError::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media_type",
                RewriteError::Drm.to_string(),
            )
            .into_response(),
            Err(error) => ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", error.to_string())
                .into_response(),
        }
    }

    fn audio(&self, response: UpstreamResponse, station_id: String, is_resource: bool) -> Response {
        let headers = copy_stream_headers(&response.headers);
        let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
        let metaint = response
            .headers
            .get("icy-metaint")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|metaint| *metaint > 0);
        let sink = self.sink.clone();
        let body = match metaint {
            None => response.body,
            Some(metaint) => {
                let mut parser = IcyParser::new(metaint);
                let id = station_id.clone();
                response
                    .body
                    .map(move |chunk| {
                        chunk.map(|chunk| {
                            parser.feed(&chunk, |metadata| {
                                if let Some((now, updated)) = parse_icy_metadata(metadata) {
                                    sink.now_playing(&id, now, updated);
                                }
                            })
                        })
                    })
                    .boxed()
            }
        };
        let logged = LoggedStream {
            inner: body,
            station_id,
            bytes: 0,
            is_resource,
            finished: false,
        };
        (status, headers, Body::from_stream(logged)).into_response()
    }
}

/// Logs how an audio stream ended, like the Go proxy (not for HLS
/// segments, and not when the client went away).
struct LoggedStream {
    inner: BoxStream<'static, Result<Bytes, String>>,
    station_id: String,
    bytes: u64,
    is_resource: bool,
    finished: bool,
}

impl futures_util::Stream for LoggedStream {
    type Item = Result<Bytes, String>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let polled = self.inner.poll_next_unpin(context);
        match &polled {
            std::task::Poll::Ready(Some(Ok(chunk))) => self.bytes += chunk.len() as u64,
            std::task::Poll::Ready(Some(Err(error))) if !self.is_resource && !self.finished => {
                self.finished = true;
                tracing::warn!(stationId = %self.station_id, bytes = self.bytes, %error, "radio stream interrupted");
            }
            std::task::Poll::Ready(None) if !self.is_resource && !self.finished => {
                self.finished = true;
                tracing::info!(stationId = %self.station_id, bytes = self.bytes, "radio upstream stream ended");
            }
            _ => {}
        }
        polled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn blocks_local_targets() {
        for url in [
            "file:///tmp/song.mp3",
            "http://localhost:8000/stream",
            "http://localhost.:8000/stream",
            "http://127.0.0.1:8000/stream",
            "http://10.0.0.1/stream",
            "http://169.254.1.1/stream",
            "http://[::1]/stream",
            "http://[fd00::1]/stream",
            "http://[::ffff:192.168.1.1]/stream",
            "http://0.0.0.0/stream",
            "radio.example/stream",
        ] {
            assert!(validate_stream_url(url).is_err(), "{url}");
        }
        assert_eq!(
            validate_stream_url("https://stream.radioparadise.com/mp3-192"),
            Ok(())
        );
        assert_eq!(
            validate_stream_url("ftp://example.com/x"),
            Err("unsafe stream URL: unsupported scheme".to_owned())
        );
    }

    #[test]
    fn icy_metadata_is_stripped_and_reported_across_chunks() {
        let mut parser = IcyParser::new(4);
        let mut reports = Vec::new();
        let stream = b"abcd\x02StreamTitle='Artist - Title';\x00\x00\x00efgh\x00ijk";
        let mut audio = Vec::new();
        for chunk in stream.chunks(3) {
            audio.extend_from_slice(
                &parser.feed(chunk, |metadata| reports.push(metadata.to_owned())),
            );
        }
        assert_eq!(audio, b"abcdefghijk");
        let (now, _) = parse_icy_metadata(&reports[0]).expect("metadata");
        assert_eq!(
            (now.artist.as_str(), now.title.as_str()),
            ("Artist", "Title")
        );
        assert_eq!(reports.len(), 1);
    }

    #[test]
    fn icy_without_a_title_keeps_the_raw_text() {
        let (now, updated) = parse_icy_metadata("StreamUrl='x';\0\0").expect("metadata");
        assert_eq!(now.raw, "StreamUrl='x';");
        assert!(now.updated_at.is_none() && updated.is_none());
        assert!(parse_icy_metadata("\0\0  ").is_none());
    }

    fn signer() -> ResourceSigner {
        ResourceSigner::new()
    }

    #[test]
    fn tokens_are_bound_to_the_station_and_expire() {
        let signer = signer();
        let token = signer.sign("station-1", "https://cdn.example/a.aac");
        assert_eq!(
            signer.verify(&token, "station-1"),
            Ok("https://cdn.example/a.aac".to_owned())
        );
        assert!(signer.verify(&token, "station-2").is_err());
        assert!(signer.verify(&format!("{token}x"), "station-1").is_err());
        assert!(ResourceSigner::new().verify(&token, "station-1").is_err());
        let expired = ResourceSigner {
            key: signer.key,
            now: || unix_now() + 2 * HLS_RESOURCE_TOKEN_TTL_SECONDS,
        };
        assert!(expired.verify(&token, "station-1").is_err());
    }

    #[test]
    fn rewrites_master_playlist_references_and_attributes() {
        let signer = signer();
        let base = Url::parse("https://radio.example/live/master.m3u8").expect("base");
        let playlist = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",URI=\"audio/index.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=128000\nlow/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=256000\nhttps://cdn.example/high/index.m3u8\n";
        let rewritten = rewrite_playlist(
            playlist,
            &base,
            "/api/v1/radio/stations/s/stream",
            "s",
            &signer,
        )
        .expect("rewrite");
        let resources: Vec<String> = rewritten
            .split(['\n', '"'])
            .filter_map(|part| part.strip_prefix("/api/v1/radio/stations/s/stream?resource="))
            .map(|token| signer.verify(token, "s").expect("token"))
            .collect();
        assert_eq!(
            resources,
            [
                "https://radio.example/live/audio/index.m3u8",
                "https://radio.example/live/low/index.m3u8",
                "https://cdn.example/high/index.m3u8",
            ]
        );
    }

    #[test]
    fn refuses_drm_malformed_and_private_playlists() {
        let signer = signer();
        let base = Url::parse("https://radio.example/index.m3u8").expect("base");
        let rewrite = |playlist: &str| rewrite_playlist(playlist, &base, "/p", "s", &signer);
        assert_eq!(
            rewrite("#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"k\"\n"),
            Err(RewriteError::Drm)
        );
        assert_eq!(
            rewrite(
                "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,KEYFORMAT=\"com.apple.streamingkeydelivery\"\n"
            ),
            Err(RewriteError::Drm)
        );
        assert!(
            rewrite("#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,KEYFORMAT=\"identity\",URI=\"key\"\n")
                .is_ok()
        );
        assert!(rewrite("segment.aac\n").is_err());
        assert!(rewrite("#EXTM3U\n#EXT-X-MEDIA:URI=\"open\n").is_err());
        assert!(rewrite("#EXTM3U\nhttp://10.0.0.1/segment.aac\n").is_err());
    }

    struct Fake {
        responses: Mutex<Vec<UpstreamResponse>>,
        fetched: Mutex<Vec<String>>,
    }

    impl Upstream for Fake {
        fn get(
            &self,
            url: Url,
            _: Option<HeaderValue>,
        ) -> BoxFuture<'static, Result<UpstreamResponse, String>> {
            self.fetched.lock().expect("lock").push(url.to_string());
            let response = self.responses.lock().expect("lock").remove(0);
            Box::pin(async move { Ok(response) })
        }
    }

    struct Sink(Mutex<Vec<NowPlaying>>);

    impl NowPlayingSink for Sink {
        fn now_playing(&self, _: &str, now: NowPlaying, _: Option<SystemTime>) {
            self.0.lock().expect("lock").push(now);
        }
    }

    fn response(content_type: &str, url: &str, body: &'static [u8]) -> UpstreamResponse {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(content_type).expect("header"),
        );
        UpstreamResponse {
            status: 200,
            headers,
            url: Url::parse(url).expect("url"),
            body: futures_util::stream::iter([Ok(Bytes::from_static(body))]).boxed(),
        }
    }

    fn station(stream_url: &str) -> Station {
        Station {
            id: "station-1".to_owned(),
            stream_url: stream_url.to_owned(),
            ..Station::default()
        }
    }

    async fn body_text(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        String::from_utf8(bytes.to_vec()).expect("utf-8")
    }

    #[tokio::test]
    async fn names_resolving_to_blocked_addresses_are_refused() {
        use reqwest::dns::Resolve;
        let name: reqwest::dns::Name = "localhost".parse().expect("name");
        let error = SafeResolver.resolve(name).await.err().expect("refused");
        assert!(
            error.to_string().contains("resolved to blocked IP"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn streams_keep_media_types_and_never_serve_a_page() {
        for (upstream, served) in [
            ("text/html; charset=utf-8", "application/octet-stream"),
            ("audio/mpeg", "audio/mpeg"),
        ] {
            let fake = Arc::new(Fake {
                responses: Mutex::new(vec![response(
                    upstream,
                    "https://radio.example/live",
                    b"<script>alert(1)</script>",
                )]),
                fetched: Mutex::new(Vec::new()),
            });
            let proxy = StreamProxy::new(fake, Arc::new(Sink(Mutex::default())));
            let query = crate::query::GoQuery::parse("");
            let response = proxy
                .stream(
                    StreamRequest {
                        path: "/p",
                        query: &query,
                        range: None,
                    },
                    &station("https://radio.example/live"),
                )
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{upstream}");
            let headers = response.headers();
            assert_eq!(headers[header::CONTENT_TYPE], served, "{upstream}");
            assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
            assert_eq!(headers[header::CONTENT_SECURITY_POLICY], "sandbox");
        }
    }

    #[tokio::test]
    async fn caller_supplied_urls_are_refused_before_any_fetch() {
        let fake = Arc::new(Fake {
            responses: Mutex::new(Vec::new()),
            fetched: Mutex::new(Vec::new()),
        });
        let proxy = StreamProxy::new(fake.clone(), Arc::new(Sink(Mutex::default())));
        let query = crate::query::GoQuery::parse("url=https%3A%2F%2Fpublic.example%2Fx");
        let response = proxy
            .stream(
                StreamRequest {
                    path: "/p",
                    query: &query,
                    range: None,
                },
                &station("https://radio.example/index.m3u8"),
            )
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(
            body_text(response)
                .await
                .contains("invalid HLS resource token")
        );
        assert!(fake.fetched.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn playlists_resolve_against_the_redirect_target_and_resources_play() {
        let fake = Arc::new(Fake {
            responses: Mutex::new(vec![
                response(
                    HLS_CONTENT_TYPE,
                    "https://cdn.example/moved/index.m3u8",
                    b"#EXTM3U\nseg.aac\n",
                ),
                response("audio/aac", "https://cdn.example/moved/seg.aac", b"audio"),
            ]),
            fetched: Mutex::new(Vec::new()),
        });
        let proxy = StreamProxy::new(fake.clone(), Arc::new(Sink(Mutex::default())));
        let empty = crate::query::GoQuery::default();
        let playlist = proxy
            .stream(
                StreamRequest {
                    path: "/p",
                    query: &empty,
                    range: None,
                },
                &station("https://radio.example/index.m3u8"),
            )
            .await;
        let text = body_text(playlist).await;
        let token = text
            .lines()
            .find_map(|line| line.strip_prefix("/p?resource="))
            .expect("rewritten segment")
            .to_owned();
        let query = crate::query::GoQuery::parse(&format!("resource={token}"));
        let segment = proxy
            .stream(
                StreamRequest {
                    path: "/p",
                    query: &query,
                    range: None,
                },
                &station("https://radio.example/index.m3u8"),
            )
            .await;
        assert_eq!(body_text(segment).await, "audio");
        assert_eq!(
            *fake.fetched.lock().expect("lock"),
            [
                "https://radio.example/index.m3u8",
                "https://cdn.example/moved/seg.aac"
            ]
        );
    }

    #[tokio::test]
    async fn icy_streams_report_now_playing() {
        let mut icy = response(
            "audio/mpeg",
            "https://radio.example/live",
            b"abcd\x02StreamTitle='A - B';\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00efgh",
        );
        icy.headers
            .insert("icy-metaint", HeaderValue::from_static("4"));
        let sink = Arc::new(Sink(Mutex::default()));
        let fake = Arc::new(Fake {
            responses: Mutex::new(vec![icy]),
            fetched: Mutex::new(Vec::new()),
        });
        let proxy = StreamProxy::new(fake, sink.clone());
        let empty = crate::query::GoQuery::default();
        let response = proxy
            .stream(
                StreamRequest {
                    path: "/p",
                    query: &empty,
                    range: None,
                },
                &station("https://radio.example/live"),
            )
            .await;
        assert_eq!(body_text(response).await, "abcdefgh");
        assert_eq!(sink.0.lock().expect("lock")[0].title, "B");
    }
}
