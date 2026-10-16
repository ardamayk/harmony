//! Desktop Managed Import parity against a real Music Server (issue #55).
//!
//! The test builds the Music Server (`cargo build -p earthly-music-server`),
//! starts it on a free loopback port with isolated Managed Storage, reads one
//! Strict Import Profile fixture per supported format from `server/testdata`,
//! and then drives the same server-owned Managed Import journey the Web Client uses
//! through the Desktop Client's native selection store, HTTP bridge, streaming
//! upload, and media proxy. Nothing in this crate parses audio, so every
//! preview field asserted below must have come from the Music Server.
//!
//! The test is ignored by default because it builds the Music Server and takes
//! several seconds; run it with
//! `mise run desktop:test:import-parity`.

use earthly_audio_core::connection::{HttpBridge, HttpRequest, HttpResponse, ServerOrigin};
use earthly_audio_core::desktop_import::{
    ImportSelection, ImportSelectionStore, ImportUploadProgress, ProgressSink, upload_selection,
};
use earthly_audio_core::media_proxy::MediaProxy;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SUPPORTED_FORMATS: [&str; 6] = ["flac", "mp3", "m4a", "ogg", "opus", "wav"];

#[derive(Clone, Debug, Deserialize)]
struct Fixture {
    filename: String,
    format: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Batch {
    id: String,
    status: String,
    revision: u64,
    files: Vec<BatchFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchFile {
    job_id: String,
    state: String,
    #[serde(default)]
    selected: bool,
    #[serde(default)]
    outcome: Option<String>,
    #[serde(default)]
    track_id: Option<String>,
    #[serde(default)]
    error_reason: Option<String>,
    #[serde(default)]
    preview: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Preview {
    job_id: String,
    status: String,
    #[serde(default)]
    duplicate_classification: Option<String>,
    file: PreviewFile,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewFile {
    original_filename: String,
    title: String,
    format: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackList {
    items: Vec<Track>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Track {
    id: String,
    title: String,
    format: String,
    size_bytes: u64,
}

struct MusicServer {
    child: Child,
    origin: ServerOrigin,
    managed_storage: PathBuf,
}

impl Drop for MusicServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Harness {
    root: PathBuf,
    server: MusicServer,
    bridge: Arc<HttpBridge>,
    store: ImportSelectionStore,
    fixtures: Vec<(Fixture, Vec<u8>)>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires the Go toolchain and starts a real Music Server; run via test:import-parity"]
async fn desktop_managed_import_matches_web_for_every_supported_format() {
    let harness = Harness::start();
    let origin = harness.server.origin.clone();

    // 1. Recursive folder selection imports one file per supported format.
    //    The Opus fixture carries the same tags as the Ogg Vorbis fixture, so
    //    it is imported in step 4 with an explicit separate Album decision,
    //    mirroring the Web journey.
    let collection = harness.root.join("selection/Collection");
    let disc = collection.join("Disc 1");
    fs::create_dir_all(&disc).expect("create disc directory");
    fs::create_dir_all(collection.join(".archive")).expect("create hidden directory");
    for (fixture, bytes) in harness.folder_fixtures() {
        fs::write(disc.join(&fixture.filename), bytes).expect("write fixture");
    }
    fs::write(disc.join("cover.jpg"), b"not audio").expect("write sidecar");
    fs::write(
        collection.join(".archive/hidden.mp3"),
        &harness.fixtures[0].1,
    )
    .expect("write hidden audio");
    fs::write(collection.join("notes.txt"), b"liner notes").expect("write notes");

    let selections = harness
        .store
        .register_paths([collection.clone()])
        .expect("register folder");
    let mut selected_names = selections
        .iter()
        .map(|selection| selection.name.as_str())
        .collect::<Vec<_>>();
    selected_names.sort_unstable();
    let mut expected_names = harness
        .folder_fixtures()
        .map(|(fixture, _)| fixture.filename.as_str())
        .collect::<Vec<_>>();
    expected_names.sort_unstable();
    assert_eq!(
        selected_names, expected_names,
        "recursive selection keeps only supported visible audio"
    );

    let batch = harness.create_batch().await;
    let mut previews = BTreeMap::new();
    for selection in &selections {
        let (fixture, bytes) = harness.fixture_named(&selection.name);
        assert_eq!(selection.size, bytes.len() as u64);
        let job_id = harness.create_job(&batch.id, &selection.id).await;
        let (response, progress) = harness.upload(selection, &job_id).await;
        assert_eq!(
            response.status,
            200,
            "{} upload: {}",
            selection.name,
            String::from_utf8_lossy(&response.body)
        );
        let preview: Preview = serde_json::from_slice(&response.body).expect("preview JSON");
        assert_eq!(preview.job_id, job_id);
        assert_eq!(
            preview.status, "awaiting_confirmation",
            "{}",
            selection.name
        );
        assert_eq!(preview.file.original_filename, selection.name);
        assert_eq!(
            preview.file.format, fixture.format,
            "server-detected format for {}",
            selection.name
        );
        assert!(
            !preview.file.title.is_empty(),
            "server-authoritative metadata for {}",
            selection.name
        );
        assert!(
            preview
                .duplicate_classification
                .as_deref()
                .unwrap_or("none")
                == "none",
            "{} should not be a duplicate in a fresh library: {:?}",
            selection.name,
            preview.duplicate_classification
        );
        assert_progress_reached_completion(&progress, bytes.len() as u64, &selection.name);
        previews.insert(job_id, preview);
    }
    let mut imported_formats = previews
        .values()
        .map(|preview| preview.file.format.clone())
        .collect::<std::collections::BTreeSet<_>>();

    let staged = harness.get_batch(&batch.id).await;
    assert!(
        staged
            .files
            .iter()
            .all(|file| file.state == "accepted" && file.selected),
        "every preview should be accepted and preselected: {staged:?}"
    );
    let selected_file_ids = staged
        .files
        .iter()
        .map(|file| file.job_id.clone())
        .collect::<Vec<_>>();
    let committed = harness
        .confirm_batch(&batch.id, staged.revision, &selected_file_ids, &[], &[])
        .await
        .unwrap_or_else(|(status, body)| panic!("confirm folder batch: HTTP {status} {body}"));
    assert_eq!(committed.status, "completed", "{committed:?}");
    let mut committed_tracks = Vec::new();
    for file in &committed.files {
        let preview = previews.get(&file.job_id).expect("preview for job");
        assert_eq!(
            file.outcome.as_deref(),
            Some("imported"),
            "{}: {file:?}",
            preview.file.original_filename
        );
        let track_id = file.track_id.clone().expect("committed Track ID");
        let (_, bytes) = harness.fixture_named(&preview.file.original_filename);
        committed_tracks.push((
            track_id,
            preview.file.title.clone(),
            preview.file.format.clone(),
            bytes.clone(),
        ));
    }
    harness
        .store
        .release(
            &selections
                .iter()
                .map(|selection| selection.id.clone())
                .collect::<Vec<_>>(),
        )
        .expect("release selections");

    // 2. The library lists each committed Track as managed, with the Music
    //    Server's own metadata and the exact uploaded byte count.
    let tracks = harness.list_tracks().await;
    for (track_id, title, format, bytes) in &committed_tracks {
        let track = tracks
            .iter()
            .find(|track| &track.id == track_id)
            .unwrap_or_else(|| panic!("Track {track_id} missing from library"));
        assert_eq!(&track.title, title);
        assert_eq!(&track.format, format);
        assert_eq!(track.size_bytes, bytes.len() as u64);
    }

    // 3. Desktop playback streams each committed Track bit-for-bit through the
    //    private loopback media proxy that native playback is restricted to.
    let proxy_origin = Arc::new(RwLock::new(Some(origin.clone())));
    let proxy = MediaProxy::start(harness.bridge.clone(), proxy_origin).expect("start media proxy");
    let client = reqwest::Client::new();
    for (track_id, _, format, bytes) in &committed_tracks {
        let response = client
            .get(format!(
                "{}/api/v1/tracks/{track_id}/stream",
                proxy.base_url()
            ))
            .send()
            .await
            .expect("stream request");
        assert_eq!(response.status(), 200, "{format} stream status");
        let streamed = response.bytes().await.expect("stream body");
        assert_eq!(streamed.as_ref(), bytes.as_slice(), "{format} stream bytes");
    }
    let outside = client
        .get(format!("{}/api/v1/health", proxy.base_url()))
        .send()
        .await
        .expect("non-media request");
    assert_eq!(
        outside.status(),
        400,
        "media proxy must only serve media paths"
    );
    drop(proxy);

    // 4. Explicit file selection: an Exact Duplicate, a server-rejected file,
    //    and a Track Replacement Candidate that needs an explicit decision resolve
    //    independently, exactly as on Web.
    let single = harness.root.join("selection/single");
    fs::create_dir_all(&single).expect("create single directory");
    let (_, mp3_bytes) = harness.fixture_with_format("mp3");
    let duplicate_path = single.join("renamed-copy.mp3");
    fs::write(&duplicate_path, mp3_bytes).expect("write duplicate");
    let broken_path = single.join("broken.flac");
    let broken_bytes: &[u8] = b"this is not a FLAC stream";
    fs::write(&broken_path, broken_bytes).expect("write broken");
    let (opus_fixture, opus_bytes) = harness.fixture_with_format("opus");
    let opus_path = single.join(&opus_fixture.filename);
    fs::write(&opus_path, opus_bytes).expect("write Opus");
    let sidecar_path = single.join("cover.jpg");
    fs::write(&sidecar_path, b"image").expect("write sidecar");
    let ogg_track_id = committed_tracks
        .iter()
        .find(|(_, _, format, _)| format == "ogg")
        .map(|(track_id, _, _, _)| track_id.clone())
        .expect("Ogg Vorbis Track");

    let file_selections = harness
        .store
        .register_paths([
            duplicate_path.clone(),
            broken_path.clone(),
            opus_path.clone(),
            sidecar_path.clone(),
        ])
        .expect("register files");
    assert_eq!(
        file_selections
            .iter()
            .map(|selection| selection.name.as_str())
            .collect::<Vec<_>>(),
        ["broken.flac", "renamed-copy.mp3", "strict-import.opus"],
        "file selection keeps only supported audio, in stable order"
    );

    let second_batch = harness.create_batch().await;
    let mut second_jobs = BTreeMap::new();
    for selection in &file_selections {
        let job_id = harness.create_job(&second_batch.id, &selection.id).await;
        let (response, progress) = harness.upload(selection, &job_id).await;
        let body: Value = serde_json::from_slice(&response.body).expect("JSON body");
        match selection.name.as_str() {
            "renamed-copy.mp3" => {
                assert_eq!(response.status, 200, "{body}");
                assert_eq!(
                    body["duplicateClassification"].as_str(),
                    Some("exact_duplicate"),
                    "{body}"
                );
            }
            "broken.flac" => {
                assert!(
                    (400..500).contains(&response.status),
                    "structured validation error expected, got {} {body}",
                    response.status
                );
                assert!(body["code"].is_string(), "{body}");
                assert!(body["message"].is_string(), "{body}");
                assert_progress_reached_completion(
                    &progress,
                    broken_bytes.len() as u64,
                    &selection.name,
                );
            }
            "strict-import.opus" => {
                assert_eq!(response.status, 200, "{body}");
                assert_eq!(body["file"]["format"].as_str(), Some("opus"), "{body}");
                assert_eq!(
                    body["duplicateClassification"].as_str(),
                    Some("none"),
                    "{body}"
                );
                let candidates = body["matchingTracks"].as_array().expect("matching Tracks");
                assert!(
                    candidates
                        .iter()
                        .any(|candidate| candidate["trackId"] == ogg_track_id.as_str()),
                    "replacement candidate should name the committed Ogg Vorbis Track: {body}"
                );
                imported_formats.insert("opus".to_owned());
            }
            other => panic!("unexpected selection {other}"),
        }
        second_jobs.insert(selection.name.clone(), job_id);
    }
    for format in SUPPORTED_FORMATS {
        assert!(
            imported_formats.contains(format),
            "{format} was not validated by the Music Server"
        );
    }

    let staged = harness.get_batch(&second_batch.id).await;
    let broken = staged
        .files
        .iter()
        .find(|file| file.job_id == second_jobs["broken.flac"])
        .expect("broken file in batch");
    assert_eq!(broken.state, "rejected", "{broken:?}");
    assert!(!broken.selected);
    assert!(
        broken
            .error_reason
            .as_deref()
            .is_some_and(|reason| !reason.is_empty()),
        "{broken:?}"
    );
    let duplicate = staged
        .files
        .iter()
        .find(|file| file.job_id == second_jobs["renamed-copy.mp3"])
        .expect("duplicate in batch");
    assert!(!duplicate.selected, "{duplicate:?}");
    let opus = staged
        .files
        .iter()
        .find(|file| file.job_id == second_jobs["strict-import.opus"])
        .expect("Opus in batch");
    assert_eq!(opus.state, "accepted");
    // Different bytes are not an Exact Duplicate, but the matching Album
    // position requires a replacement or separate Album decision.
    assert_eq!(
        opus.preview
            .as_ref()
            .map(|preview| &preview["duplicateClassification"]),
        Some(&Value::String("none".to_owned())),
        "{opus:?}"
    );

    let album_key = opus.preview.as_ref().expect("Opus preview")["file"]["albumKey"]
        .as_str()
        .expect("Album key");
    assert!(!album_key.is_empty(), "Album decision needs a server key");

    // Confirming without a decision is refused; a separate Album decision commits
    // the Opus file as its own Album edition while the other two stay out.
    let (refused_status, refused_body) = harness
        .confirm_batch(
            &second_batch.id,
            staged.revision,
            std::slice::from_ref(&opus.job_id),
            &[],
            &[],
        )
        .await
        .expect_err("matching Track requires an explicit decision");
    assert_eq!(refused_status, 409, "HTTP {refused_status} {refused_body}");
    assert_eq!(refused_body["code"], "import_revision_conflict");
    let committed = harness
        .confirm_batch(
            &second_batch.id,
            staged.revision,
            std::slice::from_ref(&opus.job_id),
            &[],
            &[json!({ "albumKey": album_key, "createSeparate": true, "artworkMode": "auto" })],
        )
        .await
        .unwrap_or_else(|(status, body)| panic!("confirm with decision: HTTP {status} {body}"));
    let mp3_track_id = committed_tracks
        .iter()
        .find(|(_, _, format, _)| format == "mp3")
        .map(|(track_id, _, _, _)| track_id.clone())
        .expect("MP3 Track");
    let mut opus_track_id = None;
    for file in &committed.files {
        if file.job_id == opus.job_id {
            assert_eq!(file.outcome.as_deref(), Some("imported"), "{file:?}");
            opus_track_id = file.track_id.clone();
        } else if file.job_id == duplicate.job_id {
            // An Exact Duplicate is rejected and points at the existing Track.
            assert_eq!(file.outcome.as_deref(), Some("rejected"), "{file:?}");
            assert_eq!(
                file.track_id.as_deref(),
                Some(mp3_track_id.as_str()),
                "{file:?}"
            );
        } else {
            assert_eq!(file.outcome.as_deref(), Some("rejected"), "{file:?}");
            assert!(file.track_id.is_none(), "{file:?}");
        }
    }
    let opus_track_id = opus_track_id.expect("Opus Track ID");
    let tracks = harness.list_tracks().await;
    assert_eq!(
        tracks.len(),
        committed_tracks.len() + 1,
        "only the new valid file adds a Track"
    );
    let opus_response = client
        .get(format!(
            "{}/api/v1/tracks/{opus_track_id}/stream",
            harness.server.origin.as_str()
        ))
        .send()
        .await
        .expect("Opus stream");
    assert_eq!(opus_response.status(), 200);
    assert_eq!(
        opus_response.bytes().await.expect("Opus bytes").as_ref(),
        opus_bytes.as_slice()
    );
    harness
        .store
        .release(
            &file_selections
                .iter()
                .map(|selection| selection.id.clone())
                .collect::<Vec<_>>(),
        )
        .expect("release file selections");

    // 5. Cancellation: a canceled upload never reaches the server, and
    //    canceling the batch removes every staged file.
    let third_batch = harness.create_batch().await;
    let cancel_path = single.join("canceled.wav");
    let (_, wav_bytes) = harness.fixture_with_format("wav");
    fs::write(&cancel_path, wav_bytes).expect("write cancel fixture");
    let cancel_selection = harness
        .store
        .register_paths([cancel_path])
        .expect("register cancel fixture")
        .remove(0);
    let job_id = harness
        .create_job(&third_batch.id, &cancel_selection.id)
        .await;
    harness
        .store
        .cancel("upload-canceled")
        .expect("cancel upload");
    let error = upload_selection(
        &harness.store,
        &harness.bridge,
        &origin,
        &cancel_selection.id,
        "upload-canceled",
        &job_id,
        Arc::new(|_| {}) as ProgressSink,
    )
    .await
    .expect_err("canceled upload fails");
    assert_eq!(error.code, "canceled");
    let staged = harness.get_batch(&third_batch.id).await;
    assert!(
        staged.files.iter().all(|file| file.state != "accepted"),
        "{staged:?}"
    );
    let canceled = harness
        .send(
            "DELETE",
            &format!("/api/v1/import-batches/{}", third_batch.id),
            None,
        )
        .await;
    assert_eq!(canceled.status, 204);
    let staging = harness.server.managed_storage.join(".staging");
    let leftover = fs::read_dir(&staging)
        .map(|entries| entries.filter_map(Result::ok).count())
        .unwrap_or(0);
    assert_eq!(leftover, 0, "staging must be empty after cancellation");

    // 6. Import History retains the terminal results through the shared API.
    let history = harness.send("GET", "/api/v1/import-history", None).await;
    assert_eq!(history.status, 200);
    let history: Value = serde_json::from_slice(&history.body).expect("history JSON");
    let items = history["items"].as_array().expect("history items");
    let result_codes = items
        .iter()
        .filter_map(|item| item["resultCode"].as_str())
        .collect::<Vec<_>>();
    assert!(
        result_codes.contains(&"completed"),
        "folder import history: {result_codes:?}"
    );
    assert!(
        result_codes.contains(&"partially_completed"),
        "mixed file import history: {result_codes:?}"
    );
    assert!(
        result_codes.contains(&"canceled"),
        "canceled batch history: {result_codes:?}"
    );
}

fn assert_progress_reached_completion(
    progress: &[ImportUploadProgress],
    total_bytes: u64,
    name: &str,
) {
    assert!(!progress.is_empty(), "{name} reported no progress");
    let mut previous = 0;
    for update in progress {
        assert_eq!(update.total_bytes, total_bytes, "{name} total");
        assert!(update.sent_bytes >= previous, "{name} progress regressed");
        previous = update.sent_bytes;
    }
    assert_eq!(previous, total_bytes, "{name} did not reach 100%");
}

impl Harness {
    fn start() -> Self {
        let root = std::env::temp_dir().join(format!(
            "earthly-desktop-parity-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("create harness root");
        let server_dir = server_dir();
        let fixtures_dir = server_dir.join("testdata");
        let manifest: Vec<Fixture> = serde_json::from_slice(
            &fs::read(fixtures_dir.join("fixtures.json")).expect("manifest"),
        )
        .expect("manifest JSON");
        let fixtures = manifest
            .into_iter()
            .map(|fixture| {
                let bytes = fs::read(fixtures_dir.join(&fixture.filename)).expect("fixture bytes");
                (fixture, bytes)
            })
            .collect::<Vec<_>>();
        let server = MusicServer::start(&server_dir, &root);
        Self {
            root,
            server,
            bridge: Arc::new(HttpBridge::new().expect("create bridge")),
            store: ImportSelectionStore::default(),
            fixtures,
        }
    }

    fn fixture_named(&self, name: &str) -> &(Fixture, Vec<u8>) {
        self.fixtures
            .iter()
            .find(|(fixture, _)| fixture.filename == name)
            .unwrap_or_else(|| panic!("no fixture named {name}"))
    }

    fn fixture_with_format(&self, format: &str) -> &(Fixture, Vec<u8>) {
        self.fixtures
            .iter()
            .find(|(fixture, _)| fixture.format == format)
            .unwrap_or_else(|| panic!("no {format} fixture"))
    }

    fn folder_fixtures(&self) -> impl Iterator<Item = &(Fixture, Vec<u8>)> {
        self.fixtures
            .iter()
            .filter(|(fixture, _)| fixture.format != "opus")
    }

    async fn send(&self, method: &str, url: &str, body: Option<Value>) -> HttpResponse {
        let mut headers = BTreeMap::from([("accept".to_owned(), "application/json".to_owned())]);
        if body.is_some() {
            headers.insert("content-type".to_owned(), "application/json".to_owned());
        }
        self.bridge
            .send(
                &self.server.origin,
                HttpRequest {
                    method: method.to_owned(),
                    url: url.to_owned(),
                    headers,
                    body: body.map(|value| value.to_string().into_bytes()),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{method} {url}: {error}"))
    }

    async fn create_batch(&self) -> Batch {
        let response = self.send("POST", "/api/v1/import-batches", None).await;
        assert_eq!(
            response.status,
            201,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        serde_json::from_slice(&response.body).expect("batch JSON")
    }

    async fn get_batch(&self, batch_id: &str) -> Batch {
        let response = self
            .send("GET", &format!("/api/v1/import-batches/{batch_id}"), None)
            .await;
        assert_eq!(
            response.status,
            200,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        serde_json::from_slice(&response.body).expect("batch JSON")
    }

    async fn create_job(&self, batch_id: &str, client_file_id: &str) -> String {
        let response = self
            .send(
                "POST",
                "/api/v1/imports",
                Some(json!({ "batchId": batch_id, "clientFileId": client_file_id })),
            )
            .await;
        assert_eq!(
            response.status,
            201,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        let job: Value = serde_json::from_slice(&response.body).expect("job JSON");
        job["id"].as_str().expect("job id").to_owned()
    }

    async fn confirm_batch(
        &self,
        batch_id: &str,
        revision: u64,
        selected_file_ids: &[String],
        duplicate_decisions: &[Value],
        album_decisions: &[Value],
    ) -> Result<Batch, (u16, Value)> {
        let mut body = json!({ "revision": revision, "selectedFileIds": selected_file_ids });
        if !duplicate_decisions.is_empty() {
            body["duplicateDecisions"] = Value::Array(duplicate_decisions.to_vec());
        }
        if !album_decisions.is_empty() {
            body["albumDecisions"] = Value::Array(album_decisions.to_vec());
        }
        let response = self
            .send(
                "POST",
                &format!("/api/v1/import-batches/{batch_id}/confirm"),
                Some(body),
            )
            .await;
        let value: Value = serde_json::from_slice(&response.body).expect("confirm JSON");
        if response.status != 200 {
            return Err((response.status, value));
        }
        Ok(serde_json::from_value(value).expect("confirmed batch JSON"))
    }

    async fn list_tracks(&self) -> Vec<Track> {
        let response = self
            .send("GET", "/api/v1/library/tracks?limit=200", None)
            .await;
        assert_eq!(
            response.status,
            200,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        let list: TrackList = serde_json::from_slice(&response.body).expect("track list JSON");
        list.items
    }

    async fn upload(
        &self,
        selection: &ImportSelection,
        job_id: &str,
    ) -> (HttpResponse, Vec<ImportUploadProgress>) {
        let progress = Arc::new(Mutex::new(Vec::new()));
        let recorded = progress.clone();
        let channel: ProgressSink = Arc::new(move |update: ImportUploadProgress| {
            recorded.lock().expect("progress lock").push(update);
        });
        let response = upload_selection(
            &self.store,
            &self.bridge,
            &self.server.origin,
            &selection.id,
            &format!("upload-{}", selection.id),
            job_id,
            channel,
        )
        .await
        .unwrap_or_else(|error| panic!("upload {}: {}", selection.name, error.message));
        let progress = progress.lock().expect("progress lock").clone();
        (response, progress)
    }
}

impl MusicServer {
    fn start(server_dir: &Path, root: &Path) -> Self {
        let binary = build_music_server(server_dir);
        let port = free_port();
        let data = root.join("server-data");
        let managed_storage = data.join("managed");
        fs::create_dir_all(&managed_storage).expect("create managed storage");
        let child = Command::new(&binary)
            .current_dir(server_dir)
            .env("SERVER_ADDR", format!("127.0.0.1:{port}"))
            .env("DATABASE_PATH", data.join("parity.db"))
            .env("MANAGED_STORAGE_PATH", &managed_storage)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start Music Server");
        let origin = ServerOrigin::parse(&format!("http://127.0.0.1:{port}")).expect("origin");
        let server = Self {
            child,
            origin,
            managed_storage,
        };
        server.wait_until_healthy();
        server
    }

    fn wait_until_healthy(&self) {
        let deadline = Instant::now() + Duration::from_secs(60);
        let address = self
            .origin
            .as_str()
            .trim_start_matches("http://")
            .to_owned();
        while Instant::now() < deadline {
            if health_check_succeeds(&address) {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("Music Server did not become healthy at {address}");
    }
}

fn health_check_succeeds(address: &str) -> bool {
    use std::io::{Read, Write};

    let Ok(mut stream) = std::net::TcpStream::connect(address) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    if stream
        .write_all(format!("GET /api/v1/health HTTP/1.0\r\nHost: {address}\r\n\r\n").as_bytes())
        .is_err()
    {
        return false;
    }
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    response.starts_with(b"HTTP/1.1 200") || response.starts_with(b"HTTP/1.0 200")
}

fn server_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../server")
        .canonicalize()
        .expect("server directory")
}

/// Builds the Music Server in the workspace and returns its binary.
fn build_music_server(server_dir: &Path) -> PathBuf {
    let status = Command::new("cargo")
        .args(["build", "--quiet", "-p", "earthly-music-server"])
        .current_dir(server_dir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("run cargo");
    assert!(
        status.success(),
        "cargo build -p earthly-music-server failed"
    );
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| server_dir.join("../target"));
    target.join("debug/earthly-music-server")
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("reserve port")
        .local_addr()
        .expect("port")
        .port()
}
