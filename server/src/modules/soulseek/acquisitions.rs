//! Soulseek Acquisitions: an Acquisition Version downloaded through slskd,
//! source after source, and handed to Managed Import.
//!
//! The background worker is the only writer of transfer state. Every poll it
//! reads the current source's transfers from slskd, records progress, and
//! moves to the next source of the version when the current one fails,
//! refuses or stalls. A file that arrived from an earlier source is kept: a
//! later source only supplies the files still missing, matched by file name
//! without the folder (case-insensitively), else by position when the source
//! has as many audio files as the acquisition.

use super::service::{Soulseek, SoulseekError};
use super::slskd::{SlskdError, TransferState};
use super::versions::{Quality, RemoteFile, Scope};
use crate::auth::DEFAULT_USER_ID;
use crate::modules::managed_import::service::BatchProgress;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio_util::sync::CancellationToken;

/// How often the worker polls slskd, and how long a source's transfer in
/// progress may move no bytes before the next source is tried. Time spent
/// queued is not counted. Tests shorten both.
#[derive(Clone, Copy, Debug)]
pub struct WorkerTiming {
    pub poll_interval: Duration,
    pub stall_timeout: Duration,
}

impl Default for WorkerTiming {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(3),
            stall_timeout: Duration::from_secs(120),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcquisitionState {
    /// Some audio file is still to arrive from slskd.
    Downloading,
    /// Every file arrived; Managed Import is taking them.
    Importing,
    /// The Import Preview awaits the listener's confirmation.
    Review,
    /// The listener confirmed the Import Batch.
    Completed,
    /// No source of the version is left for some file; the files that
    /// arrived are in the Import Preview.
    Failed,
    Cancelled,
}

impl AcquisitionState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Downloading => "downloading",
            Self::Importing => "importing",
            Self::Review => "review",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [
            Self::Downloading,
            Self::Importing,
            Self::Review,
            Self::Completed,
            Self::Failed,
            Self::Cancelled,
        ]
        .into_iter()
        .find(|state| state.name() == name)
    }

    fn is_downloading(self) -> bool {
        matches!(self, Self::Downloading | Self::Importing)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Audio,
    /// The source folder's cover image, for the Album Artwork.
    Artwork,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileState {
    Queued,
    Downloading,
    /// In slskd's download directory.
    Downloaded,
    /// Managed Import took the file into the acquisition's batch.
    Staged,
    /// No source supplied it.
    Failed,
}

impl FileState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Downloaded => "downloaded",
            Self::Staged => "staged",
            Self::Failed => "failed",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [
            Self::Queued,
            Self::Downloading,
            Self::Downloaded,
            Self::Staged,
            Self::Failed,
        ]
        .into_iter()
        .find(|state| state.name() == name)
    }

    /// Still to arrive from a source.
    fn is_missing(self) -> bool {
        matches!(self, Self::Queued | Self::Downloading)
    }
}

/// One source of the version as snapshotted at start.
#[derive(Clone, Debug, PartialEq)]
pub struct AcquisitionSource {
    pub id: String,
    pub username: String,
    pub folder: String,
    pub files: Vec<RemoteFile>,
    pub artwork: Vec<RemoteFile>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AcquisitionFile {
    pub kind: FileKind,
    /// The file name without its folder.
    pub name: String,
    /// The source it is taken from, or was last taken from.
    pub source_index: usize,
    pub remote_filename: String,
    pub size: u64,
    pub transfer_id: Option<String>,
    pub state: FileState,
    pub bytes_transferred: u64,
    pub job_id: Option<String>,
    pub outcome: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Acquisition {
    pub id: String,
    pub query: String,
    pub scope: Scope,
    pub version_key: String,
    pub quality: Quality,
    pub sources: Vec<AcquisitionSource>,
    /// The source being tried; the source count once all were tried.
    pub source_index: usize,
    pub state: AcquisitionState,
    pub batch_id: Option<String>,
    pub failure_reason: Option<String>,
    /// When the current source last made progress (Unix milliseconds).
    pub progress_at_ms: i64,
    pub created_at: String,
    pub updated_at: String,
    pub files: Vec<AcquisitionFile>,
}

impl Acquisition {
    pub fn current_source(&self) -> Option<&AcquisitionSource> {
        self.sources.get(self.source_index)
    }

    pub fn sources_tried(&self) -> usize {
        (self.source_index + 1).min(self.sources.len())
    }

    fn audio(&self) -> impl Iterator<Item = &AcquisitionFile> {
        self.files
            .iter()
            .filter(|file| file.kind == FileKind::Audio)
    }

    /// Whether an audio file waits on the current source.
    fn pending_audio(&self) -> bool {
        self.pending().any(|(_, file)| file.kind == FileKind::Audio)
    }

    /// Files waiting on the current source.
    fn pending(&self) -> impl Iterator<Item = (usize, &AcquisitionFile)> {
        self.files
            .iter()
            .enumerate()
            .filter(|(_, file)| file.source_index == self.source_index && file.state.is_missing())
    }
}

/// The name of a remote file without its folder; Soulseek paths usually
/// use backslashes.
pub fn file_name(remote: &str) -> &str {
    remote.rsplit(['\\', '/']).next().unwrap_or(remote)
}

/// Where slskd puts a completed download by default: its downloads
/// directory, then the remote file's own folder name
/// (`Transfers.Download.Destination.Subdirectory` = `${SOURCE_DIRECTORY}`),
/// then the file name.
pub fn local_path(downloads: &Path, remote: &str) -> PathBuf {
    let mut parts = remote.rsplit(['\\', '/']);
    let name = parts.next().unwrap_or(remote);
    match parts.next().filter(|folder| !folder.is_empty()) {
        Some(folder) => downloads.join(folder).join(name),
        None => downloads.join(name),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

impl Soulseek {
    /// Starts acquiring a version of a search, or returns the active
    /// acquisition of the same query, scope and version (`false`).
    pub async fn start_acquisition(
        &self,
        search_id: &str,
        version_key: &str,
        source_id: Option<&str>,
    ) -> Result<(Acquisition, bool), SoulseekError> {
        self.ensure_enabled()?;
        let resolved = self
            .resolve_version(search_id, version_key, source_id)
            .await?;
        let repo = self.repo();
        let _lock = self.acquisition_lock().await;
        if let Some(active) = repo
            .find_active(
                DEFAULT_USER_ID,
                &resolved.query,
                resolved.scope,
                &resolved.version.key,
            )
            .await
            .map_err(internal)?
        {
            return Ok((self.observe(active).await, false));
        }
        let sources: Vec<AcquisitionSource> = resolved
            .version
            .sources
            .into_iter()
            .map(|source| AcquisitionSource {
                id: source.id,
                username: source.username,
                folder: source.folder,
                files: source.files,
                artwork: source.artwork,
            })
            .collect();
        let Some(first) = sources.first() else {
            return Err(SoulseekError::VersionNotFound);
        };
        let queued = |kind: FileKind, remote: &RemoteFile| AcquisitionFile {
            kind,
            name: file_name(&remote.filename).to_owned(),
            source_index: 0,
            remote_filename: remote.filename.clone(),
            size: remote.size,
            transfer_id: None,
            state: FileState::Queued,
            bytes_transferred: 0,
            job_id: None,
            outcome: None,
        };
        let mut files: Vec<AcquisitionFile> = first
            .files
            .iter()
            .map(|remote| queued(FileKind::Audio, remote))
            .collect();
        if let Some(cover) = cover(&first.artwork) {
            files.push(queued(FileKind::Artwork, cover));
        }
        let batch = self
            .managed_import()
            .create_server_batch()
            .await
            .map_err(internal)?;
        let mut acquisition = Acquisition {
            id: uuid::Uuid::new_v4().to_string(),
            query: resolved.query,
            scope: resolved.scope,
            version_key: resolved.version.key,
            quality: resolved.version.quality,
            sources,
            source_index: 0,
            state: AcquisitionState::Downloading,
            batch_id: Some(batch.id),
            failure_reason: None,
            progress_at_ms: now_ms(),
            created_at: String::new(),
            updated_at: String::new(),
            files,
        };
        if let Err(error) = repo.insert(DEFAULT_USER_ID, &acquisition).await {
            self.cancel_batch(&acquisition).await;
            return Err(internal(error));
        }
        self.wake.notify_one();
        if let Some(stored) = repo
            .get(DEFAULT_USER_ID, &acquisition.id)
            .await
            .map_err(internal)?
        {
            acquisition = stored;
        }
        Ok((acquisition, true))
    }

    pub async fn list_acquisitions(&self) -> Result<Vec<Acquisition>, SoulseekError> {
        self.ensure_enabled()?;
        let acquisitions = self.repo().list(DEFAULT_USER_ID).await.map_err(internal)?;
        let mut observed = Vec::with_capacity(acquisitions.len());
        for acquisition in acquisitions {
            observed.push(self.observe(acquisition).await);
        }
        Ok(observed)
    }

    pub async fn get_acquisition(&self, id: &str) -> Result<Acquisition, SoulseekError> {
        self.ensure_enabled()?;
        let acquisition = self
            .repo()
            .get(DEFAULT_USER_ID, id)
            .await
            .map_err(internal)?
            .ok_or(SoulseekError::AcquisitionNotFound)?;
        Ok(self.observe(acquisition).await)
    }

    /// The acquisition as the listener sees it now: completed as soon as its
    /// batch is confirmed, cancelled as soon as its batch is cancelled
    /// through the import-batches routes, before the worker records it.
    /// A failed acquisition's batch holds the files that arrived; confirming
    /// it completes the acquisition too, as `completed` always follows
    /// confirmation, and its failure reason still names the missing files.
    async fn observe(&self, mut acquisition: Acquisition) -> Acquisition {
        if matches!(
            acquisition.state,
            AcquisitionState::Completed | AcquisitionState::Cancelled
        ) {
            return acquisition;
        }
        match self.batch_progress(&acquisition).await {
            Ok(BatchProgress::Completed) => {
                acquisition.state = AcquisitionState::Completed;
            }
            Ok(BatchProgress::Gone) if acquisition.state != AcquisitionState::Failed => {
                acquisition.state = AcquisitionState::Cancelled;
            }
            _ => {}
        }
        acquisition
    }

    /// Where the acquisition's batch is; `Gone` without a batch.
    async fn batch_progress(
        &self,
        acquisition: &Acquisition,
    ) -> Result<BatchProgress, SoulseekError> {
        let Some(batch_id) = &acquisition.batch_id else {
            return Ok(BatchProgress::Gone);
        };
        self.managed_import()
            .batch_progress(batch_id)
            .await
            .map_err(internal)
    }

    /// Cancels the acquisition's batch unless it is confirmed; logged on
    /// failure.
    async fn cancel_batch(&self, acquisition: &Acquisition) {
        let Some(batch_id) = &acquisition.batch_id else {
            return;
        };
        if let Err(error) = self
            .managed_import()
            .cancel_unconfirmed_batch(batch_id)
            .await
        {
            tracing::warn!(%error, acquisition = %acquisition.id, "cancel the acquisition's Import Batch failed");
        }
    }

    /// Cancels an active acquisition: stops slskd's transfers, deletes the
    /// downloaded copies, cancels its Import Batch and marks it cancelled. A
    /// finished one (failed, completed or cancelled) is dismissed instead:
    /// its unconfirmed batch is cancelled, it is removed, and the listener
    /// no longer sees it. Neither is possible while the batch commits.
    pub async fn cancel_acquisition(&self, id: &str) -> Result<(), SoulseekError> {
        self.ensure_enabled()?;
        let repo = self.repo();
        let _lock = self.acquisition_lock().await;
        let acquisition = repo
            .get(DEFAULT_USER_ID, id)
            .await
            .map_err(internal)?
            .ok_or(SoulseekError::AcquisitionNotFound)?;
        let progress = self.batch_progress(&acquisition).await?;
        if progress == BatchProgress::Confirming {
            return Err(SoulseekError::Conflict(
                "the acquisition's Import Batch is being committed".to_owned(),
            ));
        }
        let mut acquisition = self.observe(acquisition).await;
        self.discard_downloads(&mut acquisition).await;
        if progress == BatchProgress::Open {
            self.cancel_batch(&acquisition).await;
        }
        match acquisition.state {
            AcquisitionState::Failed
            | AcquisitionState::Completed
            | AcquisitionState::Cancelled => repo
                .delete(DEFAULT_USER_ID, &acquisition.id)
                .await
                .map_err(internal),
            AcquisitionState::Downloading
            | AcquisitionState::Importing
            | AcquisitionState::Review => {
                acquisition.state = AcquisitionState::Cancelled;
                repo.save(&acquisition).await.map_err(internal)
            }
        }
    }

    /// Removes every transfer of the acquisition from slskd and deletes the
    /// copies in slskd's download directory. Failures are logged: a copy
    /// that cannot be removed now is not worth failing the caller.
    async fn discard_downloads(&self, acquisition: &mut Acquisition) {
        let slskd = self.slskd_client();
        for file in &mut acquisition.files {
            if let (Some(slskd), Some(transfer_id)) = (slskd, file.transfer_id.take()) {
                let username = &acquisition.sources[file.source_index].username;
                if let Err(error) = slskd.remove_download(username, &transfer_id).await {
                    tracing::warn!(%error, acquisition = %acquisition.id, "remove slskd download failed");
                }
            }
            if file.state != FileState::Staged {
                self.remove_download_copy(&file.remote_filename);
            }
            if file.state.is_missing() {
                file.bytes_transferred = 0;
            }
        }
    }

    /// Deletes one download from slskd's directory, and its folder when that
    /// leaves it empty.
    fn remove_download_copy(&self, remote: &str) {
        let Some(downloads) = self.downloads_path() else {
            return;
        };
        let path = local_path(downloads, remote);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "remove downloaded copy failed");
                return;
            }
        }
        if let Some(folder) = path.parent()
            && folder != downloads
        {
            // Fails, as wanted, while the folder holds other files.
            let _ = std::fs::remove_dir(folder);
        }
    }

    /// Restart recovery, then the worker's poll loop, until the server stops
    /// or the returned handle aborts it. Nothing runs while Soulseek
    /// Acquisition is off.
    pub fn start(self: &Arc<Self>, timing: WorkerTiming) -> Option<tokio::task::AbortHandle> {
        if !self.is_enabled() {
            return None;
        }
        let soulseek = self.clone();
        let task = tokio::spawn(async move {
            soulseek.recover().await;
            loop {
                soulseek.poll(timing).await;
                tokio::select! {
                    () = tokio::time::sleep(timing.poll_interval) => {}
                    () = soulseek.wake.notified() => {}
                }
            }
        });
        Some(task.abort_handle())
    }

    /// After a restart the time the server was down is not the sources'
    /// fault: every active acquisition's stall clock starts again. The
    /// next poll reconciles with slskd's transfer list, which slskd kept.
    async fn recover(&self) {
        let repo = self.repo();
        let active = match repo.list_active().await {
            Ok(active) => active,
            Err(error) => {
                tracing::error!(%error, "Soulseek Acquisition recovery failed");
                return;
            }
        };
        let _lock = self.acquisition_lock().await;
        for (user_id, id) in active {
            let Ok(Some(mut acquisition)) = repo.get(&user_id, &id).await else {
                continue;
            };
            acquisition.progress_at_ms = now_ms();
            if let Err(error) = repo.save(&acquisition).await {
                tracing::error!(%error, acquisition = %id, "Soulseek Acquisition recovery failed");
            }
        }
    }

    async fn poll(&self, timing: WorkerTiming) {
        let repo = self.repo();
        let active = match repo.list_active().await {
            Ok(active) => active,
            Err(error) => {
                tracing::error!(%error, "list active Soulseek Acquisitions failed");
                return;
            }
        };
        for (user_id, id) in active {
            let _lock = self.acquisition_lock().await;
            let acquisition = match repo.get(&user_id, &id).await {
                Ok(Some(acquisition)) => acquisition,
                Ok(None) => continue,
                Err(error) => {
                    tracing::error!(%error, acquisition = %id, "read Soulseek Acquisition failed");
                    continue;
                }
            };
            let mut next = acquisition.clone();
            self.step(&mut next, timing).await;
            if next != acquisition
                && let Err(error) = repo.save(&next).await
            {
                tracing::error!(%error, acquisition = %id, "save Soulseek Acquisition failed");
            }
        }
    }

    /// One poll of one acquisition.
    async fn step(&self, acquisition: &mut Acquisition, timing: WorkerTiming) {
        // The listener may confirm or cancel the batch at any time through
        // the import-batches routes.
        match self.batch_progress(acquisition).await {
            Err(error) => {
                tracing::error!(%error, acquisition = %acquisition.id, "read the acquisition's Import Batch failed");
                return;
            }
            Ok(BatchProgress::Completed) => {
                self.discard_downloads(acquisition).await;
                acquisition.state = AcquisitionState::Completed;
                return;
            }
            Ok(BatchProgress::Gone) => {
                self.discard_downloads(acquisition).await;
                acquisition.state = AcquisitionState::Cancelled;
                return;
            }
            Ok(BatchProgress::Open | BatchProgress::Confirming) => {}
        }
        if acquisition.state.is_downloading() {
            self.download(acquisition, timing).await;
            // Feeding saves the acquisition after every file it hands over,
            // which can take minutes: it reads importing as soon as every
            // audio file arrived, not once Managed Import has them all.
            self.settle(acquisition);
            self.feed(acquisition).await;
        }
        self.settle(acquisition);
        if acquisition.state == AcquisitionState::Failed
            && !acquisition
                .audio()
                .any(|file| file.state == FileState::Staged)
        {
            // Nothing arrived, so there is nothing to review.
            self.cancel_batch(acquisition).await;
            acquisition.batch_id = None;
        }
    }

    /// Hands every downloaded audio file to Managed Import, then the cover
    /// image once the Albums are known. A handed-over file's download copy
    /// and slskd transfer are removed.
    async fn feed(&self, acquisition: &mut Acquisition) {
        let Some(batch_id) = acquisition.batch_id.clone() else {
            return;
        };
        let import = self.managed_import();
        for index in 0..acquisition.files.len() {
            let file = &acquisition.files[index];
            if file.kind != FileKind::Audio || file.state != FileState::Downloaded {
                continue;
            }
            let Some(path) = self.downloaded_copy(&file.remote_filename) else {
                tracing::warn!(acquisition = %acquisition.id, file = %file.name, "downloaded copy disappeared");
                acquisition.files[index].state = FileState::Failed;
                continue;
            };
            // A job from before a restart that never got its file.
            if let Some(job_id) = acquisition.files[index].job_id.take()
                && let Err(error) = import.cancel_job(&job_id).await
            {
                tracing::warn!(%error, acquisition = %acquisition.id, "cancel unfinished Managed Import job failed");
            }
            let job = match import
                .create_job(&batch_id, &uuid::Uuid::new_v4().to_string())
                .await
            {
                Ok(job) => job,
                Err(error) => {
                    // The batch was confirmed or cancelled meanwhile; the
                    // next step sees it.
                    tracing::warn!(%error, acquisition = %acquisition.id, "create Managed Import job failed");
                    return;
                }
            };
            acquisition.files[index].job_id = Some(job.id.clone());
            if let Err(error) = self.repo().save(acquisition).await {
                tracing::error!(%error, acquisition = %acquisition.id, "save Soulseek Acquisition failed");
            }
            let name = acquisition.files[index].name.clone();
            let outcome = match import
                .upload_from_path(&CancellationToken::new(), &job.id, &name, &path)
                .await
            {
                Ok(_) => "previewed",
                // The Import Preview shows why.
                Err(_) => "rejected",
            };
            let file = &mut acquisition.files[index];
            file.outcome = Some(outcome.to_owned());
            file.state = FileState::Staged;
            self.hand_over(acquisition, index).await;
        }
        let audio_done = acquisition
            .audio()
            .all(|file| matches!(file.state, FileState::Staged | FileState::Failed));
        let Some(index) = acquisition
            .files
            .iter()
            .position(|file| file.kind == FileKind::Artwork && file.state == FileState::Downloaded)
        else {
            return;
        };
        if !audio_done {
            return;
        }
        let attached = match self.downloaded_copy(&acquisition.files[index].remote_filename) {
            Some(path) => self.attach_artwork(&batch_id, &path).await,
            None => false,
        };
        let file = &mut acquisition.files[index];
        file.outcome = Some(if attached { "attached" } else { "skipped" }.to_owned());
        file.state = FileState::Staged;
        self.hand_over(acquisition, index).await;
    }

    /// Offers the cover image for every Album the batch's preview groups.
    /// An image Managed Import does not accept is skipped silently.
    async fn attach_artwork(&self, batch_id: &str, path: &Path) -> bool {
        let Ok(data) = tokio::fs::read(path).await else {
            return false;
        };
        let import = self.managed_import();
        let Ok(batch) = import.get_batch(batch_id).await else {
            return false;
        };
        let mut attached = false;
        for album in &batch.albums {
            attached |= import
                .upload_artwork(batch_id, &album.key, &data)
                .await
                .is_ok();
        }
        attached
    }

    /// Deletes a file's download copy and removes its slskd transfer once
    /// Managed Import has it.
    async fn hand_over(&self, acquisition: &mut Acquisition, index: usize) {
        let username = acquisition.sources[acquisition.files[index].source_index]
            .username
            .clone();
        let file = &mut acquisition.files[index];
        self.remove_download_copy(&file.remote_filename);
        if let (Some(slskd), Some(transfer_id)) = (self.slskd_client(), file.transfer_id.take())
            && let Err(error) = slskd.remove_download(&username, &transfer_id).await
        {
            tracing::warn!(%error, "remove finished slskd download failed");
        }
    }

    /// Reads the current source's transfers, records progress, queues what
    /// slskd does not list yet, and moves on when the source fails or
    /// stalls.
    async fn download(&self, acquisition: &mut Acquisition, timing: WorkerTiming) {
        let Some(slskd) = self.slskd_client() else {
            return;
        };
        let Some(source) = acquisition.current_source().cloned() else {
            return;
        };
        if acquisition.pending().next().is_some() {
            let transfers = match slskd.downloads(&source.username).await {
                Ok(transfers) => transfers,
                Err(error) => {
                    // slskd being away is not the source's fault.
                    tracing::warn!(%error, acquisition = %acquisition.id, "read slskd downloads failed");
                    acquisition.progress_at_ms = now_ms();
                    return;
                }
            };
            let mut progressed = false;
            // Bytes are moving, or should be, for some file of the source.
            let mut transferring = false;
            // An audio file waits in slskd's or the user's upload queue.
            let mut queued_audio = false;
            let mut failure: Option<String> = None;
            let mut unlisted: Vec<usize> = Vec::new();
            let pending: Vec<usize> = acquisition.pending().map(|(index, _)| index).collect();
            for index in pending {
                let file = &mut acquisition.files[index];
                let transfer = transfers
                    .iter()
                    .find(|transfer| Some(&transfer.id) == file.transfer_id.as_ref())
                    .or_else(|| {
                        transfers
                            .iter()
                            .rev()
                            .find(|transfer| transfer.filename == file.remote_filename)
                    });
                let Some(transfer) = transfer else {
                    file.transfer_id = None;
                    unlisted.push(index);
                    continue;
                };
                file.transfer_id = Some(transfer.id.clone());
                let before = (file.state, file.bytes_transferred);
                match &transfer.state {
                    TransferState::Queued => {
                        file.state = FileState::Queued;
                        queued_audio |= file.kind == FileKind::Audio;
                    }
                    TransferState::InProgress => {
                        transferring = true;
                        file.state = FileState::Downloading;
                        file.bytes_transferred = transfer.bytes_transferred;
                    }
                    TransferState::Succeeded => {
                        file.bytes_transferred = transfer.bytes_transferred.max(file.size);
                        // slskd moves the file in after marking it done.
                        file.state = if self.downloaded_copy(&file.remote_filename).is_some() {
                            FileState::Downloaded
                        } else {
                            FileState::Downloading
                        };
                    }
                    TransferState::Failed(reason) => {
                        if file.kind == FileKind::Audio {
                            failure.get_or_insert_with(|| {
                                format!("{} answered {reason}", source.username)
                            });
                        } else {
                            // The Album Artwork is a nicety, not a reason to
                            // leave a source.
                            file.state = FileState::Failed;
                        }
                    }
                }
                progressed |= before != (file.state, file.bytes_transferred);
            }
            if failure.is_none() && !unlisted.is_empty() {
                let files: Vec<(&str, u64)> = unlisted
                    .iter()
                    .map(|index| {
                        let file = &acquisition.files[*index];
                        (file.remote_filename.as_str(), file.size)
                    })
                    .collect();
                match slskd.enqueue(&source.username, &files).await {
                    Ok(()) => {}
                    Err(SlskdError::Refused(reason)) => failure = Some(reason),
                    Err(error) => {
                        tracing::warn!(%error, acquisition = %acquisition.id, "queue slskd download failed");
                    }
                }
            }
            let now = now_ms();
            // Waiting in a queue (requested, queued locally or remotely,
            // initializing) is not a stall: the stall clock runs only while
            // a transfer is in progress, or while nothing of the source is
            // queued at all. A remote rejection still fails the source.
            if progressed || (queued_audio && !transferring) {
                acquisition.progress_at_ms = now;
            }
            let stalled = now - acquisition.progress_at_ms
                > i64::try_from(timing.stall_timeout.as_millis()).unwrap_or(i64::MAX);
            if failure.is_none() && stalled {
                if acquisition.pending_audio() {
                    failure = Some(format!(
                        "{} made no progress for {} seconds",
                        source.username,
                        timing.stall_timeout.as_secs()
                    ));
                } else {
                    give_up_artwork(acquisition);
                }
            }
            if let Some(reason) = failure {
                self.next_source(acquisition, &reason).await;
                return;
            }
        }
        let missing = acquisition.audio().any(|file| file.state.is_missing());
        if missing && !acquisition.pending_audio() {
            // The current source lacks the files still missing.
            let reason = format!("{} does not offer the missing files", source.username);
            self.next_source(acquisition, &reason).await;
        }
    }

    /// Gives up the current source: removes its unfinished transfers and
    /// takes the missing files from the next source that offers them.
    async fn next_source(&self, acquisition: &mut Acquisition, reason: &str) {
        let current = acquisition.source_index;
        if let (Some(slskd), Some(source)) = (self.slskd_client(), acquisition.current_source()) {
            let username = source.username.clone();
            for file in &mut acquisition.files {
                if file.source_index != current || !file.state.is_missing() {
                    continue;
                }
                if let Some(transfer_id) = file.transfer_id.take()
                    && let Err(error) = slskd.remove_download(&username, &transfer_id).await
                {
                    tracing::warn!(%error, "remove failed slskd download");
                }
                file.state = FileState::Queued;
                file.bytes_transferred = 0;
            }
        }
        // The cover image comes only from the first source's folder.
        give_up_artwork(acquisition);
        tracing::info!(acquisition = %acquisition.id, %reason, "trying the next Acquisition Source");
        let mut index = current + 1;
        while index < acquisition.sources.len() && !assign(acquisition, index) {
            index += 1;
        }
        acquisition.source_index = index;
        acquisition.progress_at_ms = now_ms();
        if index >= acquisition.sources.len() {
            let missing = acquisition
                .audio()
                .filter(|file| file.state.is_missing())
                .count();
            for file in &mut acquisition.files {
                if file.state.is_missing() {
                    file.state = FileState::Failed;
                    file.transfer_id = None;
                }
            }
            let total = acquisition.audio().count();
            acquisition.failure_reason = Some(format!(
                "No source of this version is left: {missing} of {total} files did not arrive (last: {reason})"
            ));
        }
    }

    /// Moves the acquisition on: downloading while an audio file is to
    /// arrive, importing while Managed Import is still to take a file, then
    /// review, or failed when a file never arrived.
    fn settle(&self, acquisition: &mut Acquisition) {
        if !acquisition.state.is_downloading() {
            return;
        }
        if acquisition.audio().any(|file| file.state.is_missing()) {
            acquisition.state = AcquisitionState::Downloading;
            return;
        }
        let importing = acquisition.files.iter().any(|file| {
            file.state == FileState::Downloaded
                || (file.kind == FileKind::Artwork && file.state.is_missing())
        });
        acquisition.state = if importing {
            AcquisitionState::Importing
        } else if acquisition
            .audio()
            .any(|file| file.state == FileState::Failed)
        {
            AcquisitionState::Failed
        } else {
            AcquisitionState::Review
        };
    }

    /// The completed download of a remote file, when it is in slskd's
    /// download directory.
    fn downloaded_copy(&self, remote: &str) -> Option<PathBuf> {
        let path = local_path(self.downloads_path()?, remote);
        path.is_file().then_some(path)
    }
}

/// Stops waiting for a cover image that has not arrived; its transfer, if
/// any, goes with the acquisition's other transfers.
fn give_up_artwork(acquisition: &mut Acquisition) {
    for file in &mut acquisition.files {
        if file.kind == FileKind::Artwork && file.state.is_missing() {
            file.state = FileState::Failed;
        }
    }
}

/// The folder's cover image: one named cover, folder or front, else the
/// first.
fn cover(artwork: &[RemoteFile]) -> Option<&RemoteFile> {
    let named = |remote: &&RemoteFile| {
        let name = file_name(&remote.filename).to_ascii_lowercase();
        let stem = name
            .rsplit_once('.')
            .map_or(name.as_str(), |(stem, _)| stem);
        matches!(stem, "cover" | "folder" | "front")
    };
    artwork.iter().find(named).or_else(|| artwork.first())
}

/// Takes the acquisition's missing audio files from source `index`; `false`
/// when it offers none of them.
fn assign(acquisition: &mut Acquisition, index: usize) -> bool {
    let source = acquisition.sources[index].clone();
    let audio_count = acquisition.audio().count();
    let mut assigned = false;
    let mut ordinal = 0;
    for file in &mut acquisition.files {
        if file.kind != FileKind::Audio {
            continue;
        }
        let position = ordinal;
        ordinal += 1;
        if !file.state.is_missing() {
            continue;
        }
        let by_name = source
            .files
            .iter()
            .find(|remote| file_name(&remote.filename).eq_ignore_ascii_case(&file.name));
        let by_position = (source.files.len() == audio_count)
            .then(|| source.files.get(position))
            .flatten();
        let Some(remote) = by_name.or(by_position) else {
            continue;
        };
        file.source_index = index;
        file.remote_filename = remote.filename.clone();
        file.size = remote.size;
        file.transfer_id = None;
        file.state = FileState::Queued;
        file.bytes_transferred = 0;
        assigned = true;
    }
    assigned
}

fn internal(error: impl std::fmt::Display) -> SoulseekError {
    SoulseekError::Internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloads_land_in_the_remote_folder_name() {
        let downloads = Path::new("/srv/downloads");
        assert_eq!(
            local_path(
                downloads,
                r"@@alice\Daft Punk\Discovery [24-48]\01 One.flac"
            ),
            Path::new("/srv/downloads/Discovery [24-48]/01 One.flac")
        );
        assert_eq!(
            local_path(downloads, "One.flac"),
            Path::new("/srv/downloads/One.flac")
        );
        assert_eq!(file_name(r"A\B\01 One.flac"), "01 One.flac");
    }
}
