//! External Source rules: search through yt-dlp, resolve an External Track
//! once and cache where its audio is, and fetch that audio only through the
//! radio proxy's SSRF guards. Only URLs this server's own search produced
//! (signed ids) are resolved, yt-dlp is pinned to the extractor that found
//! them, and every resolved, playlist and segment URL must be a public
//! HTTP(S) address. Resolved URLs are signed and expire, so they are never
//! logged; logs name the id's fingerprint and the extractor. Artwork comes
//! only from the HTTPS URL the search signed into the id, through the same
//! guards, and is never logged either. No HTTP types: the handlers read
//! requests into [`RangeRequest`] and answer [`Media`], and `http.rs`
//! adapts the [`Fetcher`] to the radio proxy's client.

use super::timing::{CacheUse, ResolveTiming, RunTiming, StreamTiming};
use super::token::{TokenSigner, TrackRef, fingerprint, is_artwork_url};
use super::yt_dlp::{MAX_SEARCH_RESULTS, ResolvedFormat, YtDlp};
use crate::config::ExternalSourceConfig;
use crate::modules::radio::{
    HLS_RESOURCE_TOKEN_TTL_SECONDS, MAX_HLS_PLAYLIST_BYTES, ResourceSigner, RewriteError,
    is_hls_content, rewrite_playlist_with, validate_stream_url,
};
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
/// How many HLS Tracks keep their request headers (see [`HlsHeaders`]).
const HLS_HEADERS_CAPACITY: usize = 256;
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

/// A body as it arrives; an `Err` ends it early.
pub type ByteStream = BoxStream<'static, Result<Bytes, String>>;

/// Request headers for the site's servers, as `(lowercase name, value)`
/// pairs; a later value of a name replaces an earlier one.
pub type RequestHeaders = Vec<(String, String)>;

/// The part of a file a client asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum RangeRequest {
    /// No range: the whole file.
    Whole,
    /// One `bytes=<start>-[<end>]` range.
    Single { start: u64, end: Option<u64> },
    /// Any other range (suffix or multiple ranges), passed on as written.
    Other(Bytes),
}

/// The headers of an upstream media response this module reads or passes
/// on, as text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaHeaders {
    pub content_type: Option<String>,
    pub content_length: Option<String>,
    /// `bytes <first>-<last>/<total>`.
    pub content_range: Option<String>,
    pub accept_ranges: Option<String>,
    pub cache_control: Option<String>,
}

/// One upstream GET and its response, final URL included (after redirects).
pub struct Fetched {
    pub status: u16,
    pub url: Url,
    pub headers: MediaHeaders,
    pub body: ByteStream,
}

/// GETs public URLs for the External Source. The production one goes
/// through the radio proxy's SSRF-guarded client (`http.rs`); tests
/// substitute their own.
pub trait Fetcher: Send + Sync {
    fn fetch(
        &self,
        url: Url,
        range: RangeRequest,
        headers: RequestHeaders,
    ) -> BoxFuture<'static, Result<Fetched, String>>;
}

/// How much of the file a [`Media`] answer is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaStatus {
    /// The whole file.
    Full,
    /// The range asked for; `content_range` says which.
    Partial,
    /// The range asked for starts past the end of the file.
    Unsatisfiable,
}

/// Audio, an HLS segment or a key, as the client gets it.
pub struct Media {
    pub status: MediaStatus,
    pub headers: MediaHeaders,
    pub body: ByteStream,
}

impl From<Fetched> for Media {
    /// An upstream answer passed on as it is.
    fn from(response: Fetched) -> Self {
        Self {
            status: match response.status {
                206 => MediaStatus::Partial,
                416 => MediaStatus::Unsatisfiable,
                _ => MediaStatus::Full,
            },
            headers: response.headers,
            body: response.body,
        }
    }
}

/// What an HLS resource turned out to be.
pub enum Resource {
    /// A nested playlist, already rewritten.
    Playlist(String),
    Media(Media),
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
    headers: RequestHeaders,
    chunk_size: Option<u64>,
}

impl From<ResolvedFormat> for Resolved {
    fn from(resolved: ResolvedFormat) -> Self {
        let headers = resolved
            .headers
            .into_iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), value))
            .filter(|(name, _)| FORWARDED_HEADERS.contains(&name.as_str()))
            .collect();
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

/// `bytes <first>-<last>/<total>`.
fn parse_content_range(headers: &MediaHeaders) -> Option<(u64, u64, u64)> {
    let value = headers.content_range.as_deref()?;
    let (span, total) = value.trim().strip_prefix("bytes ")?.split_once('/')?;
    let (first, last) = span.split_once('-')?;
    Some((first.parse().ok()?, last.parse().ok()?, total.parse().ok()?))
}

fn byte_range(first: u64, last: u64) -> RangeRequest {
    RangeRequest::Single {
        start: first,
        end: Some(last),
    }
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
    /// stale, while it is still cached.
    fn headers(&self, id: &str) -> RequestHeaders {
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

/// The request headers each HLS Track's playlist was fetched with, kept as
/// long as the resource tokens issued in it are valid: its segments still
/// need them after the Track's URL leaves the [`ResolveCache`].
#[derive(Default)]
struct HlsHeaders {
    entries: Mutex<HashMap<String, (RequestHeaders, Instant)>>,
}

impl HlsHeaders {
    /// Keeps `headers` for the resource tokens just issued for `id`.
    fn keep(&self, id: &str, headers: RequestHeaders) {
        let lifetime = Duration::from_secs(HLS_RESOURCE_TOKEN_TTL_SECONDS.unsigned_abs());
        self.keep_until(id, headers, Instant::now() + lifetime);
    }

    fn keep_until(&self, id: &str, headers: RequestHeaders, until: Instant) {
        let mut entries = self.entries.lock().expect("HLS headers lock");
        let now = Instant::now();
        entries.retain(|_, (_, until)| *until > now);
        if !entries.contains_key(id)
            && entries.len() >= HLS_HEADERS_CAPACITY
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, (_, until))| *until)
                .map(|(id, _)| id.clone())
        {
            entries.remove(&oldest);
        }
        entries.insert(id.to_owned(), (headers, until));
    }

    fn get(&self, id: &str) -> Option<RequestHeaders> {
        let entries = self.entries.lock().expect("HLS headers lock");
        entries
            .get(id)
            .filter(|(_, until)| *until > Instant::now())
            .map(|(headers, _)| headers.clone())
    }
}

pub struct ExternalSource {
    /// `None` while the External Source is not configured.
    search_prefix: Option<String>,
    yt_dlp: YtDlp,
    tracks: TokenSigner,
    resources: ResourceSigner,
    fetcher: Arc<dyn Fetcher>,
    cache: Arc<ResolveCache>,
    hls_headers: HlsHeaders,
}

impl ExternalSource {
    pub(crate) fn with_fetcher(
        config: Option<&ExternalSourceConfig>,
        fetcher: Arc<dyn Fetcher>,
    ) -> Self {
        Self {
            search_prefix: config.map(|config| config.search_prefix.clone()),
            yt_dlp: YtDlp::new(
                config.map_or("yt-dlp", |config| config.yt_dlp.as_str()),
                config.map_or(&[], |config| config.extractor_args.as_slice()),
            ),
            tracks: TokenSigner::new(),
            resources: ResourceSigner::new(),
            fetcher,
            cache: Arc::default(),
            hls_headers: HlsHeaders::default(),
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

    /// A progressive Track's audio, `range` passed on. With `head`, a
    /// chunked Track is answered from a 1-byte probe and without a body.
    /// `timing` gets the resolve, the wait for the first upstream response
    /// and the number of upstream requests.
    pub async fn open_stream(
        &self,
        id: &str,
        range: RangeRequest,
        head: bool,
        timing: &mut StreamTiming,
    ) -> Result<Media, ExternalError> {
        let resolved = self.resolve(id, &mut timing.resolve).await?;
        if resolved.format != PlaybackFormat::Progressive {
            return Err(ExternalError::Unplayable(
                "the External Track plays as HLS; use its playlist".to_owned(),
            ));
        }
        let requested = match &range {
            RangeRequest::Whole => Some((0, None)),
            RangeRequest::Single { start, end } => Some((*start, *end)),
            RangeRequest::Other(_) => None,
        };
        let ranged = range != RangeRequest::Whole;
        let requested_at = Instant::now();
        timing.chunks.fetch_add(1, Ordering::Relaxed);
        let response = match (resolved.chunk_size, requested) {
            (Some(chunk_size), Some((start, end))) => {
                self.fetch_in_chunks(
                    id,
                    &resolved,
                    chunk_size,
                    (start, end),
                    (ranged, head),
                    timing.chunks.clone(),
                )
                .await
            }
            _ => self
                .fetch(id, &resolved.url, resolved.headers.clone(), range)
                .await
                .map(Media::from),
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
    ) -> Result<Media, ExternalError> {
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
                byte_range(start, end.map_or(chunk_last, |end| end.min(chunk_last))),
            )
            .await?;
        let Some((_, served_last, total)) =
            parse_content_range(&first.headers).filter(|_| first.status == 206)
        else {
            // A 416, or a server that ignored the range: passed on as it is.
            return Ok(first.into());
        };
        validate_stream_url(first.url.as_str()).map_err(ExternalError::Failed)?;
        let file_last = total.saturating_sub(1);
        let last = end.map_or(file_last, |end| end.min(file_last));
        let mut headers = first.headers.clone();
        headers.content_length = Some((last + 1 - start).to_string());
        headers.accept_ranges = Some("bytes".to_owned());
        let status = if ranged {
            headers.content_range = Some(format!("bytes {start}-{last}/{total}"));
            MediaStatus::Partial
        } else {
            headers.content_range = None;
            MediaStatus::Full
        };
        if head {
            return Ok(Media {
                status,
                headers,
                body: futures_util::stream::empty().boxed(),
            });
        }
        let fetcher = self.fetcher.clone();
        let url = first.url.clone();
        let request_headers = resolved.headers.clone();
        let track_hash = fingerprint(id);
        let rest = futures_util::stream::unfold(served_last + 1, move |next| {
            let fetcher = fetcher.clone();
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
                    let body: ByteStream = futures_util::stream::iter([Err(message)]).boxed();
                    Some((body, u64::MAX))
                };
                match fetcher
                    .fetch(url, byte_range(next, chunk_last), request_headers)
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
        Ok(Media {
            status,
            headers,
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
            .fetch(
                id,
                &resolved.url,
                resolved.headers.clone(),
                RangeRequest::Whole,
            )
            .await?;
        self.rewrite(id, resolved.headers.clone(), response).await
    }

    /// One URI of a rewritten playlist: a nested playlist (rewritten too),
    /// a segment or a key, fetched with the request headers its playlist
    /// was.
    pub async fn resource(
        &self,
        id: &str,
        resource: &str,
        range: RangeRequest,
    ) -> Result<Resource, ExternalError> {
        self.ensure_enabled()?;
        self.tracks.verify(id).map_err(ExternalError::BadRequest)?;
        let url = self
            .resources
            .verify(resource, &fingerprint(id))
            .map_err(ExternalError::BadRequest)?;
        let headers = self
            .hls_headers
            .get(id)
            .unwrap_or_else(|| self.cache.headers(id));
        let response = self.fetch(id, &url, headers.clone(), range).await?;
        if is_hls_content(response.headers.content_type.as_deref(), &response.url) {
            return Ok(Resource::Playlist(
                self.rewrite(id, headers, response).await?,
            ));
        }
        Ok(Resource::Media(response.into()))
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
        let failed = |message: String| {
            tracing::warn!(track = %fingerprint(id), error = %message, "External Track artwork failed");
            ExternalError::Failed(message)
        };
        if !is_artwork_url(url) {
            return Err(failed("unsafe artwork URL".to_owned()));
        }
        let parsed = Url::parse(url).map_err(|error| failed(error.to_string()))?;
        let headers = vec![(
            "accept".to_owned(),
            "image/jpeg, image/png, image/webp".to_owned(),
        )];
        let response = self
            .fetcher
            .fetch(parsed, RangeRequest::Whole, headers)
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
            .content_type
            .as_deref()
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
            .content_length
            .as_deref()
            .and_then(|value| value.parse::<u64>().ok())
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
                let waiting = Instant::now();
                let (result, run) = resolution.await;
                // A joined run started before this request and is recorded
                // by the one that started it; this one only waited.
                if cache == CacheUse::Joined {
                    timing.wait = Some(waiting.elapsed());
                } else {
                    timing.run = run;
                }
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
        headers: RequestHeaders,
        range: RangeRequest,
    ) -> Result<Fetched, ExternalError> {
        validate_stream_url(url).map_err(ExternalError::Failed)?;
        let parsed = Url::parse(url).map_err(|error| ExternalError::Failed(error.to_string()))?;
        let response = self
            .fetcher
            .fetch(parsed, range, headers)
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

    /// The playlist in `response` with every URI signed as a resource of
    /// the Track; those resources are later fetched with `headers`.
    async fn rewrite(
        &self,
        id: &str,
        headers: RequestHeaders,
        response: Fetched,
    ) -> Result<String, ExternalError> {
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
        let rewritten = rewrite_playlist_with(&String::from_utf8_lossy(&playlist), &base, |url| {
            resource_path(id, &self.resources.sign(&scope, url))
        })
        .map_err(|error| match error {
            RewriteError::Drm => ExternalError::Unplayable(error.to_string()),
            RewriteError::Invalid(message) => ExternalError::Failed(message),
        })?;
        self.hls_headers.keep(id, headers);
        Ok(rewritten)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        responses: Mutex<Vec<Fetched>>,
        fetched: Mutex<Vec<(String, RangeRequest, RequestHeaders)>>,
    }

    impl Fetcher for Fake {
        fn fetch(
            &self,
            url: Url,
            range: RangeRequest,
            headers: RequestHeaders,
        ) -> BoxFuture<'static, Result<Fetched, String>> {
            self.fetched
                .lock()
                .expect("lock")
                .push((url.to_string(), range, headers));
            let response = self.responses.lock().expect("lock").remove(0);
            Box::pin(async move { Ok(response) })
        }
    }

    /// The value a request header was sent with.
    fn header<'a>(headers: &'a RequestHeaders, name: &str) -> Option<&'a str> {
        headers
            .iter()
            .rev()
            .find(|(sent, _)| sent == name)
            .map(|(_, value)| value.as_str())
    }

    fn response(status: u16, url: &str, headers: &[(&str, &str)], body: &'static [u8]) -> Fetched {
        let mut map = MediaHeaders::default();
        for (name, value) in headers {
            let value = Some((*value).to_owned());
            match *name {
                "content-type" => map.content_type = value,
                "content-length" => map.content_length = value,
                "content-range" => map.content_range = value,
                "accept-ranges" => map.accept_ranges = value,
                "cache-control" => map.cache_control = value,
                other => panic!("not a media header: {other}"),
            }
        }
        let body: ByteStream = futures_util::stream::iter([Ok(Bytes::from_static(body))]).boxed();
        Fetched {
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

    fn agent(value: &str) -> RequestHeaders {
        vec![("user-agent".to_owned(), value.to_owned())]
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
        let source = ExternalSource::with_fetcher(Some(&config()), fake);
        let id = source
            .tracks
            .sign(&TrackRef::new("https://soundcloud.com/a/b", "Soundcloud"))
            .expect("id");
        // Resolved a while ago, so a refusal now means the URL went stale.
        source.cache.insert_resolved_at(
            &id,
            Arc::new(Resolved {
                url: url.to_owned(),
                format,
                headers: agent("yt-dlp-agent"),
                chunk_size,
            }),
            CACHE_TTL,
            Instant::now()
                .checked_sub(REFUSAL_GRACE)
                .expect("an earlier instant"),
        );
        (source, id)
    }

    fn bytes(start: u64, end: Option<u64>) -> RangeRequest {
        RangeRequest::Single { start, end }
    }

    async fn text(body: ByteStream) -> String {
        let chunks: Vec<_> = body.collect().await;
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
            .open_stream(&id, bytes(0, Some(1)), false, &mut StreamTiming::default())
            .await
            .expect("stream");
        assert_eq!(upstream.status, MediaStatus::Partial);
        assert_eq!(
            upstream.headers.content_range.as_deref(),
            Some("bytes 0-1/10")
        );
        assert_eq!(text(upstream.body).await, "ab");
        let fetched = fake.fetched.lock().expect("lock");
        assert_eq!(fetched[0].0, "https://media.example/a.m4a");
        assert_eq!(fetched[0].1, bytes(0, Some(1)));
        assert_eq!(header(&fetched[0].2, "user-agent"), Some("yt-dlp-agent"));
    }

    #[tokio::test]
    async fn other_ranges_and_upstream_statuses_are_passed_on() {
        let fake = Arc::new(Fake::default());
        fake.responses.lock().expect("lock").extend([
            response(200, "https://media.example/a.m4a", &[], b"ab"),
            response(416, "https://media.example/a.m4a", &[], b""),
        ]);
        let (source, id) = resolved_source(
            fake.clone(),
            "https://media.example/a.m4a",
            PlaybackFormat::Progressive,
        );
        let suffix = RangeRequest::Other(Bytes::from_static(b"bytes=-500"));
        let whole = source
            .open_stream(&id, suffix.clone(), false, &mut StreamTiming::default())
            .await
            .expect("stream");
        assert_eq!(whole.status, MediaStatus::Full);
        let past = source
            .open_stream(&id, bytes(20, None), false, &mut StreamTiming::default())
            .await
            .expect("stream");
        assert_eq!(past.status, MediaStatus::Unsatisfiable);
        let fetched = fake.fetched.lock().expect("lock");
        assert_eq!(fetched[0].1, suffix);
        assert_eq!(fetched[1].1, bytes(20, None));
    }

    fn chunk(first: u64, last: u64, body: &'static [u8]) -> Fetched {
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
            .map(|(_, range, _)| match range {
                RangeRequest::Single {
                    start,
                    end: Some(end),
                } => format!("bytes={start}-{end}"),
                other => panic!("not a bounded range: {other:?}"),
            })
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
            .open_stream(
                &id,
                RangeRequest::Whole,
                false,
                &mut StreamTiming::default(),
            )
            .await
            .expect("stream");
        assert_eq!(whole.status, MediaStatus::Full);
        assert_eq!(whole.headers.content_length.as_deref(), Some("10"));
        assert!(whole.headers.content_range.is_none());
        assert_eq!(text(whole.body).await, "0123456789");
        assert_eq!(ranges(&fake), ["bytes=0-3", "bytes=4-7", "bytes=8-9"]);

        // A browser's open-ended range, and a bounded one inside a chunk.
        fake.responses.lock().expect("lock").extend([
            chunk(5, 8, b"5678"),
            chunk(9, 9, b"9"),
            chunk(2, 3, b"23"),
        ]);
        let tail = source
            .open_stream(&id, bytes(5, None), false, &mut StreamTiming::default())
            .await
            .expect("stream");
        assert_eq!(tail.status, MediaStatus::Partial);
        assert_eq!(tail.headers.content_range.as_deref(), Some("bytes 5-9/10"));
        assert_eq!(tail.headers.content_length.as_deref(), Some("5"));
        assert_eq!(text(tail.body).await, "56789");
        let part = source
            .open_stream(&id, bytes(2, Some(3)), false, &mut StreamTiming::default())
            .await
            .expect("stream");
        assert_eq!(part.headers.content_range.as_deref(), Some("bytes 2-3/10"));
        assert_eq!(text(part.body).await, "23");
        assert_eq!(ranges(&fake), ["bytes=5-8", "bytes=9-9", "bytes=2-3"]);

        // Past the end: the upstream 416 is passed on.
        fake.responses.lock().expect("lock").push(response(
            416,
            "https://media.example/a.webm",
            &[],
            b"",
        ));
        let past = source
            .open_stream(&id, bytes(20, None), false, &mut StreamTiming::default())
            .await
            .expect("stream");
        assert_eq!(past.status, MediaStatus::Unsatisfiable);
    }

    async fn chunks(response: Media) -> Vec<Result<String, String>> {
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
            .open_stream(&id, bytes(1, None), false, &mut StreamTiming::default())
            .await
            .expect("stream");
        assert_eq!(open.status, MediaStatus::Partial);
        assert_eq!(open.headers.content_range.as_deref(), Some("bytes 1-9/10"));
        assert_eq!(open.headers.content_length.as_deref(), Some("9"));
        assert_eq!(open.headers.accept_ranges.as_deref(), Some("bytes"));
        assert_eq!(open.headers.content_type.as_deref(), Some("audio/webm"));
        assert_eq!(text(open.body).await, "123456789");
        {
            let fetched = fake.fetched.lock().expect("lock");
            assert!(fetched.iter().all(|(url, _, headers)| {
                url == "https://media.example/a.webm"
                    && header(headers, "user-agent") == Some("yt-dlp-agent")
            }));
        }
        assert_eq!(ranges(&fake), ["bytes=1-4", "bytes=5-8", "bytes=9-9"]);

        // A bounded range larger than a chunk, across a boundary.
        fake.responses
            .lock()
            .expect("lock")
            .extend([chunk(2, 5, b"2345"), chunk(6, 6, b"6")]);
        let bounded = source
            .open_stream(&id, bytes(2, Some(6)), false, &mut StreamTiming::default())
            .await
            .expect("stream");
        assert_eq!(bounded.status, MediaStatus::Partial);
        assert_eq!(
            bounded.headers.content_range.as_deref(),
            Some("bytes 2-6/10")
        );
        assert_eq!(bounded.headers.content_length.as_deref(), Some("5"));
        assert_eq!(text(bounded.body).await, "23456");
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
            .open_stream(&id, RangeRequest::Whole, true, &mut StreamTiming::default())
            .await
            .expect("head");
        assert_eq!(head.status, MediaStatus::Full);
        assert_eq!(head.headers.content_length.as_deref(), Some("10"));
        assert_eq!(head.headers.accept_ranges.as_deref(), Some("bytes"));
        assert_eq!(head.headers.content_type.as_deref(), Some("audio/webm"));
        assert!(head.headers.content_range.is_none());
        assert_eq!(text(head.body).await, "");
        let ranged = source
            .open_stream(&id, bytes(4, None), true, &mut StreamTiming::default())
            .await
            .expect("head");
        assert_eq!(ranged.status, MediaStatus::Partial);
        assert_eq!(
            ranged.headers.content_range.as_deref(),
            Some("bytes 4-9/10")
        );
        assert_eq!(ranged.headers.content_length.as_deref(), Some("6"));
        assert_eq!(text(ranged.body).await, "");
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
            .open_stream(
                &id,
                RangeRequest::Whole,
                false,
                &mut StreamTiming::default(),
            )
            .await
            .expect("stream");
        assert_eq!(whole.status, MediaStatus::Full);
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
            .open_stream(
                &id,
                RangeRequest::Whole,
                false,
                &mut StreamTiming::default(),
            )
            .await
            .expect("stream");
        let body = chunks(whole).await;
        assert_eq!(body.len(), 2, "{body:?}");
        assert!(body[1].is_err());
    }

    #[tokio::test]
    async fn private_resolved_urls_are_refused_before_any_fetch() {
        let fake = Arc::new(Fake::default());
        for url in ["http://10.0.0.1/a.mp3", "http://localhost/a.mp3"] {
            let (source, id) = resolved_source(fake.clone(), url, PlaybackFormat::Progressive);
            assert!(matches!(
                source.open_stream(&id, RangeRequest::Whole, false, &mut StreamTiming::default()).await,
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
                .open_stream(
                    &id,
                    RangeRequest::Whole,
                    false,
                    &mut StreamTiming::default()
                )
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

    /// The resource tokens of a rewritten playlist of `id`.
    fn resource_tokens(playlist: &str, id: &str) -> Vec<String> {
        let prefix = format!("/api/v1/external/tracks/{id}/resources/");
        playlist
            .split(['\n', '"'])
            .filter_map(|part| part.strip_prefix(&prefix))
            .map(str::to_owned)
            .collect()
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
        let tokens = resource_tokens(&playlist, &id);
        assert_eq!(tokens.len(), 2, "{playlist}");
        assert!(!playlist.contains("cdn.example"));
        let Resource::Media(segment) = source
            .resource(&id, &tokens[1], RangeRequest::Whole)
            .await
            .expect("segment")
        else {
            panic!("a segment is media");
        };
        assert_eq!(segment.status, MediaStatus::Full);
        assert_eq!(text(segment.body).await, "segment");
        {
            let fetched = fake.fetched.lock().expect("lock");
            assert_eq!(fetched[1].0, "https://cdn.example/a/seg1.m4s");
            assert_eq!(header(&fetched[1].2, "user-agent"), Some("yt-dlp-agent"));
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
                source.resource(track, resource, RangeRequest::Whole).await,
                Err(ExternalError::BadRequest(_))
            ));
        }
        assert_eq!(fake.fetched.lock().expect("lock").len(), 2);
    }

    // Resolving other Tracks drops a Track's stale URL from the resolve
    // cache while a player still fetches the segments its playlist signed;
    // the site refuses those without yt-dlp's headers.
    #[tokio::test]
    async fn hls_resources_keep_their_headers_after_the_track_leaves_the_resolve_cache() {
        let fake = Arc::new(Fake::default());
        fake.responses.lock().expect("lock").extend([
            response(
                200,
                "https://cdn.example/a/playlist.m3u8",
                &[("content-type", "application/vnd.apple.mpegurl")],
                b"#EXTM3U\n#EXTINF:10,\nseg1.m4s\n",
            ),
            response(
                200,
                "https://cdn.example/a/seg1.m4s",
                &[("content-type", "audio/mp4")],
                b"segment",
            ),
        ]);
        let (source, a) = resolved_source(
            fake.clone(),
            "https://cdn.example/a/playlist.m3u8",
            PlaybackFormat::Hls,
        );
        let playlist = source.playlist(&a).await.expect("playlist");
        let tokens = resource_tokens(&playlist, &a);
        assert_eq!(tokens.len(), 1, "{playlist}");

        // A's URL goes stale, and resolving B drops it from the cache.
        let resolved = source.cache.get(&a).expect("cached");
        source.cache.insert(&a, resolved, Duration::ZERO);
        let b = source
            .tracks
            .sign(&TrackRef::new("https://soundcloud.com/a/c", "Soundcloud"))
            .expect("id");
        source.cache.insert(
            &b,
            Arc::new(Resolved {
                url: "https://cdn.example/b/playlist.m3u8".to_owned(),
                format: PlaybackFormat::Hls,
                headers: agent("other-agent"),
                chunk_size: None,
            }),
            CACHE_TTL,
        );
        assert!(source.cache.headers(&a).is_empty());

        let Resource::Media(segment) = source
            .resource(&a, &tokens[0], RangeRequest::Whole)
            .await
            .expect("segment")
        else {
            panic!("a segment is media");
        };
        assert_eq!(text(segment.body).await, "segment");
        let fetched = fake.fetched.lock().expect("lock");
        assert_eq!(fetched[1].0, "https://cdn.example/a/seg1.m4s");
        assert_eq!(header(&fetched[1].2, "user-agent"), Some("yt-dlp-agent"));
    }

    #[test]
    fn hls_headers_expire_with_their_tokens_and_are_bounded() {
        let store = HlsHeaders::default();
        store.keep("a", agent("a"));
        assert_eq!(store.get("a"), Some(agent("a")));
        store.keep_until("stale", agent("stale"), Instant::now());
        assert_eq!(store.get("stale"), None);

        let later = Instant::now() + Duration::from_secs(60);
        for index in 0..HLS_HEADERS_CAPACITY {
            store.keep_until(
                &format!("t{index}"),
                agent("t"),
                later + Duration::from_secs(index as u64),
            );
        }
        // Full: the entry that expires first made room.
        assert_eq!(store.get("t0"), None);
        assert_eq!(store.get("a"), Some(agent("a")));
        assert_eq!(
            store.entries.lock().expect("lock").len(),
            HLS_HEADERS_CAPACITY
        );
    }

    #[tokio::test]
    async fn nested_playlists_are_rewritten_with_the_same_headers() {
        let fake = Arc::new(Fake::default());
        fake.responses.lock().expect("lock").extend([
            response(
                200,
                "https://cdn.example/a/master.m3u8",
                &[("content-type", "application/vnd.apple.mpegurl")],
                b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\naudio.m3u8\n",
            ),
            response(
                200,
                "https://cdn.example/a/audio.m3u8",
                &[],
                b"#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"k\"\n",
            ),
        ]);
        let (source, id) = resolved_source(
            fake.clone(),
            "https://cdn.example/a/master.m3u8",
            PlaybackFormat::Hls,
        );
        let playlist = source.playlist(&id).await.expect("playlist");
        let tokens = resource_tokens(&playlist, &id);
        // A nested DRM playlist is unplayable (422), like a top-level one.
        assert!(matches!(
            source.resource(&id, &tokens[0], RangeRequest::Whole).await,
            Err(ExternalError::Unplayable(_))
        ));
        let fetched = fake.fetched.lock().expect("lock");
        assert_eq!(header(&fetched[1].2, "user-agent"), Some("yt-dlp-agent"));
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
            source.open_stream(&id, RangeRequest::Whole, false, &mut StreamTiming::default()).await,
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
                .open_stream(
                    &id,
                    RangeRequest::Whole,
                    false,
                    &mut StreamTiming::default()
                )
                .await
                .is_err()
        );
        assert!(source.cache.get(&id).is_some());
    }

    #[tokio::test]
    async fn nothing_runs_while_disabled() {
        let source = ExternalSource::with_fetcher(None, Arc::new(Fake::default()));
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
            source.resource("x", "y", RangeRequest::Whole).await,
            Err(ExternalError::Disabled)
        ));
        assert_eq!(source.artwork("x").await, Err(ExternalError::Disabled));
    }

    /// A source and the id of a Track whose search found `artwork`.
    fn artwork_source(fake: Arc<Fake>, artwork: Option<&str>) -> (ExternalSource, String) {
        let source = ExternalSource::with_fetcher(Some(&config()), fake);
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
            assert_eq!(fetched[0].1, RangeRequest::Whole);
            assert_eq!(
                header(&fetched[0].2, "accept"),
                Some("image/jpeg, image/png, image/webp")
            );
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
        let source = ExternalSource::with_fetcher(Some(&config()), Arc::new(Fake::default()));
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

    /// External Track ids and resource tokens decode to page and media
    /// URLs, so text sent to Sentry keeps only the route around them.
    #[test]
    fn ids_and_resource_tokens_do_not_reach_sentry() {
        let (source, id) = artwork_source(Arc::new(Fake::default()), Some(ARTWORK));
        let resource = source.resources.sign(
            &fingerprint(&id),
            "https://media.example/seg-1.aac?sig=secret",
        );
        for (path, route) in [
            (stream_path(&id), "/api/v1/external/tracks/<id>/stream"),
            (
                playlist_path(&id),
                "/api/v1/external/tracks/<id>/playlist.m3u8",
            ),
            (
                resource_path(&id, &resource),
                "/api/v1/external/tracks/<id>/resources/<id>",
            ),
            (
                format!("{TRACKS_PATH}/{id}/artwork"),
                "/api/v1/external/tracks/<id>/artwork",
            ),
        ] {
            assert_eq!(
                crate::observability::scrub_text(&format!("GET {path}?range=1 failed")),
                format!("GET {route} failed")
            );
        }
    }

    /// A resolution another request started, with a long yt-dlp run.
    fn in_flight(source: &ExternalSource, id: &str, run: RunTiming) {
        let resolution = async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            (Err(ExternalError::Failed("upstream".to_owned())), run)
        }
        .boxed()
        .shared();
        source
            .cache
            .in_flight
            .lock()
            .expect("lock")
            .insert(id.to_owned(), resolution);
    }

    #[tokio::test]
    async fn a_joined_resolve_records_its_wait_not_the_shared_run() {
        let (source, id) = artwork_source(Arc::new(Fake::default()), None);
        in_flight(
            &source,
            &id,
            RunTiming {
                queue: Some(Duration::from_secs(5)),
                run: Some(Duration::from_secs(10)),
                parse: Some(Duration::from_millis(1)),
            },
        );
        let mut timing = ResolveTiming::default();
        assert!(source.playback(&id, &mut timing).await.is_err());
        assert_eq!(timing.cache, Some(CacheUse::Joined));
        assert_eq!(timing.run, RunTiming::default());
        let wait = timing.wait.expect("wait");
        assert!(wait >= Duration::from_millis(15), "{wait:?}");
        assert!(wait <= timing.total.expect("total"));
    }

    #[tokio::test]
    async fn a_resolve_that_runs_yt_dlp_records_the_run() {
        let (source, id) = artwork_source(Arc::new(Fake::default()), None);
        let mut timing = ResolveTiming::default();
        // yt-dlp is not installed here: the run fails after its turn.
        assert!(source.playback(&id, &mut timing).await.is_err());
        assert_eq!(timing.cache, Some(CacheUse::Miss));
        assert_eq!(timing.wait, None);
        assert!(timing.run.queue.is_some(), "{:?}", timing.run);
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
        assert_eq!(resolved.headers, agent("agent"));
    }
}
