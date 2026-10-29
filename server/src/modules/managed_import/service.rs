//! Managed Import jobs, batches, uploads and commits (managedimport/service.go,
//! import_progress.go, import_session.go).
//!
//! Go's request context becomes a [`CancellationToken`]: handlers cancel it
//! when the request goes away, and cleanup steps that Go runs "without
//! cancel" simply run.

use super::commit::CommitData;
use super::errors::{Error, Result, Sentinel, ValidationError};
use super::identity::album_identity_key;
use super::model::*;
use super::plan::{album_position_conflict, decode_import_plan};
use super::preview::album_preview_key;
use super::repository::Repository;
use super::storage::{PlacedFiles, Requirement, StagedUpload, Storage};
use super::types::{CommitIdentity, CommitJournal};
use super::{identity, replacement};
use crate::inspect::{self, MediaInspection, NormalizedMetadata};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio_util::sync::CancellationToken;

pub const IMPORT_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(15 * 60);
pub const IMPORT_CLEANUP_INTERVAL: Duration = Duration::from_secs(60);
const VALIDATION_CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);
/// The least advance between two persisted validation progress values.
const VALIDATION_PROGRESS_PERSIST_STEP_PERCENT: i64 = 5;

/// Where an Import Batch is in its life, as [`Service::batch_progress`]
/// reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchProgress {
    /// Files may still be added; not yet confirmed.
    Open,
    /// The listener confirmed it and its files are being committed.
    Confirming,
    /// Committed.
    Completed,
    /// Cancelled, or never existed.
    Gone,
}

struct ActiveUpload {
    cancel: CancellationToken,
    done: CancellationToken,
    last_activity: SystemTime,
    phase: String,
    transferred_bytes: i64,
    total_bytes: i64,
}

#[derive(Default)]
struct ActiveState {
    uploads: HashMap<String, ActiveUpload>,
    canceling: HashSet<String>,
}

type Active = Arc<Mutex<ActiveState>>;

fn record_activity(active: &Active, job_id: &str, count: i64) {
    let mut state = active.lock().unwrap();
    if let Some(upload) = state.uploads.get_mut(job_id) {
        upload.last_activity = SystemTime::now();
        upload.transferred_bytes += count;
    }
}

pub struct Service {
    pub(super) repo: Repository,
    pub(super) storage: Arc<Storage>,
    /// `managedImportCommitMu`.
    commit_lock: tokio::sync::Mutex<()>,
    /// `managedImportBatchConfirmationMu`.
    batch_lock: tokio::sync::Mutex<()>,
    upload_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    active: Active,
    /// `permanentTrackDeletionMu`.
    pub(super) deletion_lock: tokio::sync::Mutex<()>,
    pub(super) queue_events: Option<Arc<crate::modules::playback::QueueEventBroker>>,
    /// Runs after each durable commit or replacement phase; tests use it to
    /// stop there (`commitPhaseHook`, `replacementPhaseHook`).
    phase_hook: Option<Hook>,
    /// Runs after the database commit returns (`commitResultHook`).
    result_hook: Option<Hook>,
}

pub type Hook = Arc<dyn Fn(&str) -> Result<()> + Send + Sync>;

/// Holds one job's upload lock; the map entry goes with its last holder.
struct UploadGuard<'a> {
    service: &'a Service,
    job_id: String,
    guard: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl Drop for UploadGuard<'_> {
    fn drop(&mut self) {
        drop(self.guard.take());
        let mut locks = self.service.upload_locks.lock().unwrap();
        if let Some(lock) = locks.get(&self.job_id)
            && Arc::strong_count(lock) == 1
        {
            locks.remove(&self.job_id);
        }
    }
}

/// `safeOriginalFilename`.
pub fn safe_original_filename(value: &str) -> Result<String> {
    if value.contains(['\0', '\r', '\n']) {
        return Err(Error::wrap(
            Sentinel::InvalidUpload,
            "filename contains forbidden characters",
        ));
    }
    let value = value.trim();
    // filepath.Base("") is ".", so an empty name fails as a path too.
    if value.contains(['/', '\\']) || value.is_empty() {
        return Err(Error::wrap(
            Sentinel::InvalidUpload,
            "filename must not contain path segments",
        ));
    }
    if value == "." || value.len() > MAX_ORIGINAL_FILENAME_BYTES {
        return Err(Error::wrap(
            Sentinel::InvalidUpload,
            "filename is missing or too long",
        ));
    }
    Ok(value.to_owned())
}

/// `failureDetails`: the stored error code and reason of a failure.
pub fn failure_details(error: &Error) -> (String, String) {
    if let Some(validation) = error.as_validation() {
        return (validation.code.clone(), validation.reason.clone());
    }
    let (code, reason) = if error.is(Sentinel::UploadTooLarge) {
        (
            "upload_too_large",
            "file exceeds the configured per-file byte limit",
        )
    } else if error.is(Sentinel::BatchTooLarge) {
        (
            "batch_upload_too_large",
            "batch exceeds the configured byte limit",
        )
    } else if error.is(Sentinel::InvalidUpload) {
        ("invalid_upload", "upload is invalid")
    } else if error.is(Sentinel::InsufficientStorage) {
        (
            "insufficient_storage",
            "Managed Storage does not have enough capacity for this import and its safety reserve",
        )
    } else if error.is(Sentinel::UnsafeStoragePath) {
        (
            "unsafe_storage_path",
            "Managed Storage path failed containment checks",
        )
    } else if error.is(Sentinel::TrackNotFound) {
        ("track_not_found", "the Track to replace no longer exists")
    } else if error.is(Sentinel::NotManagedTrack) {
        ("not_managed_track", "only Managed Tracks can be replaced")
    } else if error.is(Sentinel::ReplacementConflict) {
        (
            "replacement_preview_changed",
            "the Track to replace changed during validation",
        )
    } else if error.is(Sentinel::Cancelled) {
        ("validation_cancelled", "file validation was canceled")
    } else {
        ("inspection_failed", "file validation failed")
    };
    (code.to_owned(), reason.to_owned())
}

fn validation_cancellation_error() -> Error {
    Error::from_inspection(inspect::errors::cancelled())
}

fn staged_file_changed(reason: &str, message: &str) -> Error {
    Error::validation(
        ValidationError::new("staged_file_changed", "file", reason),
        message,
    )
}

fn is_multi_disc(metadata: &NormalizedMetadata) -> bool {
    metadata.disc_position.number > 1 || metadata.disc_position.total > 1
}

/// `previewFromInspection` and `previewFileFromInspection`.
pub fn preview_from_inspection(
    job: &ImportJob,
    original_filename: &str,
    status: &str,
    revision: i64,
    inspection: &MediaInspection,
) -> Preview {
    let metadata = &inspection.metadata;
    let audio = &inspection.audio;
    Preview {
        job_id: job.id().to_owned(),
        status: status.to_owned(),
        revision,
        file: PreviewFile {
            album_key: identity::sha256_hex(&album_identity_key(metadata)),
            content_sha256: inspection.file_sha256.clone(),
            artwork_warning: inspection.album_artwork.warning.clone(),
            album_artist_warning: if metadata.album_artists_from_artists {
                inspect::ALBUM_ARTIST_FROM_ARTIST_WARNING.to_owned()
            } else {
                String::new()
            },
            original_filename: original_filename.to_owned(),
            title: metadata.title.clone(),
            title_key: identity::normalize_identity(&metadata.title),
            has_disc_number: metadata.has_disc_number,
            artists: Some(metadata.artists.clone()),
            album_artists: Some(metadata.album_artists.clone()),
            album: metadata.album.clone(),
            genres: Some(metadata.genres.clone()),
            track_no: metadata.track_position.number,
            track_total: metadata.track_position.total,
            disc_no: metadata.disc_position.number,
            disc_total: metadata.disc_position.total,
            year: metadata.year,
            duration_ms: audio.duration_ms,
            format: audio.format.clone(),
            container: audio.container.clone(),
            codec: audio.codec.clone(),
            sample_rate_hz: audio.sample_rate_hz,
            channel_count: audio.channel_count,
            bit_depth: audio.bit_depth,
            bitrate_kbps: audio.bitrate_kbps,
            artwork_media_type: inspection.album_artwork.mime_type.clone(),
            size_bytes: 0,
        },
        duplicate_classification: DUPLICATE_NONE.to_owned(),
        ..Preview::default()
    }
}

/// `batchFilePhase`.
fn batch_file_phase(file: &BatchFile, status: &str) -> &'static str {
    if file.state == BATCH_FILE_COMPLETED || !file.outcome.is_empty() {
        "completed"
    } else if status == BATCH_STATUS_CONFIRMING && file.selected {
        "committing"
    } else if file.state == BATCH_FILE_ACCEPTED {
        "ready"
    } else if file.state == BATCH_FILE_REJECTED || !file.error_code.is_empty() {
        "failed"
    } else {
        "queued"
    }
}

fn separate_album_identity_key(job: &ImportJob, metadata: &NormalizedMetadata) -> String {
    let edition = if job.batch_id.is_empty() {
        job.id()
    } else {
        &job.batch_id
    };
    format!(
        "{}\u{1f}managed-import-edition:{edition}",
        album_identity_key(metadata)
    )
}

impl Service {
    pub fn new(repo: Repository, storage: Arc<Storage>) -> Self {
        Self {
            repo,
            storage,
            commit_lock: tokio::sync::Mutex::new(()),
            batch_lock: tokio::sync::Mutex::new(()),
            upload_locks: Mutex::new(HashMap::new()),
            active: Arc::new(Mutex::new(ActiveState::default())),
            deletion_lock: tokio::sync::Mutex::new(()),
            queue_events: None,
            phase_hook: None,
            result_hook: None,
        }
    }

    pub fn with_queue_events(
        mut self,
        events: Arc<crate::modules::playback::QueueEventBroker>,
    ) -> Self {
        self.queue_events = Some(events);
        self
    }

    pub(super) async fn commit_lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.commit_lock.lock().await
    }

    pub(super) async fn batch_lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.batch_lock.lock().await
    }

    pub(super) fn after_result(&self) -> Result<()> {
        match &self.result_hook {
            Some(hook) => hook(""),
            None => Ok(()),
        }
    }

    pub(super) fn after_replacement_phase(&self, phase: &str) -> Result<()> {
        self.after_commit_phase(&format!("replacement:{phase}"))
    }

    #[cfg(test)]
    pub(super) fn with_hooks(mut self, phase: Option<Hook>, result: Option<Hook>) -> Self {
        self.phase_hook = phase;
        self.result_hook = result;
        self
    }

    fn after_commit_phase(&self, phase: &str) -> Result<()> {
        match &self.phase_hook {
            Some(hook) => hook(phase),
            None => Ok(()),
        }
    }

    async fn lock_upload(&self, job_id: &str) -> UploadGuard<'_> {
        let lock = self
            .upload_locks
            .lock()
            .unwrap()
            .entry(job_id.to_owned())
            .or_default()
            .clone();
        let guard = lock.lock_owned().await;
        UploadGuard {
            service: self,
            job_id: job_id.to_owned(),
            guard: Some(guard),
        }
    }

    async fn lock_uploads(&self, jobs: &[ImportJob]) -> Vec<UploadGuard<'_>> {
        let mut guards = Vec::with_capacity(jobs.len());
        for job in jobs {
            guards.push(self.lock_upload(job.id()).await);
        }
        guards
    }

    pub async fn create_job(&self, batch_id: &str, client_file_id: &str) -> Result<Job> {
        if batch_id.is_empty() {
            return self.repo.create_job(batch_id, client_file_id).await;
        }
        if uuid::Uuid::parse_str(client_file_id).is_err() {
            return Err(Error::wrap(
                Sentinel::InvalidUpload,
                "clientFileId must be a UUID",
            ));
        }
        let _batch = self.batch_lock.lock().await;
        self.repo.create_job(batch_id, client_file_id).await
    }

    pub async fn create_batch(&self) -> Result<Batch> {
        self.repo.create_batch(false).await
    }

    /// A batch the server fills itself, such as a Soulseek Acquisition's.
    /// No Playback Client holds it, so it is exempt from client-liveness
    /// and restart cleanup and lives until confirmed or cancelled (ADR 0036,
    /// an exception to ADR 0018). It is read, confirmed and cancelled
    /// through the same routes as any batch.
    pub async fn create_server_batch(&self) -> Result<Batch> {
        self.repo.create_batch(true).await
    }

    /// Where the batch is in its life, for a module that fills a batch
    /// itself.
    pub async fn batch_progress(&self, batch_id: &str) -> Result<BatchProgress> {
        match self.repo.get_batch_status(batch_id).await {
            Ok(status) if status == BATCH_STATUS_COMPLETED => Ok(BatchProgress::Completed),
            Ok(status) if status == BATCH_STATUS_CONFIRMING => Ok(BatchProgress::Confirming),
            Ok(_) => Ok(BatchProgress::Open),
            Err(error) if error.is(Sentinel::NotFound) => Ok(BatchProgress::Gone),
            Err(error) => Err(error),
        }
    }

    /// Cancels a batch unless it is already confirmed; a confirmed or gone
    /// batch is left as it is.
    pub async fn cancel_unconfirmed_batch(&self, batch_id: &str) -> Result<()> {
        match self.cancel_batch(batch_id).await {
            Err(error) if error.is(Sentinel::InvalidState) => Ok(()),
            other => other,
        }
    }

    pub async fn get_batch(&self, batch_id: &str) -> Result<Batch> {
        let mut batch = self.repo.get_batch(batch_id).await?;
        self.add_batch_progress(&mut batch);
        self.populate_album_previews(&mut batch).await?;
        Ok(batch)
    }

    fn add_batch_progress(&self, batch: &mut Batch) {
        let state = self.active.lock().unwrap();
        for file in &mut batch.files {
            file.phase = batch_file_phase(file, &batch.status).to_owned();
            if let Some(active) = state.uploads.get(&file.job_id)
                && file.state == BATCH_FILE_UNRESOLVED
            {
                file.phase = active.phase.clone();
                file.transferred_bytes = active.transferred_bytes;
                file.total_bytes = active.total_bytes;
            }
        }
    }

    /// Stores an uploaded cover for one Album of an uploading Batch
    /// (artwork-handlers.go UploadArtwork).
    pub async fn upload_artwork(
        &self,
        batch_id: &str,
        album_key: &str,
        data: &[u8],
    ) -> Result<ArtworkOption> {
        let artwork = inspect::artwork::validate_uploaded(data).map_err(Error::from_inspection)?;
        let _batch = self.batch_lock.lock().await;
        let batch = self.get_batch(batch_id).await?;
        if batch.status != BATCH_STATUS_UPLOADING {
            return Err(Error::sentinel(Sentinel::InvalidState));
        }
        if !batch.albums.iter().any(|album| album.key == album_key) {
            return Err(Error::sentinel(Sentinel::NotFound));
        }
        self.storage.preflight(Requirement {
            selected_bytes: 0,
            temporary_bytes: data.len() as i64,
        })?;
        let id = format!("upload-{album_key}");
        self.repo
            .save_artwork(batch_id, &id, "", album_key, &artwork)
            .await?;
        Ok(ArtworkOption {
            id,
            job_id: String::new(),
            media_type: artwork.mime_type,
            content_sha256: artwork.sha256,
        })
    }

    pub async fn get_artwork(
        &self,
        batch_id: &str,
        artwork_id: &str,
    ) -> Result<inspect::AlbumArtwork> {
        self.repo.load_artwork(batch_id, artwork_id).await
    }

    pub async fn list_history(&self) -> Result<HistoryList> {
        self.repo.list_history().await
    }

    pub async fn get_job(&self, job_id: &str) -> Result<Job> {
        Ok(self.repo.get_job(job_id).await?.job)
    }

    /// Liveness only; serialized against cancellation so a fresh heartbeat
    /// wins before cleanup rechecks eligibility.
    pub async fn heartbeat_batch(&self, batch_id: &str) -> Result<()> {
        let _batch = self.batch_lock.lock().await;
        self.repo.heartbeat_batch(batch_id).await
    }

    pub async fn cancel_batch(&self, batch_id: &str) -> Result<()> {
        match self.cancel_batch_before(batch_id, None).await {
            Err(error) if error.is(Sentinel::NotFound) => Ok(()),
            other => other,
        }
    }

    async fn cancel_batch_before(
        &self,
        batch_id: &str,
        updated_before: Option<(&str, SystemTime)>,
    ) -> Result<()> {
        let _batch = self.batch_lock.lock().await;
        let batch = self.repo.get_batch(batch_id).await?;
        if batch.status == BATCH_STATUS_COMPLETED {
            return Err(Error::sentinel(Sentinel::InvalidState));
        }
        let jobs = self.repo.list_batch_jobs(batch_id).await?;
        let quiesced = self
            .quiesce_uploads(&jobs, updated_before.map(|(_, time)| time))
            .await;
        let result = async {
            if !quiesced? {
                return Ok(());
            }
            let _uploads = self.lock_uploads(&jobs).await;
            if let Some((before, _)) = updated_before
                && !self
                    .repo
                    .is_batch_uncommitted_before(batch_id, before)
                    .await?
            {
                return Ok(());
            }
            let jobs = self.repo.list_batch_jobs(batch_id).await?;
            self.remove_uncommitted_staging(&jobs)?;
            self.repo.delete_batch(batch_id).await
        }
        .await;
        self.clear_canceling(&jobs);
        result
    }

    pub async fn cancel_job(&self, job_id: &str) -> Result<()> {
        match self.cancel_job_before(job_id, None).await {
            Err(error) if error.is(Sentinel::NotFound) => Ok(()),
            other => other,
        }
    }

    async fn cancel_job_before(
        &self,
        job_id: &str,
        updated_before: Option<(&str, SystemTime)>,
    ) -> Result<()> {
        let _batch = self.batch_lock.lock().await;
        let job = self.repo.get_job(job_id).await?;
        if !job.batch_id.is_empty()
            && self.repo.get_batch_status(&job.batch_id).await? == BATCH_STATUS_COMPLETED
        {
            return Err(Error::sentinel(Sentinel::InvalidState));
        }
        let jobs = [job];
        let quiesced = self
            .quiesce_uploads(&jobs, updated_before.map(|(_, time)| time))
            .await;
        let result = async {
            if !quiesced? {
                return Ok(());
            }
            let _upload = self.lock_upload(job_id).await;
            if let Some((before, _)) = updated_before
                && !self
                    .repo
                    .is_standalone_job_uncommitted_before(job_id, before)
                    .await?
            {
                return Ok(());
            }
            let job = self.repo.get_job(job_id).await?;
            if job.status() == STATUS_COMMITTED {
                return Err(Error::sentinel(Sentinel::InvalidState));
            }
            if self
                .repo
                .find_incomplete_replacement_journal(job_id)
                .await?
                .is_some()
            {
                return Err(Error::sentinel(Sentinel::InvalidState));
            }
            self.remove_uncommitted_staging(std::slice::from_ref(&job))?;
            self.repo.delete_job(job_id).await
        }
        .await;
        self.clear_canceling(&jobs);
        result
    }

    /// Cancels the jobs' running uploads and waits for them to stop; `false`
    /// when an upload was active after `updated_before`.
    async fn quiesce_uploads(
        &self,
        jobs: &[ImportJob],
        updated_before: Option<SystemTime>,
    ) -> Result<bool> {
        let waiting: Vec<CancellationToken> = {
            let mut state = self.active.lock().unwrap();
            if let Some(before) = updated_before
                && jobs.iter().any(|job| {
                    state
                        .uploads
                        .get(job.id())
                        .is_some_and(|active| active.last_activity > before)
                })
            {
                return Ok(false);
            }
            let mut waiting = Vec::new();
            for job in jobs {
                state.canceling.insert(job.id().to_owned());
                if let Some(active) = state.uploads.get(job.id()) {
                    active.cancel.cancel();
                    waiting.push(active.done.clone());
                }
            }
            waiting
        };
        for done in waiting {
            if tokio::time::timeout(VALIDATION_CLEANUP_TIMEOUT, done.cancelled())
                .await
                .is_err()
            {
                return Err(Error::new(
                    "wait for Managed Import upload cancellation: context deadline exceeded",
                ));
            }
        }
        Ok(true)
    }

    fn clear_canceling(&self, jobs: &[ImportJob]) {
        let mut state = self.active.lock().unwrap();
        for job in jobs {
            state.canceling.remove(job.id());
        }
    }

    /// Registers the job's upload and a token that cancels it with the
    /// request or a cancellation of the job.
    fn start_active_upload(&self, job_id: &str, request: &CancellationToken) -> CancellationToken {
        let cancel = request.child_token();
        let mut state = self.active.lock().unwrap();
        if state.canceling.contains(job_id) {
            cancel.cancel();
        }
        state.uploads.insert(
            job_id.to_owned(),
            ActiveUpload {
                cancel: cancel.clone(),
                done: CancellationToken::new(),
                last_activity: SystemTime::now(),
                phase: "uploading".to_owned(),
                transferred_bytes: 0,
                total_bytes: 0,
            },
        );
        cancel
    }

    fn finish_active_upload(&self, job_id: &str, cancel: &CancellationToken) {
        cancel.cancel();
        // Uploads of one job are serialized, so the entry is this upload's.
        if let Some(active) = self.active.lock().unwrap().uploads.remove(job_id) {
            active.done.cancel();
        }
    }

    fn set_upload_phase(&self, job_id: &str, phase: &str) {
        if let Some(active) = self.active.lock().unwrap().uploads.get_mut(job_id) {
            active.phase = phase.to_owned();
        }
    }

    fn remove_uncommitted_staging(&self, jobs: &[ImportJob]) -> Result<()> {
        let failures = jobs
            .iter()
            .filter(|job| job.status() != STATUS_COMMITTED && !job.staged_file_path.is_empty())
            .map(|job| {
                self.storage
                    .remove_staged(&job.staged_file_path)
                    .err()
                    .map(|error| {
                        error.context(format!(
                            "remove staging for Managed Import Job {:?}",
                            job.id()
                        ))
                    })
            })
            .collect::<Vec<_>>();
        match Error::join(failures) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub async fn cleanup_inactive(&self, now: SystemTime) -> Result<()> {
        let cutoff = now.checked_sub(IMPORT_INACTIVITY_TIMEOUT).unwrap_or(now);
        self.cleanup_uncommitted(Some(cutoff)).await
    }

    pub async fn cleanup_restart(&self) -> Result<()> {
        self.recover_commits().await?;
        let keep: HashSet<String> = self
            .repo
            .list_server_owned_staged_paths()
            .await?
            .into_iter()
            .collect();
        let staging = self
            .storage
            .remove_all_staged_except(&keep)
            .err()
            .map(|error| error.context("remove Managed Import restart staging"));
        let reconcile = self.cleanup_uncommitted(None).await.err();
        match Error::join([staging, reconcile]) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub async fn recover_commits(&self) -> Result<()> {
        let _commit = self.commit_lock.lock().await;
        let journals = self.repo.list_incomplete_commit_journals().await?;
        let mut failures = Vec::new();
        for journal in journals {
            failures.push(self.recover_commit(&journal).await.err().map(|error| {
                error.context(format!("recover Managed Import commit {:?}", journal.id))
            }));
        }
        match Error::join(failures) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn recover_commit(&self, journal: &CommitJournal) -> Result<()> {
        let placement = match self.storage.placement_from_journal(journal) {
            Ok(placement) => placement,
            Err(error) => {
                let reason = self
                    .repo
                    .record_commit_recovery_reason(&journal.id, "unsafe journaled storage path")
                    .await
                    .err();
                return Err(Error::join([Some(error), reason]).unwrap());
            }
        };
        match journal.phase.as_str() {
            COMMIT_PHASE_PREPARED | COMMIT_PHASE_PLACED | COMMIT_PHASE_VERIFIED => {
                self.rollback_uncommitted_journal(journal, &placement).await
            }
            COMMIT_PHASE_DATABASE_COMMITTED | COMMIT_PHASE_CLEANED => {
                self.complete_pending_journal(journal, &placement).await
            }
            phase => Err(Error::new(format!(
                "unsupported Managed Import commit phase {phase:?}"
            ))),
        }
    }

    async fn record_reason_with(&self, journal_id: &str, reason: String, error: Error) -> Error {
        let recorded = self
            .repo
            .record_commit_recovery_reason(journal_id, &reason)
            .await
            .err();
        Error::join([Some(error), recorded]).unwrap()
    }

    async fn rollback_uncommitted_journal(
        &self,
        journal: &CommitJournal,
        placement: &PlacedFiles,
    ) -> Result<()> {
        let reason = format!("restart rolled back commit from {} phase", journal.phase);
        match std::fs::metadata(&journal.audio_file_path) {
            Ok(_) => {
                if let Err(error) = self.storage.rollback(placement) {
                    return Err(self
                        .record_reason_with(
                            &journal.id,
                            format!("{reason}; canonical rollback failed"),
                            error,
                        )
                        .await);
                }
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                let error = Error::new(format!("stat {}: {error}", journal.audio_file_path));
                return Err(self
                    .record_reason_with(
                        &journal.id,
                        format!("{reason}; canonical state unreadable"),
                        error,
                    )
                    .await);
            }
            Err(_) => {
                if let Err(error) = self.storage.cleanup_prepared_placement(placement) {
                    return Err(self
                        .record_reason_with(
                            &journal.id,
                            format!("{reason}; prepared placement cleanup failed"),
                            error,
                        )
                        .await);
                }
            }
        }
        self.repo
            .rollback_commit_journal(&journal.id, &reason)
            .await
    }

    async fn complete_pending_journal(
        &self,
        journal: &CommitJournal,
        placement: &PlacedFiles,
    ) -> Result<()> {
        if let Err(verification) =
            self.storage
                .verify_placement(placement, &journal.audio_sha256, &journal.artwork_sha256)
        {
            let reason = format!(
                "restart rolled back commit because canonical verification failed: {verification}"
            );
            self.repo.rollback_pending_commit(journal, &reason).await?;
            if let Err(error) = self.storage.rollback(placement) {
                return Err(self
                    .record_reason_with(
                        &journal.id,
                        format!("{reason}; filesystem rollback failed"),
                        error,
                    )
                    .await);
            }
            return self
                .repo
                .rollback_commit_journal(&journal.id, &reason)
                .await;
        }
        if journal.phase == COMMIT_PHASE_DATABASE_COMMITTED {
            self.storage.cleanup_placement(placement)?;
            self.repo
                .update_commit_phase(&journal.id, COMMIT_PHASE_CLEANED)
                .await?;
        }
        let job = self.repo.get_job(&journal.job_id).await?;
        self.repo
            .finalize_commit(&job, &journal.track_id, &journal.id)
            .await?;
        Ok(())
    }

    async fn cleanup_uncommitted(&self, updated_before: Option<SystemTime>) -> Result<()> {
        let before = updated_before.map(crate::gojson::sqlite_time);
        let batch_ids = self
            .repo
            .list_uncommitted_batch_ids(before.as_deref())
            .await
            .map_err(|error| error.context("list uncommitted Managed Import Batches"))?;
        let job_ids = self
            .repo
            .list_uncommitted_standalone_job_ids(before.as_deref())
            .await
            .map_err(|error| error.context("list uncommitted Managed Import Jobs"))?;
        let bound = before.as_deref().zip(updated_before);
        let mut failures = Vec::new();
        for batch_id in batch_ids {
            match self.cancel_batch_before(&batch_id, bound).await {
                Err(error) if !error.is(Sentinel::NotFound) => {
                    failures.push(Some(
                        error.context(format!("cleanup Managed Import Batch {batch_id:?}")),
                    ));
                }
                _ => {}
            }
        }
        for job_id in job_ids {
            match self.cancel_job_before(&job_id, bound).await {
                Err(error) if !error.is(Sentinel::NotFound) => {
                    failures.push(Some(
                        error.context(format!("cleanup Managed Import Job {job_id:?}")),
                    ));
                }
                _ => {}
            }
        }
        match Error::join(failures) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Streams an upload into staging, validates it through the Strict
    /// Import Profile and stores its Import Preview.
    pub async fn upload<S>(
        &self,
        request: &CancellationToken,
        job_id: &str,
        original_filename: &str,
        body: S,
        content_length: i64,
    ) -> Result<Preview>
    where
        S: Stream<Item = Result<Bytes>> + Unpin + Send,
    {
        let _upload = self.lock_upload(job_id).await;
        let cancel = self.start_active_upload(job_id, request);
        let result = self
            .upload_locked(&cancel, job_id, original_filename, body, content_length)
            .await;
        self.finish_active_upload(job_id, &cancel);
        result
    }

    /// Feeds a file the server already has, such as a Soulseek download,
    /// into the job: the same staging, SHA-256, Strict Import Profile and
    /// Import Preview as [`Service::upload`]; only the byte source differs.
    pub async fn upload_from_path(
        &self,
        request: &CancellationToken,
        job_id: &str,
        original_filename: &str,
        path: &std::path::Path,
    ) -> Result<Preview> {
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|error| Error::new(format!("open {}: {error}", path.display())))?;
        let length = file
            .metadata()
            .await
            .map_err(|error| Error::new(format!("stat {}: {error}", path.display())))?
            .len();
        let body = tokio_util::io::ReaderStream::new(file).map(|chunk| {
            chunk.map_err(|error| {
                Error::wrap(
                    Sentinel::UploadInterrupted,
                    format!("read server file: {error}"),
                )
            })
        });
        self.upload(
            request,
            job_id,
            original_filename,
            Box::pin(body),
            i64::try_from(length).unwrap_or(i64::MAX),
        )
        .await
    }

    async fn upload_locked<S>(
        &self,
        cancel: &CancellationToken,
        job_id: &str,
        original_filename: &str,
        body: S,
        content_length: i64,
    ) -> Result<Preview>
    where
        S: Stream<Item = Result<Bytes>> + Unpin + Send,
    {
        if cancel.is_cancelled() {
            return Err(Error::sentinel(Sentinel::Cancelled));
        }
        if let Some(active) = self.active.lock().unwrap().uploads.get_mut(job_id) {
            active.total_bytes = content_length.max(0);
        }
        let job = self.get_uploading_job(job_id).await?;
        let original_filename = match safe_original_filename(original_filename) {
            Ok(name) => name,
            Err(error) => {
                let rejected = self.mark_rejected(&job, "", &error).await.err();
                return Err(Error::join([Some(error), rejected]).unwrap());
            }
        };
        if let Err(error) = self.reserve_batch_upload(&job, content_length).await {
            return Err(self
                .handle_upload_failure(cancel, &job, &original_filename, "", error)
                .await);
        }
        let active = self.active.clone();
        let activity_id = job_id.to_owned();
        let token = cancel.clone();
        let body = body.map(move |chunk| {
            if token.is_cancelled() {
                return Err(Error::sentinel(Sentinel::Cancelled));
            }
            if let Ok(chunk) = &chunk {
                record_activity(&active, &activity_id, chunk.len() as i64);
            }
            chunk
        });
        let reserved = std::sync::atomic::AtomicI64::new(0);
        let reserving = !job.batch_id.is_empty() && content_length < 0;
        let batch_limit = self.batch_limit();
        let body = body.then(|chunk| {
            let reserved = &reserved;
            let job_id = job.id();
            async move {
                let chunk = chunk?;
                if reserving && !chunk.is_empty() {
                    let total = reserved
                        .fetch_add(chunk.len() as i64, std::sync::atomic::Ordering::Relaxed)
                        + chunk.len() as i64;
                    self.repo
                        .reserve_batch_upload(job_id, total, batch_limit)
                        .await?;
                }
                Ok(chunk)
            }
        });
        let staged = match self
            .storage
            .stage_upload(Box::pin(body), content_length)
            .await
        {
            Ok(staged) => staged,
            Err(error) => {
                return Err(self
                    .handle_upload_failure(cancel, &job, &original_filename, "", error)
                    .await);
            }
        };
        if !job.batch_id.is_empty()
            && let Err(error) = self
                .repo
                .reserve_batch_upload(job.id(), staged.size, batch_limit)
                .await
        {
            return Err(self
                .handle_upload_failure(cancel, &job, &original_filename, &staged.path, error)
                .await);
        }
        self.set_upload_phase(job.id(), "validating");
        let inspection = match self.validate_staged_upload(cancel, &job, &staged).await {
            Ok(inspection) => inspection,
            Err(error) => {
                return Err(self
                    .handle_upload_failure(cancel, &job, &original_filename, &staged.path, error)
                    .await);
            }
        };
        self.persist_preview(cancel, &job, &original_filename, &staged, inspection)
            .await
    }

    fn batch_limit(&self) -> i64 {
        self.storage.batch_limit()
    }

    /// Runs the Strict Import Profile on a blocking thread; with a job the
    /// progress persists in steps.
    async fn inspect(
        &self,
        path: &str,
        cancel: &CancellationToken,
        progress_job: Option<&str>,
    ) -> Result<MediaInspection> {
        let path = std::path::PathBuf::from(path);
        let token = cancel.clone();
        let handle = tokio::runtime::Handle::current();
        let repo = self.repo.clone();
        let active = self.active.clone();
        let progress_job = progress_job.map(str::to_owned);
        let side: Arc<Mutex<Option<Error>>> = Arc::new(Mutex::new(None));
        let side_error = side.clone();
        let inspected = tokio::task::spawn_blocking(move || {
            let is_cancelled = || token.is_cancelled();
            let mut last_persisted = -1i64;
            let mut report = |progress: inspect::Progress| -> inspect::errors::Result<()> {
                let Some(job_id) = &progress_job else {
                    return Ok(());
                };
                record_activity(&active, job_id, 0);
                if progress.percent <= last_persisted {
                    return Ok(());
                }
                let is_milestone = progress.percent >= 100
                    || progress.percent - last_persisted
                        >= VALIDATION_PROGRESS_PERSIST_STEP_PERCENT;
                if last_persisted >= 0 && !is_milestone {
                    return Ok(());
                }
                if let Err(error) =
                    handle.block_on(repo.update_validation_progress(job_id, progress.percent))
                {
                    let message = error.to_string();
                    *side_error.lock().unwrap() = Some(error);
                    return Err(inspect::errors::other(message));
                }
                last_persisted = progress.percent;
                Ok(())
            };
            let mut ctx = inspect::Context::new(&is_cancelled, Some(&mut report));
            inspect::inspect(&path, &mut ctx)
        })
        .await
        .map_err(|error| Error::new(format!("inspect Managed Import file: {error}")))?;
        match inspected {
            Ok(inspection) => Ok(inspection),
            Err(failure) => match side.lock().unwrap().take() {
                Some(error) => Err(error),
                None => Err(Error::from_inspection(failure)),
            },
        }
    }

    pub(super) async fn inspect_staged_job(
        &self,
        cancel: &CancellationToken,
        job: &ImportJob,
    ) -> Result<MediaInspection> {
        let inspection = self.inspect(&job.staged_file_path, cancel, None).await?;
        self.apply_import_plan(job, inspection).await
    }

    async fn handle_upload_failure(
        &self,
        cancel: &CancellationToken,
        job: &ImportJob,
        original_filename: &str,
        staged_path: &str,
        error: Error,
    ) -> Error {
        let retryable = !job.batch_id.is_empty()
            && (error.is(Sentinel::UploadInterrupted)
                || cancel.is_cancelled()
                || error.is(Sentinel::Cancelled));
        if retryable {
            return self
                .mark_upload_interrupted(job, original_filename, staged_path, error)
                .await;
        }
        if staged_path.is_empty() {
            let rejected = self
                .mark_rejected(job, original_filename, &error)
                .await
                .err();
            return Error::join([Some(error), rejected]).unwrap();
        }
        self.fail_upload(job, original_filename, staged_path, error)
            .await
    }

    async fn reserve_batch_upload(&self, job: &ImportJob, content_length: i64) -> Result<()> {
        if job.batch_id.is_empty() {
            return Ok(());
        }
        let size = self.storage.upload_reservation_size(content_length)?;
        self.repo
            .reserve_batch_upload(job.id(), size, self.batch_limit())
            .await
    }

    async fn get_uploading_job(&self, job_id: &str) -> Result<ImportJob> {
        let job = self.repo.get_job(job_id).await?;
        if job.status() != STATUS_UPLOADING {
            return Err(Error::sentinel(Sentinel::InvalidState));
        }
        if !job.batch_id.is_empty()
            && self.repo.get_batch_status(&job.batch_id).await? != BATCH_STATUS_UPLOADING
        {
            return Err(Error::sentinel(Sentinel::InvalidState));
        }
        Ok(job)
    }

    async fn validate_staged_upload(
        &self,
        cancel: &CancellationToken,
        job: &ImportJob,
        staged: &StagedUpload,
    ) -> Result<MediaInspection> {
        let inspection = match self.inspect(&staged.path, cancel, Some(job.id())).await {
            Ok(inspection) => inspection,
            Err(_) if cancel.is_cancelled() => return Err(validation_cancellation_error()),
            Err(error) => return Err(error),
        };
        if inspection.file_sha256 != staged.sha256 {
            return Err(staged_file_changed(
                "staged file differs from uploaded bytes",
                "staged file hash differs from uploaded bytes",
            ));
        }
        if job.batch_id.is_empty()
            && let Err(error) = self
                .validate_album_positions_excluding(
                    cancel,
                    job.id(),
                    &inspection.metadata,
                    &job.replace_track_id,
                )
                .await
        {
            return Err(if cancel.is_cancelled() {
                validation_cancellation_error()
            } else {
                error
            });
        }
        Ok(inspection)
    }

    async fn persist_preview(
        &self,
        cancel: &CancellationToken,
        job: &ImportJob,
        original_filename: &str,
        staged: &StagedUpload,
        inspection: MediaInspection,
    ) -> Result<Preview> {
        let fail = |error: Error| {
            self.handle_upload_failure(cancel, job, original_filename, &staged.path, error)
        };
        let (classification, candidates) =
            match self.repo.classify_duplicate(&inspection.file_sha256).await {
                Ok(classified) => classified,
                Err(error) => return Err(fail(error).await),
            };
        let status = if classification == DUPLICATE_EXACT {
            STATUS_FAILED
        } else {
            STATUS_AWAITING_CONFIRMATION
        };
        let mut preview = preview_from_inspection(
            job,
            original_filename,
            status,
            job.revision() + 1,
            &inspection,
        );
        preview.file.size_bytes = staged.size;
        preview.duplicate_classification = classification.to_owned();
        preview.duplicate_candidates = Some(candidates.clone());
        preview.matching_tracks = match self
            .repo
            .position_candidates(&inspection.metadata, &job.replace_track_id)
            .await
        {
            Ok(candidates) => Some(candidates),
            Err(error) => return Err(fail(error).await),
        };
        if !job.replace_track_id.is_empty() && classification != DUPLICATE_EXACT {
            let state = match self
                .build_replacement_state(cancel, job, &inspection, &staged.path)
                .await
            {
                Ok(state) => state,
                Err(error) => return Err(fail(error).await),
            };
            preview.duplicate_classification = DUPLICATE_NONE.to_owned();
            preview.duplicate_candidates = None;
            replacement::attach(&mut preview, &state);
        }
        if !job.batch_id.is_empty()
            && !inspection.album_artwork.sha256.is_empty()
            && let Err(error) = self
                .repo
                .save_artwork(
                    &job.batch_id,
                    job.id(),
                    job.id(),
                    &album_preview_key(&preview.file),
                    &inspection.album_artwork,
                )
                .await
        {
            return Err(fail(error).await);
        }
        let preview_json = crate::gojson::to_string(&preview);
        if classification == DUPLICATE_EXACT {
            if let Err(error) = self
                .repo
                .mark_exact_duplicate(
                    job.id(),
                    original_filename,
                    &staged.path,
                    &staged.sha256,
                    &preview_json,
                    &candidates[0].track_id,
                    staged.size,
                )
                .await
            {
                return Err(fail(error).await);
            }
            self.storage.remove_staged(&staged.path)?;
            self.repo.clear_staged_file(job.id()).await?;
            return Ok(preview);
        }
        if let Err(error) = self.preflight_commit(staged.size, &inspection) {
            return Err(fail(error).await);
        }
        match self
            .repo
            .mark_preview(
                job.id(),
                original_filename,
                &staged.path,
                &staged.sha256,
                &preview_json,
                staged.size,
                self.batch_limit(),
            )
            .await
        {
            Ok(marked) => {
                preview.status = marked.job.status;
                preview.revision = marked.job.revision;
                Ok(preview)
            }
            Err(error) => {
                self.recover_preview_failure(cancel, job, original_filename, &staged.path, error)
                    .await
            }
        }
    }

    async fn recover_preview_failure(
        &self,
        cancel: &CancellationToken,
        original: &ImportJob,
        original_filename: &str,
        staged_path: &str,
        error: Error,
    ) -> Result<Preview> {
        if error.is(Sentinel::BatchTooLarge) {
            return Err(self
                .fail_upload(original, original_filename, staged_path, error)
                .await);
        }
        let current = self.repo.get_job(original.id()).await;
        if let Ok(job) = &current
            && job.status() == STATUS_AWAITING_CONFIRMATION
        {
            return serde_json::from_str(&job.preview_json)
                .map_err(|error| Error::new(format!("decode recovered Import Preview: {error}")));
        }
        let error = if cancel.is_cancelled() {
            validation_cancellation_error()
        } else {
            error
        };
        let error = Error::join([Some(error), current.err()]).unwrap();
        Err(self
            .handle_upload_failure(cancel, original, original_filename, staged_path, error)
            .await)
    }

    async fn validate_album_positions(
        &self,
        cancel: &CancellationToken,
        job_id: &str,
        metadata: &NormalizedMetadata,
    ) -> Result<()> {
        self.validate_album_positions_excluding(cancel, job_id, metadata, "")
            .await
    }

    /// Ignores one existing Track, which a Track Replacement overwrites.
    pub(super) async fn validate_album_positions_excluding(
        &self,
        cancel: &CancellationToken,
        job_id: &str,
        metadata: &NormalizedMetadata,
        excluded_track_id: &str,
    ) -> Result<()> {
        if metadata.has_disc_number
            || !self
                .requires_disc_number(cancel, job_id, metadata, excluded_track_id)
                .await?
        {
            return self
                .validate_existing_album_totals(metadata, excluded_track_id)
                .await;
        }
        let reason = "DISCNUMBER is required for a known multi-disc Album";
        Err(Error::validation(
            ValidationError::new("invalid_metadata", "DISCNUMBER", reason),
            reason,
        ))
    }

    async fn requires_disc_number(
        &self,
        cancel: &CancellationToken,
        job_id: &str,
        metadata: &NormalizedMetadata,
        excluded_track_id: &str,
    ) -> Result<bool> {
        if self
            .repo
            .album_requires_disc_number(metadata, excluded_track_id)
            .await?
        {
            return Ok(true);
        }
        let album_key = album_identity_key(metadata);
        for path in self.repo.awaiting_preview_paths(job_id).await? {
            let sibling = self
                .inspect(&path, cancel, None)
                .await
                .map_err(|error| error.context("inspect awaiting sibling file"))?;
            if album_identity_key(&sibling.metadata) == album_key
                && is_multi_disc(&sibling.metadata)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn validate_existing_album_totals(
        &self,
        metadata: &NormalizedMetadata,
        excluded_track_id: &str,
    ) -> Result<()> {
        let field = self
            .repo
            .album_position_total_conflict(metadata, excluded_track_id)
            .await?;
        if field.is_empty() {
            return Ok(());
        }
        let reason = "position total conflicts with the existing Album";
        Err(Error::validation(
            ValidationError::new("invalid_metadata", field, reason),
            reason,
        ))
    }

    pub async fn confirm_with_decision(
        &self,
        cancel: &CancellationToken,
        job_id: &str,
        revision: i64,
        decision: &str,
    ) -> Result<ImportResult> {
        let _batch = self.batch_lock.lock().await;
        let job = self.repo.get_job(job_id).await?;
        if !job.batch_id.is_empty() {
            return Err(Error::sentinel(Sentinel::InvalidState));
        }
        if !job.replace_track_id.is_empty() {
            return Err(Error::sentinel(Sentinel::ReplacementRequired));
        }
        let separate = decision == DUPLICATE_ACTION_IMPORT_SEPARATELY;
        self.confirm_job_with_edition(cancel, &job, revision, decision, separate)
            .await
    }

    async fn confirm_job_with_edition(
        &self,
        cancel: &CancellationToken,
        job: &ImportJob,
        revision: i64,
        decision: &str,
        separate: bool,
    ) -> Result<ImportResult> {
        let _commit = self.commit_lock.lock().await;
        let mut job = self.repo.get_job(job.id()).await?;
        if let Some(pending) = self
            .repo
            .find_incomplete_replacement_journal(job.id())
            .await?
        {
            self.recover_replacement(&pending).await?;
            job = self.repo.get_job(job.id()).await?;
        }
        if let Some(journal) = self.repo.find_incomplete_commit_journal(job.id()).await? {
            self.recover_commit(&journal).await?;
            job = self.repo.get_job(job.id()).await?;
        }
        if job.status() == STATUS_COMMITTED {
            if revision != job.revision() - 1 {
                return Err(Error::sentinel(Sentinel::RevisionConflict));
            }
            return Ok(ImportResult {
                job_id: job.id().to_owned(),
                status: job.job.status.clone(),
                revision: job.revision(),
                track_id: job.job.track_id.clone(),
            });
        }
        if job.status() == STATUS_FAILED && job.error_code() == ERROR_CODE_EXACT_DUPLICATE {
            if revision != job.revision() {
                return Err(Error::sentinel(Sentinel::RevisionConflict));
            }
            if !job.staged_file_path.is_empty() {
                return Err(self.finish_exact_duplicate_cleanup(&job).await);
            }
            return Err(Error::sentinel(Sentinel::ExactDuplicate));
        }
        if job.status() != STATUS_AWAITING_CONFIRMATION {
            return Err(Error::sentinel(Sentinel::InvalidState));
        }
        if revision != job.revision() {
            return Err(Error::sentinel(Sentinel::RevisionConflict));
        }
        self.confirm_awaiting_job(cancel, job, decision, separate)
            .await
    }

    async fn confirm_awaiting_job(
        &self,
        cancel: &CancellationToken,
        mut job: ImportJob,
        decision: &str,
        separate: bool,
    ) -> Result<ImportResult> {
        let staged_bytes = self.storage.staged_file_size(&job.staged_file_path)?;
        let mut inspection = self.inspect_staged_job(cancel, &job).await?;
        self.validate_album_positions(cancel, job.id(), &inspection.metadata)
            .await?;
        if inspection.file_sha256 != job.content_sha256 {
            return Err(staged_file_changed(
                "staged file changed after Import Preview",
                "staged file hash changed after Import Preview",
            ));
        }
        let (classification, _) = self
            .repo
            .classify_duplicate(&inspection.file_sha256)
            .await?;
        if classification == DUPLICATE_EXACT {
            return Err(self.reject_exact_duplicate(&job).await);
        }
        if separate {
            inspection.metadata.album_identity_key =
                separate_album_identity_key(&job, &inspection.metadata);
        }
        let candidates = self
            .repo
            .position_candidates(&inspection.metadata, "")
            .await?;
        if !candidates.is_empty() {
            if candidates.len() != 1
                || identity::normalize_identity(&candidates[0].title)
                    != identity::normalize_identity(&inspection.metadata.title)
            {
                return Err(album_position_conflict());
            }
            if decision != DUPLICATE_ACTION_REPLACE_EXISTING {
                return Err(Error::wrap(
                    Sentinel::InvalidUpload,
                    "confirm Track Replacement or skip this file",
                ));
            }
            if !job.replace_track_id.is_empty() && job.replace_track_id != candidates[0].track_id {
                return Err(Error::sentinel(Sentinel::RevisionConflict));
            }
            let plan = decode_import_plan(&job)?;
            if plan.target_revision != 0 && plan.target_revision != candidates[0].revision {
                return Err(Error::sentinel(Sentinel::RevisionConflict));
            }
            job.replace_track_id = candidates[0].track_id.clone();
            self.repo
                .set_job_replace_target(job.id(), &job.replace_track_id)
                .await?;
            let state = self
                .build_replacement_state(cancel, &job, &inspection, &job.staged_file_path)
                .await?;
            let result = self.commit_replacement(&job, state).await?;
            return Ok(ImportResult {
                job_id: result.job_id,
                status: result.status,
                revision: result.revision,
                track_id: result.track_id,
            });
        }
        if !decision.is_empty() && decision != DUPLICATE_ACTION_IMPORT_SEPARATELY {
            return Err(Error::sentinel(Sentinel::RevisionConflict));
        }
        self.preflight_commit(staged_bytes, &inspection)?;
        let album_key = if separate {
            separate_album_identity_key(&job, &inspection.metadata)
        } else {
            album_identity_key(&inspection.metadata)
        };
        self.commit(&job, &inspection, &album_key).await
    }

    pub(super) async fn reject_exact_duplicate(&self, job: &ImportJob) -> Error {
        if let Err(error) = self.repo.mark_confirmation_exact_duplicate(job.id()).await {
            return Error::join([Some(Error::sentinel(Sentinel::ExactDuplicate)), Some(error)])
                .unwrap();
        }
        self.finish_exact_duplicate_cleanup(job).await
    }

    pub(super) async fn finish_exact_duplicate_cleanup(&self, job: &ImportJob) -> Error {
        let duplicate = Error::sentinel(Sentinel::ExactDuplicate);
        if let Err(error) = self.storage.remove_staged(&job.staged_file_path) {
            return Error::join([Some(duplicate), Some(error)]).unwrap();
        }
        if let Err(error) = self.repo.clear_staged_file(job.id()).await {
            return Error::join([Some(duplicate), Some(error)]).unwrap();
        }
        duplicate
    }

    pub async fn confirm_batch(
        &self,
        cancel: &CancellationToken,
        batch_id: &str,
        confirmation: &BatchConfirmation,
    ) -> Result<Batch> {
        let _batch = self.batch_lock.lock().await;
        let mut selected = Vec::new();
        for id in confirmation
            .selected_file_ids
            .as_deref()
            .unwrap_or_default()
        {
            if id.is_empty() || selected.contains(id) {
                return Err(Error::wrap(
                    Sentinel::InvalidUpload,
                    "selected file IDs must be unique and non-empty",
                ));
            }
            selected.push(id.clone());
        }
        let batch = self.repo.get_batch(batch_id).await?;
        if batch.status == BATCH_STATUS_COMPLETED {
            if confirmation.revision != batch.revision - 2 {
                return Err(Error::sentinel(Sentinel::RevisionConflict));
            }
            return Ok(batch);
        }
        let jobs = self.repo.list_batch_jobs(batch_id).await?;
        if batch.status == BATCH_STATUS_UPLOADING {
            if confirmation.revision != batch.revision {
                return Err(Error::sentinel(Sentinel::RevisionConflict));
            }
            self.prepare_batch_plans(&batch, &jobs, &mut selected, confirmation)
                .await?;
        }
        self.start_or_resume_batch(&batch, confirmation.revision, &selected)
            .await?;
        let jobs = self.repo.list_batch_jobs(batch_id).await?;
        self.confirm_batch_jobs(cancel, &jobs).await?;
        self.complete_batch(batch_id).await
    }

    async fn start_or_resume_batch(
        &self,
        batch: &Batch,
        revision: i64,
        selected: &[String],
    ) -> Result<()> {
        if batch.status == BATCH_STATUS_CONFIRMING {
            if revision != batch.revision - 1 {
                return Err(Error::sentinel(Sentinel::RevisionConflict));
            }
            return Ok(());
        }
        if batch.status != BATCH_STATUS_UPLOADING {
            return Err(Error::sentinel(Sentinel::InvalidState));
        }
        self.repo
            .start_batch_confirmation(&batch.id, revision, selected)
            .await
    }

    async fn confirm_batch_jobs(
        &self,
        cancel: &CancellationToken,
        jobs: &[ImportJob],
    ) -> Result<()> {
        for job in jobs {
            if !job.outcome.is_empty() {
                continue;
            }
            if !job.selected {
                let (mut outcome, mut code, mut reason) = (
                    OUTCOME_NOT_ATTEMPTED,
                    job.error_code().to_owned(),
                    job.error_reason.clone(),
                );
                let plan = decode_import_plan(job)?;
                if plan.skip_exact
                    && !job.content_sha256.is_empty()
                    && self.repo.content_hash_exists(&job.content_sha256).await?
                {
                    outcome = OUTCOME_REJECTED;
                    code = ERROR_CODE_EXACT_DUPLICATE.to_owned();
                    reason = "identical file already exists in the library".to_owned();
                }
                self.finish_uncommitted_batch_file(cancel, job, outcome, &code, &reason)
                    .await?;
                continue;
            }
            let plan = decode_import_plan(job)?;
            if let Err(error) = self
                .confirm_job_with_edition(cancel, job, job.revision(), &plan.action, false)
                .await
            {
                self.handle_batch_confirmation_error(cancel, job, error)
                    .await?;
            }
        }
        Ok(())
    }

    async fn handle_batch_confirmation_error(
        &self,
        cancel: &CancellationToken,
        job: &ImportJob,
        error: Error,
    ) -> Result<()> {
        if error.is(Sentinel::ExactDuplicate) {
            let persisted = self.repo.get_job(job.id()).await;
            return match persisted {
                Err(get) => Err(Error::join([Some(error), Some(get)]).unwrap()),
                Ok(persisted)
                    if persisted.outcome == OUTCOME_REJECTED
                        && persisted.error_code() == ERROR_CODE_EXACT_DUPLICATE =>
                {
                    Ok(())
                }
                Ok(_) => Err(error),
            };
        }
        if cancel.is_cancelled() {
            return Err(
                Error::join([Some(error), Some(Error::sentinel(Sentinel::Cancelled))]).unwrap(),
            );
        }
        let persisted = match self.repo.get_job(job.id()).await {
            Ok(persisted) => persisted,
            Err(get) => return Err(Error::join([Some(error), Some(get)]).unwrap()),
        };
        if persisted.status() == STATUS_COMMITTED {
            return Ok(());
        }
        match self.repo.find_incomplete_commit_journal(job.id()).await {
            Err(journal) => return Err(Error::join([Some(error), Some(journal)]).unwrap()),
            Ok(Some(_)) => return Err(error),
            Ok(None) => {}
        }
        match self
            .repo
            .find_incomplete_replacement_journal(job.id())
            .await
        {
            Err(journal) => return Err(Error::join([Some(error), Some(journal)]).unwrap()),
            Ok(Some(_)) => return Err(error),
            Ok(None) => {}
        }
        let (code, reason) = failure_details(&error);
        self.finish_uncommitted_batch_file(cancel, job, OUTCOME_FAILED, &code, &reason)
            .await
            .map_err(|finish| Error::join([Some(error), Some(finish)]).unwrap())
    }

    async fn complete_batch(&self, batch_id: &str) -> Result<Batch> {
        if let Err(error) = self.repo.complete_batch(batch_id).await {
            let batch = self.repo.get_batch(batch_id).await;
            if let Ok(batch) = &batch
                && batch.status == BATCH_STATUS_COMPLETED
            {
                return Ok(batch.clone());
            }
            return Err(Error::join([Some(error), batch.err()]).unwrap());
        }
        self.repo.get_batch(batch_id).await
    }

    async fn finish_uncommitted_batch_file(
        &self,
        cancel: &CancellationToken,
        job: &ImportJob,
        outcome: &str,
        error_code: &str,
        reason: &str,
    ) -> Result<()> {
        if cancel.is_cancelled() {
            return Err(Error::sentinel(Sentinel::Cancelled));
        }
        if !job.staged_file_path.is_empty() {
            self.storage
                .remove_staged(&job.staged_file_path)
                .map_err(|error| error.context("remove uncommitted Managed Import staging file"))?;
        }
        let error_code = if outcome == OUTCOME_FAILED && error_code.is_empty() {
            "commit_failed"
        } else {
            error_code
        };
        self.repo
            .mark_batch_file_outcome(job.id(), outcome, error_code, reason)
            .await
    }

    pub(super) fn preflight_commit(
        &self,
        staged_bytes: i64,
        inspection: &MediaInspection,
    ) -> Result<()> {
        self.storage.preflight(Requirement {
            selected_bytes: staged_bytes,
            temporary_bytes: inspection.album_artwork.data.len() as i64,
        })
    }

    /// The journaled commit: prepare, place, verify, write hidden, clean up,
    /// publish.
    async fn commit(
        &self,
        job: &ImportJob,
        inspection: &MediaInspection,
        album_key: &str,
    ) -> Result<ImportResult> {
        let identity = self
            .repo
            .resolve_commit_identity(&inspection.metadata.album_artists, album_key)
            .await?;
        let planned = self
            .storage
            .plan_placement(&job.staged_file_path, inspection, &identity)?;
        let journal = CommitJournal {
            id: uuid::Uuid::new_v4().to_string(),
            job_id: job.id().to_owned(),
            track_id: identity.track_id.clone(),
            phase: COMMIT_PHASE_PREPARED.to_owned(),
            staged_file_path: job.staged_file_path.clone(),
            audio_file_path: planned.audio_path.clone(),
            artwork_file_path: planned.artwork_path.clone(),
            audio_sha256: inspection.file_sha256.clone(),
            artwork_sha256: inspection.album_artwork.sha256.clone(),
            ..CommitJournal::default()
        };
        self.repo.create_commit_journal(&journal).await?;
        self.after_commit_phase(COMMIT_PHASE_PREPARED)?;
        let placement = self
            .place_and_verify_commit(job, inspection, &identity, &journal)
            .await?;
        self.persist_and_finalize_commit(
            job, inspection, &identity, &journal, &placement, album_key,
        )
        .await
    }

    async fn place_and_verify_commit(
        &self,
        job: &ImportJob,
        inspection: &MediaInspection,
        identity: &CommitIdentity,
        journal: &CommitJournal,
    ) -> Result<PlacedFiles> {
        let handle = tokio::runtime::Handle::current();
        let repo = self.repo.clone();
        let journal_id = journal.id.clone();
        let mark = move || -> Result<()> {
            tokio::task::block_in_place(|| {
                handle.block_on(repo.mark_commit_artwork_created(&journal_id))
            })
        };
        let placed = self
            .storage
            .place(&job.staged_file_path, inspection, identity, Some(&mark));
        let placement = match placed {
            Ok(placement) => placement,
            Err(error) => {
                let journaled = self
                    .repo
                    .rollback_commit_journal(&journal.id, "canonical placement failed")
                    .await
                    .err();
                return Err(Error::join([Some(error), journaled]).unwrap());
            }
        };
        self.repo
            .mark_commit_placed(&journal.id, placement.artwork_created)
            .await?;
        self.after_commit_phase(COMMIT_PHASE_PLACED)?;
        if let Err(error) = self.storage.verify_placement(
            &placement,
            &journal.audio_sha256,
            &journal.artwork_sha256,
        ) {
            let rolled_back = self
                .rollback_filesystem_commit(journal, &placement, "canonical verification failed")
                .await
                .err();
            return Err(Error::join([Some(error), rolled_back]).unwrap());
        }
        self.repo
            .update_commit_phase(&journal.id, COMMIT_PHASE_VERIFIED)
            .await?;
        self.after_commit_phase(COMMIT_PHASE_VERIFIED)?;
        Ok(placement)
    }

    async fn persist_and_finalize_commit(
        &self,
        job: &ImportJob,
        inspection: &MediaInspection,
        identity: &CommitIdentity,
        journal: &CommitJournal,
        placement: &PlacedFiles,
        album_key: &str,
    ) -> Result<ImportResult> {
        let data = CommitData {
            job,
            identity,
            placement,
            inspection,
            album_key,
        };
        let mut committed = self.repo.commit_pending(&data, &journal.id).await;
        if committed.is_ok()
            && let Some(hook) = &self.result_hook
        {
            committed = hook("");
        }
        if let Err(error) = committed {
            let persisted = match self.repo.get_commit_journal(&journal.id).await {
                Ok(persisted) => persisted,
                Err(read) => return Err(Error::join([Some(error), Some(read)]).unwrap()),
            };
            if persisted.phase == COMMIT_PHASE_VERIFIED {
                let rolled_back = self
                    .rollback_filesystem_commit(&persisted, placement, "database commit failed")
                    .await
                    .err();
                return Err(Error::join([Some(error), rolled_back]).unwrap());
            }
            return Err(error);
        }
        self.after_commit_phase(COMMIT_PHASE_DATABASE_COMMITTED)?;
        self.storage.cleanup_placement(placement)?;
        self.repo
            .update_commit_phase(&journal.id, COMMIT_PHASE_CLEANED)
            .await?;
        self.after_commit_phase(COMMIT_PHASE_CLEANED)?;
        self.repo
            .finalize_commit(job, &identity.track_id, &journal.id)
            .await
    }

    async fn rollback_filesystem_commit(
        &self,
        journal: &CommitJournal,
        placement: &PlacedFiles,
        reason: &str,
    ) -> Result<()> {
        if let Err(error) = self.storage.rollback(placement) {
            return Err(self
                .record_reason_with(
                    &journal.id,
                    format!("{reason}; filesystem rollback failed"),
                    error,
                )
                .await);
        }
        self.repo.rollback_commit_journal(&journal.id, reason).await
    }

    async fn fail_upload(
        &self,
        job: &ImportJob,
        original_filename: &str,
        staged_path: &str,
        error: Error,
    ) -> Error {
        let (code, reason) = failure_details(&error);
        let (field, issues) = match error.as_validation() {
            Some(validation) => (validation.field.clone(), validation.validation_issues()),
            None => (String::new(), Vec::new()),
        };
        let removed = self.storage.remove_staged(staged_path).err();
        let marked = self
            .repo
            .mark_failed(job.id(), original_filename, &code, &field, &reason, &issues)
            .await
            .err();
        Error::join([Some(error), removed, marked]).unwrap()
    }

    async fn mark_upload_interrupted(
        &self,
        job: &ImportJob,
        original_filename: &str,
        staged_path: &str,
        error: Error,
    ) -> Error {
        let removed = if staged_path.is_empty() {
            None
        } else {
            self.storage.remove_staged(staged_path).err()
        };
        let marked = self
            .repo
            .mark_upload_interrupted(job.id(), original_filename)
            .await
            .err();
        Error::join([
            Some(Error::sentinel(Sentinel::UploadInterrupted)),
            Some(error),
            removed,
            marked,
        ])
        .unwrap()
    }

    async fn mark_rejected(
        &self,
        job: &ImportJob,
        original_filename: &str,
        error: &Error,
    ) -> Result<()> {
        if job.batch_id.is_empty() {
            return Ok(());
        }
        let (code, reason) = failure_details(error);
        self.repo
            .mark_failed(job.id(), original_filename, &code, "file", &reason, &[])
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_must_be_plain_names() {
        assert_eq!(safe_original_filename("  song.flac ").unwrap(), "song.flac");
        assert_eq!(safe_original_filename("..").unwrap(), "..");
        for bad in ["a/b.flac", "a\\b", "x\ny", "", ".", &"a".repeat(256)] {
            assert!(
                safe_original_filename(bad)
                    .unwrap_err()
                    .is(Sentinel::InvalidUpload),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn failure_details_follow_go() {
        let validation = Error::validation(ValidationError::new("c", "f", "r"), "x");
        assert_eq!(
            failure_details(&validation),
            ("c".to_owned(), "r".to_owned())
        );
        let storage = Error::wrap(Sentinel::InsufficientStorage, "x");
        assert_eq!(failure_details(&storage).0, "insufficient_storage");
        assert_eq!(failure_details(&Error::new("x")).0, "inspection_failed");
    }
}
