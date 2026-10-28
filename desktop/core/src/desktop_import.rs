use crate::connection::{ConnectionError, HttpBridge, HttpResponse, ServerOrigin};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::Dir;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio_util::io::ReaderStream;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const SUPPORTED_EXTENSIONS: &[&str] = &["flac", "mp3", "m4a", "ogg", "opus", "wav"];

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSelection {
    pub id: String,
    pub name: String,
    pub size: u64,
}

#[derive(Default)]
pub struct ImportSelectionStore {
    selections: Mutex<HashMap<String, SelectedFile>>,
    cancellations: Mutex<HashMap<String, CancellationToken>>,
}

#[derive(Clone)]
struct SelectedFile {
    path: PathBuf,
    identity: FileIdentity,
    name: String,
    size: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    values: [u64; 4],
}

impl ImportSelectionStore {
    pub fn register_paths(
        &self,
        roots: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Vec<ImportSelection>, DesktopImportError> {
        let mut selected_files = Vec::new();
        for root in roots {
            collect_audio_files(&root, false, &mut selected_files)?;
        }
        selected_files.sort_by(|left, right| left.path.cmp(&right.path));
        self.store_selections(selected_files)
    }

    fn store_selections(
        &self,
        selected_files: Vec<SelectedFile>,
    ) -> Result<Vec<ImportSelection>, DesktopImportError> {
        let mut stored = self.selections.lock().map_err(|_| state_error())?;
        let mut selections = Vec::with_capacity(selected_files.len());
        for selected_file in selected_files {
            let id = Uuid::new_v4().to_string();
            selections.push(ImportSelection {
                id: id.clone(),
                name: selected_file.name.clone(),
                size: selected_file.size,
            });
            stored.insert(id, selected_file);
        }
        Ok(selections)
    }

    #[cfg(test)]
    fn register_candidates(
        &self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Vec<ImportSelection>, DesktopImportError> {
        let selected_files = paths
            .into_iter()
            .map(|path| {
                let file = open_verified_file(&path, None)?;
                selected_file(path, file)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.store_selections(selected_files)
    }

    #[cfg(test)]
    fn selection_count(&self) -> usize {
        self.selections.lock().expect("selection store").len()
    }

    fn selection(&self, id: &str) -> Result<SelectedFile, DesktopImportError> {
        let selections = self.selections.lock().map_err(|_| state_error())?;
        selections
            .get(id)
            .cloned()
            .ok_or_else(invalid_selection_error)
    }

    #[cfg(test)]
    pub fn open_file(&self, id: &str) -> Result<fs::File, DesktopImportError> {
        let selection = self.selection(id)?;
        open_verified_file(&selection.path, Some(selection.identity))
    }

    pub fn release(&self, ids: &[String]) -> Result<(), DesktopImportError> {
        let mut selections = self.selections.lock().map_err(|_| state_error())?;
        for id in ids {
            selections.remove(id);
        }
        Ok(())
    }

    pub fn begin_upload(&self, upload_id: &str) -> Result<CancellationToken, DesktopImportError> {
        let mut cancellations = self.cancellations.lock().map_err(|_| state_error())?;
        Ok(cancellations
            .entry(upload_id.to_owned())
            .or_insert_with(CancellationToken::new)
            .clone())
    }

    pub fn cancel(&self, upload_id: &str) -> Result<(), DesktopImportError> {
        self.cancellations
            .lock()
            .map_err(|_| state_error())?
            .entry(upload_id.to_owned())
            .or_insert_with(CancellationToken::new)
            .cancel();
        Ok(())
    }

    pub fn finish_upload(&self, upload_id: &str) -> Result<(), DesktopImportError> {
        self.cancellations
            .lock()
            .map_err(|_| state_error())?
            .remove(upload_id);
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportUploadProgress {
    pub sent_bytes: u64,
    pub total_bytes: u64,
}

/// Receives upload progress; the shell forwards it to the renderer that
/// started the upload.
pub type ProgressSink = Arc<dyn Fn(ImportUploadProgress) + Send + Sync>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopImportError {
    pub code: &'static str,
    pub message: String,
}

impl From<ConnectionError> for DesktopImportError {
    fn from(error: ConnectionError) -> Self {
        Self {
            code: "transport_error",
            message: error.message,
        }
    }
}

pub async fn upload_selection(
    store: &ImportSelectionStore,
    bridge: &HttpBridge,
    origin: &ServerOrigin,
    selection_id: &str,
    upload_id: &str,
    job_id: &str,
    on_progress: ProgressSink,
) -> Result<HttpResponse, DesktopImportError> {
    let cancellation = store.begin_upload(upload_id)?;
    let result = upload_selection_inner(
        store,
        bridge,
        origin,
        selection_id,
        job_id,
        on_progress,
        cancellation.clone(),
    )
    .await;
    store.finish_upload(upload_id)?;
    if cancellation.is_cancelled() {
        return Err(canceled_error());
    }
    result
}

async fn upload_selection_inner(
    store: &ImportSelectionStore,
    bridge: &HttpBridge,
    origin: &ServerOrigin,
    selection_id: &str,
    job_id: &str,
    on_progress: ProgressSink,
    cancellation: CancellationToken,
) -> Result<HttpResponse, DesktopImportError> {
    let (filename, content_length, body) = cancellable_preparation(
        &cancellation,
        prepare_upload(store, selection_id, on_progress),
    )
    .await?;
    let upload_path = import_upload_path(job_id)?;
    tokio::select! {
        _ = cancellation.cancelled() => Err(canceled_error()),
        result = bridge.send_import(
            origin,
            &upload_path,
            &filename,
            content_type(&filename),
            content_length,
            body,
        ) => result.map_err(Into::into),
    }
}

async fn prepare_upload(
    store: &ImportSelectionStore,
    selection_id: &str,
    on_progress: ProgressSink,
) -> Result<(String, u64, reqwest::Body), DesktopImportError> {
    let selection = store.selection(selection_id)?;
    let filename = selection.name;
    let file = tokio::task::spawn_blocking(move || {
        open_verified_file(&selection.path, Some(selection.identity))
    })
    .await
    .map_err(|_| state_error())??;
    let total_bytes = file.metadata().map_err(|_| selection_error())?.len();
    let file = tokio::fs::File::from_std(file);
    Ok((
        filename,
        total_bytes,
        progress_body(file, total_bytes, on_progress),
    ))
}

async fn cancellable_preparation<T, F>(
    cancellation: &CancellationToken,
    preparation: F,
) -> Result<T, DesktopImportError>
where
    F: Future<Output = Result<T, DesktopImportError>>,
{
    tokio::select! {
        _ = cancellation.cancelled() => Err(canceled_error()),
        result = preparation => result,
    }
}

#[derive(Default)]
struct ProgressThrottle {
    last_percentage: u8,
}

impl ProgressThrottle {
    fn update(&mut self, sent_bytes: u64, total_bytes: u64) -> Option<ImportUploadProgress> {
        if total_bytes == 0 {
            return None;
        }
        let percentage = ((sent_bytes as u128 * 100) / total_bytes as u128).min(100) as u8;
        if percentage <= self.last_percentage {
            return None;
        }
        self.last_percentage = percentage;
        Some(ImportUploadProgress {
            sent_bytes,
            total_bytes,
        })
    }
}

fn progress_body(
    file: tokio::fs::File,
    total_bytes: u64,
    on_progress: ProgressSink,
) -> reqwest::Body {
    let sent_bytes = Arc::new(AtomicU64::new(0));
    let progress_bytes = sent_bytes.clone();
    let mut progress_throttle = ProgressThrottle::default();
    let stream = ReaderStream::new(file).map(move |chunk| {
        let chunk = chunk?;
        let sent =
            progress_bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed) + chunk.len() as u64;
        if let Some(progress) = progress_throttle.update(sent, total_bytes) {
            on_progress(progress);
        }
        Ok::<_, std::io::Error>(chunk)
    });
    reqwest::Body::wrap_stream(stream)
}

pub fn import_upload_path(job_id: &str) -> Result<String, DesktopImportError> {
    if job_id.is_empty()
        || !job_id
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || value == b'-' || value == b'_')
    {
        return Err(DesktopImportError {
            code: "invalid_request",
            message: "Managed Import job ID is invalid.".to_owned(),
        });
    }
    Ok(format!("/api/v1/imports/{job_id}/file"))
}

fn collect_audio_files(
    path: &Path,
    has_hidden_parent: bool,
    files: &mut Vec<SelectedFile>,
) -> Result<(), DesktopImportError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| selection_error())?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    let is_hidden = has_hidden_parent
        || path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.starts_with('.'));
    if is_hidden {
        return Ok(());
    }
    if metadata.is_dir() {
        let directory = open_verified_directory(path)?;
        collect_directory_files(&directory, path, is_hidden, files)?;
    } else if is_supported_regular_audio(path, metadata.is_file()) {
        if path.file_name().and_then(|value| value.to_str()).is_none() {
            eprintln!("Desktop import skipped a selected path with a non-Unicode filename");
            return Ok(());
        }
        let file = open_verified_file(path, None)?;
        files.push(selected_file(path.to_owned(), file)?);
    }
    Ok(())
}

fn collect_directory_files(
    directory: &Dir,
    path: &Path,
    has_hidden_parent: bool,
    files: &mut Vec<SelectedFile>,
) -> Result<(), DesktopImportError> {
    for entry in directory.entries().map_err(|_| selection_error())? {
        let entry = entry.map_err(|_| selection_error())?;
        let name = entry.file_name();
        if name.to_str().is_none() {
            eprintln!("Desktop import skipped a selected path with a non-Unicode filename");
            continue;
        }
        let child_path = path.join(&name);
        let is_hidden = has_hidden_parent || name.to_string_lossy().starts_with('.');
        if is_hidden {
            continue;
        }
        let file_type = entry.file_type().map_err(|_| selection_error())?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let child = directory
                .open_dir_nofollow(&name)
                .map_err(|_| selection_error())?;
            collect_directory_files(&child, &child_path, false, files)?;
        } else if is_supported_regular_audio(&child_path, file_type.is_file()) {
            let mut options = cap_std::fs::OpenOptions::new();
            options.read(true).follow(FollowSymlinks::No);
            let file = directory
                .open_with(&name, &options)
                .map_err(|_| selection_error())?
                .into_std();
            files.push(selected_file(child_path, file)?);
        }
    }
    Ok(())
}

fn selected_file(path: PathBuf, file: fs::File) -> Result<SelectedFile, DesktopImportError> {
    let metadata = file.metadata().map_err(|_| selection_error())?;
    if !metadata.is_file() {
        return Err(selection_error());
    }
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(selection_error)?
        .to_owned();
    Ok(SelectedFile {
        identity: handle_identity(&file, &metadata),
        path,
        name,
        size: metadata.len(),
    })
}

fn open_verified_directory(path: &Path) -> Result<Dir, DesktopImportError> {
    let selected_metadata = fs::symlink_metadata(path).map_err(|_| selection_error())?;
    if !selected_metadata.is_dir() || selected_metadata.file_type().is_symlink() {
        return Err(selection_error());
    }
    let file = open_directory_without_following(path).map_err(|_| selection_error())?;
    let opened_metadata = file.metadata().map_err(|_| selection_error())?;
    if !opened_metadata.is_dir()
        || handle_identity(&file, &opened_metadata) != path_identity(path, &selected_metadata)
    {
        return Err(selection_error());
    }
    Ok(Dir::from_std_file(file))
}

fn open_verified_file(
    path: &Path,
    expected_identity: Option<FileIdentity>,
) -> Result<fs::File, DesktopImportError> {
    let selected_metadata = fs::symlink_metadata(path).map_err(|_| selection_error())?;
    if !selected_metadata.is_file() || selected_metadata.file_type().is_symlink() {
        return Err(selection_error());
    }
    let file = open_without_following(path).map_err(|_| selection_error())?;
    let opened_metadata = file.metadata().map_err(|_| selection_error())?;
    let selected_identity =
        expected_identity.unwrap_or_else(|| path_identity(path, &selected_metadata));
    if !opened_metadata.is_file() || handle_identity(&file, &opened_metadata) != selected_identity {
        return Err(selection_error());
    }
    Ok(file)
}

#[cfg(unix)]
fn open_without_following(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(unix)]
fn open_directory_without_following(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(windows)]
fn open_without_following(path: &Path) -> std::io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(windows)]
fn open_directory_without_following(path: &Path) -> std::io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(unix)]
fn file_identity(metadata: &fs::Metadata) -> FileIdentity {
    use std::os::unix::fs::MetadataExt;

    FileIdentity {
        values: [metadata.dev(), metadata.ino(), 0, 0],
    }
}

#[cfg(unix)]
fn path_identity(_path: &Path, metadata: &fs::Metadata) -> FileIdentity {
    file_identity(metadata)
}

#[cfg(unix)]
fn handle_identity(_file: &fs::File, metadata: &fs::Metadata) -> FileIdentity {
    file_identity(metadata)
}

/// Windows path metadata carries no volume serial or file index, so the
/// identity of a selected path is read from a handle opened without
/// following reparse points.
#[cfg(windows)]
fn path_identity(path: &Path, metadata: &fs::Metadata) -> FileIdentity {
    let file = if metadata.is_dir() {
        open_directory_without_following(path)
    } else {
        open_without_following(path)
    };
    match file {
        Ok(file) => handle_identity(&file, metadata),
        Err(_) => unreadable_identity(1),
    }
}

#[cfg(windows)]
fn handle_identity(file: &fs::File, _metadata: &fs::Metadata) -> FileIdentity {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle belongs to `file`, which outlives this call.
    let result =
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information) };
    if result.is_err() {
        return unreadable_identity(2);
    }
    FileIdentity {
        values: [
            u64::from(information.dwVolumeSerialNumber),
            (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
            0,
            0,
        ],
    }
}

/// An identity that never equals another one, so a file whose identity
/// cannot be read is refused instead of matching.
#[cfg(windows)]
fn unreadable_identity(source: u64) -> FileIdentity {
    FileIdentity {
        values: [u64::MAX, u64::MAX, source, Uuid::new_v4().as_u64_pair().0],
    }
}

fn is_supported_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| SUPPORTED_EXTENSIONS.contains(&value.to_ascii_lowercase().as_str()))
}

fn is_supported_regular_audio(path: &Path, is_file: bool) -> bool {
    is_file && is_supported_audio(path)
}

fn content_type(filename: &str) -> &'static str {
    match Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("flac") => "audio/flac",
        Some("mp3") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        Some("ogg") | Some("opus") => "audio/ogg",
        Some("wav") => "audio/wav",
        _ => "application/octet-stream",
    }
}

fn selection_error() -> DesktopImportError {
    DesktopImportError {
        code: "selection_unavailable",
        message: "Selected import file could not be read.".to_owned(),
    }
}

fn invalid_selection_error() -> DesktopImportError {
    DesktopImportError {
        code: "invalid_selection",
        message: "Selected import file is no longer available. Select it again.".to_owned(),
    }
}

fn state_error() -> DesktopImportError {
    DesktopImportError {
        code: "state_unavailable",
        message: "Desktop import state is unavailable.".to_owned(),
    }
}

fn canceled_error() -> DesktopImportError {
    DesktopImportError {
        code: "canceled",
        message: "Managed Import upload canceled.".to_owned(),
    }
}

#[cfg(test)]
// The directory-walk tests build FIFOs and symlinks, which are Unix-only.
#[cfg_attr(not(unix), allow(unused_imports))]
mod tests {
    use super::{
        ImportSelectionStore, ImportUploadProgress, ProgressThrottle, cancellable_preparation,
        collect_directory_files, content_type, import_upload_path, is_supported_regular_audio,
        open_verified_directory, upload_selection,
    };
    use crate::connection::{HttpBridge, ServerOrigin};
    use std::fs;
    use std::io::Read;
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio_util::sync::CancellationToken;

    #[test]
    fn recursive_selection_returns_only_supported_visible_audio() {
        let root = std::env::temp_dir().join(format!(
            "earthly-import-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        fs::create_dir_all(root.join("disc/.hidden")).expect("create fixture");
        for name in [
            "album.flac",
            "song.mp3",
            "recording.m4a",
            "track.ogg",
            "voice.opus",
            "archive.wav",
        ] {
            fs::write(root.join("disc").join(name), b"audio").expect("write audio");
        }
        fs::write(root.join("disc/cover.jpg"), b"image").expect("write sidecar");
        fs::write(root.join("disc/.hidden/secret.mp3"), b"audio").expect("write hidden");

        let store = ImportSelectionStore::default();
        let selected = store
            .register_paths([root.clone()])
            .expect("register folder");

        assert_eq!(selected.len(), 6);
        assert_eq!(
            selected
                .iter()
                .map(|selection| selection.name.as_str())
                .collect::<Vec<_>>(),
            [
                "album.flac",
                "archive.wav",
                "recording.m4a",
                "song.mp3",
                "track.ogg",
                "voice.opus"
            ]
        );
        assert!(selected.iter().all(|selection| selection.size == 5));
        assert!(
            selected
                .iter()
                .all(|selection| !selection.id.contains("track"))
        );

        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn every_supported_format_uses_an_audio_content_type() {
        for filename in [
            "track.flac",
            "track.mp3",
            "track.m4a",
            "track.ogg",
            "track.opus",
            "track.wav",
        ] {
            assert!(content_type(filename).starts_with("audio/"), "{filename}");
        }
    }

    #[test]
    fn active_upload_can_be_canceled_without_removing_selection() {
        let store = ImportSelectionStore::default();
        let cancellation = store.begin_upload("upload-1").expect("begin upload");

        store.cancel("upload-1").expect("cancel upload");

        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn cancellation_arriving_before_upload_registration_is_retained() {
        let store = ImportSelectionStore::default();

        store.cancel("upload-1").expect("cancel upload");
        let cancellation = store.begin_upload("upload-1").expect("begin upload");

        assert!(cancellation.is_cancelled());
    }

    #[tokio::test]
    async fn cancellation_interrupts_pending_file_preparation() {
        let cancellation = CancellationToken::new();
        let pending_cancellation = cancellation.clone();
        let preparation = async {
            std::future::pending::<()>().await;
            Ok::<_, super::DesktopImportError>(())
        };
        let pending = tokio::spawn(async move {
            cancellable_preparation(&pending_cancellation, preparation).await
        });

        cancellation.cancel();
        let error = tokio::time::timeout(Duration::from_millis(100), pending)
            .await
            .expect("preparation stops promptly")
            .expect("preparation task")
            .expect_err("preparation is canceled");

        assert_eq!(error.code, "canceled");
    }

    #[test]
    fn progress_is_coalesced_to_percentage_transitions() {
        let mut throttle = ProgressThrottle::default();
        let updates = (1..=256)
            .filter_map(|chunk| throttle.update(chunk * 4096, 1024 * 1024))
            .collect::<Vec<_>>();

        assert_eq!(updates.len(), 100);
        assert_eq!(
            updates.last().map(|progress| progress.sent_bytes),
            Some(1024 * 1024)
        );
    }

    #[test]
    fn selection_remains_retryable_until_explicit_release() {
        let fixture = temporary_fixture_path("flac");
        fs::write(&fixture, b"audio bytes").expect("write fixture");
        let store = ImportSelectionStore::default();
        let selection = store
            .register_paths([fixture.clone()])
            .expect("register file")
            .remove(0);

        store.open_file(&selection.id).expect("first upload");
        store.open_file(&selection.id).expect("retry upload");
        store
            .release(std::slice::from_ref(&selection.id))
            .expect("release selection");

        assert!(store.open_file(&selection.id).is_err());
        fs::remove_file(fixture).expect("remove fixture");
    }

    #[test]
    fn failed_registration_does_not_retain_partial_selections() {
        let fixture = temporary_fixture_path("flac");
        let missing = temporary_fixture_path("mp3");
        fs::write(&fixture, b"audio bytes").expect("write fixture");
        let store = ImportSelectionStore::default();

        assert!(
            store
                .register_candidates([fixture.clone(), missing])
                .is_err()
        );
        assert_eq!(store.selection_count(), 0);

        fs::remove_file(fixture).expect("remove fixture");
    }

    #[cfg(unix)]
    #[test]
    fn special_file_with_audio_extension_is_not_an_import_candidate() {
        let fifo = temporary_fixture_path("mp3");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("create FIFO");
        assert!(status.success());
        let file_type = fs::symlink_metadata(&fifo)
            .expect("FIFO metadata")
            .file_type();

        assert!(!is_supported_regular_audio(&fifo, file_type.is_file()));

        fs::remove_file(fifo).expect("remove FIFO");
    }

    #[cfg(unix)]
    #[test]
    fn directory_traversal_stays_bound_to_open_directory_handle() {
        use std::os::unix::fs::symlink;

        let root = temporary_fixture_path("root");
        let album = root.join("album");
        let moved_album = root.join("moved-album");
        let outside = temporary_fixture_path("outside");
        fs::create_dir_all(&album).expect("create selected directory");
        fs::create_dir_all(&outside).expect("create outside directory");
        fs::write(album.join("song.flac"), b"selected").expect("write selected file");
        fs::write(outside.join("secret.flac"), b"secret").expect("write outside file");
        let directory = open_verified_directory(&album).expect("pin selected directory");
        fs::rename(&album, &moved_album).expect("move selected directory");
        symlink(&outside, &album).expect("replace directory with symlink");
        let mut selections = Vec::new();

        collect_directory_files(&directory, &album, false, &mut selections)
            .expect("traverse pinned directory");

        assert_eq!(selections.len(), 1);
        assert_eq!(selections[0].name, "song.flac");
        fs::remove_file(album).expect("remove replacement symlink");
        fs::remove_dir_all(root).expect("remove selected tree");
        fs::remove_dir_all(outside).expect("remove outside tree");
    }

    #[cfg(unix)]
    #[test]
    fn selected_file_rejects_a_replacement_symlink() {
        use std::os::unix::fs::symlink;

        let fixture = temporary_fixture_path("flac");
        let replacement = temporary_fixture_path("secret");
        fs::write(&fixture, b"selected bytes").expect("write selected file");
        fs::write(&replacement, b"secret bytes").expect("write replacement");
        let store = ImportSelectionStore::default();
        let selection = store
            .register_paths([fixture.clone()])
            .expect("register file")
            .remove(0);
        fs::remove_file(&fixture).expect("remove selected path");
        symlink(&replacement, &fixture).expect("replace path with symlink");

        assert!(store.open_file(&selection.id).is_err());
        fs::remove_file(fixture).expect("remove symlink");
        fs::remove_file(replacement).expect("remove replacement");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancellation_interrupts_wait_for_server_response() {
        let fixture = temporary_fixture_path("flac");
        fs::write(&fixture, b"audio bytes").expect("write fixture");
        let store = Arc::new(ImportSelectionStore::default());
        let selection = store
            .register_paths([fixture.clone()])
            .expect("register file")
            .remove(0);
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind server");
        let origin = ServerOrigin::parse(&format!(
            "http://{}",
            listener.local_addr().expect("server address")
        ))
        .expect("parse origin");
        let (request_sender, request_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).expect("read request");
            assert!(read > 0, "request must contain headers");
            request_sender.send(()).expect("request received");
            release_receiver.recv().expect("release server");
        });
        let upload_store = store.clone();
        let upload = tokio::spawn(async move {
            upload_selection(
                &upload_store,
                &HttpBridge::new().expect("create bridge"),
                &origin,
                &selection.id,
                "upload-1",
                "job-1",
                Arc::new(|_| {}) as super::ProgressSink,
            )
            .await
        });
        request_receiver.recv().expect("wait for request");

        store.cancel("upload-1").expect("cancel upload");
        let error = tokio::time::timeout(Duration::from_millis(500), upload)
            .await
            .expect("upload stops promptly")
            .expect("upload task")
            .expect_err("upload is canceled");

        assert_eq!(error.code, "canceled");
        release_sender.send(()).expect("release server");
        fs::remove_file(fixture).expect("remove fixture");
    }

    /// Binds a listener that must never be connected to and reports whether a
    /// connection arrived within a short grace period.
    fn unexpected_connection_probe() -> (ServerOrigin, mpsc::Receiver<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind probe");
        let origin = ServerOrigin::parse(&format!(
            "http://{}",
            listener.local_addr().expect("probe address")
        ))
        .expect("parse origin");
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            if listener.accept().is_ok() {
                let _ = sender.send(());
            }
        });
        (origin, receiver)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unknown_selection_id_never_opens_a_connection() {
        let store = ImportSelectionStore::default();
        let (origin, connected) = unexpected_connection_probe();

        let error = upload_selection(
            &store,
            &HttpBridge::new().expect("create bridge"),
            &origin,
            "not-a-registered-selection",
            "upload-1",
            "job-1",
            Arc::new(|_| {}) as super::ProgressSink,
        )
        .await
        .expect_err("unknown selection fails");

        assert_eq!(error.code, "invalid_selection");
        assert!(
            connected.recv_timeout(Duration::from_millis(200)).is_err(),
            "a forged selection ID must not reach the network"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn hostile_job_id_never_opens_a_connection() {
        let fixture = temporary_fixture_path("flac");
        fs::write(&fixture, b"audio bytes").expect("write fixture");
        let store = ImportSelectionStore::default();
        let selection = store
            .register_paths([fixture.clone()])
            .expect("register file")
            .remove(0);
        let (origin, connected) = unexpected_connection_probe();

        let error = upload_selection(
            &store,
            &HttpBridge::new().expect("create bridge"),
            &origin,
            &selection.id,
            "upload-1",
            "../../attacker.example/collect",
            Arc::new(|_| {}) as super::ProgressSink,
        )
        .await
        .expect_err("hostile job id fails");

        assert_eq!(error.code, "invalid_request");
        assert!(
            connected.recv_timeout(Duration::from_millis(200)).is_err(),
            "an invalid job ID must not reach the network"
        );
        fs::remove_file(fixture).expect("remove fixture");
    }

    #[test]
    fn explicit_file_selection_skips_unsupported_and_missing_extensions() {
        let root = temporary_fixture_path("files");
        fs::create_dir_all(&root).expect("create root");
        let track = root.join("Track One.flac");
        let sidecar = root.join("cover.jpg");
        let bare = root.join("README");
        fs::write(&track, b"flac").expect("write track");
        fs::write(&sidecar, b"jpeg").expect("write sidecar");
        fs::write(&bare, b"text").expect("write bare");
        let store = ImportSelectionStore::default();

        let selections = store
            .register_paths([track.clone(), sidecar, bare])
            .expect("register explicit files");

        assert_eq!(selections.len(), 1);
        assert_eq!(selections[0].name, "Track One.flac");
        assert!(selections[0].id.parse::<uuid::Uuid>().is_ok());
        assert!(!selections[0].id.contains("Track"));
        fs::remove_dir_all(root).expect("remove root");
    }

    /// Captures one PUT request in full (headers and exact body) and answers
    /// with an empty JSON object.
    fn capture_upload() -> (ServerOrigin, mpsc::Receiver<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind capture server");
        let origin = ServerOrigin::parse(&format!(
            "http://{}",
            listener.local_addr().expect("capture address")
        ))
        .expect("parse origin");
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept upload");
            let mut request = Vec::new();
            let mut chunk = [0_u8; 4096];
            let mut expected_total = None;
            loop {
                let read = stream.read(&mut chunk).expect("read upload");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                if expected_total.is_none()
                    && let Some(header_end) =
                        request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    let headers =
                        String::from_utf8_lossy(&request[..header_end]).to_ascii_lowercase();
                    let content_length = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .expect("content-length header");
                    expected_total = Some(header_end + 4 + content_length);
                }
                if expected_total.is_some_and(|total| request.len() >= total) {
                    break;
                }
            }
            std::io::Write::write_all(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
            )
            .expect("write response");
            sender.send(request).expect("deliver request");
        });
        (origin, receiver)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn every_supported_format_uploads_byte_for_byte_without_client_metadata() {
        for extension in super::SUPPORTED_EXTENSIONS {
            let fixture = temporary_fixture_path(extension);
            let payload = (0..=255_u8)
                .cycle()
                .take(3 * 4096 + 17)
                .collect::<Vec<u8>>();
            fs::write(&fixture, &payload).expect("write fixture");
            let store = ImportSelectionStore::default();
            let selection = store
                .register_paths([fixture.clone()])
                .expect("register file")
                .remove(0);
            let (origin, captured) = capture_upload();
            let progress = Arc::new(std::sync::Mutex::new(Vec::new()));
            let recorded = progress.clone();
            let channel: super::ProgressSink = Arc::new(move |update: ImportUploadProgress| {
                recorded.lock().expect("progress lock").push(update);
            });

            let response = upload_selection(
                &store,
                &HttpBridge::new().expect("create bridge"),
                &origin,
                &selection.id,
                "upload-1",
                "job-1",
                channel,
            )
            .await
            .unwrap_or_else(|error| panic!("{extension}: {}", error.message));
            assert_eq!(response.status, 200, "{extension}");

            let request = captured.recv().expect("captured request");
            let header_end = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .expect("header terminator");
            let headers = String::from_utf8_lossy(&request[..header_end]).to_ascii_lowercase();
            let body = &request[header_end + 4..];
            assert_eq!(
                body,
                payload.as_slice(),
                "{extension} body must be untouched"
            );
            assert!(
                headers.starts_with("put /api/v1/imports/job-1/file http/1.1"),
                "{headers}"
            );
            assert!(
                headers.contains(&format!("content-length: {}", payload.len())),
                "{headers}"
            );
            assert!(
                headers.contains(&format!(
                    "x-import-filename: {}",
                    selection.name.to_ascii_lowercase()
                )),
                "{headers}"
            );
            let header_names = headers
                .lines()
                .skip(1)
                .filter_map(|line| line.split_once(':').map(|(name, _)| name.trim().to_owned()))
                .collect::<Vec<_>>();
            for name in &header_names {
                assert!(
                    [
                        "accept",
                        "content-type",
                        "content-length",
                        "x-import-filename",
                        "x-import-filename-encoding",
                        "host",
                        "user-agent",
                        "accept-encoding",
                    ]
                    .contains(&name.as_str()),
                    "{extension}: unexpected header {name} (native code must not send metadata)"
                );
            }
            let progress = progress.lock().expect("progress lock").clone();
            assert_eq!(
                progress.last().map(|update| update.sent_bytes),
                Some(payload.len() as u64),
                "{extension} progress reaches the full size"
            );
            fs::remove_file(fixture).expect("remove fixture");
        }
    }

    #[test]
    fn upload_path_rejects_values_that_can_escape_import_endpoint() {
        assert_eq!(
            import_upload_path("01990b44-f903-7b30-a9b1-b851396cdd45").expect("valid job id"),
            "/api/v1/imports/01990b44-f903-7b30-a9b1-b851396cdd45/file"
        );
        assert!(import_upload_path("../health").is_err());
        assert!(import_upload_path("job?redirect=http://attacker.example").is_err());
    }

    fn temporary_fixture_path(extension: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "earthly-import-{}.{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos(),
            extension
        ))
    }
}
