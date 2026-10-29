//! External Source rules: search through yt-dlp, resolve an External Track
//! once and cache where its audio is, and fetch that audio only through the
//! radio proxy's SSRF guards. Only URLs this server's own search produced
//! (signed ids) are resolved, yt-dlp is pinned to the extractor that found
//! them, and every resolved, playlist and segment URL must be a public
//! HTTP(S) address. Resolved URLs are signed and expire, so they are never
//! logged; logs name the id's fingerprint and the extractor. Artwork comes
//! only from the HTTPS URL the search signed into the id, through the same
//! guards, and is never logged either.

use super::timing::{CacheUse, ResolveTiming, RunTiming, StreamTiming};
use super::token::{TokenSigner, TrackRef, fingerprint, is_artwork_url};
use super::yt_dlp::{MAX_SEARCH_RESULTS, ResolvedFormat, YtDlp};
use crate::config::ExternalSourceConfig;
use crate::modules::radio::{
    MAX_HLS_PLAYLIST_BYTES, ResourceSigner, RewriteError, SafeUpstream, Upstream, UpstreamResponse,
    is_hls_playlist, rewrite_playlist_with, validate_stream_url,
};
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use bytes::Bytes;
use futures_util::FutureExt;
use futures_util::StreamExt;
use futures_util::future::{BoxFuture, Shared};
use futures_util::stream::BoxStream;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;

pub const DEFAULT_SEARCH_LIMIT: usize = 10;
const MAX_QUERY_CHARS: usize = 200;
const TRACKS_PATH: &str = "/api/v1/external/tracks";
const CACHE_TTL: Duration = Duration::from_secs(600);
const CACHE_CAPACITY: usize = 64;
/// A resolved URL is dropped this long before its own `expire` time.
const EXPIRY_MARGIN_SECONDS: i64 = 30;
/// A URL refused within this long of its resolution is kept: running
/// yt-dlp again at once would only print the same URL.
const REFUSAL_GRACE: Duration = Duration::from_secs(60);
/// An External Track's artwork must arrive within this, and be at most
/// `ARTWORK_MAX_BYTES` of one of `ARTWORK_MEDIA_TYPES`.
const ARTWORK_TIMEOUT: Duration = Duration::from_secs(15);
const ARTWORK_MAX_BYTES: usize = 2 << 20;
const ARTWORK_MEDIA_TYPES: &[&str] = &["image/jpeg", "image/png", "image/webp"];
/// The request headers from yt-dlp's `http_headers` that are forwarded.
const FORWARDED_HEADERS: &[&str] = &[
    "user-agent",
    "accept",
    "accept-language",
    "referer",
    "origin",
    "cookie",
    "sec-fetch-mode",
];

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ExternalError {
    #[error("the External Source is not configured")]
    Disabled,
    #[error("{0}")]
    BadRequest(String),
    /// No audio this server can play: live, DRM, no HTTP(S) audio, or the
    /// wrong endpoint for the Track's format.
    #[error("{0}")]
    Unplayable(String),
    /// The search found no artwork for the External Track.
    #[error("the External Track has no artwork")]
    ArtworkMissing,
    /// yt-dlp failed or printed something unreadable, or the site's server
    /// did not deliver.
    #[error("{0}")]
    Failed(String),
    /// yt-dlp could not be started.
    #[error("{0}")]
    Unavailable(String),
    #[error("{0}")]
    Timeout(String),
}

/// One search result. `id` is the signed reference the other endpoints take.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalTrack {
    pub id: String,
    pub title: String,
    pub artist_name: Option<String>,
    pub duration_ms: Option<i64>,
    pub webpage_url: Option<String>,
    pub extractor: String,
    /// Whether `/artwork` serves a cover for it.
    pub has_artwork: bool,
}

/// An External Track's cover, read whole.
#[derive(Debug, PartialEq)]
pub struct Artwork {
    pub media_type: &'static str,
    pub data: Bytes,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchResults {
    pub source: String,
    pub items: Vec<ExternalTrack>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackFormat {
    Progressive,
    Hls,
}

impl PlaybackFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Progressive => "progressive",
            Self::Hls => "hls",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playback {
    pub format: PlaybackFormat,
    pub stream_path: String,
}

/// What an HLS resource turned out to be.
pub enum Resource {
    /// A nested playlist, already rewritten.
    Playlist(String),
    Media(UpstreamResponse),
}

pub fn stream_path(id: &str) -> String {
    format!("{TRACKS_PATH}/{id}/stream")
}

pub fn playlist_path(id: &str) -> String {
    format!("{TRACKS_PATH}/{id}/playlist.m3u8")
}

fn resource_path(id: &str, resource: &str) -> String {
    format!("{TRACKS_PATH}/{id}/resources/{resource}")
}

/// Where an External Track's audio is, as resolved by yt-dlp.
#[derive(Debug)]
struct Resolved {
    url: String,
    format: PlaybackFormat,
    headers: HeaderMap,
    chunk_size: Option<u64>,
}

impl From<ResolvedFormat> for Resolved {
    fn from(resolved: ResolvedFormat) -> Self {
        let mut headers = HeaderMap::new();
        for (name, value) in resolved.headers {
            let Ok(name) = HeaderName::from_bytes(name.to_ascii_lowercase().as_bytes()) else {
                continue;
            };
            if !FORWARDED_HEADERS.contains(&name.as_str()) {
                continue;
            }
            if let Ok(value) = HeaderValue::from_str(&value) {
                headers.insert(name, value);
            }
        }
        Self {
            url: resolved.url,
            format: if resolved.hls {
                PlaybackFormat::Hls
            } else {
                PlaybackFormat::Progressive
            },
            headers,
            chunk_size: resolved.chunk_size,
        }
    }
}

/// A single `bytes=<start>-[<end>]` range; `None` for anything else
/// (suffix and multiple ranges), which is passed on as it is.
fn parse_range(value: &HeaderValue) -> Option<(u64, Option<u64>)> {
    let spec = value.to_str().ok()?.trim().strip_prefix("bytes=")?;
    let (start, end) = spec.split_once('-')?;
    let start = start.trim().parse().ok()?;
    let end = match end.trim() {
        "" => None,
        end => Some(end.parse().ok()?),
    };
    (end.is_none_or(|end| end >= start)).then_some((start, end))
}

/// `Content-Range: bytes <first>-<last>/<total>`.
fn parse_content_range(headers: &HeaderMap) -> Option<(u64, u64, u64)> {
    let value = headers
        .get(axum::http::header::CONTENT_RANGE)?
        .to_str()
        .ok()?;
    let (span, total) = value.trim().strip_prefix("bytes ")?.split_once('/')?;
    let (first, last) = span.split_once('-')?;
    Some((first.parse().ok()?, last.parse().ok()?, total.parse().ok()?))
}

fn byte_range(first: u64, last: u64) -> HeaderValue {
    HeaderValue::from_str(&format!("bytes={first}-{last}")).expect("range header")
}

/// How long a resolved URL may be reused: ten minutes, or less when its
/// `expire`/`expires` query parameter (Unix seconds) says so.
fn cache_lifetime(url: &str, now: i64) -> Duration {
    let expires = Url::parse(url).ok().and_then(|url| {
        url.query_pairs()
            .find(|(name, _)| name == "expire" || name == "expires")
            .and_then(|(_, value)| value.parse::<i64>().ok())
    });
    match expires {
        Some(expires) => {
            let left = (expires - now - EXPIRY_MARGIN_SECONDS).max(0) as u64;
            CACHE_TTL.min(Duration::from_secs(left))
        }
        None => CACHE_TTL,
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// A yt-dlp resolution and how long its run took, shared by every request
/// waiting for it.
type Resolution = Shared<BoxFuture<'static, (Result<Arc<Resolved>, ExternalError>, RunTiming)>>;

/// A cached resolution, when it is stale, and when it was made.
type CacheEntry = (Arc<Resolved>, Instant, Instant);

/// Resolved URLs by id (never audio), and the resolutions running.
#[derive(Default)]
struct ResolveCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
    in_flight: Mutex<HashMap<String, Resolution>>,
}

impl ResolveCache {
    fn get(&self, id: &str) -> Option<Arc<Resolved>> {
        let entries = self.entries.lock().expect("resolve cache lock");
        entries
            .get(id)
            .filter(|(_, until, _)| *until > Instant::now())
            .map(|(resolved, _, _)| resolved.clone())
    }

    /// The request headers the Track resolved with, even after its URL is
    /// stale: HLS segments signed earlier still need them.
    fn headers(&self, id: &str) -> HeaderMap {
        let entries = self.entries.lock().expect("resolve cache lock");
        entries
            .get(id)
            .map(|(resolved, _, _)| resolved.headers.clone())
            .unwrap_or_default()
    }

    fn insert(&self, id: &str, resolved: Arc<Resolved>, lifetime: Duration) {
        self.insert_resolved_at(id, resolved, lifetime, Instant::now());
    }

    fn insert_resolved_at(
        &self,
        id: &str,
        resolved: Arc<Resolved>,
        lifetime: Duration,
        resolved_at: Instant,
    ) {
        let mut entries = self.entries.lock().expect("resolve cache lock");
        let now = Instant::now();
        entries.retain(|_, (_, until, _)| *until > now);
        if entries.len() >= CACHE_CAPACITY
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, (_, until, _))| *until)
                .map(|(id, _)| id.clone())
        {
            entries.remove(&oldest);
        }
        entries.insert(id.to_owned(), (resolved, now + lifetime, resolved_at));
    }

    /// Drops the Track's URL after the site refused it, unless it was
    /// resolved moments ago (see [`REFUSAL_GRACE`]).
    fn forget_refused(&self, id: &str) {
        let mut entries = self.entries.lock().expect("resolve cache lock");
        if entries
            .get(id)
            .is_some_and(|(_, _, resolved_at)| resolved_at.elapsed() >= REFUSAL_GRACE)
        {
            entries.remove(id);
        }
    }
}

pub struct ExternalSource {
    /// `None` while the External Source is not configured.
    search_prefix: Option<String>,
    yt_dlp: YtDlp,
    tracks: TokenSigner,
    resources: ResourceSigner,
    upstream: Arc<dyn Upstream>,
    cache: Arc<ResolveCache>,
}

impl ExternalSource {
    pub fn new(config: Option<&ExternalSourceConfig>) -> Self {
        Self::with_upstream(config, Arc::new(SafeUpstream::new()))
    }

    pub(crate) fn with_upstream(
        config: Option<&ExternalSourceConfig>,
        upstream: Arc<dyn Upstream>,
    ) -> Self {
        Self {
            search_prefix: config.map(|config| config.search_prefix.clone()),
            yt_dlp: YtDlp::new(
                config.map_or("yt-dlp", |config| config.yt_dlp.as_str()),
                config.map_or(&[], |config| config.extractor_args.as_slice()),
            ),
            tracks: TokenSigner::new(),
            resources: ResourceSigner::new(),
            upstream,
            cache: Arc::default(),
        }
    }

    /// Shorter yt-dlp time limits than the 20 s search and 30 s resolve.
    pub fn with_timeouts(mut self, search: Duration, resolve: Duration) -> Self {
        self.yt_dlp.search_timeout = search;
        self.yt_dlp.resolve_timeout = resolve;
        self
    }

    pub fn is_enabled(&self) -> bool {
        self.search_prefix.is_some()
    }

    pub fn ensure_enabled(&self) -> Result<&str, ExternalError> {
        self.search_prefix.as_deref().ok_or(ExternalError::Disabled)
    }

    /// `timing` gets the yt-dlp run's timings.
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
        timing: &mut RunTiming,
    ) -> Result<SearchResults, ExternalError> {
        let prefix = self.ensure_enabled()?;
        let query: String = query
            .chars()
            .filter(|character| !character.is_control())
            .collect();
        let query = query.trim();
        if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
            return Err(ExternalError::BadRequest(format!(
                "q must be 1 to {MAX_QUERY_CHARS} characters"
            )));
        }
        if !(1..=MAX_SEARCH_RESULTS).contains(&limit) {
            return Err(ExternalError::BadRequest(format!(
                "limit must be between 1 and {MAX_SEARCH_RESULTS}"
            )));
        }
        let items = self
            .yt_dlp
            .search(prefix, query, limit, timing)
            .await
            .inspect_err(|error| tracing::warn!(%error, "External Source search failed"))?;
        let items = items
            .into_iter()
            .filter_map(|item| {
                let track = TrackRef::new(&item.webpage_url, &item.extractor);
                // An id too long with its artwork is issued without it.
                let found_artwork = item.artwork_url.is_some();
                let (id, has_artwork) = match self.tracks.sign(&TrackRef {
                    artwork_url: item.artwork_url,
                    ..track.clone()
                }) {
                    Some(id) => (id, found_artwork),
                    None => (self.tracks.sign(&track)?, false),
                };
                Some(ExternalTrack {
                    has_artwork,
                    id,
                    title: item.title,
                    artist_name: item.artist_name,
                    duration_ms: item.duration_ms,
                    webpage_url: Some(item.webpage_url),
                    extractor: item.extractor,
                })
            })
            .collect();
        Ok(SearchResults {
            source: prefix.to_owned(),
            items,
        })
    }

    /// How a client plays the Track: its stream or its HLS playlist.
    pub async fn playback(
        &self,
        id: &str,
        timing: &mut ResolveTiming,
    ) -> Result<Playback, ExternalError> {
        let resolved = self.resolve(id, timing).await?;
        Ok(Playback {
            format: resolved.format,
            stream_path: match resolved.format {
                PlaybackFormat::Progressive => stream_path(id),
                PlaybackFormat::Hls => playlist_path(id),
            },
        })
    }

    /// The upstream response for a progressive Track, `range` passed on.
    /// With `head`, a chunked Track is answered from a 1-byte probe and
    /// without a body. `timing` gets the resolve, the wait for the first
    /// upstream response and the number of upstream requests.
    pub async fn open_stream(
        &self,
        id: &str,
        range: Option<HeaderValue>,
        head: bool,
        timing: &mut StreamTiming,
    ) -> Result<UpstreamResponse, ExternalError> {
        let resolved = self.resolve(id, &mut timing.resolve).await?;
        if resolved.format != PlaybackFormat::Progressive {
            return Err(ExternalError::Unplayable(
                "the External Track plays as HLS; use its playlist".to_owned(),
            ));
        }
        let requested = match &range {
            None => Some((0, None)),
            Some(value) => parse_range(value),
        };
        let requested_at = Instant::now();
        timing.chunks.fetch_add(1, Ordering::Relaxed);
        let response = match (resolved.chunk_size, requested) {
            (Some(chunk_size), Some((start, end))) => {
                self.fetch_in_chunks(
                    id,
                    &resolved,
                    chunk_size,
                    (start, end),
                    (range.is_some(), head),
                    timing.chunks.clone(),
                )
                .await
            }
            _ => {
                self.fetch(id, &resolved.url, resolved.headers.clone(), range)
                    .await
            }
        };
        timing.upstream_ttfb = Some(requested_at.elapsed());
        response
    }

    /// Serves `start..=end` (to the end of the file without `end`) as one
    /// response from upstream ranges of at most `chunk_size` bytes, fetched
    /// one after another as the client reads, the way yt-dlp downloads such
    /// a file. With `head` only `start` is fetched, for the headers. Each
    /// later chunk's request is counted in `chunks`.
    async fn fetch_in_chunks(
        &self,
        id: &str,
        resolved: &Resolved,
        chunk_size: u64,
        (start, end): (u64, Option<u64>),
        (ranged, head): (bool, bool),
        chunks: Arc<AtomicU64>,
    ) -> Result<UpstreamResponse, ExternalError> {
        use axum::http::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE};
        let chunk_last = if head {
            start
        } else {
            start.saturating_add(chunk_size - 1)
        };
        let first = self
            .fetch(
                id,
                &resolved.url,
                resolved.headers.clone(),
                Some(byte_range(
                    start,
                    end.map_or(chunk_last, |end| end.min(chunk_last)),
                )),
            )
            .await?;
        let Some((_, served_last, total)) =
            parse_content_range(&first.headers).filter(|_| first.status == 206)
        else {
            // A 416, or a server that ignored the range: passed on as it is.
            return Ok(first);
        };
        validate_stream_url(first.url.as_str()).map_err(ExternalError::Failed)?;
        let file_last = total.saturating_sub(1);
        let last = end.map_or(file_last, |end| end.min(file_last));
        let mut headers = first.headers.clone();
        headers.insert(CONTENT_LENGTH, HeaderValue::from(last + 1 - start));
        headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        let status = if ranged {
            let range = format!("bytes {start}-{last}/{total}");
            headers.insert(
                CONTENT_RANGE,
                HeaderValue::from_str(&range).expect("content range"),
            );
            206
        } else {
            headers.remove(CONTENT_RANGE);
            200
        };
        if head {
            return Ok(UpstreamResponse {
                status,
                headers,
                url: first.url,
                body: futures_util::stream::empty().boxed(),
            });
        }
        let upstream = self.upstream.clone();
        let url = first.url.clone();
        let request_headers = resolved.headers.clone();
        let track_hash = fingerprint(id);
        let rest = futures_util::stream::unfold(served_last + 1, move |next| {
            let upstream = upstream.clone();
            let url = url.clone();
            let request_headers = request_headers.clone();
            let track_hash = track_hash.clone();
            let chunks = chunks.clone();
            async move {
                if next > last {
                    return None;
                }
                chunks.fetch_add(1, Ordering::Relaxed);
                let chunk_last = next.saturating_add(chunk_size - 1).min(last);
                // The body ends here; the URL is never logged.
                let failed = |message: String| {
                    tracing::warn!(track = %track_hash, error = %message, "External Track stream ended early");
                    let body: BoxStream<'static, Result<Bytes, String>> =
                        futures_util::stream::iter([Err(message)]).boxed();
                    Some((body, u64::MAX))
                };
                match upstream
                    .get_with_headers(url, Some(byte_range(next, chunk_last)), request_headers)
                    .await
                {
                    Ok(response)
                        if response.status == 206
                            && parse_content_range(&response.headers)
                                .is_some_and(|(first, last, _)| (first, last) == (next, chunk_last)) =>
                    {
                        Some((response.body, chunk_last.saturating_add(1)))
                    }
                    Ok(response) => failed(format!(
                        "the External Track's stream answered {} for a later chunk",
                        response.status
                    )),
                    Err(_) => failed("the External Track's stream failed".to_owned()),
                }
            }
        })
        .flatten();
        Ok(UpstreamResponse {
            status,
            headers,
            url: first.url,
            body: first.body.chain(rest).boxed(),
        })
    }

    /// The HLS playlist of an HLS Track, every URI rewritten to a signed
    /// resource of this Track.
    pub async fn playlist(&self, id: &str) -> Result<String, ExternalError> {
        let resolved = self.resolve(id, &mut ResolveTiming::default()).await?;
        if resolved.format != PlaybackFormat::Hls {
            return Err(ExternalError::Unplayable(
                "the External Track plays progressively; use its stream".to_owned(),
            ));
        }
        let response = self
            .fetch(id, &resolved.url, resolved.headers.clone(), None)
            .await?;
        self.rewrite(id, response).await
    }

    /// One URI of a rewritten playlist: a nested playlist (rewritten too),
    /// a segment or a key.
    pub async fn resource(
        &self,
        id: &str,
        resource: &str,
        range: Option<HeaderValue>,
    ) -> Result<Resource, ExternalError> {
        self.ensure_enabled()?;
        self.tracks.verify(id).map_err(ExternalError::BadRequest)?;
        let url = self
            .resources
            .verify(resource, &fingerprint(id))
            .map_err(ExternalError::BadRequest)?;
        let response = self.fetch(id, &url, self.cache.headers(id), range).await?;
        if is_hls_playlist(&response) {
            return Ok(Resource::Playlist(self.rewrite(id, response).await?));
        }
        Ok(Resource::Media(response))
    }

    /// The cover the search chose for the Track, fetched whole through the
    /// SSRF-guarded client: a JPEG, PNG or WebP image of at most 2 MiB.
    pub async fn artwork(&self, id: &str) -> Result<Artwork, ExternalError> {
        self.ensure_enabled()?;
        let track = self.tracks.verify(id).map_err(ExternalError::BadRequest)?;
        let url = track.artwork_url.ok_or(ExternalError::ArtworkMissing)?;
        tokio::time::timeout(ARTWORK_TIMEOUT, self.fetch_artwork(id, &url))
            .await
            .map_err(|_| {
                ExternalError::Timeout(format!(
                    "the External Track's artwork did not arrive within {} s",
                    ARTWORK_TIMEOUT.as_secs()
                ))
            })?
    }

    async fn fetch_artwork(&self, id: &str, url: &str) -> Result<Artwork, ExternalError> {
        use axum::http::header::{ACCEPT, CONTENT_LENGTH, CONTENT_TYPE};
        let failed = |message: String| {
            tracing::warn!(track = %fingerprint(id), error = %message, "External Track artwork failed");
            ExternalError::Failed(message)
        };
        if !is_artwork_url(url) {
            return Err(failed("unsafe artwork URL".to_owned()));
        }
        let parsed = Url::parse(url).map_err(|error| failed(error.to_string()))?;
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("image/jpeg, image/png, image/webp"),
        );
        let response = self
            .upstream
            .get_with_headers(parsed, None, headers)
            .await
            .map_err(|error| {
                // The client's error names the URL; only its cause is kept.
                let cause = error
                    .split_once("\": ")
                    .map_or("request failed", |(_, cause)| cause);
                failed(format!("the External Track's artwork failed: {cause}"))
            })?;
        if response.status != 200 {
            return Err(failed(format!(
                "the External Track's artwork answered {}",
                response.status
            )));
        }
        let media_type = response
            .headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| {
                value
                    .split(';')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_ascii_lowercase()
            })
            .and_then(|value| {
                ARTWORK_MEDIA_TYPES
                    .iter()
                    .copied()
                    .find(|allowed| *allowed == value)
            })
            .ok_or_else(|| failed("the External Track's artwork is not an image".to_owned()))?;
        let too_large = || failed("the External Track's artwork exceeds 2 MiB".to_owned());
        if response
            .headers
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok()?.parse::<u64>().ok())
            .is_some_and(|length| length > ARTWORK_MAX_BYTES as u64)
        {
            return Err(too_large());
        }
        let mut body = response.body;
        let mut data = Vec::new();
        while let Some(chunk) = body.next().await {
            let chunk = chunk
                .map_err(|_| failed("failed to read the External Track's artwork".to_owned()))?;
            if data.len() + chunk.len() > ARTWORK_MAX_BYTES {
                return Err(too_large());
            }
            data.extend_from_slice(&chunk);
        }
        Ok(Artwork {
            media_type,
            data: Bytes::from(data),
        })
    }

    /// The Track's resolved audio; `timing` gets where it came from and how
    /// long it took.
    async fn resolve(
        &self,
        id: &str,
        timing: &mut ResolveTiming,
    ) -> Result<Arc<Resolved>, ExternalError> {
        let started = Instant::now();
        self.ensure_enabled()?;
        let track = self.tracks.verify(id).map_err(ExternalError::BadRequest)?;
        timing.extractor = Some(track.extractor.clone());
        let result = match self.cache.get(id) {
            Some(resolved) => {
                timing.cache = Some(CacheUse::Hit);
                Ok(resolved)
            }
            None => {
                let (resolution, cache) = self.resolution(id, track);
                timing.cache = Some(cache);
                let (result, run) = resolution.await;
                timing.run = run;
                result
            }
        };
        timing.total = Some(started.elapsed());
        timing.format = result
            .as_ref()
            .ok()
            .map(|resolved| resolved.format.as_str());
        result
    }

    /// The resolution running for `id`, starting one if none is, and which
    /// of the two it was. It runs on its own task, so it finishes (and is
    /// cached) for the next caller even when the request that started it
    /// goes away.
    fn resolution(&self, id: &str, track: TrackRef) -> (Resolution, CacheUse) {
        let mut in_flight = self.cache.in_flight.lock().expect("resolutions lock");
        if let Some(existing) = in_flight.get(id) {
            return (existing.clone(), CacheUse::Joined);
        }
        let yt_dlp = self.yt_dlp.clone();
        let cache = self.cache.clone();
        let key = id.to_owned();
        let task = tokio::spawn(async move {
            let track_hash = fingerprint(&key);
            let mut run = RunTiming::default();
            let result = yt_dlp
                .resolve(&track.url, &track.extractor, &mut run)
                .await
                .map(|format| Arc::new(Resolved::from(format)));
            match &result {
                Ok(resolved) => {
                    tracing::info!(track = %track_hash, extractor = %track.extractor, format = ?resolved.format, "External Track resolved");
                    cache.insert(
                        &key,
                        resolved.clone(),
                        cache_lifetime(&resolved.url, unix_now()),
                    );
                }
                Err(error) => {
                    tracing::warn!(track = %track_hash, extractor = %track.extractor, %error, "External Track not resolved");
                }
            }
            cache
                .in_flight
                .lock()
                .expect("resolutions lock")
                .remove(&key);
            (result, run)
        });
        let shared = async move {
            task.await.unwrap_or_else(|error| {
                (
                    Err(ExternalError::Failed(format!(
                        "External Track resolution stopped: {error}"
                    ))),
                    RunTiming::default(),
                )
            })
        }
        .boxed()
        .shared();
        in_flight.insert(id.to_owned(), shared.clone());
        (shared, CacheUse::Miss)
    }

    /// GETs a public HTTP(S) URL through the SSRF-guarded client. A
    /// refused, expired or missing URL is forgotten (unless just resolved),
    /// so the next request resolves the Track again.
    async fn fetch(
        &self,
        id: &str,
        url: &str,
        headers: HeaderMap,
        range: Option<HeaderValue>,
    ) -> Result<UpstreamResponse, ExternalError> {
        validate_stream_url(url).map_err(ExternalError::Failed)?;
        let parsed = Url::parse(url).map_err(|error| ExternalError::Failed(error.to_string()))?;
        let response = self
            .upstream
            .get_with_headers(parsed, range, headers)
            .await
            .map_err(|error| {
                // The client's error names the URL; only its cause is kept.
                let cause = error
                    .split_once("\": ")
                    .map_or("request failed", |(_, cause)| cause);
                tracing::warn!(track = %fingerprint(id), error = %cause, "External Track stream request failed");
                ExternalError::Failed(format!("the External Track's stream failed: {cause}"))
            })?;
        match response.status {
            200..=299 | 416 => Ok(response),
            status => {
                if matches!(status, 401 | 403 | 404 | 410) {
                    self.cache.forget_refused(id);
                }
                Err(ExternalError::Failed(format!(
                    "the External Track's stream answered {status}"
                )))
            }
        }
    }

    async fn rewrite(&self, id: &str, response: UpstreamResponse) -> Result<String, ExternalError> {
        if !(200..300).contains(&response.status) {
            return Err(ExternalError::Failed(format!(
                "the External Track's playlist answered {}",
                response.status
            )));
        }
        let base = response.url.clone();
        let mut body = response.body;
        let mut playlist = Vec::new();
        while let Some(chunk) = body.next().await {
            let chunk = chunk
                .map_err(|_| ExternalError::Failed("failed to read the HLS playlist".to_owned()))?;
            playlist.extend_from_slice(&chunk);
            if playlist.len() > MAX_HLS_PLAYLIST_BYTES {
                return Err(ExternalError::Failed(
                    "HLS playlist exceeds size limit".to_owned(),
                ));
            }
        }
        let scope = fingerprint(id);
        rewrite_playlist_with(&String::from_utf8_lossy(&playlist), &base, |url| {
            resource_path(id, &self.resources.sign(&scope, url))
        })
        .map_err(|error| match error {
            RewriteError::Drm => ExternalError::Unplayable(error.to_string()),
            RewriteError::Invalid(message) => ExternalError::Failed(message),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        responses: Mutex<Vec<UpstreamResponse>>,
        fetched: Mutex<Vec<(String, Option<HeaderValue>, HeaderMap)>>,
    }

    impl Upstream for Fake {
        fn get(
            &self,
            _: Url,
            _: Option<HeaderValue>,
        ) -> BoxFuture<'static, Result<UpstreamResponse, String>> {
            panic!("the External Source sends its own headers")
        }

        fn get_with_headers(
            &self,
            url: Url,
            range: Option<HeaderValue>,
            headers: HeaderMap,
        ) -> BoxFuture<'static, Result<UpstreamResponse, String>> {
            self.fetched
                .lock()
                .expect("lock")
                .push((url.to_string(), range, headers));
            let response = self.responses.lock().expect("lock").remove(0);
            Box::pin(async move { Ok(response) })
        }
    }

    fn response(
        status: u16,
        url: &str,
        headers: &[(&str, &str)],
        body: &'static [u8],
    ) -> UpstreamResponse {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(
                HeaderName::from_bytes(name.as_bytes()).expect("name"),
                HeaderValue::from_str(value).expect("value"),
            );
        }
        let body: BoxStream<'static, Result<Bytes, String>> =
            futures_util::stream::iter([Ok(Bytes::from_static(body))]).boxed();
        UpstreamResponse {
            status,
            headers: map,
            url: Url::parse(url).expect("url"),
            body,
        }
    }

    fn config() -> ExternalSourceConfig {
        ExternalSourceConfig {
            search_prefix: "scsearch".to_owned(),
            // Never started: these tests seed the resolve cache.
            yt_dlp: "/nonexistent/yt-dlp".to_owned(),
            extractor_args: Vec::new(),
            name: "SoundCloud".to_owned(),
        }
    }

    /// A source whose one Track already resolved to `url`.
    fn resolved_source(
        fake: Arc<Fake>,
        url: &str,
        format: PlaybackFormat,
    ) -> (ExternalSource, String) {
        chunked_source(fake, url, format, None)
    }

    fn chunked_source(
        fake: Arc<Fake>,
        url: &str,
        format: PlaybackFormat,
        chunk_size: Option<u64>,
    ) -> (ExternalSource, String) {
        let source = ExternalSource::with_upstream(Some(&config()), fake);
        let id = source
            .tracks
            .sign(&TrackRef::new("https://soundcloud.com/a/b", "Soundcloud"))
            .expect("id");
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", HeaderValue::from_static("yt-dlp-agent"));
        // Resolved a while ago, so a refusal now means the URL went stale.
        source.cache.insert_resolved_at(
            &id,
            Arc::new(Resolved {
                url: url.to_owned(),
                format,
                headers,
                chunk_size,
            }),
            CACHE_TTL,
            Instant::now()
                .checked_sub(REFUSAL_GRACE)
                .expect("an earlier instant"),
        );
        (source, id)
    }

    async fn text(response: UpstreamResponse) -> String {
        let chunks: Vec<_> = response.body.collect().await;
        chunks
            .into_iter()
            .map(|chunk| String::from_utf8(chunk.expect("chunk").to_vec()).expect("utf-8"))
            .collect()
    }

    #[tokio::test]
    async fn progressive_streams_pass_ranges_and_resolved_headers_on() {
        let fake = Arc::new(Fake::default());
        fake.responses.lock().expect("lock").push(response(
            206,
            "https://media.example/a.m4a",
            &[("content-range", "bytes 0-1/10")],
            b"ab",
        ));
        let (source, id) = resolved_source(
            fake.clone(),
            "https://media.example/a.m4a",
            PlaybackFormat::Progressive,
        );
        let playback = source
            .playback(&id, &mut ResolveTiming::default())
            .await
            .expect("playback");
        assert_eq!(
            playback.stream_path,
            format!("/api/v1/external/tracks/{id}/stream")
        );
        let upstream = source
            .open_stream(
                &id,
                Some(HeaderValue::from_static("bytes=0-1")),
                false,
                &mut StreamTiming::default(),
            )
            .await
            .expect("stream");
        assert_eq!(upstream.status, 206);
        assert_eq!(text(upstream).await, "ab");
        let fetched = fake.fetched.lock().expect("lock");
        assert_eq!(fetched[0].0, "https://media.example/a.m4a");
        assert_eq!(fetched[0].1.as_ref().expect("range"), "bytes=0-1");
        assert_eq!(fetched[0].2["user-agent"], "yt-dlp-agent");
    }

    fn chunk(first: u64, last: u64, body: &'static [u8]) -> UpstreamResponse {
        response(
            206,
            "https://media.example/a.webm",
            &[
                ("content-type", "audio/webm"),
                ("content-range", &format!("bytes {first}-{last}/10")),
            ],
            body,
        )
    }

    fn ranges(fake: &Fake) -> Vec<String> {
        fake.fetched
            .lock()
            .expect("lock")
            .drain(..)
            .map(|(_, range, _)| range.expect("range").to_str().expect("text").to_owned())
            .collect()
    }

    // YouTube serves an audio file only in ranges of yt-dlp's
    // http_chunk_size; a whole-file GET stalls.
    #[tokio::test]
    async fn chunked_sources_are_fetched_in_bounded_ranges() {
        let fake = Arc::new(Fake::default());
        let (source, id) = chunked_source(
            fake.clone(),
            "https://media.example/a.webm",
            PlaybackFormat::Progressive,
            Some(4),
        );
        fake.responses.lock().expect("lock").extend([
            chunk(0, 3, b"0123"),
            chunk(4, 7, b"4567"),
            chunk(8, 9, b"89"),
        ]);
        let whole = source
            .open_stream(&id, None, false, &mut StreamTiming::default())
            .await
            .expect("stream");
        assert_eq!(whole.status, 200);
        assert_eq!(whole.headers[axum::http::header::CONTENT_LENGTH], "10");
        assert!(
            whole
                .headers
                .get(axum::http::header::CONTENT_RANGE)
                .is_none()
        );
        assert_eq!(text(whole).await, "0123456789");
        assert_eq!(ranges(&fake), ["bytes=0-3", "bytes=4-7", "bytes=8-9"]);

        // A browser's open-ended range, and a bounded one inside a chunk.
        fake.responses.lock().expect("lock").extend([
            chunk(5, 8, b"5678"),
            chunk(9, 9, b"9"),
            chunk(2, 3, b"23"),
        ]);
        let tail = source
            .open_stream(
                &id,
                Some(HeaderValue::from_static("bytes=5-")),
                false,
                &mut StreamTiming::default(),
            )
            .await
            .expect("stream");
        assert_eq!(tail.status, 206);
        assert_eq!(
            tail.headers[axum::http::header::CONTENT_RANGE],
            "bytes 5-9/10"
        );
        assert_eq!(tail.headers[axum::http::header::CONTENT_LENGTH], "5");
        assert_eq!(text(tail).await, "56789");
        let part = source
            .open_stream(
                &id,
                Some(HeaderValue::from_static("bytes=2-3")),
                false,
                &mut StreamTiming::default(),
            )
            .await
            .expect("stream");
        assert_eq!(
            part.headers[axum::http::header::CONTENT_RANGE],
            "bytes 2-3/10"
        );
        assert_eq!(text(part).await, "23");
        assert_eq!(ranges(&fake), ["bytes=5-8", "bytes=9-9", "bytes=2-3"]);

        // Past the end: the upstream 416 is passed on.
        fake.responses.lock().expect("lock").push(response(
            416,
            "https://media.example/a.webm",
            &[],
            b"",
        ));
        let past = source
            .open_stream(
                &id,
                Some(HeaderValue::from_static("bytes=20-")),
                false,
                &mut StreamTiming::default(),
            )
            .await
            .expect("stream");
        assert_eq!(past.status, 416);
    }

    async fn chunks(response: UpstreamResponse) -> Vec<Result<String, String>> {
        response
            .body
            .map(|chunk| chunk.map(|bytes| String::from_utf8(bytes.to_vec()).expect("utf-8")))
            .collect()
            .await
    }

    #[tokio::test]
    async fn chunked_ranges_are_split_at_chunk_boundaries_with_resolved_headers() {
        let fake = Arc::new(Fake::default());
        let (source, id) = chunked_source(
            fake.clone(),
            "https://media.example/a.webm",
            PlaybackFormat::Progressive,
            Some(4),
        );
        // An open range chained over three upstream requests.
        fake.responses.lock().expect("lock").extend([
            chunk(1, 4, b"1234"),
            chunk(5, 8, b"5678"),
            chunk(9, 9, b"9"),
        ]);
        let open = source
            .open_stream(
                &id,
                Some(HeaderValue::from_static("bytes=1-")),
                false,
                &mut StreamTiming::default(),
            )
            .await
            .expect("stream");
        assert_eq!(open.status, 206);
        assert_eq!(
            open.headers[axum::http::header::CONTENT_RANGE],
            "bytes 1-9/10"
        );
        assert_eq!(open.headers[axum::http::header::CONTENT_LENGTH], "9");
        assert_eq!(open.headers[axum::http::header::ACCEPT_RANGES], "bytes");
        assert_eq!(open.headers[axum::http::header::CONTENT_TYPE], "audio/webm");
        assert_eq!(text(open).await, "123456789");
        {
            let fetched = fake.fetched.lock().expect("lock");
            assert!(fetched.iter().all(|(url, _, headers)| {
                url == "https://media.example/a.webm" && headers["user-agent"] == "yt-dlp-agent"
            }));
        }
        assert_eq!(ranges(&fake), ["bytes=1-4", "bytes=5-8", "bytes=9-9"]);

        // A bounded range larger than a chunk, across a boundary.
        fake.responses
            .lock()
            .expect("lock")
            .extend([chunk(2, 5, b"2345"), chunk(6, 6, b"6")]);
        let bounded = source
            .open_stream(
                &id,
                Some(HeaderValue::from_static("bytes=2-6")),
                false,
                &mut StreamTiming::default(),
            )
            .await
            .expect("stream");
        assert_eq!(bounded.status, 206);
        assert_eq!(
            bounded.headers[axum::http::header::CONTENT_RANGE],
            "bytes 2-6/10"
        );
        assert_eq!(bounded.headers[axum::http::header::CONTENT_LENGTH], "5");
        assert_eq!(text(bounded).await, "23456");
        assert_eq!(ranges(&fake), ["bytes=2-5", "bytes=6-6"]);
    }

    #[tokio::test]
    async fn chunked_heads_answer_from_a_one_byte_probe() {
        let fake = Arc::new(Fake::default());
        let (source, id) = chunked_source(
            fake.clone(),
            "https://media.example/a.webm",
            PlaybackFormat::Progressive,
            Some(4),
        );
        fake.responses
            .lock()
            .expect("lock")
            .extend([chunk(0, 0, b"0"), chunk(4, 4, b"4")]);
        let head = source
            .open_stream(&id, None, true, &mut StreamTiming::default())
            .await
            .expect("head");
        assert_eq!(head.status, 200);
        assert_eq!(head.headers[axum::http::header::CONTENT_LENGTH], "10");
        assert_eq!(head.headers[axum::http::header::ACCEPT_RANGES], "bytes");
        assert_eq!(head.headers[axum::http::header::CONTENT_TYPE], "audio/webm");
        assert!(
            head.headers
                .get(axum::http::header::CONTENT_RANGE)
                .is_none()
        );
        assert_eq!(text(head).await, "");
        let ranged = source
            .open_stream(
                &id,
                Some(HeaderValue::from_static("bytes=4-")),
                true,
                &mut StreamTiming::default(),
            )
            .await
            .expect("head");
        assert_eq!(ranged.status, 206);
        assert_eq!(
            ranged.headers[axum::http::header::CONTENT_RANGE],
            "bytes 4-9/10"
        );
        assert_eq!(ranged.headers[axum::http::header::CONTENT_LENGTH], "6");
        assert_eq!(text(ranged).await, "");
        assert_eq!(ranges(&fake), ["bytes=0-0", "bytes=4-4"]);
    }

    #[tokio::test]
    async fn chunked_bodies_end_at_an_upstream_error() {
        let fake = Arc::new(Fake::default());
        let (source, id) = chunked_source(
            fake.clone(),
            "https://media.example/a.webm",
            PlaybackFormat::Progressive,
            Some(4),
        );
        fake.responses.lock().expect("lock").extend([
            chunk(0, 3, b"0123"),
            response(403, "https://media.example/a.webm", &[], b""),
        ]);
        let whole = source
            .open_stream(&id, None, false, &mut StreamTiming::default())
            .await
            .expect("stream");
        assert_eq!(whole.status, 200);
        let body = chunks(whole).await;
        assert_eq!(body.len(), 2, "{body:?}");
        assert_eq!(body[0], Ok("0123".to_owned()));
        assert!(
            matches!(&body[1], Err(message) if message.contains("403") && !message.contains("media.example"))
        );
        assert_eq!(ranges(&fake), ["bytes=0-3", "bytes=4-7"]);

        // A later chunk that is not the range asked for ends it too.
        fake.responses
            .lock()
            .expect("lock")
            .extend([chunk(0, 3, b"0123"), chunk(0, 3, b"0123")]);
        let whole = source
            .open_stream(&id, None, false, &mut StreamTiming::default())
            .await
            .expect("stream");
        let body = chunks(whole).await;
        assert_eq!(body.len(), 2, "{body:?}");
        assert!(body[1].is_err());
    }

    #[test]
    fn reads_single_byte_ranges_only() {
        let range = |value: &'static str| parse_range(&HeaderValue::from_static(value));
        assert_eq!(range("bytes=0-"), Some((0, None)));
        assert_eq!(range("bytes=5-9"), Some((5, Some(9))));
        for other in [
            "bytes=-500",
            "bytes=0-1,4-5",
            "bytes=9-5",
            "items=0-1",
            "bytes=a-",
        ] {
            assert_eq!(range(other), None, "{other}");
        }
    }

    #[tokio::test]
    async fn private_resolved_urls_are_refused_before_any_fetch() {
        let fake = Arc::new(Fake::default());
        for url in ["http://10.0.0.1/a.mp3", "http://localhost/a.mp3"] {
            let (source, id) = resolved_source(fake.clone(), url, PlaybackFormat::Progressive);
            assert!(matches!(
                source.open_stream(&id, None, false, &mut StreamTiming::default()).await,
                Err(ExternalError::Failed(message)) if message.contains("unsafe stream URL")
            ));
        }
        assert!(fake.fetched.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn each_endpoint_plays_only_its_format() {
        let fake = Arc::new(Fake::default());
        let (source, id) = resolved_source(
            fake.clone(),
            "https://cdn.example/a.m3u8",
            PlaybackFormat::Hls,
        );
        assert!(matches!(
            source
                .open_stream(&id, None, false, &mut StreamTiming::default())
                .await,
            Err(ExternalError::Unplayable(_))
        ));
        assert_eq!(
            source
                .playback(&id, &mut ResolveTiming::default())
                .await
                .expect("playback"),
            Playback {
                format: PlaybackFormat::Hls,
                stream_path: format!("/api/v1/external/tracks/{id}/playlist.m3u8"),
            }
        );
        let (source, id) = resolved_source(
            fake.clone(),
            "https://media.example/a.m4a",
            PlaybackFormat::Progressive,
        );
        assert!(matches!(
            source.playlist(&id).await,
            Err(ExternalError::Unplayable(_))
        ));
        assert!(fake.fetched.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn hls_uris_go_through_signed_resources_of_the_same_track() {
        let fake = Arc::new(Fake::default());
        fake.responses.lock().expect("lock").extend([
            response(
                200,
                "https://cdn.example/a/playlist.m3u8",
                &[("content-type", "application/vnd.apple.mpegurl")],
                b"#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:10,\nseg1.m4s\n",
            ),
            response(
                200,
                "https://cdn.example/a/seg1.m4s",
                &[("content-type", "audio/mp4")],
                b"segment",
            ),
        ]);
        let (source, id) = resolved_source(
            fake.clone(),
            "https://cdn.example/a/playlist.m3u8",
            PlaybackFormat::Hls,
        );
        let playlist = source.playlist(&id).await.expect("playlist");
        let prefix = format!("/api/v1/external/tracks/{id}/resources/");
        let tokens: Vec<String> = playlist
            .split(['\n', '"'])
            .filter_map(|part| part.strip_prefix(&prefix))
            .map(str::to_owned)
            .collect();
        assert_eq!(tokens.len(), 2, "{playlist}");
        assert!(!playlist.contains("cdn.example"));
        let Resource::Media(segment) = source
            .resource(&id, &tokens[1], None)
            .await
            .expect("segment")
        else {
            panic!("a segment is media");
        };
        assert_eq!(text(segment).await, "segment");
        {
            let fetched = fake.fetched.lock().expect("lock");
            assert_eq!(fetched[1].0, "https://cdn.example/a/seg1.m4s");
            assert_eq!(fetched[1].2["user-agent"], "yt-dlp-agent");
        }
        // Bound to this Track's id, and only as signed.
        let other = source
            .tracks
            .sign(&TrackRef::new(
                "https://soundcloud.com/a/other",
                "Soundcloud",
            ))
            .expect("id");
        for (track, resource) in [
            (other.as_str(), tokens[1].as_str()),
            (id.as_str(), &format!("{}x", tokens[1])),
            (id.as_str(), "https%3A%2F%2Fcdn.example%2Fx"),
        ] {
            assert!(matches!(
                source.resource(track, resource, None).await,
                Err(ExternalError::BadRequest(_))
            ));
        }
        assert_eq!(fake.fetched.lock().expect("lock").len(), 2);
    }

    #[tokio::test]
    async fn drm_and_private_playlist_uris_are_refused() {
        let fake = Arc::new(Fake::default());
        fake.responses.lock().expect("lock").extend([
            response(
                200,
                "https://cdn.example/a.m3u8",
                &[],
                b"#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"k\"\n",
            ),
            response(
                200,
                "https://cdn.example/a.m3u8",
                &[],
                b"#EXTM3U\nhttp://192.168.1.1/seg.aac\n",
            ),
        ]);
        let (source, id) = resolved_source(
            fake.clone(),
            "https://cdn.example/a.m3u8",
            PlaybackFormat::Hls,
        );
        assert!(matches!(
            source.playlist(&id).await,
            Err(ExternalError::Unplayable(_))
        ));
        assert!(matches!(
            source.playlist(&id).await,
            Err(ExternalError::Failed(_))
        ));
    }

    #[tokio::test]
    async fn refused_urls_are_resolved_again_next_time() {
        let fake = Arc::new(Fake::default());
        fake.responses.lock().expect("lock").push(response(
            403,
            "https://media.example/a.m4a",
            &[],
            b"",
        ));
        let (source, id) = resolved_source(
            fake.clone(),
            "https://media.example/a.m4a",
            PlaybackFormat::Progressive,
        );
        assert!(matches!(
            source.open_stream(&id, None, false, &mut StreamTiming::default()).await,
            Err(ExternalError::Failed(message)) if message.contains("403")
        ));
        assert!(source.cache.get(&id).is_none());
        // The next request runs yt-dlp, which this test has not installed.
        assert!(matches!(
            source.playback(&id, &mut ResolveTiming::default()).await,
            Err(ExternalError::Unavailable(_))
        ));
    }

    // Running yt-dlp again moments after it printed the URL would only
    // print the same URL again.
    #[tokio::test]
    async fn urls_refused_just_after_resolving_are_kept() {
        let fake = Arc::new(Fake::default());
        fake.responses.lock().expect("lock").push(response(
            403,
            "https://media.example/a.m4a",
            &[],
            b"",
        ));
        let (source, id) = resolved_source(
            fake.clone(),
            "https://media.example/a.m4a",
            PlaybackFormat::Progressive,
        );
        let resolved = source.cache.get(&id).expect("cached");
        source.cache.insert(&id, resolved, CACHE_TTL);
        assert!(
            source
                .open_stream(&id, None, false, &mut StreamTiming::default())
                .await
                .is_err()
        );
        assert!(source.cache.get(&id).is_some());
    }

    #[tokio::test]
    async fn nothing_runs_while_disabled() {
        let source = ExternalSource::with_upstream(None, Arc::new(Fake::default()));
        assert!(!source.is_enabled());
        assert_eq!(
            source
                .search("daft punk", 10, &mut RunTiming::default())
                .await,
            Err(ExternalError::Disabled)
        );
        assert_eq!(
            source.playback("x", &mut ResolveTiming::default()).await,
            Err(ExternalError::Disabled)
        );
        assert!(matches!(
            source.resource("x", "y", None).await,
            Err(ExternalError::Disabled)
        ));
        assert_eq!(source.artwork("x").await, Err(ExternalError::Disabled));
    }

    /// A source and the id of a Track whose search found `artwork`.
    fn artwork_source(fake: Arc<Fake>, artwork: Option<&str>) -> (ExternalSource, String) {
        let source = ExternalSource::with_upstream(Some(&config()), fake);
        let id = source
            .tracks
            .sign(&TrackRef {
                artwork_url: artwork.map(str::to_owned),
                ..TrackRef::new("https://soundcloud.com/a/b", "Soundcloud")
            })
            .expect("id");
        (source, id)
    }

    const ARTWORK: &str = "https://i1.sndcdn.com/artworks-x-t300x300.jpg";

    #[tokio::test]
    async fn artwork_is_fetched_from_the_signed_url_only_as_a_small_image() {
        let fake = Arc::new(Fake::default());
        fake.responses.lock().expect("lock").push(response(
            200,
            ARTWORK,
            &[("content-type", "image/JPEG; charset=binary")],
            b"\xff\xd8\xff",
        ));
        let (source, id) = artwork_source(fake.clone(), Some(ARTWORK));
        assert_eq!(
            source.artwork(&id).await,
            Ok(Artwork {
                media_type: "image/jpeg",
                data: Bytes::from_static(b"\xff\xd8\xff"),
            })
        );
        {
            let fetched = fake.fetched.lock().expect("lock");
            assert_eq!(fetched[0].0, ARTWORK);
            assert!(fetched[0].1.is_none());
        }

        let large: &'static [u8] = Box::leak(vec![0_u8; ARTWORK_MAX_BYTES + 1].into_boxed_slice());
        fake.responses.lock().expect("lock").extend([
            response(200, ARTWORK, &[("content-type", "text/html")], b"<html>"),
            response(200, ARTWORK, &[], b"x"),
            response(
                200,
                ARTWORK,
                &[("content-type", "image/svg+xml")],
                b"<svg/>",
            ),
            response(200, ARTWORK, &[("content-type", "image/png")], large),
            response(
                200,
                ARTWORK,
                &[("content-type", "image/png"), ("content-length", "3000000")],
                b"x",
            ),
            response(404, ARTWORK, &[("content-type", "image/png")], b""),
        ]);
        for _ in 0..6 {
            assert!(matches!(
                source.artwork(&id).await,
                Err(ExternalError::Failed(message)) if !message.contains("sndcdn")
            ));
        }
    }

    #[tokio::test]
    async fn artwork_needs_a_signed_id_that_has_artwork() {
        let fake = Arc::new(Fake::default());
        let (source, id) = artwork_source(fake.clone(), None);
        assert_eq!(
            source.artwork(&id).await,
            Err(ExternalError::ArtworkMissing)
        );
        let (source, id) = artwork_source(fake.clone(), Some(ARTWORK));
        let (payload, signature) = id.split_once('.').expect("dot");
        for bad in [format!("{payload}A.{signature}"), "not-an-id".to_owned()] {
            assert!(matches!(
                source.artwork(&bad).await,
                Err(ExternalError::BadRequest(_))
            ));
        }
        assert!(fake.fetched.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn queries_and_limits_are_checked_before_yt_dlp_runs() {
        let source = ExternalSource::with_upstream(Some(&config()), Arc::new(Fake::default()));
        for (query, limit) in [
            ("", 10),
            ("\u{7}  \n", 10),
            (&*"a".repeat(201), 10),
            ("a", 0),
            ("a", 21),
        ] {
            assert!(
                matches!(
                    source.search(query, limit, &mut RunTiming::default()).await,
                    Err(ExternalError::BadRequest(_))
                ),
                "{query:?} {limit}"
            );
        }
        assert!(matches!(
            source
                .playback("not-an-id", &mut ResolveTiming::default())
                .await,
            Err(ExternalError::BadRequest(_))
        ));
    }

    #[test]
    fn resolved_urls_are_reused_until_shortly_before_they_expire() {
        let now = 1_000_000;
        assert_eq!(cache_lifetime("https://a.example/x", now), CACHE_TTL);
        assert_eq!(
            cache_lifetime(&format!("https://a.example/x?expire={}", now + 130), now),
            Duration::from_secs(100)
        );
        assert_eq!(
            cache_lifetime(
                &format!("https://a.example/x?a=1&expires={}", now + 10),
                now
            ),
            Duration::ZERO
        );
        assert_eq!(
            cache_lifetime(&format!("https://a.example/x?expire={}", now + 86_400), now),
            CACHE_TTL
        );
    }

    #[test]
    fn only_known_request_headers_are_forwarded() {
        let resolved = Resolved::from(ResolvedFormat {
            url: "https://a.example/x".to_owned(),
            hls: false,
            headers: vec![
                ("User-Agent".to_owned(), "agent".to_owned()),
                ("Host".to_owned(), "internal".to_owned()),
                ("Range".to_owned(), "bytes=5-".to_owned()),
                ("Bad Name".to_owned(), "x".to_owned()),
            ],
            chunk_size: None,
        });
        assert_eq!(resolved.headers.len(), 1);
        assert_eq!(resolved.headers["user-agent"], "agent");
    }
}
