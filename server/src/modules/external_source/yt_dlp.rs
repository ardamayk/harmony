//! Running the operator's yt-dlp and reading what it prints. yt-dlp gets an
//! argument vector, never a shell; the query and URL always follow `--`, so
//! they cannot be read as options. Every run is time-limited, its output
//! capped, and the process killed when the request goes away.

use super::service::ExternalError;
use super::token::is_extractor_key;
use crate::modules::radio::validate_stream_url;
use serde_json::Value;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::Semaphore;

pub const SEARCH_TIMEOUT: Duration = Duration::from_secs(20);
pub const RESOLVE_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_SEARCH_RESULTS: usize = 20;
const SEARCH_OUTPUT_LIMIT_BYTES: usize = 4 << 20;
const RESOLVE_OUTPUT_LIMIT_BYTES: usize = 8 << 20;
const ERROR_OUTPUT_LIMIT_BYTES: usize = 64 << 10;
const ERROR_MESSAGE_LIMIT_CHARS: usize = 300;
const CONCURRENCY: usize = 2;
/// A smaller `http_chunk_size` would only multiply upstream requests.
const MIN_CHUNK_SIZE_BYTES: u64 = 256 << 10;
/// Audio that plays over plain HTTP(S) first (not DASH fragments), then HLS
/// audio, then any audio-only format.
pub const FORMAT_SELECTOR: &str =
    "bestaudio[protocol^=http][protocol!*=dash]/bestaudio[protocol*=m3u8]/best[vcodec=none]";

/// One search result as yt-dlp listed it.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchItem {
    pub webpage_url: String,
    pub extractor: String,
    pub title: String,
    pub artist_name: Option<String>,
    pub duration_ms: Option<i64>,
}

/// The audio yt-dlp chose for a page.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedFormat {
    pub url: String,
    pub hls: bool,
    /// Request headers yt-dlp would send for it, such as User-Agent.
    pub headers: Vec<(String, String)>,
    /// yt-dlp's `http_chunk_size`: the site serves the file only in byte
    /// ranges of at most this size (YouTube stalls a whole-file GET).
    pub chunk_size: Option<u64>,
}

#[derive(Clone)]
pub struct YtDlp {
    binary: String,
    pub search_timeout: Duration,
    pub resolve_timeout: Duration,
    permits: Arc<Semaphore>,
}

impl YtDlp {
    pub fn new(binary: &str) -> Self {
        Self {
            binary: binary.to_owned(),
            search_timeout: SEARCH_TIMEOUT,
            resolve_timeout: RESOLVE_TIMEOUT,
            permits: Arc::new(Semaphore::new(CONCURRENCY)),
        }
    }

    pub async fn search(
        &self,
        prefix: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchItem>, ExternalError> {
        let limit = limit.clamp(1, MAX_SEARCH_RESULTS);
        let args = search_args(prefix, query, limit);
        let output = self
            .run(args, self.search_timeout, SEARCH_OUTPUT_LIMIT_BYTES)
            .await?;
        parse_search(&output, limit)
    }

    pub async fn resolve(
        &self,
        url: &str,
        extractor: &str,
    ) -> Result<ResolvedFormat, ExternalError> {
        let output = self
            .run(
                resolve_args(url, extractor),
                self.resolve_timeout,
                RESOLVE_OUTPUT_LIMIT_BYTES,
            )
            .await?;
        parse_resolved(&output)
    }

    /// Runs yt-dlp with at most [`CONCURRENCY`] runs at a time; waiting for
    /// a turn counts toward the timeout.
    async fn run(
        &self,
        args: Vec<String>,
        timeout: Duration,
        output_limit: usize,
    ) -> Result<Vec<u8>, ExternalError> {
        let work = async {
            let _permit = self
                .permits
                .acquire()
                .await
                .map_err(|error| ExternalError::Failed(error.to_string()))?;
            let mut child = tokio::process::Command::new(&self.binary)
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .map_err(|error| {
                    ExternalError::Unavailable(format!("yt-dlp could not be started: {error}"))
                })?;
            let stdout = child.stdout.take().expect("piped stdout");
            let stderr = child.stderr.take().expect("piped stderr");
            // Too much output fails at once; dropping `child` kills it.
            let (output, errors) = tokio::try_join!(
                async {
                    let (output, truncated) = read_capped(stdout, output_limit, false).await;
                    if truncated {
                        return Err(ExternalError::Failed(
                            "yt-dlp output exceeds the size limit".to_owned(),
                        ));
                    }
                    Ok(output)
                },
                async { Ok(read_capped(stderr, ERROR_OUTPUT_LIMIT_BYTES, true).await.0) },
            )?;
            let status = child
                .wait()
                .await
                .map_err(|error| ExternalError::Failed(format!("yt-dlp failed: {error}")))?;
            if !status.success() {
                return Err(ExternalError::Failed(
                    last_error_line(&errors).unwrap_or_else(|| format!("yt-dlp {status}")),
                ));
            }
            Ok(output)
        };
        tokio::time::timeout(timeout, work).await.map_err(|_| {
            ExternalError::Timeout(format!(
                "yt-dlp did not finish within {} s",
                timeout.as_secs_f64()
            ))
        })?
    }
}

/// Reads at most `limit` bytes and whether there was more. With `drain` it
/// reads on to the end, so yt-dlp never blocks writing to it; without, it
/// stops at the limit.
async fn read_capped(
    mut reader: impl AsyncRead + Unpin,
    limit: usize,
    drain: bool,
) -> (Vec<u8>, bool) {
    let mut kept = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    while let Ok(read) = reader.read(&mut buffer).await {
        if read == 0 {
            break;
        }
        let room = limit.saturating_sub(kept.len());
        if read > room {
            truncated = true;
            if !drain {
                break;
            }
        }
        kept.extend_from_slice(&buffer[..read.min(room)]);
    }
    (kept, truncated)
}

/// yt-dlp's last `ERROR:` line, which says what failed (and when the site
/// changed, that yt-dlp needs an update), shortened to 300 characters.
fn last_error_line(stderr: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stderr);
    let line = text
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with("ERROR:"))?;
    Some(line.chars().take(ERROR_MESSAGE_LIMIT_CHARS).collect())
}

pub fn search_args(prefix: &str, query: &str, limit: usize) -> Vec<String> {
    [
        "--ignore-config",
        "--no-warnings",
        "--flat-playlist",
        "--dump-json",
        "--socket-timeout",
        "15",
        "--",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain([format!("{prefix}{limit}:{query}")])
    .collect()
}

pub fn resolve_args(url: &str, extractor: &str) -> Vec<String> {
    [
        "--ignore-config",
        "--no-warnings",
        "--no-playlist",
        "--dump-json",
        "--use-extractors",
        extractor,
        "-f",
        FORMAT_SELECTOR,
        "--",
        url,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn json_lines(output: &[u8]) -> Result<Vec<Value>, ExternalError> {
    String::from_utf8_lossy(output)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| match serde_json::from_str::<Value>(line) {
            Ok(value) if value.is_object() => Ok(value),
            _ => Err(ExternalError::Failed(
                "yt-dlp printed output that is not JSON".to_owned(),
            )),
        })
        .collect()
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// Reads `--flat-playlist --dump-json` lines. Results yt-dlp could not
/// resolve again safely (no extractor, the generic extractor, a page that
/// is not a public web URL) are left out.
pub fn parse_search(output: &[u8], limit: usize) -> Result<Vec<SearchItem>, ExternalError> {
    let items = json_lines(output)?
        .iter()
        .filter_map(|entry| {
            let extractor = text(entry, "ie_key").or_else(|| text(entry, "extractor_key"))?;
            if !is_extractor_key(&extractor) || extractor.eq_ignore_ascii_case("generic") {
                return None;
            }
            let webpage_url = text(entry, "webpage_url").or_else(|| text(entry, "url"))?;
            validate_stream_url(&webpage_url).ok()?;
            let duration_ms = entry
                .get("duration")
                .and_then(Value::as_f64)
                .filter(|duration| duration.is_finite() && *duration >= 0.0)
                .map(|duration| (duration * 1000.0).round() as i64);
            Some(SearchItem {
                title: text(entry, "title").unwrap_or_else(|| webpage_url.clone()),
                artist_name: ["artist", "uploader", "channel"]
                    .iter()
                    .find_map(|key| text(entry, key)),
                webpage_url,
                extractor,
                duration_ms,
            })
        })
        .take(limit)
        .collect();
    Ok(items)
}

/// Reads the `--dump-json` of one page after `-f` chose a format: the
/// chosen format's `url`, `protocol` and `http_headers` are top-level.
pub fn parse_resolved(output: &[u8]) -> Result<ResolvedFormat, ExternalError> {
    let unplayable = |message: &str| Err(ExternalError::Unplayable(message.to_owned()));
    let entries = json_lines(output)?;
    let entry = match entries.as_slice() {
        [] => {
            return Err(ExternalError::Failed(
                "yt-dlp printed no External Track".to_owned(),
            ));
        }
        [entry] => entry,
        _ => return unplayable("the External Track is a playlist, not one track"),
    };
    if entry.get("is_live").and_then(Value::as_bool) == Some(true)
        || matches!(
            text(entry, "live_status").as_deref(),
            Some("is_live" | "is_upcoming")
        )
    {
        return unplayable("live External Tracks cannot be played");
    }
    if entry.get("has_drm").and_then(Value::as_bool) == Some(true) {
        return unplayable("the External Track is DRM-protected");
    }
    if text(entry, "acodec").as_deref() == Some("none") {
        return unplayable("the External Track has no audio");
    }
    let Some(url) = text(entry, "url").filter(|url| {
        url::Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
    }) else {
        return unplayable("the External Track has no HTTP audio");
    };
    let hls = match text(entry, "protocol").as_deref() {
        Some("http" | "https") => false,
        Some("m3u8" | "m3u8_native") => true,
        _ => return unplayable("the External Track's audio uses an unsupported protocol"),
    };
    let headers = entry
        .get("http_headers")
        .and_then(Value::as_object)
        .map(|headers| {
            headers
                .iter()
                .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let chunk_size = entry
        .pointer("/downloader_options/http_chunk_size")
        .and_then(Value::as_u64)
        .filter(|size| *size > 0)
        .map(|size| size.max(MIN_CHUNK_SIZE_BYTES));
    Ok(ResolvedFormat {
        url,
        hls,
        headers,
        chunk_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_is_one_argument_after_the_end_of_options() {
        let args = search_args("scsearch", "--exec rm -rf / ; daft punk", 3);
        let end = args.iter().position(|arg| arg == "--").expect("--");
        assert_eq!(
            args[end + 1..],
            ["scsearch3:--exec rm -rf / ; daft punk".to_owned()]
        );
        assert_eq!(args[0], "--ignore-config");
        let args = resolve_args("https://soundcloud.com/a/b", "Soundcloud");
        assert_eq!(
            args[args.len() - 6..],
            [
                "--use-extractors",
                "Soundcloud",
                "-f",
                FORMAT_SELECTOR,
                "--",
                "https://soundcloud.com/a/b"
            ]
            .map(str::to_owned)
        );
    }

    // Shapes measured from yt-dlp 2026.10.26 (`scsearch`, `ytsearch`).
    #[test]
    fn reads_soundcloud_and_youtube_search_results() {
        let output = br#"{"_type": "url", "ie_key": "Soundcloud", "id": "254112221", "url": "https://api.soundcloud.com/tracks/soundcloud%3Atracks%3A254112221", "webpage_url": "https://soundcloud.com/daftpunkofficialmusic/around-the-world", "title": "Around the World", "duration": 30.0, "uploader": "Daft Punk", "artist": "Daft Punk"}
{"_type": "url", "ie_key": "Youtube", "id": "TCd6PfxOy0Y", "url": "https://www.youtube.com/watch?v=TCd6PfxOy0Y", "title": "Veridis Quo", "duration": 346, "uploader": null, "channel": "Daft Punk"}
{"_type": "url", "ie_key": "Generic", "url": "https://example.com/x", "title": "generic"}
{"_type": "url", "ie_key": "Soundcloud", "webpage_url": "http://127.0.0.1/x", "title": "private"}
{"_type": "url", "url": "https://example.com/y", "title": "no extractor"}
{"_type": "url", "ie_key": "Soundcloud", "webpage_url": "https://soundcloud.com/a/b", "duration": 246.381}
"#;
        let items = parse_search(output, 10).expect("items");
        assert_eq!(
            items,
            [
                SearchItem {
                    webpage_url: "https://soundcloud.com/daftpunkofficialmusic/around-the-world"
                        .to_owned(),
                    extractor: "Soundcloud".to_owned(),
                    title: "Around the World".to_owned(),
                    artist_name: Some("Daft Punk".to_owned()),
                    duration_ms: Some(30_000),
                },
                SearchItem {
                    webpage_url: "https://www.youtube.com/watch?v=TCd6PfxOy0Y".to_owned(),
                    extractor: "Youtube".to_owned(),
                    title: "Veridis Quo".to_owned(),
                    artist_name: Some("Daft Punk".to_owned()),
                    duration_ms: Some(346_000),
                },
                SearchItem {
                    webpage_url: "https://soundcloud.com/a/b".to_owned(),
                    extractor: "Soundcloud".to_owned(),
                    title: "https://soundcloud.com/a/b".to_owned(),
                    artist_name: None,
                    duration_ms: Some(246_381),
                },
            ]
        );
        assert_eq!(parse_search(output, 1).expect("items").len(), 1);
        assert!(parse_search(b"", 3).expect("none").is_empty());
        assert!(matches!(
            parse_search(b"<html>", 3),
            Err(ExternalError::Failed(_))
        ));
    }

    #[test]
    fn reads_progressive_and_hls_formats() {
        let youtube = br#"{"_type": "video", "extractor_key": "Youtube", "format_id": "251", "protocol": "https", "acodec": "opus", "vcodec": "none", "url": "https://rr7.googlevideo.com/videoplayback?expire=1793249335", "live_status": "not_live", "has_drm": false, "http_headers": {"User-Agent": "Mozilla/5.0", "Accept": "*/*"}, "downloader_options": {"http_chunk_size": 10485760}}"#;
        let resolved = parse_resolved(youtube).expect("resolved");
        assert!(!resolved.hls);
        assert_eq!(resolved.chunk_size, Some(10_485_760));
        assert_eq!(
            resolved.url,
            "https://rr7.googlevideo.com/videoplayback?expire=1793249335"
        );
        assert!(
            resolved
                .headers
                .contains(&("User-Agent".to_owned(), "Mozilla/5.0".to_owned()))
        );
        let soundcloud = br#"{"format_id": "hls_aac_96k", "protocol": "m3u8_native", "acodec": "mp4a.40.2", "vcodec": "none", "url": "https://playback.media-streaming.soundcloud.cloud/x/aac_96k/playlist.m3u8"}"#;
        let resolved = parse_resolved(soundcloud).expect("resolved");
        assert!(resolved.hls);
        assert_eq!(resolved.chunk_size, None);
    }

    #[test]
    fn refuses_what_cannot_be_played() {
        for output in [
            r#"{"protocol": "https", "url": "https://a.example/x", "is_live": true}"#,
            r#"{"protocol": "https", "url": "https://a.example/x", "live_status": "is_upcoming"}"#,
            r#"{"protocol": "https", "url": "https://a.example/x", "has_drm": true}"#,
            r#"{"protocol": "https", "url": "https://a.example/x", "acodec": "none"}"#,
            r#"{"protocol": "http_dash_segments", "url": "https://a.example/x"}"#,
            r#"{"protocol": "https", "url": "file:///etc/passwd"}"#,
            r#"{"protocol": "https"}"#,
            "{\"protocol\": \"https\", \"url\": \"https://a.example/1\"}\n{\"protocol\": \"https\", \"url\": \"https://a.example/2\"}",
        ] {
            assert!(
                matches!(
                    parse_resolved(output.as_bytes()),
                    Err(ExternalError::Unplayable(_))
                ),
                "{output}"
            );
        }
        assert!(matches!(parse_resolved(b""), Err(ExternalError::Failed(_))));
        assert!(matches!(
            parse_resolved(b"not json"),
            Err(ExternalError::Failed(_))
        ));
    }

    #[test]
    fn keeps_the_last_error_line_shortened() {
        let stderr = format!(
            "[soundcloud] x: Downloading\nERROR: first\nWARNING: w\nERROR: [soundcloud] {}\n",
            "y".repeat(400)
        );
        let line = last_error_line(stderr.as_bytes()).expect("line");
        assert!(line.starts_with("ERROR: [soundcloud] y"));
        assert_eq!(line.chars().count(), 300);
        assert_eq!(last_error_line(b"Traceback\n"), None);
    }
}
