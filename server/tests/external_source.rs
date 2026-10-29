//! The External Source through the assembled router, with a fake yt-dlp:
//! a shell script written at test time that records its arguments and
//! prints canned output. Nothing here reaches the network.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use earthly_music_server::app::{self, AppState};
use earthly_music_server::config::{Config, ExternalSourceConfig};
use earthly_music_server::db;
use earthly_music_server::dependencies::Report;
use earthly_music_server::modules::external_source::ExternalSource;
use http_body_util::BodyExt;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

const SEARCH_OUTPUT: &str = r#"{"_type": "url", "ie_key": "Soundcloud", "id": "254112221", "url": "https://api.soundcloud.com/tracks/soundcloud%3Atracks%3A254112221", "webpage_url": "https://soundcloud.com/daftpunkofficialmusic/around-the-world", "title": "Around the World", "duration": 30.0, "uploader": "Daft Punk", "artist": "Daft Punk", "thumbnail": null, "thumbnails": [{"id": "large", "url": "https://i1.sndcdn.com/artworks-x-large.jpg", "width": 100, "height": 100}, {"id": "t300x300", "url": "https://i1.sndcdn.com/artworks-x-t300x300.jpg", "width": 300, "height": 300}, {"id": "original", "url": "https://i1.sndcdn.com/artworks-x-original.jpg"}]}
{"_type": "url", "ie_key": "Soundcloud", "id": "88335161", "url": "https://api.soundcloud.com/tracks/soundcloud%3Atracks%3A88335161", "webpage_url": "https://soundcloud.com/eldjkb/get-lucky-original-version", "title": "Get Lucky", "duration": 246.381, "uploader": "DJ KB"}
{"_type": "url", "ie_key": "Soundcloud", "webpage_url": "https://soundcloud.com/a/third", "title": "Third"}
"#;

/// A progressive format whose URL is on loopback, which the proxy refuses.
const PROGRESSIVE_OUTPUT: &str = r#"{"_type": "video", "extractor_key": "Soundcloud", "format_id": "http_mp3_1_0", "protocol": "https", "acodec": "mp3", "vcodec": "none", "url": "http://127.0.0.1:9/a.mp3", "http_headers": {"User-Agent": "Mozilla/5.0"}}"#;
const HLS_OUTPUT: &str = r#"{"_type": "video", "extractor_key": "Soundcloud", "format_id": "hls_aac_96k", "protocol": "m3u8_native", "acodec": "mp4a.40.2", "vcodec": "none", "url": "http://127.0.0.1:9/playlist.m3u8"}"#;

/// A fake yt-dlp in its own directory. It appends its arguments to `args`
/// (one per line, `---` after each run), then runs `body`.
struct FakeYtDlp {
    directory: tempfile::TempDir,
}

impl FakeYtDlp {
    fn new(body: &str) -> Self {
        let directory = tempfile::tempdir().expect("tempdir");
        let script = format!(
            "#!/bin/sh\ndir=\"$(dirname \"$0\")\"\nfor arg in \"$@\"; do printf '%s\\n' \"$arg\"; done >> \"$dir/args\"\necho --- >> \"$dir/args\"\n{body}\n"
        );
        let binary = directory.path().join("yt-dlp");
        std::fs::write(&binary, script).expect("write fake yt-dlp");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        Self { directory }
    }

    /// Prints `search` for searches and `resolve` for anything else.
    fn printing(search: &str, resolve: &str) -> Self {
        let fake = Self::new(
            "case \" $* \" in\n  *\" --flat-playlist \"*) cat \"$dir/search.out\" ;;\n  *) cat \"$dir/resolve.out\" ;;\nesac",
        );
        std::fs::write(fake.directory.path().join("search.out"), search).expect("write");
        std::fs::write(fake.directory.path().join("resolve.out"), resolve).expect("write");
        fake
    }

    fn binary(&self) -> PathBuf {
        self.directory.path().join("yt-dlp")
    }

    /// The arguments of every run so far.
    fn runs(&self) -> Vec<Vec<String>> {
        let Ok(text) = std::fs::read_to_string(self.directory.path().join("args")) else {
            return Vec::new();
        };
        text.split_terminator("---\n")
            .map(|run| run.lines().map(str::to_owned).collect())
            .collect()
    }
}

async fn router(directory: &Path, binary: Option<&Path>, timeout: Duration) -> axum::Router {
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let pool = db::open_and_migrate(directory.join("app.db").to_str().unwrap(), &migrations)
        .await
        .expect("database");
    let managed = directory.join("managed");
    let mut config = Config::from_lookup(|name| match name {
        "MANAGED_STORAGE_PATH" => Some(managed.to_string_lossy().into_owned()),
        "RADIO_BROWSER_BASE_URL" => Some("http://127.0.0.1:9".to_owned()),
        "ARTIST_INFO_ENABLED" | "LYRICS_LOOKUP_ENABLED" => Some("false".to_owned()),
        _ => None,
    })
    .expect("config");
    config.external_source = binary.map(|binary| ExternalSourceConfig {
        search_prefix: "scsearch".to_owned(),
        yt_dlp: binary.to_string_lossy().into_owned(),
    });
    let source =
        ExternalSource::new(config.external_source.as_ref()).with_timeouts(timeout, timeout);
    let mut state = AppState::new(config, Report::default(), pool);
    state.external_source = Arc::new(source);
    app::router(state)
}

async fn get(router: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

const TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test(flavor = "multi_thread")]
async fn a_disabled_source_answers_404_and_is_not_advertised() {
    let directory = tempfile::tempdir().unwrap();
    let router = router(directory.path(), None, TIMEOUT).await;
    for uri in [
        "/api/v1/external/search?q=daft+punk",
        "/api/v1/external/search?limit=nope",
        "/api/v1/external/tracks/x/playback",
        "/api/v1/external/tracks/x/stream",
        "/api/v1/external/tracks/x/playlist.m3u8",
        "/api/v1/external/tracks/x/resources/y",
        "/api/v1/external/tracks/x/artwork",
    ] {
        let (status, body) = get(&router, uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(body["code"], "external_source_disabled", "{uri}");
    }
    let (_, health) = get(&router, "/api/v1/health").await;
    assert!(
        !health["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::from("external-source.v1"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn search_passes_the_query_as_one_argument_after_the_end_of_options() {
    let directory = tempfile::tempdir().unwrap();
    let fake = FakeYtDlp::printing(SEARCH_OUTPUT, "");
    let router = router(directory.path(), Some(&fake.binary()), TIMEOUT).await;
    let (status, body) = get(
        &router,
        "/api/v1/external/search?q=--exec+id%3B+rm+-rf+%2F+%24(id)&limit=2",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        fake.runs(),
        [[
            "--ignore-config",
            "--no-warnings",
            "--flat-playlist",
            "--dump-json",
            "--socket-timeout",
            "15",
            "--",
            "scsearch2:--exec id; rm -rf / $(id)",
        ]
        .map(str::to_owned)]
    );
    assert_eq!(body["source"], "scsearch");
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["title"], "Around the World");
    assert_eq!(items[0]["artistName"], "Daft Punk");
    assert_eq!(items[0]["durationMs"], 30_000);
    assert_eq!(
        items[0]["webpageUrl"],
        "https://soundcloud.com/daftpunkofficialmusic/around-the-world"
    );
    assert_eq!(items[0]["extractor"], "Soundcloud");
    assert_eq!(items[0]["hasArtwork"], true);
    assert_eq!(items[1]["artistName"], "DJ KB");
    assert_eq!(items[1]["hasArtwork"], false);
    assert_eq!(items[1]["durationMs"], 246_381);
    let id = items[0]["id"].as_str().unwrap();
    assert!(id.len() <= 2048 && !id.contains("soundcloud.com") && !id.contains("sndcdn"));

    let (_, health) = get(&router, "/api/v1/health").await;
    assert!(
        health["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::from("external-source.v1"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_queries_and_ids_are_refused_without_running_yt_dlp() {
    let directory = tempfile::tempdir().unwrap();
    let fake = FakeYtDlp::printing(SEARCH_OUTPUT, PROGRESSIVE_OUTPUT);
    let router = router(directory.path(), Some(&fake.binary()), TIMEOUT).await;
    let long = "a".repeat(201);
    for uri in [
        "/api/v1/external/search".to_owned(),
        "/api/v1/external/search?q=%20%20".to_owned(),
        format!("/api/v1/external/search?q={long}"),
        "/api/v1/external/search?q=a&limit=0".to_owned(),
        "/api/v1/external/search?q=a&limit=21".to_owned(),
        "/api/v1/external/search?q=a&limit=ten".to_owned(),
    ] {
        let (status, body) = get(&router, &uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
        assert_eq!(body["code"], "bad_request", "{uri}");
    }
    let (_, body) = get(&router, "/api/v1/external/search?q=daft+punk").await;
    let id = body["items"][0]["id"].as_str().unwrap().to_owned();
    let (payload, signature) = id.split_once('.').unwrap();
    for tampered in [
        format!("{payload}A.{signature}"),
        format!("{payload}.{}", &signature[1..]),
        "eyJ1IjoiaHR0cDovLzE2OS4yNTQuMTY5LjI1NC8iLCJlIjoiR2VuZXJpYyJ9.AAAA".to_owned(),
    ] {
        for endpoint in [
            "playback",
            "stream",
            "playlist.m3u8",
            "resources/x.y",
            "artwork",
        ] {
            let uri = format!("/api/v1/external/tracks/{tampered}/{endpoint}");
            let (status, body) = get(&router, &uri).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
            assert_eq!(body["code"], "bad_request", "{uri}");
        }
    }
    assert_eq!(fake.runs().len(), 1, "only the one valid search ran");
}

#[tokio::test(flavor = "multi_thread")]
async fn playback_resolves_once_with_the_extractor_pinned() {
    let directory = tempfile::tempdir().unwrap();
    let fake = FakeYtDlp::printing(SEARCH_OUTPUT, PROGRESSIVE_OUTPUT);
    let router = router(directory.path(), Some(&fake.binary()), TIMEOUT).await;
    let (_, body) = get(&router, "/api/v1/external/search?q=daft+punk").await;
    let id = body["items"][0]["id"].as_str().unwrap().to_owned();
    for _ in 0..2 {
        let (status, playback) =
            get(&router, &format!("/api/v1/external/tracks/{id}/playback")).await;
        assert_eq!(status, StatusCode::OK, "{playback}");
        assert_eq!(playback["format"], "progressive");
        assert_eq!(
            playback["streamPath"],
            format!("/api/v1/external/tracks/{id}/stream")
        );
    }
    let runs = fake.runs();
    assert_eq!(runs.len(), 2, "one search, one resolve: {runs:?}");
    assert_eq!(
        runs[1],
        [
            "--ignore-config",
            "--no-warnings",
            "--no-playlist",
            "--dump-json",
            "--use-extractors",
            "Soundcloud",
            "-f",
            "bestaudio[protocol^=http][protocol!*=dash]/bestaudio[protocol*=m3u8]/best[vcodec=none]",
            "--",
            "https://soundcloud.com/daftpunkofficialmusic/around-the-world",
        ]
        .map(str::to_owned)
    );
    // The resolved URL is on loopback: refused before any request.
    let (status, body) = get(&router, &format!("/api/v1/external/tracks/{id}/stream")).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["code"], "external_source_failed");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("unsafe stream URL")
            && !body["message"].as_str().unwrap().contains("127.0.0.1"),
        "{body}"
    );
    let (status, body) = get(
        &router,
        &format!("/api/v1/external/tracks/{id}/playlist.m3u8"),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "external_track_unplayable");
    assert_eq!(fake.runs().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn hls_tracks_play_through_their_playlist() {
    let directory = tempfile::tempdir().unwrap();
    let fake = FakeYtDlp::printing(SEARCH_OUTPUT, HLS_OUTPUT);
    let router = router(directory.path(), Some(&fake.binary()), TIMEOUT).await;
    let (_, body) = get(&router, "/api/v1/external/search?q=daft+punk").await;
    let id = body["items"][1]["id"].as_str().unwrap().to_owned();
    let (status, playback) = get(&router, &format!("/api/v1/external/tracks/{id}/playback")).await;
    assert_eq!(status, StatusCode::OK, "{playback}");
    assert_eq!(playback["format"], "hls");
    assert_eq!(
        playback["streamPath"],
        format!("/api/v1/external/tracks/{id}/playlist.m3u8")
    );
    let (status, body) = get(&router, &format!("/api/v1/external/tracks/{id}/stream")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "external_track_unplayable");
    let (status, body) = get(
        &router,
        &format!("/api/v1/external/tracks/{id}/playlist.m3u8"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["code"], "external_source_failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn yt_dlp_failures_map_to_their_codes() {
    let error = "ERROR: [soundcloud] around-the-world: Unable to download JSON metadata; please report this issue on https://github.com/yt-dlp/yt-dlp/issues?q= , filling out the appropriate issue template. Confirm you are on the latest version using yt-dlp -U";
    let failing = FakeYtDlp::new(&format!(
        "echo '[soundcloud] Downloading' >&2\necho '{error}' >&2\nexit 1"
    ));
    let garbage = FakeYtDlp::new("echo '<html>not json</html>'");
    let slow = FakeYtDlp::new("exec sleep 10");
    let missing = Path::new("/nonexistent/yt-dlp").to_path_buf();
    for (binary, timeout, status, code) in [
        (
            failing.binary(),
            TIMEOUT,
            StatusCode::BAD_GATEWAY,
            "external_source_failed",
        ),
        (
            garbage.binary(),
            TIMEOUT,
            StatusCode::BAD_GATEWAY,
            "external_source_failed",
        ),
        (
            slow.binary(),
            Duration::from_millis(300),
            StatusCode::GATEWAY_TIMEOUT,
            "external_source_timeout",
        ),
        (
            missing,
            TIMEOUT,
            StatusCode::SERVICE_UNAVAILABLE,
            "external_source_unavailable",
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let router = router(directory.path(), Some(&binary), timeout).await;
        let started = std::time::Instant::now();
        let (actual, body) = get(&router, "/api/v1/external/search?q=daft+punk").await;
        assert_eq!(
            (actual, body["code"].as_str().unwrap()),
            (status, code),
            "{body}"
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        if code == "external_source_failed" && binary == failing.binary() {
            assert_eq!(body["message"], error.chars().take(300).collect::<String>());
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_track_without_artwork_answers_404_without_a_request() {
    let directory = tempfile::tempdir().unwrap();
    let fake = FakeYtDlp::printing(SEARCH_OUTPUT, "");
    let router = router(directory.path(), Some(&fake.binary()), TIMEOUT).await;
    let (_, body) = get(&router, "/api/v1/external/search?q=daft+punk").await;
    let id = body["items"][1]["id"].as_str().unwrap().to_owned();
    let (status, body) = get(&router, &format!("/api/v1/external/tracks/{id}/artwork")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "external_track_artwork_missing");
    assert_eq!(fake.runs().len(), 1, "artwork never runs yt-dlp");
}

/// Whether `pid` still runs: a killed child that is not yet reaped is a
/// zombie, which counts as gone.
fn is_running(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    let state = stat
        .rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().next());
    !matches!(state, Some("Z" | "X") | None)
}

async fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !check() {
        assert!(std::time::Instant::now() < deadline, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A client that goes away mid-search must not keep yt-dlp running: hyper
/// drops the handler future when the connection closes, which drops the
/// child (`kill_on_drop`) and its turn. Both turns are taken by searches
/// whose clients disconnect; a third search must then run at once.
#[tokio::test(flavor = "multi_thread")]
async fn a_disconnected_search_kills_yt_dlp_and_gives_back_its_turn() {
    use tokio::io::AsyncWriteExt;

    let directory = tempfile::tempdir().unwrap();
    let fake = FakeYtDlp::new(
        "case \" $* \" in\n  *slow*) echo $$ >> \"$dir/pids\"; exec sleep 30 ;;\n  *) cat \"$dir/search.out\" ;;\nesac",
    );
    std::fs::write(fake.directory.path().join("search.out"), SEARCH_OUTPUT).expect("write");
    let router = router(directory.path(), Some(&fake.binary()), TIMEOUT).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(axum::serve(listener, router.clone()).into_future());

    let mut clients = Vec::new();
    for _ in 0..2 {
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect");
        stream
            .write_all(
                format!("GET /api/v1/external/search?q=slow HTTP/1.1\r\nHost: {address}\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .expect("request");
        clients.push(stream);
    }
    let pids_file = fake.directory.path().join("pids");
    let pids = || -> Vec<u32> {
        std::fs::read_to_string(&pids_file)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.trim().parse().ok())
            .collect()
    };
    eventually("both slow searches started yt-dlp", || pids().len() == 2).await;
    let started = pids();
    assert!(started.iter().all(|&pid| is_running(pid)));

    drop(clients);
    for pid in started {
        eventually(&format!("yt-dlp {pid} was killed"), || !is_running(pid)).await;
    }

    let begun = std::time::Instant::now();
    let (status, body) = get(&router, "/api/v1/external/search?q=daft+punk").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        begun.elapsed() < Duration::from_secs(3),
        "the next search waited {:?} for a turn",
        begun.elapsed()
    );
    server.abort();
}
