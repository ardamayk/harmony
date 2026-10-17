//! Serving a Track's file the way Go's http.ServeContent does
//! (playback/stream.go, net/http/fs.go): Last-Modified, the RFC 7232
//! preconditions in their order, If-Range, single and multipart byte
//! ranges, and 416 for ranges that miss the file. No ETag is set, as in Go.

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

/// Go caps the ranges it reads from one header.
const MAX_RANGES: usize = 200;

fn content_type(path: &Path) -> String {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "mp3" => "audio/mpeg".to_owned(),
        "flac" => "audio/flac".to_owned(),
        "ogg" | "opus" => "audio/ogg".to_owned(),
        "m4a" => "audio/mp4".to_owned(),
        "wav" => "audio/wav".to_owned(),
        _ => mime_guess::from_ext(&extension)
            .first_raw()
            .unwrap_or("application/octet-stream")
            .to_owned(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Range {
    start: u64,
    length: u64,
}

impl Range {
    fn content_range(self, size: u64) -> String {
        format!(
            "bytes {}-{}/{}",
            self.start,
            self.start + self.length - 1,
            size
        )
    }
}

#[derive(Debug, PartialEq)]
enum RangeError {
    Invalid,
    NoOverlap,
}

/// Go's parseRange over RFC 7233 byte ranges.
fn parse_range(header: &str, size: u64) -> Result<Vec<Range>, RangeError> {
    if header.is_empty() {
        return Ok(Vec::new());
    }
    let Some(specs) = header.strip_prefix("bytes=") else {
        return Err(RangeError::Invalid);
    };
    if specs.matches(',').count() + 1 > MAX_RANGES {
        return Ok(Vec::new());
    }
    let mut ranges = Vec::new();
    let mut no_overlap = false;
    for spec in specs.split(',') {
        let spec = spec.trim_matches([' ', '\t']);
        if spec.is_empty() {
            continue;
        }
        let (start, end) = spec.split_once('-').ok_or(RangeError::Invalid)?;
        let (start, end) = (
            start.trim_matches([' ', '\t']),
            end.trim_matches([' ', '\t']),
        );
        if start.is_empty() {
            if end.is_empty() || end.starts_with('-') {
                return Err(RangeError::Invalid);
            }
            let suffix: i64 = end.parse().map_err(|_| RangeError::Invalid)?;
            if suffix < 0 {
                return Err(RangeError::Invalid);
            }
            let suffix = (suffix as u64).min(size);
            // `bytes=-0` (or any suffix of an empty file) selects no bytes.
            if suffix == 0 {
                no_overlap = true;
                continue;
            }
            ranges.push(Range {
                start: size - suffix,
                length: suffix,
            });
            continue;
        }
        let first: i64 = start.parse().map_err(|_| RangeError::Invalid)?;
        if first < 0 {
            return Err(RangeError::Invalid);
        }
        let first = first as u64;
        if first >= size {
            no_overlap = true;
            continue;
        }
        let length = if end.is_empty() {
            size - first
        } else {
            let last: i64 = end.parse().map_err(|_| RangeError::Invalid)?;
            if (first as i64) > last {
                return Err(RangeError::Invalid);
            }
            (last as u64).min(size - 1) - first + 1
        };
        ranges.push(Range {
            start: first,
            length,
        });
    }
    if no_overlap && ranges.is_empty() {
        return Err(RangeError::NoOverlap);
    }
    Ok(ranges)
}

#[derive(PartialEq)]
enum Condition {
    None,
    True,
    False,
}

fn header_text(headers: &HeaderMap, name: header::HeaderName) -> &str {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
}

/// Go's scanETag: a valid `"…"` or `W/"…"` at the start, and what follows.
fn scan_etag(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_matches([' ', '\t']);
    let start = if text.starts_with("W/") { 2 } else { 0 };
    let bytes = text.as_bytes();
    if bytes.len() < start + 2 || bytes[start] != b'"' {
        return None;
    }
    for (index, &byte) in bytes.iter().enumerate().skip(start + 1) {
        match byte {
            b'"' => return Some((&text[..=index], &text[index + 1..])),
            0x21 | 0x23..=0x7e | 0x80..=0xff => {}
            _ => return None,
        }
    }
    None
}

/// If-Match against a response without an ETag: only `*` matches.
fn check_if_match(headers: &HeaderMap) -> Condition {
    let mut value = header_text(headers, header::IF_MATCH);
    if value.is_empty() {
        return Condition::None;
    }
    loop {
        value = value.trim_matches([' ', '\t']);
        if value.is_empty() {
            break;
        }
        if let Some(rest) = value.strip_prefix(',') {
            value = rest;
            continue;
        }
        if value.starts_with('*') {
            return Condition::True;
        }
        match scan_etag(value) {
            Some((_, rest)) => value = rest,
            None => break,
        }
    }
    Condition::False
}

/// If-None-Match against a response without an ETag: only `*` fails.
fn check_if_none_match(headers: &HeaderMap) -> Condition {
    let mut value = header_text(headers, header::IF_NONE_MATCH);
    if value.is_empty() {
        return Condition::None;
    }
    loop {
        value = value.trim_matches([' ', '\t']);
        if value.is_empty() {
            break;
        }
        if let Some(rest) = value.strip_prefix(',') {
            value = rest;
            continue;
        }
        if value.starts_with('*') {
            return Condition::False;
        }
        match scan_etag(value) {
            // A weak match against the empty ETag needs an empty tag, which
            // scan_etag never yields, so this tag does not match.
            Some((_, rest)) => value = rest,
            None => break,
        }
    }
    Condition::True
}

fn seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// Go's isZeroTime: no time, or exactly the Unix epoch.
fn is_zero_time(modified: Option<SystemTime>) -> bool {
    modified.is_none_or(|modified| modified == UNIX_EPOCH)
}

fn parse_time(text: &str) -> Option<u64> {
    httpdate::parse_http_date(text).ok().map(seconds)
}

fn check_if_unmodified_since(headers: &HeaderMap, modified: Option<SystemTime>) -> Condition {
    let value = header_text(headers, header::IF_UNMODIFIED_SINCE);
    if value.is_empty() || is_zero_time(modified) {
        return Condition::None;
    }
    let Some(since) = parse_time(value) else {
        return Condition::None;
    };
    if seconds(modified.expect("checked")) <= since {
        Condition::True
    } else {
        Condition::False
    }
}

fn check_if_modified_since(
    method: &Method,
    headers: &HeaderMap,
    modified: Option<SystemTime>,
) -> Condition {
    if method != Method::GET && method != Method::HEAD {
        return Condition::None;
    }
    let value = header_text(headers, header::IF_MODIFIED_SINCE);
    if value.is_empty() || is_zero_time(modified) {
        return Condition::None;
    }
    let Some(since) = parse_time(value) else {
        return Condition::None;
    };
    if seconds(modified.expect("checked")) <= since {
        Condition::False
    } else {
        Condition::True
    }
}

fn check_if_range(method: &Method, headers: &HeaderMap, modified: Option<SystemTime>) -> Condition {
    if method != Method::GET && method != Method::HEAD {
        return Condition::None;
    }
    let value = header_text(headers, header::IF_RANGE);
    if value.is_empty() {
        return Condition::None;
    }
    if scan_etag(value).is_some() {
        // Strong match against the empty ETag never succeeds.
        return Condition::False;
    }
    let Some(modified) = modified else {
        return Condition::False;
    };
    match parse_time(value) {
        Some(time) if time == seconds(modified) => Condition::True,
        _ => Condition::False,
    }
}

/// Go's http.Error: text/plain with a trailing newline.
fn plain_error(status: StatusCode, message: &str, mut headers: HeaderMap) -> Response {
    for name in [
        header::CACHE_CONTROL,
        header::CONTENT_ENCODING,
        header::ETAG,
        header::LAST_MODIFIED,
    ] {
        headers.remove(name);
    }
    headers.remove(header::CONTENT_LENGTH);
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    (status, headers, format!("{message}\n")).into_response()
}

pub enum ServeError {
    NotFound,
    Io(std::io::Error),
}

fn random_boundary() -> String {
    let mut bytes = Vec::with_capacity(30);
    while bytes.len() < 30 {
        bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    }
    hex::encode(&bytes[..30])
}

/// Serves `path` with Go's ServeContent rules; `None` body for HEAD.
pub async fn serve(
    method: &Method,
    request: &HeaderMap,
    path: &Path,
) -> Result<Response, ServeError> {
    let missing = |error: std::io::Error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ServeError::NotFound
        } else {
            ServeError::Io(error)
        }
    };
    let metadata = tokio::fs::metadata(path).await.map_err(missing)?;
    let mut file = tokio::fs::File::open(path).await.map_err(missing)?;
    let size = metadata.len();
    let modified = metadata.modified().ok();
    let content_type = content_type(path);

    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&content_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    if !is_zero_time(modified) {
        let modified = UNIX_EPOCH + Duration::from_secs(seconds(modified.expect("checked")));
        headers.insert(
            header::LAST_MODIFIED,
            HeaderValue::from_str(&httpdate::fmt_http_date(modified)).expect("HTTP date"),
        );
    }

    let mut condition = check_if_match(request);
    if condition == Condition::None {
        condition = check_if_unmodified_since(request, modified);
    }
    if condition == Condition::False {
        return Ok((StatusCode::PRECONDITION_FAILED, headers).into_response());
    }
    let not_modified = |mut headers: HeaderMap| {
        headers.remove(header::CONTENT_TYPE);
        headers.remove(header::CONTENT_LENGTH);
        headers.remove(header::CONTENT_ENCODING);
        (StatusCode::NOT_MODIFIED, headers).into_response()
    };
    match check_if_none_match(request) {
        Condition::False if method == Method::GET || method == Method::HEAD => {
            return Ok(not_modified(headers));
        }
        Condition::False => return Ok((StatusCode::PRECONDITION_FAILED, headers).into_response()),
        Condition::None
            if check_if_modified_since(method, request, modified) == Condition::False =>
        {
            return Ok(not_modified(headers));
        }
        _ => {}
    }
    let mut range_header = header_text(request, header::RANGE);
    if !range_header.is_empty() && check_if_range(method, request, modified) == Condition::False {
        range_header = "";
    }

    let mut ranges = match parse_range(range_header, size) {
        Ok(ranges) => ranges,
        Err(RangeError::NoOverlap) if size == 0 => Vec::new(),
        Err(RangeError::NoOverlap) => {
            headers.insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes */{size}")).expect("content range"),
            );
            return Ok(plain_error(
                StatusCode::RANGE_NOT_SATISFIABLE,
                "invalid range: failed to overlap",
                headers,
            ));
        }
        Err(RangeError::Invalid) => {
            return Ok(plain_error(
                StatusCode::RANGE_NOT_SATISFIABLE,
                "invalid range",
                headers,
            ));
        }
    };
    if ranges.iter().map(|range| range.length).sum::<u64>() > size {
        ranges.clear();
    }

    let is_head = method == Method::HEAD;
    match ranges.as_slice() {
        [] => {
            headers.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
            let body = if is_head {
                Body::empty()
            } else {
                Body::from_stream(ReaderStream::new(file))
            };
            Ok((StatusCode::OK, headers, body).into_response())
        }
        [range] => {
            file.seek(std::io::SeekFrom::Start(range.start))
                .await
                .map_err(ServeError::Io)?;
            headers.insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&range.content_range(size)).expect("content range"),
            );
            headers.insert(header::CONTENT_LENGTH, HeaderValue::from(range.length));
            let body = if is_head {
                Body::empty()
            } else {
                Body::from_stream(ReaderStream::new(file.take(range.length)))
            };
            Ok((StatusCode::PARTIAL_CONTENT, headers, body).into_response())
        }
        ranges => {
            let boundary = random_boundary();
            let part_header = |index: usize, range: &Range| {
                let separator = if index == 0 {
                    format!("--{boundary}\r\n")
                } else {
                    format!("\r\n--{boundary}\r\n")
                };
                format!(
                    "{separator}Content-Range: {}\r\nContent-Type: {content_type}\r\n\r\n",
                    range.content_range(size)
                )
            };
            let closing = format!("\r\n--{boundary}--\r\n");
            let length: u64 = ranges
                .iter()
                .enumerate()
                .map(|(index, range)| part_header(index, range).len() as u64 + range.length)
                .sum::<u64>()
                + closing.len() as u64;
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(&format!("multipart/byteranges; boundary={boundary}"))
                    .expect("content type"),
            );
            headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
            if is_head {
                return Ok((StatusCode::PARTIAL_CONTENT, headers, Body::empty()).into_response());
            }
            let parts: Vec<(String, Range)> = ranges
                .iter()
                .enumerate()
                .map(|(index, range)| (part_header(index, range), *range))
                .collect();
            let path = path.to_owned();
            let stream = futures_util::stream::iter(parts)
                .then(move |(header, range)| {
                    let path = path.clone();
                    async move {
                        // Each part streams from the file, so a large range is never
                        // held in memory at once.
                        let head =
                            futures_util::stream::once(std::future::ready(
                                Ok::<_, std::io::Error>(bytes::Bytes::from(header)),
                            ));
                        let opened = async {
                            let mut file = tokio::fs::File::open(&path).await?;
                            file.seek(std::io::SeekFrom::Start(range.start)).await?;
                            Ok::<_, std::io::Error>(file)
                        };
                        match opened.await {
                            Ok(file) => head
                                .chain(ReaderStream::new(file.take(range.length)))
                                .left_stream(),
                            Err(error) => head
                                .chain(futures_util::stream::once(std::future::ready(Err(error))))
                                .right_stream(),
                        }
                    }
                })
                .flatten()
                .chain(futures_util::stream::once(async move {
                    Ok::<_, std::io::Error>(bytes::Bytes::from(closing))
                }));
            Ok((
                StatusCode::PARTIAL_CONTENT,
                headers,
                Body::from_stream(stream),
            )
                .into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_parse_like_go() {
        let range = |start, length| Range { start, length };
        assert_eq!(parse_range("bytes=0-99", 1000), Ok(vec![range(0, 100)]));
        assert_eq!(parse_range("bytes=900-", 1000), Ok(vec![range(900, 100)]));
        assert_eq!(parse_range("bytes=-100", 1000), Ok(vec![range(900, 100)]));
        assert_eq!(parse_range("bytes=-5000", 1000), Ok(vec![range(0, 1000)]));
        assert_eq!(
            parse_range("bytes=990-2000", 1000),
            Ok(vec![range(990, 10)])
        );
        assert_eq!(
            parse_range("bytes= 0-1 , 5-6", 1000),
            Ok(vec![range(0, 2), range(5, 2)])
        );
        assert_eq!(parse_range("bytes=2000-", 1000), Err(RangeError::NoOverlap));
        assert_eq!(parse_range("bytes=-0", 1000), Err(RangeError::NoOverlap));
        assert_eq!(parse_range("bytes=-0,0-0", 1000), Ok(vec![range(0, 1)]));
        assert_eq!(parse_range("bytes=2000-,0-0", 1000), Ok(vec![range(0, 1)]));
        assert_eq!(parse_range("items=0-1", 1000), Err(RangeError::Invalid));
        assert_eq!(parse_range("bytes=5-1", 1000), Err(RangeError::Invalid));
        assert_eq!(parse_range("bytes=x-1", 1000), Err(RangeError::Invalid));
        assert_eq!(parse_range("bytes=--1", 1000), Err(RangeError::Invalid));
        assert_eq!(parse_range("", 1000), Ok(vec![]));
    }

    #[test]
    fn etags_scan_like_go() {
        assert_eq!(
            scan_etag(" \"abc\", W/\"d\""),
            Some(("\"abc\"", ", W/\"d\""))
        );
        assert_eq!(scan_etag("W/\"x\""), Some(("W/\"x\"", "")));
        assert_eq!(scan_etag("abc"), None);
        assert_eq!(scan_etag("\"open"), None);
    }

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(name.clone(), HeaderValue::from_str(value).expect("value"));
        }
        headers
    }

    #[test]
    fn preconditions_without_an_etag() {
        assert!(check_if_match(&headers(&[(header::IF_MATCH, "*")])) == Condition::True);
        assert!(check_if_match(&headers(&[(header::IF_MATCH, "\"a\"")])) == Condition::False);
        assert!(check_if_none_match(&headers(&[(header::IF_NONE_MATCH, "*")])) == Condition::False);
        assert!(
            check_if_none_match(&headers(&[(header::IF_NONE_MATCH, "\"a\"")])) == Condition::True
        );
        let modified = Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000));
        let since = headers(&[(header::IF_MODIFIED_SINCE, "Tue, 14 Nov 2023 22:13:20 GMT")]);
        assert!(check_if_modified_since(&Method::GET, &since, modified) == Condition::False);
        let range_date = headers(&[(header::IF_RANGE, "Tue, 14 Nov 2023 22:13:20 GMT")]);
        assert!(check_if_range(&Method::GET, &range_date, modified) == Condition::True);
        let range_tag = headers(&[(header::IF_RANGE, "\"tag\"")]);
        assert!(check_if_range(&Method::GET, &range_tag, modified) == Condition::False);
    }

    async fn body(response: Response) -> Vec<u8> {
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body")
            .to_vec()
    }

    #[tokio::test]
    async fn serves_whole_files_single_and_multiple_ranges() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("song.flac");
        std::fs::write(&path, b"0123456789").expect("write");

        let whole = serve(&Method::GET, &HeaderMap::new(), &path)
            .await
            .ok()
            .expect("whole");
        assert_eq!(whole.headers()[header::CONTENT_TYPE], "audio/flac");
        assert_eq!(whole.headers()[header::CONTENT_LENGTH], "10");
        assert_eq!(body(whole).await, b"0123456789");

        let part = serve(
            &Method::GET,
            &headers(&[(header::RANGE, "bytes=2-4")]),
            &path,
        )
        .await
        .ok()
        .expect("range");
        assert_eq!(part.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(part.headers()[header::CONTENT_RANGE], "bytes 2-4/10");
        assert_eq!(body(part).await, b"234");

        let parts = serve(
            &Method::GET,
            &headers(&[(header::RANGE, "bytes=0-0,8-9")]),
            &path,
        )
        .await
        .ok()
        .expect("ranges");
        let declared: usize = parts.headers()[header::CONTENT_LENGTH]
            .to_str()
            .expect("length")
            .parse()
            .expect("number");
        let content_type = parts.headers()[header::CONTENT_TYPE]
            .to_str()
            .expect("type")
            .to_owned();
        let boundary = content_type
            .strip_prefix("multipart/byteranges; boundary=")
            .expect("multipart")
            .to_owned();
        let text = String::from_utf8(body(parts).await).expect("utf-8");
        assert_eq!(text.len(), declared);
        assert_eq!(
            text,
            format!(
                "--{boundary}\r\nContent-Range: bytes 0-0/10\r\nContent-Type: audio/flac\r\n\r\n0\r\n--{boundary}\r\nContent-Range: bytes 8-9/10\r\nContent-Type: audio/flac\r\n\r\n89\r\n--{boundary}--\r\n"
            )
        );

        let beyond = serve(
            &Method::GET,
            &headers(&[(header::RANGE, "bytes=50-")]),
            &path,
        )
        .await
        .ok()
        .expect("416");
        assert_eq!(beyond.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(beyond.headers()[header::CONTENT_RANGE], "bytes */10");
        assert_eq!(body(beyond).await, b"invalid range: failed to overlap\n");

        let head = serve(&Method::HEAD, &HeaderMap::new(), &path)
            .await
            .ok()
            .expect("head");
        assert_eq!(head.headers()[header::CONTENT_LENGTH], "10");
        assert!(body(head).await.is_empty());
    }
}
