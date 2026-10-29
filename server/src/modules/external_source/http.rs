//! The External Source's HTTP transport adapter: the service's upstream
//! GETs go through the radio proxy's SSRF-guarded [`Upstream`], and request
//! and media headers cross here between `http` types and the service's own.

use super::service::{
    ExternalSource, Fetched, Fetcher, MediaHeaders, RangeRequest, RequestHeaders,
};
use crate::config::ExternalSourceConfig;
use crate::modules::radio::{SafeUpstream, Upstream, UpstreamResponse};
use axum::http::{HeaderMap, HeaderName, HeaderValue, header};
use futures_util::FutureExt;
use futures_util::future::BoxFuture;
use std::sync::Arc;

impl ExternalSource {
    pub fn new(config: Option<&ExternalSourceConfig>) -> Self {
        Self::with_fetcher(
            config,
            Arc::new(UpstreamFetcher(Arc::new(SafeUpstream::new()))),
        )
    }
}

/// A [`Fetcher`] over the radio proxy's [`Upstream`].
pub(super) struct UpstreamFetcher(pub Arc<dyn Upstream>);

impl Fetcher for UpstreamFetcher {
    fn fetch(
        &self,
        url: url::Url,
        range: RangeRequest,
        headers: RequestHeaders,
    ) -> BoxFuture<'static, Result<Fetched, String>> {
        self.0
            .get_with_headers(url, range_header(&range), request_headers(&headers))
            .map(|result| result.map(fetched))
            .boxed()
    }
}

/// The upstream `Range` for `range`.
fn range_header(range: &RangeRequest) -> Option<HeaderValue> {
    match range {
        RangeRequest::Whole => None,
        RangeRequest::Single { start, end } => {
            let end = end.map(|end| end.to_string()).unwrap_or_default();
            Some(HeaderValue::from_str(&format!("bytes={start}-{end}")).expect("range header"))
        }
        // The client's own header value, passed on as it was.
        RangeRequest::Other(raw) => HeaderValue::from_bytes(raw).ok(),
    }
}

/// The request headers that are valid HTTP; a later value of a name
/// replaces an earlier one.
fn request_headers(headers: &RequestHeaders) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            map.insert(name, value);
        }
    }
    map
}

fn fetched(response: UpstreamResponse) -> Fetched {
    Fetched {
        status: response.status,
        headers: media_headers(&response.headers),
        url: response.url,
        body: response.body,
    }
}

const MEDIA_HEADERS: [HeaderName; 5] = [
    header::CONTENT_TYPE,
    header::CONTENT_LENGTH,
    header::CONTENT_RANGE,
    header::ACCEPT_RANGES,
    header::CACHE_CONTROL,
];

/// The upstream headers the service reads or passes on, as text.
pub(super) fn media_headers(headers: &HeaderMap) -> MediaHeaders {
    let [
        content_type,
        content_length,
        content_range,
        accept_ranges,
        cache_control,
    ] = MEDIA_HEADERS.map(|name| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    });
    MediaHeaders {
        content_type,
        content_length,
        content_range,
        accept_ranges,
        cache_control,
    }
}

/// `headers` as an `http` header map, for the stream proxy's rules (see
/// [`crate::modules::radio::copy_stream_headers`]) to pick what the client
/// gets.
pub(super) fn header_map(headers: &MediaHeaders) -> HeaderMap {
    let mut map = HeaderMap::new();
    let values = [
        &headers.content_type,
        &headers.content_length,
        &headers.content_range,
        &headers.accept_ranges,
        &headers.cache_control,
    ];
    for (name, value) in MEDIA_HEADERS.into_iter().zip(values) {
        if let Some(value) = value
            .as_deref()
            .and_then(|value| HeaderValue::from_str(value).ok())
        {
            map.insert(name, value);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures_util::StreamExt;
    use std::sync::Mutex;
    use url::Url;

    #[derive(Default)]
    struct Recorder {
        requests: Mutex<Vec<(Option<HeaderValue>, HeaderMap)>>,
    }

    impl Upstream for Recorder {
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
            self.requests.lock().expect("lock").push((range, headers));
            let mut response_headers = HeaderMap::new();
            for (name, value) in [
                ("content-type", "audio/mp4"),
                ("content-range", "bytes 0-1/10"),
                ("set-cookie", "a=b"),
            ] {
                response_headers.insert(name, HeaderValue::from_static(value));
            }
            Box::pin(async move {
                Ok(UpstreamResponse {
                    status: 206,
                    headers: response_headers,
                    url,
                    body: futures_util::stream::iter([Ok(Bytes::from_static(b"ab"))]).boxed(),
                })
            })
        }
    }

    #[tokio::test]
    async fn ranges_and_request_headers_reach_the_upstream_as_http() {
        let upstream = Arc::new(Recorder::default());
        let fetcher = UpstreamFetcher(upstream.clone());
        let url = Url::parse("https://media.example/a").expect("url");
        let headers: RequestHeaders = vec![
            ("user-agent".to_owned(), "first".to_owned()),
            ("user-agent".to_owned(), "agent".to_owned()),
            ("referer".to_owned(), "bad\nvalue".to_owned()),
        ];
        for range in [
            RangeRequest::Whole,
            RangeRequest::Single {
                start: 5,
                end: None,
            },
            RangeRequest::Single {
                start: 0,
                end: Some(3),
            },
            RangeRequest::Other(Bytes::from_static(b"bytes=-500")),
        ] {
            let fetched = fetcher
                .fetch(url.clone(), range, headers.clone())
                .await
                .expect("fetched");
            assert_eq!(fetched.status, 206);
            assert_eq!(
                fetched.headers,
                MediaHeaders {
                    content_type: Some("audio/mp4".to_owned()),
                    content_range: Some("bytes 0-1/10".to_owned()),
                    ..MediaHeaders::default()
                }
            );
        }
        let requests = upstream.requests.lock().expect("lock");
        let ranges: Vec<_> = requests
            .iter()
            .map(|(range, _)| range.as_ref().map(|range| range.to_str().expect("text")))
            .collect();
        assert_eq!(
            ranges,
            [
                None,
                Some("bytes=5-"),
                Some("bytes=0-3"),
                Some("bytes=-500")
            ]
        );
        assert!(
            requests
                .iter()
                .all(|(_, headers)| { headers.len() == 1 && headers["user-agent"] == "agent" })
        );
    }

    #[test]
    fn media_headers_round_trip() {
        let headers = MediaHeaders {
            content_type: Some("audio/webm".to_owned()),
            content_length: Some("10".to_owned()),
            content_range: None,
            accept_ranges: Some("bytes".to_owned()),
            cache_control: Some("max-age=60".to_owned()),
        };
        assert_eq!(media_headers(&header_map(&headers)), headers);
    }
}
