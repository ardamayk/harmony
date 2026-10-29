//! Where External Source time goes. yt-dlp and the service fill in plain
//! timing records as they work; each handler holds a log guard that writes
//! one info line when its request ends, or when the request goes away first
//! (`outcome` "cancelled"), and builds the request's `Server-Timing` value.
//! Lines carry milliseconds, counts, the Track's fingerprint and the
//! query's length: never a URL or the query itself. Requests refused before
//! any work (source disabled, bad request, no artwork) are not logged.

use super::service::ExternalError;
use super::token::fingerprint;
use axum::http::HeaderValue;
use bytes::Bytes;
use futures_util::stream::{BoxStream, Stream, StreamExt};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

fn ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// One yt-dlp run: the wait for a turn, the process from spawn to exit, and
/// reading its output. A step not reached stays `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RunTiming {
    pub queue: Option<Duration>,
    pub run: Option<Duration>,
    pub parse: Option<Duration>,
}

/// How a resolve was answered: from the cache, by a new yt-dlp run, or by
/// joining the run another request started.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CacheUse {
    Hit,
    Miss,
    Joined,
}

impl CacheUse {
    fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Joined => "joined",
        }
    }
}

/// One resolve. `run` is the yt-dlp run it waited for, also one it joined;
/// `total` stays `None` until it finishes.
#[derive(Clone, Debug, Default)]
pub struct ResolveTiming {
    pub cache: Option<CacheUse>,
    pub run: RunTiming,
    pub total: Option<Duration>,
    pub format: Option<&'static str>,
    pub extractor: Option<String>,
}

/// One stream request: its resolve, the wait for the first upstream
/// response headers, and how many upstream requests it made (one per chunk).
#[derive(Debug, Default)]
pub struct StreamTiming {
    pub resolve: ResolveTiming,
    pub upstream_ttfb: Option<Duration>,
    pub chunks: Arc<AtomicU64>,
}

/// A `Server-Timing` value: `name;dur=<ms>` and `name;desc=<token>` entries.
#[derive(Default)]
struct ServerTiming(Vec<String>);

impl ServerTiming {
    fn dur(mut self, name: &str, duration: Option<Duration>) -> Self {
        if let Some(duration) = duration {
            self.0.push(format!("{name};dur={}", ms(duration)));
        }
        self
    }

    fn desc(mut self, name: &str, token: Option<&str>) -> Self {
        if let Some(token) = token {
            self.0.push(format!("{name};desc={token}"));
        }
        self
    }

    fn header(self) -> Option<HeaderValue> {
        (!self.0.is_empty())
            .then(|| HeaderValue::from_str(&self.0.join(", ")).ok())
            .flatten()
    }
}

/// How a request ended, shared by every log line.
pub struct End {
    started: Instant,
    total: Option<Duration>,
    outcome: Option<&'static str>,
    status: Option<u16>,
    quiet: bool,
}

impl End {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            total: None,
            outcome: None,
            status: None,
            quiet: false,
        }
    }

    pub fn ok(&mut self, status: u16) {
        self.status = Some(status);
        self.finish("ok");
    }

    /// Records a failure answered with `status`; one refused before any
    /// work is not logged.
    pub fn fail(&mut self, error: &ExternalError, status: u16) {
        self.status = Some(status);
        match error {
            ExternalError::Disabled
            | ExternalError::BadRequest(_)
            | ExternalError::ArtworkMissing => self.quiet = true,
            ExternalError::Timeout(_) => self.finish("timeout"),
            _ => self.finish("failed"),
        }
    }

    fn finish(&mut self, outcome: &'static str) {
        self.outcome.get_or_insert(outcome);
        self.total.get_or_insert_with(|| self.started.elapsed());
    }

    fn total(&self) -> Duration {
        self.total.unwrap_or_else(|| self.started.elapsed())
    }

    /// The line's outcome and total, unless it is not logged.
    fn line(&self) -> Option<(&'static str, u64)> {
        (!self.quiet).then(|| (self.outcome.unwrap_or("cancelled"), ms(self.total())))
    }
}

/// One search: `run` is filled in by yt-dlp.
pub struct SearchLog {
    pub end: End,
    pub run: RunTiming,
    query_len: usize,
    results: Option<usize>,
}

impl SearchLog {
    pub fn start(query: &str) -> Self {
        Self {
            end: End::new(),
            run: RunTiming::default(),
            query_len: query.chars().count(),
            results: None,
        }
    }

    pub fn found(&mut self, results: usize) {
        self.results = Some(results);
        self.end.ok(200);
    }

    pub fn server_timing(&self) -> Option<HeaderValue> {
        ServerTiming::default()
            .dur("queue", self.run.queue)
            .dur("ytdlp", self.run.run)
            .dur("parse", self.run.parse)
            .dur("total", Some(self.end.total()))
            .header()
    }
}

impl Drop for SearchLog {
    fn drop(&mut self) {
        let Some((outcome, total_ms)) = self.end.line() else {
            return;
        };
        tracing::info!(
            query_len = self.query_len,
            queue_ms = self.run.queue.map(ms),
            spawn_to_exit_ms = self.run.run.map(ms),
            parse_ms = self.run.parse.map(ms),
            total_ms,
            results = self.results,
            status = self.end.status,
            outcome,
            "External Source search"
        );
    }
}

/// One playback request, which resolves the Track.
pub struct ResolveLog {
    pub end: End,
    pub timing: ResolveTiming,
    track: String,
}

impl ResolveLog {
    pub fn start(id: &str) -> Self {
        Self {
            end: End::new(),
            timing: ResolveTiming::default(),
            track: fingerprint(id),
        }
    }

    pub fn server_timing(&self) -> Option<HeaderValue> {
        ServerTiming::default()
            .desc("cache", self.timing.cache.map(CacheUse::as_str))
            .dur("queue", self.timing.run.queue)
            .dur("ytdlp", self.timing.run.run)
            .dur("total", Some(self.end.total()))
            .header()
    }
}

impl Drop for ResolveLog {
    fn drop(&mut self) {
        let Some((outcome, total_ms)) = self.end.line() else {
            return;
        };
        tracing::info!(
            track = %self.track,
            cache = self.timing.cache.map(CacheUse::as_str),
            queue_ms = self.timing.run.queue.map(ms),
            yt_dlp_ms = self.timing.run.run.map(ms),
            total_ms,
            format = self.timing.format,
            extractor = self.timing.extractor.as_deref(),
            status = self.end.status,
            outcome,
            "External Track resolve"
        );
    }
}

/// One stream request. Its line is written when the body ends or is
/// dropped; a HEAD's at once.
pub struct StreamLog {
    pub end: End,
    pub timing: StreamTiming,
    track: String,
    range: String,
    bytes: u64,
}

impl StreamLog {
    pub fn start(id: &str, range: Option<&HeaderValue>, head: bool) -> Self {
        Self {
            end: End::new(),
            timing: StreamTiming::default(),
            track: fingerprint(id),
            range: range_label(range, head),
            bytes: 0,
        }
    }

    pub fn server_timing(&self) -> Option<HeaderValue> {
        ServerTiming::default()
            .desc("cache", self.timing.resolve.cache.map(CacheUse::as_str))
            .dur("resolve", self.timing.resolve.total)
            .dur("ttfb", self.timing.upstream_ttfb)
            .header()
    }

    /// `body`, counted, with this line written when it ends.
    pub fn watch(
        mut self,
        status: u16,
        body: BoxStream<'static, Result<Bytes, String>>,
    ) -> BoxStream<'static, Result<Bytes, String>> {
        self.end.status = Some(status);
        if self.range == "head" {
            self.end.finish("ok");
            return body;
        }
        Watched {
            body,
            log: Some(self),
        }
        .boxed()
    }
}

/// The client's `Range` as `<first>-[<last>]`, or `none`, or `head`.
fn range_label(range: Option<&HeaderValue>, head: bool) -> String {
    if head {
        return "head".to_owned();
    }
    let Some(range) = range else {
        return "none".to_owned();
    };
    let spec = range.to_str().unwrap_or_default().trim();
    let spec = spec.strip_prefix("bytes=").unwrap_or(spec);
    if !spec.is_empty()
        && spec.len() <= 64
        && spec
            .chars()
            .all(|character| character.is_ascii_digit() || matches!(character, '-' | ',' | ' '))
    {
        spec.to_owned()
    } else {
        "invalid".to_owned()
    }
}

impl Drop for StreamLog {
    fn drop(&mut self) {
        let Some((outcome, total_ms)) = self.end.line() else {
            return;
        };
        tracing::info!(
            track = %self.track,
            range = %self.range,
            cache = self.timing.resolve.cache.map(CacheUse::as_str),
            resolve_ms = self.timing.resolve.total.map(ms),
            upstream_ttfb_ms = self.timing.upstream_ttfb.map(ms),
            chunks = self.timing.chunks.load(Ordering::Relaxed),
            bytes = self.bytes,
            total_ms,
            status = self.end.status,
            outcome,
            "External Track stream"
        );
    }
}

/// A stream body that counts what it sends and ends its log line.
struct Watched {
    body: BoxStream<'static, Result<Bytes, String>>,
    log: Option<StreamLog>,
}

impl Stream for Watched {
    type Item = Result<Bytes, String>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let item = std::task::ready!(self.body.poll_next_unpin(cx));
        if let Some(log) = &mut self.log {
            match &item {
                Some(Ok(bytes)) => log.bytes += bytes.len() as u64,
                Some(Err(_)) => log.end.finish("failed"),
                None => log.end.finish("ok"),
            }
        }
        if !matches!(item, Some(Ok(_))) {
            // Written now, not when hyper gets round to dropping the body.
            self.log = None;
        }
        Poll::Ready(item)
    }
}

/// One artwork request.
pub struct ArtworkLog {
    pub end: End,
    track: String,
    bytes: Option<usize>,
}

impl ArtworkLog {
    pub fn start(id: &str) -> Self {
        Self {
            end: End::new(),
            track: fingerprint(id),
            bytes: None,
        }
    }

    pub fn served(&mut self, bytes: usize) {
        self.bytes = Some(bytes);
        self.end.ok(200);
    }

    pub fn server_timing(&self) -> Option<HeaderValue> {
        ServerTiming::default()
            .dur("total", Some(self.end.total()))
            .header()
    }
}

impl Drop for ArtworkLog {
    fn drop(&mut self) {
        let Some((outcome, total_ms)) = self.end.line() else {
            return;
        };
        tracing::info!(
            track = %self.track,
            bytes = self.bytes,
            total_ms,
            status = self.end.status,
            outcome,
            "External Track artwork"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;
    use serde_json::Value;
    use std::io::Write;
    use std::sync::Mutex;

    #[derive(Clone, Default)]
    struct Lines(Arc<Mutex<Vec<u8>>>);

    impl Write for Lines {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lock").extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Lines {
        fn read(&self) -> Vec<Value> {
            String::from_utf8(self.0.lock().expect("lock").clone())
                .expect("utf-8")
                .lines()
                .map(|line| serde_json::from_str(line).expect("json line"))
                .collect()
        }
    }

    /// The JSON lines `run` logs, the way the server writes them.
    fn logged(run: impl FnOnce()) -> Vec<Value> {
        let lines = Lines::default();
        let writer = lines.clone();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, run);
        lines.read()
    }

    #[test]
    fn a_search_dropped_mid_run_is_logged_as_cancelled() {
        let lines = logged(|| {
            let search = async {
                let mut log = SearchLog::start("daft punk");
                log.run.queue = Some(Duration::from_millis(12));
                std::future::pending::<()>().await;
                log.found(3);
            };
            // Polled once, then dropped like a disconnected request.
            assert!(search.now_or_never().is_none());
        });
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line["message"], "External Source search");
        assert_eq!(line["outcome"], "cancelled");
        assert_eq!(line["query_len"], 9);
        assert_eq!(line["queue_ms"], 12);
        assert!(line["total_ms"].is_u64());
        assert!(line.get("spawn_to_exit_ms").is_none());
        assert!(line.get("results").is_none());
    }

    #[test]
    fn finished_and_refused_searches() {
        let lines = logged(|| {
            let mut log = SearchLog::start("q");
            log.run = RunTiming {
                queue: Some(Duration::from_millis(12)),
                run: Some(Duration::from_millis(1100)),
                parse: Some(Duration::from_millis(1)),
            };
            log.found(2);
            let timing = log.server_timing().expect("server timing");
            let timing = timing.to_str().expect("text");
            assert!(
                timing.starts_with("queue;dur=12, ytdlp;dur=1100, parse;dur=1, total;dur="),
                "{timing}"
            );
            drop(log);
            let mut refused = SearchLog::start("");
            refused
                .end
                .fail(&ExternalError::BadRequest("q".into()), 400);
            drop(refused);
            let mut slow = SearchLog::start("q");
            slow.end.fail(&ExternalError::Timeout("t".into()), 504);
        });
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["outcome"], "ok");
        assert_eq!(lines[0]["spawn_to_exit_ms"], 1100);
        assert_eq!(lines[0]["results"], 2);
        assert_eq!(lines[0]["status"], 200);
        assert_eq!(lines[1]["outcome"], "timeout");
        assert_eq!(lines[1]["status"], 504);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stream_lines_count_the_body_and_end_with_it() {
        let lines = Lines::default();
        let writer = lines.clone();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_writer(move || writer.clone())
            .finish();
        let _default = tracing::subscriber::set_default(subscriber);
        let body = || {
            futures_util::stream::iter([
                Ok(Bytes::from_static(b"ab")),
                Ok(Bytes::from_static(b"cde")),
            ])
            .boxed()
        };

        let mut log = StreamLog::start("id", Some(&HeaderValue::from_static("bytes=0-")), false);
        log.timing.resolve.cache = Some(CacheUse::Hit);
        log.timing.resolve.total = Some(Duration::from_millis(1));
        log.timing.upstream_ttfb = Some(Duration::from_millis(250));
        log.timing.chunks.fetch_add(2, Ordering::Relaxed);
        assert_eq!(
            log.server_timing().expect("server timing"),
            "cache;desc=hit, resolve;dur=1, ttfb;dur=250"
        );
        let mut watched = log.watch(206, body());
        while watched.next().await.is_some() {}
        // Written at the end of the body, not when it is dropped.
        assert_eq!(lines.read().len(), 1);
        drop(watched);

        let log = StreamLog::start("id", None, false);
        let mut watched = log.watch(200, body());
        watched.next().await;
        drop(watched);

        drop(StreamLog::start("id", None, true).watch(200, body()));

        let lines = lines.read();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["message"], "External Track stream");
        assert_eq!(lines[0]["track"], fingerprint("id"));
        assert_eq!(lines[0]["range"], "0-");
        assert_eq!(lines[0]["cache"], "hit");
        assert_eq!(lines[0]["upstream_ttfb_ms"], 250);
        assert_eq!(lines[0]["chunks"], 2);
        assert_eq!(lines[0]["bytes"], 5);
        assert_eq!(lines[0]["status"], 206);
        assert_eq!(lines[0]["outcome"], "ok");
        assert_eq!(
            (&lines[1]["range"], &lines[1]["bytes"], &lines[1]["outcome"]),
            (
                &Value::from("none"),
                &Value::from(2),
                &Value::from("cancelled")
            )
        );
        assert_eq!(
            (&lines[2]["range"], &lines[2]["outcome"]),
            (&Value::from("head"), &Value::from("ok"))
        );
    }

    #[test]
    fn ranges_are_logged_as_the_client_sent_them() {
        let label =
            |value: &'static str| range_label(Some(&HeaderValue::from_static(value)), false);
        assert_eq!(label("bytes=0-"), "0-");
        assert_eq!(label("bytes=123-456"), "123-456");
        assert_eq!(label("bytes=-500"), "-500");
        assert_eq!(label("items=https://a.example/"), "invalid");
        assert_eq!(range_label(None, false), "none");
        assert_eq!(range_label(None, true), "head");
    }
}
