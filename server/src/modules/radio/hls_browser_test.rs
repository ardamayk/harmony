//! A real browser plays an HLS station through the stream proxy
//! (stream_browser_integration_test.go). Runs with
//! `RUN_HLS_BROWSER_INTEGRATION=1 cargo test -p earthly-music-server hls_browser -- --nocapture`;
//! it needs Playwright's Chromium (`pnpm exec playwright install chromium`).

use super::stream::{NowPlayingSink, StreamProxy, StreamRequest, Upstream, UpstreamResponse};
use super::types::{NowPlaying, Station};
use axum::Router;
use axum::extract::{OriginalUri, Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, header};
use axum::response::Response;
use axum::routing::get;
use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tower_http::cors::CorsLayer;
use url::Url;

const FIXTURE_ORIGIN: &str = "http://hls-fixture.example";

/// Serves the HLS fixture in place of the station's origin and records
/// every path the proxy fetched.
#[derive(Default)]
struct Fixture {
    paths: Mutex<Vec<String>>,
}

impl Upstream for Fixture {
    fn get(
        &self,
        url: Url,
        _: Option<HeaderValue>,
    ) -> BoxFuture<'static, Result<UpstreamResponse, String>> {
        self.paths.lock().unwrap().push(url.path().to_owned());
        let (status, content_type, body): (u16, &str, Vec<u8>) = match url.path() {
            "/master.m3u8" => (
                200,
                "application/vnd.apple.mpegurl",
                b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=128000,CODECS=\"mp4a.40.2\"\nmedia/index.m3u8\n".to_vec(),
            ),
            "/media/index.m3u8" => (
                200,
                "application/vnd.apple.mpegurl",
                b"#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:6.0,\nsegment.ts\n#EXT-X-ENDLIST\n"
                    .to_vec(),
            ),
            "/media/segment.ts" => {
                let mut packet = vec![0u8; 188];
                packet[0] = 0x47;
                (200, "video/mp2t", packet)
            }
            other => (404, "text/plain", format!("unknown fixture path {other:?}").into_bytes()),
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(content_type).unwrap(),
        );
        Box::pin(async move {
            Ok(UpstreamResponse {
                status,
                headers,
                url,
                body: futures_util::stream::once(async move { Ok(Bytes::from(body)) }).boxed(),
            })
        })
    }
}

struct Discard;

impl NowPlayingSink for Discard {
    fn now_playing(&self, _: &str, _: NowPlaying, _: Option<SystemTime>) {}
}

type Shared = (Arc<StreamProxy>, Station);

async fn stream(
    State((proxy, station)): State<Shared>,
    Path(_): Path<String>,
    uri: OriginalUri,
    query: crate::query::GoQuery,
    headers: HeaderMap,
) -> Response {
    let request = StreamRequest {
        path: uri.path(),
        query: &query,
        range: headers.get(header::RANGE).cloned(),
    };
    proxy.stream(request, &station).await
}

#[tokio::test(flavor = "multi_thread")]
async fn hls_browser_plays_through_the_proxy() {
    if std::env::var("RUN_HLS_BROWSER_INTEGRATION").as_deref() != Ok("1") {
        eprintln!("set RUN_HLS_BROWSER_INTEGRATION=1 to run real browser integration");
        return;
    }
    let fixture = Arc::new(Fixture::default());
    let proxy = Arc::new(StreamProxy::new(fixture.clone(), Arc::new(Discard)));
    let station = Station {
        id: "browser-hls-fixture".into(),
        name: "Browser HLS Fixture".into(),
        stream_url: format!("{FIXTURE_ORIGIN}/master.m3u8"),
        ..Station::default()
    };
    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods([Method::GET, Method::OPTIONS])
        .allow_headers([header::RANGE])
        .expose_headers([header::CONTENT_RANGE, header::CONTENT_TYPE]);
    let app = Router::new()
        .route("/api/v1/radio/stations/{station_id}/stream", get(stream))
        .layer(cors)
        .with_state((proxy, station.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let web = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../web");
    let run = tokio::process::Command::new("pnpm")
        .args([
            "exec",
            "playwright",
            "test",
            "e2e/radio-hls-proxy.spec.ts",
            "--config=playwright.hls.config.ts",
        ])
        .current_dir(web)
        .env(
            "HLS_PROXY_URL",
            format!(
                "http://{address}/api/v1/radio/stations/{}/stream",
                station.id
            ),
        )
        .env("HLS_UPSTREAM_ORIGIN", FIXTURE_ORIGIN)
        .env("PLAYWRIGHT_HTML_OPEN", "never")
        .output();
    // Two attempts with CI's retry, plus browser startup.
    let output = tokio::time::timeout(Duration::from_secs(180), run)
        .await
        .expect("Playwright finished in time")
        .expect("run Playwright");
    println!(
        "Playwright output:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "run Playwright HLS integration");
    let paths = fixture.paths.lock().unwrap();
    for expected in ["/master.m3u8", "/media/index.m3u8", "/media/segment.ts"] {
        assert!(
            paths.iter().any(|path| path == expected),
            "upstream paths = {paths:?}, missing {expected:?}"
        );
    }
}
