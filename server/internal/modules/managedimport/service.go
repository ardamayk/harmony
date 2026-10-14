package managedimport

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/google/uuid"
)

type Service struct {
	repo             *Repository
	storage          *Storage
	inspector        library.MediaInspector
	uploadLocksMu    sync.Mutex
	uploadLocks      map[string]*uploadLock
	activeUploadsMu  sync.Mutex
	activeUploads    map[string]*activeUpload
	cancelingJobs    map[string]bool
	commitPhaseHook  func(commitPhase) error
	commitResultHook func() error
	queueEvents      QueueInvalidationPublisher

	replacementPhaseHook func(replacementPhase) error
	// albumDeletionTrackHook runs before each per-Track deletion of an Album
	// deletion; tests use it to inject a failure part-way through the run.
	albumDeletionTrackHook func(trackID string) error
}

var managedImportCommitMu sync.Mutex
var managedImportBatchConfirmationMu sync.Mutex

type uploadLock struct {
	mutex sync.Mutex
	users int
}

type activeUpload struct {
	cancel           context.CancelFunc
	done             chan struct{}
	lastActivity     time.Time
	phase            string
	transferredBytes int64
	totalBytes       int64
}

type uploadActivityReader struct {
	ctx    context.Context
	source io.Reader
	onRead func(int)
}

func (reader *uploadActivityReader) Read(buffer []byte) (int, error) {
	if err := reader.ctx.Err(); err != nil {
		return 0, err
	}
	read, err := reader.source.Read(buffer)
	if read > 0 {
		reader.onRead(read)
	}
	return read, err
}

func NewService(repo *Repository, storage *Storage, inspector library.MediaInspector) *Service {
	return &Service{
		repo:          repo,
		storage:       storage,
		inspector:     inspector,
		uploadLocks:   make(map[string]*uploadLock),
		activeUploads: make(map[string]*activeUpload),
		cancelingJobs: make(map[string]bool),
		queueEvents:   discardQueueInvalidations{},
	}
}

func (service *Service) CreateJob(ctx context.Context, batchID, clientFileID string) (Job, error) {
	if batchID != "" {
		if _, err := uuid.Parse(clientFileID); err != nil {
			return Job{}, fmt.Errorf("%w: clientFileId must be a UUID", ErrInvalidUpload)
		}
		managedImportBatchConfirmationMu.Lock()
		defer managedImportBatchConfirmationMu.Unlock()
	}
	return service.repo.CreateJob(ctx, batchID, clientFileID)
}

func (service *Service) CreateBatch(ctx context.Context, options BatchOptions) (Batch, error) {
	return service.repo.CreateBatch(ctx, options)
}

func (service *Service) GetBatch(ctx context.Context, batchID string) (Batch, error) {
	batch, err := service.repo.GetBatch(ctx, batchID)
	if err == nil {
		service.addBatchProgress(&batch)
		err = service.populateAlbumPreviews(ctx, &batch)
	}
	return batch, err
}

func (service *Service) ListHistory(ctx context.Context) (HistoryList, error) {
	return service.repo.ListHistory(ctx)
}

func (service *Service) GetJob(ctx context.Context, jobID string) (Job, error) {
	job, err := service.repo.GetJob(ctx, jobID)
	if err != nil {
		return Job{}, err
	}
	return job.Job, nil
}

func (service *Service) CancelBatch(ctx context.Context, batchID string) error {
	err := service.cancelBatch(ctx, batchID, nil)
	if errors.Is(err, ErrNotFound) {
		return nil
	}
	return err
}

func (service *Service) cancelBatch(ctx context.Context, batchID string, updatedBefore *time.Time) error {
	managedImportBatchConfirmationMu.Lock()
	defer managedImportBatchConfirmationMu.Unlock()
	batch, err := service.repo.GetBatch(ctx, batchID)
	if err != nil {
		return err
	}
	if batch.Status == BATCH_STATUS_COMPLETED {
		return ErrInvalidState
	}
	jobs, err := service.repo.ListBatchJobs(ctx, batchID)
	if err != nil {
		return err
	}
	isEligible, quiesceErr := service.quiesceUploads(ctx, jobs, updatedBefore)
	defer service.clearCancelingJobs(jobs)
	if quiesceErr != nil || !isEligible {
		return quiesceErr
	}
	unlockUploads := service.lockUploads(jobs)
	defer unlockUploads()
	if updatedBefore != nil {
		isEligible, eligibilityErr := service.repo.IsBatchUncommittedBefore(ctx, batchID, *updatedBefore)
		if eligibilityErr != nil || !isEligible {
			return eligibilityErr
		}
	}
	jobs, err = service.repo.ListBatchJobs(ctx, batchID)
	if err != nil {
		return err
	}
	if err := service.removeUncommittedStaging(jobs); err != nil {
		return err
	}
	return service.repo.DeleteBatch(ctx, batchID)
}

func (service *Service) CancelJob(ctx context.Context, jobID string) error {
	err := service.cancelJob(ctx, jobID, nil)
	if errors.Is(err, ErrNotFound) {
		return nil
	}
	return err
}

func (service *Service) cancelJob(ctx context.Context, jobID string, updatedBefore *time.Time) error {
	managedImportBatchConfirmationMu.Lock()
	defer managedImportBatchConfirmationMu.Unlock()
	job, err := service.repo.GetJob(ctx, jobID)
	if err != nil {
		return err
	}
	if job.BatchID != "" {
		batchStatus, statusErr := service.repo.GetBatchStatus(ctx, job.BatchID)
		if statusErr != nil {
			return statusErr
		}
		if batchStatus == BATCH_STATUS_COMPLETED {
			return ErrInvalidState
		}
	}
	isEligible, quiesceErr := service.quiesceUploads(ctx, []importJob{job}, updatedBefore)
	defer service.clearCancelingJobs([]importJob{job})
	if quiesceErr != nil || !isEligible {
		return quiesceErr
	}
	unlockUpload := service.lockUpload(jobID)
	defer unlockUpload()
	if updatedBefore != nil {
		isEligible, eligibilityErr := service.repo.IsStandaloneJobUncommittedBefore(ctx, jobID, *updatedBefore)
		if eligibilityErr != nil || !isEligible {
			return eligibilityErr
		}
	}
	job, err = service.repo.GetJob(ctx, jobID)
	if err != nil {
		return err
	}
	if job.Status == STATUS_COMMITTED {
		return ErrInvalidState
	}
	if _, hasReplacement, journalErr := service.repo.FindIncompleteReplacementJournal(ctx, jobID); journalErr != nil || hasReplacement {
		return errors.Join(journalErr, ErrInvalidState)
	}
	if err := service.removeUncommittedStaging([]importJob{job}); err != nil {
		return err
	}
	return service.repo.DeleteJob(ctx, jobID)
}

func (service *Service) lockUploads(jobs []importJob) func() {
	unlocks := make([]func(), 0, len(jobs))
	for _, job := range jobs {
		unlocks = append(unlocks, service.lockUpload(job.ID))
	}
	return func() {
		for index := len(unlocks) - 1; index >= 0; index-- {
			unlocks[index]()
		}
	}
}

func (service *Service) quiesceUploads(ctx context.Context, jobs []importJob, updatedBefore *time.Time) (bool, error) {
	service.activeUploadsMu.Lock()
	activeUploads := make([]*activeUpload, 0, len(jobs))
	for _, job := range jobs {
		active := service.activeUploads[job.ID]
		if updatedBefore != nil && active != nil && active.lastActivity.After(*updatedBefore) {
			service.activeUploadsMu.Unlock()
			return false, nil
		}
	}
	for _, job := range jobs {
		service.cancelingJobs[job.ID] = true
		if active := service.activeUploads[job.ID]; active != nil {
			active.cancel()
			activeUploads = append(activeUploads, active)
		}
	}
	service.activeUploadsMu.Unlock()
	waitCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), VALIDATION_CLEANUP_TIMEOUT)
	defer cancel()
	for _, active := range activeUploads {
		select {
		case <-active.done:
		case <-waitCtx.Done():
			return false, fmt.Errorf("wait for Managed Import upload cancellation: %w", waitCtx.Err())
		}
	}
	return true, nil
}

func (service *Service) clearCancelingJobs(jobs []importJob) {
	service.activeUploadsMu.Lock()
	defer service.activeUploadsMu.Unlock()
	for _, job := range jobs {
		delete(service.cancelingJobs, job.ID)
	}
}

func (service *Service) startActiveUpload(ctx context.Context, jobID string) (context.Context, *activeUpload) {
	uploadCtx, cancel := context.WithCancel(ctx)
	active := &activeUpload{cancel: cancel, done: make(chan struct{}), lastActivity: time.Now(), phase: "uploading"}
	service.activeUploadsMu.Lock()
	service.activeUploads[jobID] = active
	if service.cancelingJobs[jobID] {
		cancel()
	}
	service.activeUploadsMu.Unlock()
	return uploadCtx, active
}

func (service *Service) finishActiveUpload(jobID string, active *activeUpload) {
	active.cancel()
	service.activeUploadsMu.Lock()
	if service.activeUploads[jobID] == active {
		delete(service.activeUploads, jobID)
	}
	close(active.done)
	service.activeUploadsMu.Unlock()
}

func (service *Service) recordUploadActivity(jobID string, count int) {
	service.activeUploadsMu.Lock()
	defer service.activeUploadsMu.Unlock()
	if active := service.activeUploads[jobID]; active != nil {
		active.lastActivity = time.Now()
		active.transferredBytes += int64(count)
	}
}

func (service *Service) removeUncommittedStaging(jobs []importJob) error {
	var cleanupErr error
	for _, job := range jobs {
		if job.Status == STATUS_COMMITTED || job.StagedFilePath == "" {
			continue
		}
		if err := service.storage.RemoveStaged(job.StagedFilePath); err != nil {
			cleanupErr = errors.Join(cleanupErr, fmt.Errorf("remove staging for Managed Import Job %q: %w", job.ID, err))
		}
	}
	return cleanupErr
}

func (service *Service) CleanupInactive(ctx context.Context, now time.Time) error {
	cutoff := now.UTC().Add(-IMPORT_INACTIVITY_TIMEOUT)
	return service.cleanupUncommitted(ctx, &cutoff)
}

func (service *Service) CleanupRestart(ctx context.Context) error {
	recoveryErr := service.RecoverCommits(ctx)
	if recoveryErr != nil {
		return recoveryErr
	}
	stagingErr := service.storage.RemoveAllStaged()
	reconcileErr := service.cleanupUncommitted(ctx, nil)
	if stagingErr != nil {
		stagingErr = fmt.Errorf("remove Managed Import restart staging: %w", stagingErr)
	}
	return errors.Join(stagingErr, reconcileErr)
}

func (service *Service) RecoverCommits(ctx context.Context) error {
	managedImportCommitMu.Lock()
	defer managedImportCommitMu.Unlock()
	journals, err := service.repo.ListIncompleteCommitJournals(ctx)
	if err != nil {
		return err
	}
	var recoveryErr error
	for _, journal := range journals {
		if err := service.recoverCommit(ctx, journal); err != nil {
			recoveryErr = errors.Join(recoveryErr, fmt.Errorf("recover Managed Import commit %q: %w", journal.ID, err))
		}
	}
	return recoveryErr
}

func (service *Service) recoverCommit(ctx context.Context, journal commitJournal) error {
	placement, err := service.storage.placementFromJournal(journal)
	if err != nil {
		reasonErr := service.repo.RecordCommitRecoveryReason(ctx, journal.ID, "unsafe journaled storage path")
		return errors.Join(err, reasonErr)
	}
	switch journal.Phase {
	case COMMIT_PHASE_PREPARED, COMMIT_PHASE_PLACED, COMMIT_PHASE_VERIFIED:
		return service.rollbackUncommittedJournal(ctx, journal, placement)
	case COMMIT_PHASE_DATABASE_COMMITTED, COMMIT_PHASE_CLEANED:
		return service.completePendingJournal(ctx, journal, placement)
	default:
		return fmt.Errorf("unsupported Managed Import commit phase %q", journal.Phase)
	}
}

func (service *Service) rollbackUncommittedJournal(ctx context.Context, journal commitJournal, placement placedFiles) error {
	reason := fmt.Sprintf("restart rolled back commit from %s phase", journal.Phase)
	_, canonicalErr := os.Stat(journal.AudioFilePath)
	if canonicalErr == nil {
		if err := service.storage.Rollback(placement); err != nil {
			reasonErr := service.repo.RecordCommitRecoveryReason(ctx, journal.ID, reason+"; canonical rollback failed")
			return errors.Join(err, reasonErr)
		}
	} else if !errors.Is(canonicalErr, os.ErrNotExist) {
		reasonErr := service.repo.RecordCommitRecoveryReason(ctx, journal.ID, reason+"; canonical state unreadable")
		return errors.Join(canonicalErr, reasonErr)
	} else if err := service.storage.CleanupPreparedPlacement(placement); err != nil {
		reasonErr := service.repo.RecordCommitRecoveryReason(ctx, journal.ID, reason+"; prepared placement cleanup failed")
		return errors.Join(err, reasonErr)
	}
	return service.repo.RollbackCommitJournal(ctx, journal.ID, reason)
}

func (service *Service) completePendingJournal(ctx context.Context, journal commitJournal, placement placedFiles) error {
	if err := service.storage.VerifyPlacement(placement, journal.AudioSHA256, journal.ArtworkSHA256); err != nil {
		reason := fmt.Sprintf("restart rolled back commit because canonical verification failed: %v", err)
		databaseErr := service.repo.RollbackPendingCommit(ctx, journal, reason)
		if databaseErr != nil {
			return databaseErr
		}
		storageErr := service.storage.Rollback(placement)
		if storageErr != nil {
			reasonErr := service.repo.RecordCommitRecoveryReason(ctx, journal.ID, reason+"; filesystem rollback failed")
			return errors.Join(storageErr, reasonErr)
		}
		return service.repo.RollbackCommitJournal(ctx, journal.ID, reason)
	}
	if journal.Phase == COMMIT_PHASE_DATABASE_COMMITTED {
		if err := service.storage.CleanupPlacement(placement); err != nil {
			return err
		}
		if err := service.repo.UpdateCommitPhase(ctx, journal.ID, COMMIT_PHASE_CLEANED); err != nil {
			return err
		}
	}
	job, err := service.repo.GetJob(ctx, journal.JobID)
	if err != nil {
		return err
	}
	_, err = service.repo.FinalizeCommit(ctx, job, journal.TrackID, journal.ID)
	return err
}

func (service *Service) cleanupUncommitted(ctx context.Context, updatedBefore *time.Time) error {
	batchIDs, err := service.repo.ListUncommittedBatchIDs(ctx, updatedBefore)
	if err != nil {
		return fmt.Errorf("list uncommitted Managed Import Batches: %w", err)
	}
	jobIDs, err := service.repo.ListUncommittedStandaloneJobIDs(ctx, updatedBefore)
	if err != nil {
		return fmt.Errorf("list uncommitted Managed Import Jobs: %w", err)
	}
	var cleanupErr error
	for _, batchID := range batchIDs {
		if err := service.cancelBatch(ctx, batchID, updatedBefore); err != nil && !errors.Is(err, ErrNotFound) {
			cleanupErr = errors.Join(cleanupErr, fmt.Errorf("cleanup Managed Import Batch %q: %w", batchID, err))
		}
	}
	for _, jobID := range jobIDs {
		if err := service.cancelJob(ctx, jobID, updatedBefore); err != nil && !errors.Is(err, ErrNotFound) {
			cleanupErr = errors.Join(cleanupErr, fmt.Errorf("cleanup Managed Import Job %q: %w", jobID, err))
		}
	}
	return cleanupErr
}

func (service *Service) Upload(ctx context.Context, jobID, originalFilename string, body io.Reader, contentLength int64) (Preview, error) {
	unlock := service.lockUpload(jobID)
	ctx, active := service.startActiveUpload(ctx, jobID)
	defer func() {
		unlock()
		service.finishActiveUpload(jobID, active)
	}()
	if source, ok := body.(interface{ Interrupt() error }); ok {
		stopInterrupt := context.AfterFunc(ctx, func() {
			if err := source.Interrupt(); err != nil {
				slog.ErrorContext(ctx, "interrupt Managed Import upload read", "jobId", jobID, "error", err)
			}
		})
		defer stopInterrupt()
	}
	if err := ctx.Err(); err != nil {
		return Preview{}, err
	}
	service.activeUploadsMu.Lock()
	active.totalBytes = max(0, contentLength)
	service.activeUploadsMu.Unlock()
	job, err := service.getUploadingJob(ctx, jobID)
	if err != nil {
		return Preview{}, err
	}
	originalFilename, err = safeOriginalFilename(originalFilename)
	if err != nil {
		return Preview{}, errors.Join(err, service.markRejected(ctx, job, originalFilename, err))
	}
	err = service.reserveBatchUpload(ctx, job, contentLength)
	if err != nil {
		return Preview{}, service.handleUploadFailure(ctx, job, originalFilename, "", err)
	}
	body = &uploadActivityReader{ctx: ctx, source: body, onRead: func(count int) { service.recordUploadActivity(jobID, count) }}
	body = service.batchUploadReader(ctx, job, body, contentLength)
	upload, err := service.storage.StageUpload(body, contentLength)
	if err != nil {
		return Preview{}, service.handleUploadFailure(ctx, job, originalFilename, "", err)
	}
	if job.BatchID != "" {
		err = service.repo.ReserveBatchUpload(ctx, job.ID, upload.Size, service.storage.batchLimit)
		if err != nil {
			return Preview{}, service.handleUploadFailure(ctx, job, originalFilename, upload.Path, err)
		}
	}
	service.setUploadPhase(job.ID, "validating")
	inspection, err := service.validateStagedUpload(ctx, job, upload)
	if err != nil {
		return Preview{}, service.handleUploadFailure(ctx, job, originalFilename, upload.Path, err)
	}
	return service.persistPreview(ctx, job, originalFilename, upload, inspection)
}

func (service *Service) inspectStagedJob(ctx context.Context, job importJob) (library.MediaInspection, error) {
	inspection, err := service.inspector.Inspect(ctx, job.StagedFilePath, nil)
	if err != nil {
		return library.MediaInspection{}, validationError(err)
	}
	return service.applyImportPlan(ctx, job, inspection)
}

func (service *Service) handleUploadFailure(ctx context.Context, job importJob, originalFilename, stagedPath string, uploadErr error) error {
	if isRetryableUploadInterruption(job, ctx, uploadErr) {
		return service.markUploadInterrupted(ctx, job, originalFilename, stagedPath, uploadErr)
	}
	if stagedPath == "" {
		return errors.Join(uploadErr, service.markRejected(ctx, job, originalFilename, uploadErr))
	}
	return service.failUpload(ctx, job, originalFilename, stagedPath, uploadErr)
}

func isRetryableUploadInterruption(job importJob, ctx context.Context, err error) bool {
	return job.BatchID != "" &&
		(errors.Is(err, ErrUploadInterrupted) || ctx.Err() != nil || errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded))
}

type batchReservationReader struct {
	source  io.Reader
	reserve func(int64) error
	read    int64
}

func (reader *batchReservationReader) Read(buffer []byte) (int, error) {
	read, readErr := reader.source.Read(buffer)
	reader.read += int64(read)
	if read > 0 {
		if reserveErr := reader.reserve(reader.read); reserveErr != nil {
			return read, reserveErr
		}
	}
	return read, readErr
}

func (service *Service) batchUploadReader(ctx context.Context, job importJob, body io.Reader, contentLength int64) io.Reader {
	if job.BatchID == "" || contentLength >= 0 {
		return body
	}
	return &batchReservationReader{
		source: body,
		reserve: func(uploadSize int64) error {
			return service.repo.ReserveBatchUpload(ctx, job.ID, uploadSize, service.storage.batchLimit)
		},
	}
}

func (service *Service) lockUpload(jobID string) func() {
	service.uploadLocksMu.Lock()
	lock := service.uploadLocks[jobID]
	if lock == nil {
		lock = &uploadLock{}
		service.uploadLocks[jobID] = lock
	}
	lock.users++
	service.uploadLocksMu.Unlock()
	lock.mutex.Lock()
	return func() {
		lock.mutex.Unlock()
		service.uploadLocksMu.Lock()
		lock.users--
		if lock.users == 0 {
			delete(service.uploadLocks, jobID)
		}
		service.uploadLocksMu.Unlock()
	}
}

func (service *Service) reserveBatchUpload(ctx context.Context, job importJob, contentLength int64) error {
	if job.BatchID == "" {
		return nil
	}
	reservationSize, err := service.storage.UploadReservationSize(contentLength)
	if err != nil {
		return err
	}
	return service.repo.ReserveBatchUpload(ctx, job.ID, reservationSize, service.storage.batchLimit)
}

func (service *Service) getUploadingJob(ctx context.Context, jobID string) (importJob, error) {
	job, err := service.repo.GetJob(ctx, jobID)
	if err != nil || job.Status != STATUS_UPLOADING {
		if err == nil {
			err = ErrInvalidState
		}
		return importJob{}, err
	}
	if job.BatchID == "" {
		return job, nil
	}
	batchStatus, err := service.repo.GetBatchStatus(ctx, job.BatchID)
	if err != nil {
		return importJob{}, err
	}
	if batchStatus != BATCH_STATUS_UPLOADING {
		return importJob{}, ErrInvalidState
	}
	return job, nil
}

func (service *Service) validateStagedUpload(ctx context.Context, job importJob, upload stagedUpload) (library.MediaInspection, error) {
	inspection, err := service.inspector.Inspect(ctx, upload.Path, service.validationProgressReporter(ctx, job.ID))
	if err != nil {
		if ctx.Err() != nil {
			return library.MediaInspection{}, validationCancellationError(ctx)
		}
		return library.MediaInspection{}, validationError(err)
	}
	if inspection.FileSHA256 != upload.SHA256 {
		return library.MediaInspection{}, &ValidationError{Code: "staged_file_changed", Field: "file", Reason: "staged file differs from uploaded bytes", Err: errors.New("staged file hash differs from uploaded bytes")}
	}
	if job.BatchID == "" {
		if err := service.validateAlbumPositionsExcluding(ctx, job.ID, inspection.Metadata, job.ReplaceTrackID); err != nil {
			if ctx.Err() != nil {
				err = validationCancellationError(ctx)
			}
			return library.MediaInspection{}, err
		}
	}
	return inspection, nil
}

func (service *Service) persistPreview(ctx context.Context, job importJob, originalFilename string, upload stagedUpload, inspection library.MediaInspection) (Preview, error) {
	classification, candidates, err := service.repo.ClassifyDuplicate(ctx, inspection)
	if err != nil {
		return Preview{}, service.handleUploadFailure(ctx, job, originalFilename, upload.Path, err)
	}
	previewJob := job
	previewJob.Status = STATUS_AWAITING_CONFIRMATION
	if classification == DUPLICATE_EXACT {
		previewJob.Status = STATUS_FAILED
	}
	previewJob.Revision++
	previewJob.OriginalFilename = originalFilename
	preview := previewFromInspection(previewJob, inspection)
	preview.File.SizeBytes = upload.Size
	preview.DuplicateClassification = classification
	preview.DuplicateCandidates = candidates
	preview.MatchingTracks, err = service.repo.positionCandidates(ctx, inspection.Metadata, job.ReplaceTrackID)
	if err != nil {
		return Preview{}, service.handleUploadFailure(ctx, job, originalFilename, upload.Path, err)
	}
	if job.ReplaceTrackID != "" && classification != DUPLICATE_EXACT {
		state, stateErr := service.buildReplacementState(ctx, job, inspection, upload.Path)
		if stateErr != nil {
			return Preview{}, service.handleUploadFailure(ctx, job, originalFilename, upload.Path, stateErr)
		}
		preview.DuplicateClassification = DUPLICATE_NONE
		preview.DuplicateCandidates = nil
		preview.Replacement = &state.Preview
	}
	if job.BatchID != "" && inspection.AlbumArtwork.SHA256 != "" {
		if err = service.repo.saveArtwork(ctx, job.BatchID, job.ID, job.ID, albumPreviewKey(preview.File), inspection.AlbumArtwork); err != nil {
			return Preview{}, service.handleUploadFailure(ctx, job, originalFilename, upload.Path, err)
		}
	}
	previewBytes, err := json.Marshal(preview)
	if err != nil {
		return Preview{}, service.handleUploadFailure(ctx, job, originalFilename, upload.Path, fmt.Errorf("encode Import Preview: %w", err))
	}
	if classification == DUPLICATE_EXACT {
		markErr := service.repo.MarkExactDuplicate(ctx, job.ID, originalFilename, upload.Path, upload.SHA256, string(previewBytes), candidates[0].TrackID, upload.Size)
		if markErr != nil {
			return Preview{}, service.handleUploadFailure(ctx, job, originalFilename, upload.Path, markErr)
		}
		removeErr := service.storage.RemoveStaged(upload.Path)
		if removeErr != nil {
			return Preview{}, removeErr
		}
		if clearErr := service.repo.ClearStagedFile(ctx, job.ID); clearErr != nil {
			return Preview{}, clearErr
		}
		return preview, nil
	}
	if preflightErr := service.preflightCommit(upload.Size, inspection); preflightErr != nil {
		return Preview{}, service.handleUploadFailure(ctx, job, originalFilename, upload.Path, preflightErr)
	}
	markedJob, err := service.repo.MarkPreview(ctx, job.ID, originalFilename, upload.Path, upload.SHA256, string(previewBytes), upload.Size, service.storage.batchLimit)
	if err != nil {
		return service.recoverPreviewFailure(ctx, job, originalFilename, upload.Path, err, inspection)
	}
	preview.Status = markedJob.Status
	preview.Revision = markedJob.Revision
	return preview, nil
}

func (service *Service) recoverPreviewFailure(ctx context.Context, originalJob importJob, originalFilename, stagedPath string, transitionErr error, inspection library.MediaInspection) (Preview, error) {
	if errors.Is(transitionErr, ErrBatchTooLarge) {
		return Preview{}, service.failUpload(ctx, originalJob, originalFilename, stagedPath, transitionErr)
	}
	recoveryCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), VALIDATION_CLEANUP_TIMEOUT)
	defer cancel()
	job, getErr := service.repo.GetJob(recoveryCtx, originalJob.ID)
	if getErr == nil && job.Status == STATUS_AWAITING_CONFIRMATION {
		var preview Preview
		if err := json.Unmarshal([]byte(job.PreviewJSON), &preview); err != nil {
			return Preview{}, fmt.Errorf("decode recovered Import Preview: %w", err)
		}
		return preview, nil
	}
	if ctx.Err() != nil {
		transitionErr = validationCancellationError(ctx)
	}
	return Preview{}, service.handleUploadFailure(ctx, originalJob, originalFilename, stagedPath, errors.Join(transitionErr, getErr))
}

func validationCancellationError(ctx context.Context) error {
	return validationError(&library.InspectionError{Code: library.INSPECTION_ERROR_VALIDATION_CANCELLED, Field: "validation", Err: ctx.Err()})
}

func (service *Service) validateAlbumPositions(ctx context.Context, jobID string, metadata library.NormalizedMediaMetadata) error {
	return service.validateAlbumPositionsExcluding(ctx, jobID, metadata, "")
}

// validateAlbumPositionsExcluding ignores one existing Track, which a Track Replacement is about to overwrite.
func (service *Service) validateAlbumPositionsExcluding(ctx context.Context, jobID string, metadata library.NormalizedMediaMetadata, excludedTrackID string) error {
	if metadata.HasDiscNumber {
		return service.validateExistingAlbumTotals(ctx, metadata, excludedTrackID)
	}
	requiresDiscNumber, err := service.requiresDiscNumber(ctx, jobID, metadata, excludedTrackID)
	if err != nil {
		return err
	}
	if !requiresDiscNumber {
		return service.validateExistingAlbumTotals(ctx, metadata, excludedTrackID)
	}
	return &ValidationError{
		Code:   string(library.INSPECTION_ERROR_INVALID_METADATA),
		Field:  "DISCNUMBER",
		Reason: "DISCNUMBER is required for a known multi-disc Album",
		Err:    errors.New("DISCNUMBER is required for a known multi-disc Album"),
	}
}

func (service *Service) requiresDiscNumber(ctx context.Context, jobID string, metadata library.NormalizedMediaMetadata, excludedTrackID string) (bool, error) {
	requiresDiscNumber, err := service.repo.AlbumRequiresDiscNumber(ctx, metadata, excludedTrackID)
	if err != nil || requiresDiscNumber {
		return requiresDiscNumber, err
	}
	return service.awaitingSiblingRequiresDiscNumber(ctx, jobID, metadata)
}

func (service *Service) awaitingSiblingRequiresDiscNumber(ctx context.Context, jobID string, metadata library.NormalizedMediaMetadata) (bool, error) {
	paths, err := service.repo.AwaitingPreviewPaths(ctx, jobID)
	if err != nil {
		return false, err
	}
	albumKey := albumIdentityKey(metadata)
	for _, path := range paths {
		inspection, inspectionErr := service.inspector.Inspect(ctx, path, nil)
		if inspectionErr != nil {
			return false, fmt.Errorf("inspect awaiting sibling file: %w", inspectionErr)
		}
		sibling := inspection.Metadata
		if albumIdentityKey(sibling) == albumKey && isMultiDisc(sibling) {
			return true, nil
		}
	}
	return false, nil
}

func isMultiDisc(metadata library.NormalizedMediaMetadata) bool {
	return metadata.DiscPosition.Number > 1 || metadata.DiscPosition.Total > 1
}

func (service *Service) validateExistingAlbumTotals(ctx context.Context, metadata library.NormalizedMediaMetadata, excludedTrackID string) error {
	field, err := service.repo.AlbumPositionTotalConflict(ctx, metadata, excludedTrackID)
	if err != nil {
		return err
	}
	if field == "" {
		return nil
	}
	return &ValidationError{
		Code:   string(library.INSPECTION_ERROR_INVALID_METADATA),
		Field:  field,
		Reason: "position total conflicts with the existing Album",
		Err:    errors.New("position total conflicts with the existing Album"),
	}
}

func (service *Service) Confirm(ctx context.Context, jobID string, revision int) (Result, error) {
	return service.ConfirmWithDecision(ctx, jobID, revision, "")
}

func (service *Service) ConfirmWithDecision(ctx context.Context, jobID string, revision int, decision DuplicateAction) (Result, error) {
	managedImportBatchConfirmationMu.Lock()
	defer managedImportBatchConfirmationMu.Unlock()
	job, err := service.repo.GetJob(ctx, jobID)
	if err != nil {
		return Result{}, err
	}
	if job.BatchID != "" {
		return Result{}, ErrInvalidState
	}
	if job.ReplaceTrackID != "" {
		return Result{}, ErrReplacementRequired
	}
	return service.confirmJob(ctx, job, revision, decision)
}

func (service *Service) confirmJob(ctx context.Context, job importJob, revision int, decision DuplicateAction) (Result, error) {
	return service.confirmJobWithEdition(ctx, job, revision, decision, decision == DUPLICATE_ACTION_IMPORT_SEPARATELY)
}

func (service *Service) confirmJobWithEdition(ctx context.Context, job importJob, revision int, decision DuplicateAction, isSeparateEdition bool) (Result, error) {
	managedImportCommitMu.Lock()
	defer managedImportCommitMu.Unlock()
	job, err := service.repo.GetJob(ctx, job.ID)
	if err != nil {
		return Result{}, err
	}
	pendingReplacement, hasReplacement, err := service.repo.FindIncompleteReplacementJournal(ctx, job.ID)
	if err != nil {
		return Result{}, err
	}
	if hasReplacement {
		if err = service.recoverReplacement(ctx, pendingReplacement); err != nil {
			return Result{}, err
		}
		job, err = service.repo.GetJob(ctx, job.ID)
		if err != nil {
			return Result{}, err
		}
	}
	journal, hasJournal, err := service.repo.FindIncompleteCommitJournal(ctx, job.ID)
	if err != nil {
		return Result{}, err
	}
	if hasJournal {
		if recoveryErr := service.recoverCommit(ctx, journal); recoveryErr != nil {
			return Result{}, recoveryErr
		}
		job, err = service.repo.GetJob(ctx, job.ID)
		if err != nil {
			return Result{}, err
		}
	}
	if job.Status == STATUS_COMMITTED {
		if revision != job.Revision-1 {
			return Result{}, ErrRevisionConflict
		}
		return Result{JobID: job.ID, Status: job.Status, Revision: job.Revision, TrackID: job.TrackID}, nil
	}
	if job.Status == STATUS_FAILED && job.ErrorCode == ERROR_CODE_EXACT_DUPLICATE {
		if revision != job.Revision {
			return Result{}, ErrRevisionConflict
		}
		if job.StagedFilePath != "" {
			return Result{}, service.finishExactDuplicateCleanup(ctx, job)
		}
		return Result{}, ErrExactDuplicate
	}
	if job.Status != STATUS_AWAITING_CONFIRMATION {
		return Result{}, ErrInvalidState
	}
	if revision != job.Revision {
		return Result{}, ErrRevisionConflict
	}
	return service.confirmAwaitingJob(ctx, job, decision, isSeparateEdition)
}

func (service *Service) confirmAwaitingJob(ctx context.Context, job importJob, decision DuplicateAction, isSeparateEdition bool) (Result, error) {
	stagedBytes, err := service.storage.StagedFileSize(job.StagedFilePath)
	if err != nil {
		return Result{}, err
	}
	inspection, err := service.inspectStagedJob(ctx, job)
	if err != nil {
		return Result{}, err
	}
	if positionErr := service.validateAlbumPositions(ctx, job.ID, inspection.Metadata); positionErr != nil {
		return Result{}, positionErr
	}
	if inspection.FileSHA256 != job.ContentSHA256 {
		return Result{}, &ValidationError{
			Code:   "staged_file_changed",
			Field:  "file",
			Reason: "staged file changed after Import Preview",
			Err:    errors.New("staged file hash changed after Import Preview"),
		}
	}
	classification, _, err := service.repo.ClassifyDuplicate(ctx, inspection)
	if err != nil {
		return Result{}, err
	}
	if classification == DUPLICATE_EXACT {
		return Result{}, service.rejectExactDuplicate(ctx, job)
	}
	if isSeparateEdition {
		inspection.Metadata.AlbumIdentityKey = separateAlbumIdentityKey(job, inspection.Metadata)
	}
	candidates, err := service.repo.positionCandidates(ctx, inspection.Metadata, "")
	if err != nil {
		return Result{}, err
	}
	if len(candidates) > 0 {
		if len(candidates) != 1 || normalizeIdentity(candidates[0].Title) != normalizeIdentity(inspection.Metadata.Title) {
			return Result{}, albumPositionConflict()
		}
		if decision != DUPLICATE_ACTION_REPLACE_EXISTING {
			return Result{}, fmt.Errorf("%w: confirm Track Replacement or skip this file", ErrInvalidUpload)
		}
		if job.ReplaceTrackID != "" && job.ReplaceTrackID != candidates[0].TrackID {
			return Result{}, ErrRevisionConflict
		}
		plan, err := decodeImportPlan(job)
		if err != nil {
			return Result{}, err
		}
		if plan.TargetRevision != 0 && plan.TargetRevision != candidates[0].Revision {
			return Result{}, ErrRevisionConflict
		}
		job.ReplaceTrackID = candidates[0].TrackID
		if err = service.repo.SetJobReplaceTarget(ctx, job.ID, job.ReplaceTrackID); err != nil {
			return Result{}, err
		}
		state, err := service.buildReplacementState(ctx, job, inspection, job.StagedFilePath)
		if err != nil {
			return Result{}, err
		}
		result, err := service.commitReplacement(ctx, job, state)
		return Result{JobID: result.JobID, Status: result.Status, Revision: result.Revision, TrackID: result.TrackID}, err
	}
	if decision != "" && decision != DUPLICATE_ACTION_IMPORT_SEPARATELY {
		return Result{}, ErrRevisionConflict
	}
	if preflightErr := service.preflightCommit(stagedBytes, inspection); preflightErr != nil {
		return Result{}, preflightErr
	}
	albumKey := albumIdentityKey(inspection.Metadata)
	if isSeparateEdition {
		albumKey = separateAlbumIdentityKey(job, inspection.Metadata)
	}
	return service.commit(ctx, job, inspection, albumKey)
}

func separateAlbumIdentityKey(job importJob, metadata library.NormalizedMediaMetadata) string {
	editionID := job.ID
	if job.BatchID != "" {
		editionID = job.BatchID
	}
	return albumIdentityKey(metadata) + "\x1fmanaged-import-edition:" + editionID
}

func (service *Service) rejectExactDuplicate(ctx context.Context, job importJob) error {
	cleanupCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), VALIDATION_CLEANUP_TIMEOUT)
	defer cancel()
	if err := service.repo.MarkConfirmationExactDuplicate(cleanupCtx, job.ID); err != nil {
		return errors.Join(ErrExactDuplicate, err)
	}
	return service.finishExactDuplicateCleanup(cleanupCtx, job)
}

func (service *Service) finishExactDuplicateCleanup(ctx context.Context, job importJob) error {
	if err := service.storage.RemoveStaged(job.StagedFilePath); err != nil {
		return errors.Join(ErrExactDuplicate, err)
	}
	if err := service.repo.ClearStagedFile(ctx, job.ID); err != nil {
		return errors.Join(ErrExactDuplicate, err)
	}
	return ErrExactDuplicate
}

func (service *Service) ConfirmBatch(ctx context.Context, batchID string, confirmation BatchConfirmation) (Batch, error) {
	managedImportBatchConfirmationMu.Lock()
	defer managedImportBatchConfirmationMu.Unlock()
	selectedIDs, err := selectedFileIDs(confirmation.SelectedFileIDs)
	if err != nil {
		return Batch{}, err
	}
	batch, err := service.repo.GetBatch(ctx, batchID)
	if err != nil {
		return Batch{}, err
	}
	if batch.Status == BATCH_STATUS_COMPLETED {
		if confirmation.Revision != batch.Revision-2 {
			return Batch{}, ErrRevisionConflict
		}
		return batch, nil
	}
	jobs, err := service.repo.ListBatchJobs(ctx, batchID)
	if err != nil {
		return Batch{}, err
	}
	if batch.Status == BATCH_STATUS_UPLOADING {
		if confirmation.Revision != batch.Revision {
			return Batch{}, ErrRevisionConflict
		}
		err = service.prepareBatchPlans(ctx, batch, jobs, selectedIDs, confirmation)
		if err != nil {
			return Batch{}, err
		}
	}

	err = service.startOrResumeBatch(ctx, batch, confirmation.Revision, selectedIDs)
	if err != nil {
		return Batch{}, err
	}
	jobs, err = service.repo.ListBatchJobs(ctx, batchID)
	if err != nil {
		return Batch{}, err
	}
	if err := service.confirmBatchJobs(ctx, jobs); err != nil {
		return Batch{}, err
	}
	return service.completeBatch(ctx, batchID)
}

func decodeStoredPreview(job importJob) (Preview, error) {
	var preview Preview
	if err := json.Unmarshal([]byte(job.PreviewJSON), &preview); err != nil {
		return Preview{}, fmt.Errorf("decode duplicate preview for job %q: %w", job.ID, err)
	}
	return preview, nil
}

func previewAlbumKey(file PreviewFile) string {
	credits := normalizeCredits(file.AlbumArtists)
	return strings.Join(credits, "\x1e") + "\x1f" + normalizeIdentity(file.Album)
}

func normalizeCredits(credits []string) []string {
	normalized := make([]string, len(credits))
	for index, credit := range credits {
		normalized[index] = normalizeIdentity(credit)
	}
	return normalized
}

func selectedFileIDs(fileIDs []string) (map[string]bool, error) {
	selectedIDs := make(map[string]bool, len(fileIDs))
	for _, jobID := range fileIDs {
		if jobID == "" || selectedIDs[jobID] {
			return nil, fmt.Errorf("%w: selected file IDs must be unique and non-empty", ErrInvalidUpload)
		}
		selectedIDs[jobID] = true
	}
	return selectedIDs, nil
}

func (service *Service) startOrResumeBatch(ctx context.Context, batch Batch, revision int, selectedIDs map[string]bool) error {
	if batch.Status == BATCH_STATUS_CONFIRMING {
		if revision != batch.Revision-1 {
			return ErrRevisionConflict
		}
		return nil
	}
	if batch.Status != BATCH_STATUS_UPLOADING {
		return ErrInvalidState
	}
	return service.repo.StartBatchConfirmation(ctx, batch.ID, revision, selectedIDs)
}

func (service *Service) confirmBatchJobs(ctx context.Context, jobs []importJob) error {
	for _, job := range jobs {
		if job.Outcome != "" {
			continue
		}
		if !job.Selected {
			outcome, code, reason := OUTCOME_NOT_ATTEMPTED, job.ErrorCode, job.ErrorReason
			plan, err := decodeImportPlan(job)
			if err != nil {
				return err
			}
			if plan.SkipExact && job.ContentSHA256 != "" {
				exists, err := service.repo.ContentHashExists(ctx, job.ContentSHA256)
				if err != nil {
					return err
				}
				if exists {
					outcome, code, reason = OUTCOME_REJECTED, ERROR_CODE_EXACT_DUPLICATE, "identical file already exists in the library"
				}
			}
			if err := service.finishUncommittedBatchFile(ctx, job, outcome, code, reason); err != nil {
				return err
			}
			continue
		}
		plan, planErr := decodeImportPlan(job)
		if planErr != nil {
			return planErr
		}
		if _, err := service.confirmJobWithEdition(ctx, job, job.Revision, plan.Action, false); err != nil {
			if handleErr := service.handleBatchConfirmationError(ctx, job, err); handleErr != nil {
				return handleErr
			}
		}
	}
	return nil
}

func (service *Service) handleBatchConfirmationError(ctx context.Context, job importJob, confirmationErr error) error {
	if errors.Is(confirmationErr, ErrExactDuplicate) {
		persistedJob, err := service.repo.GetJob(ctx, job.ID)
		if err != nil {
			return errors.Join(confirmationErr, err)
		}
		if persistedJob.Outcome == OUTCOME_REJECTED && persistedJob.ErrorCode == ERROR_CODE_EXACT_DUPLICATE {
			return nil
		}
		return confirmationErr
	}
	if ctx.Err() != nil {
		return errors.Join(confirmationErr, ctx.Err())
	}
	persistedJob, err := service.repo.GetJob(ctx, job.ID)
	if err != nil {
		return errors.Join(confirmationErr, err)
	}
	if persistedJob.Status == STATUS_COMMITTED {
		return nil
	}
	_, hasJournal, journalErr := service.repo.FindIncompleteCommitJournal(ctx, job.ID)
	if journalErr != nil {
		return errors.Join(confirmationErr, journalErr)
	}
	if hasJournal {
		return confirmationErr
	}
	_, hasReplacement, replacementErr := service.repo.FindIncompleteReplacementJournal(ctx, job.ID)
	if replacementErr != nil {
		return errors.Join(confirmationErr, replacementErr)
	}
	if hasReplacement {
		return confirmationErr
	}
	errorCode, reason := failureDetails(confirmationErr)
	finishErr := service.finishUncommittedBatchFile(ctx, job, OUTCOME_FAILED, errorCode, reason)
	if finishErr != nil {
		return errors.Join(confirmationErr, finishErr)
	}
	return nil
}

func (service *Service) completeBatch(ctx context.Context, batchID string) (Batch, error) {
	if err := service.repo.CompleteBatch(ctx, batchID); err != nil {
		batch, getErr := service.repo.GetBatch(ctx, batchID)
		if getErr == nil && batch.Status == BATCH_STATUS_COMPLETED {
			return batch, nil
		}
		return Batch{}, errors.Join(err, getErr)
	}
	return service.repo.GetBatch(ctx, batchID)
}

func (service *Service) finishUncommittedBatchFile(ctx context.Context, job importJob, outcome ImportOutcome, errorCode, reason string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	cleanupCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), VALIDATION_CLEANUP_TIMEOUT)
	defer cancel()
	if job.StagedFilePath != "" {
		if err := service.storage.RemoveStaged(job.StagedFilePath); err != nil {
			return fmt.Errorf("remove uncommitted Managed Import staging file: %w", err)
		}
	}
	if outcome == OUTCOME_FAILED && errorCode == "" {
		errorCode = "commit_failed"
	}
	return service.repo.MarkBatchFileOutcome(cleanupCtx, job.ID, outcome, errorCode, reason)
}

func (service *Service) preflightCommit(stagedBytes int64, inspection library.MediaInspection) error {
	return service.storage.Preflight(StorageRequirement{
		SelectedBytes:  stagedBytes,
		TemporaryBytes: int64(len(inspection.AlbumArtwork.Data)),
	})
}

func (service *Service) commit(ctx context.Context, job importJob, inspection library.MediaInspection, albumKey string) (Result, error) {
	identity, journal, err := service.prepareCommit(ctx, job, inspection, albumKey)
	if err != nil {
		return Result{}, err
	}
	placement, err := service.placeAndVerifyCommit(ctx, job, inspection, identity, journal)
	if err != nil {
		return Result{}, err
	}
	return service.persistAndFinalizeCommit(ctx, job, inspection, identity, journal, placement, albumKey)
}

func (service *Service) prepareCommit(ctx context.Context, job importJob, inspection library.MediaInspection, albumKey string) (commitIdentity, commitJournal, error) {
	identity, resolveErr := service.repo.ResolveCommitIdentity(ctx, inspection.Metadata, albumKey)
	if resolveErr != nil {
		return commitIdentity{}, commitJournal{}, resolveErr
	}
	plannedPlacement, planErr := service.storage.planPlacement(job.StagedFilePath, inspection, identity)
	if planErr != nil {
		return commitIdentity{}, commitJournal{}, planErr
	}
	journal := commitJournal{
		ID: uuid.NewString(), JobID: job.ID, TrackID: identity.TrackID, Phase: COMMIT_PHASE_PREPARED,
		StagedFilePath: job.StagedFilePath, AudioFilePath: plannedPlacement.AudioPath,
		ArtworkFilePath: plannedPlacement.ArtworkPath, AudioSHA256: inspection.FileSHA256,
		ArtworkSHA256: inspection.AlbumArtwork.SHA256,
	}
	if err := service.repo.CreateCommitJournal(ctx, journal); err != nil {
		return commitIdentity{}, commitJournal{}, err
	}
	if err := service.afterCommitPhase(COMMIT_PHASE_PREPARED); err != nil {
		return commitIdentity{}, commitJournal{}, err
	}
	return identity, journal, nil
}

func (service *Service) placeAndVerifyCommit(ctx context.Context, job importJob, inspection library.MediaInspection, identity commitIdentity, journal commitJournal) (placedFiles, error) {
	placement, placementErr := service.storage.Place(job.StagedFilePath, inspection, identity, func() error {
		journal.IsArtworkCreated = true
		return service.repo.MarkCommitArtworkCreated(ctx, journal.ID)
	})
	if placementErr != nil {
		journalErr := service.repo.RollbackCommitJournal(context.WithoutCancel(ctx), journal.ID, "canonical placement failed")
		return placedFiles{}, errors.Join(placementErr, journalErr)
	}
	journal.IsArtworkCreated = placement.artworkCreated
	if err := service.repo.MarkCommitPlaced(ctx, journal.ID, placement.artworkCreated); err != nil {
		return placedFiles{}, err
	}
	if err := service.afterCommitPhase(COMMIT_PHASE_PLACED); err != nil {
		return placedFiles{}, err
	}
	if err := service.storage.VerifyPlacement(placement, journal.AudioSHA256, journal.ArtworkSHA256); err != nil {
		return placedFiles{}, errors.Join(err, service.rollbackFilesystemCommit(ctx, journal, placement, "canonical verification failed"))
	}
	if err := service.repo.UpdateCommitPhase(ctx, journal.ID, COMMIT_PHASE_VERIFIED); err != nil {
		return placedFiles{}, err
	}
	if err := service.afterCommitPhase(COMMIT_PHASE_VERIFIED); err != nil {
		return placedFiles{}, err
	}
	return placement, nil
}

func (service *Service) persistAndFinalizeCommit(ctx context.Context, job importJob, inspection library.MediaInspection, identity commitIdentity, journal commitJournal, placement placedFiles, albumKey string) (Result, error) {
	data := commitData{
		Job:        job,
		Identity:   identity,
		Placement:  placement,
		Inspection: inspection,
		AlbumKey:   albumKey,
	}
	commitErr := service.repo.CommitPending(ctx, data, journal.ID)
	if commitErr == nil && service.commitResultHook != nil {
		commitErr = service.commitResultHook()
	}
	if commitErr != nil {
		persisted, journalErr := service.repo.GetCommitJournal(context.WithoutCancel(ctx), journal.ID)
		if journalErr != nil {
			return Result{}, errors.Join(commitErr, journalErr)
		}
		if persisted.Phase == COMMIT_PHASE_VERIFIED {
			return Result{}, errors.Join(commitErr, service.rollbackFilesystemCommit(ctx, persisted, placement, "database commit failed"))
		}
		return Result{}, commitErr
	}
	if err := service.afterCommitPhase(COMMIT_PHASE_DATABASE_COMMITTED); err != nil {
		return Result{}, err
	}
	if err := service.storage.CleanupPlacement(placement); err != nil {
		return Result{}, err
	}
	if err := service.repo.UpdateCommitPhase(ctx, journal.ID, COMMIT_PHASE_CLEANED); err != nil {
		return Result{}, err
	}
	if err := service.afterCommitPhase(COMMIT_PHASE_CLEANED); err != nil {
		return Result{}, err
	}
	return service.repo.FinalizeCommit(ctx, job, identity.TrackID, journal.ID)
}

func (service *Service) afterCommitPhase(phase commitPhase) error {
	if service.commitPhaseHook == nil {
		return nil
	}
	return service.commitPhaseHook(phase)
}

func (service *Service) rollbackFilesystemCommit(ctx context.Context, journal commitJournal, placement placedFiles, reason string) error {
	rollbackErr := service.storage.Rollback(placement)
	if rollbackErr != nil {
		reasonErr := service.repo.RecordCommitRecoveryReason(context.WithoutCancel(ctx), journal.ID, reason+"; filesystem rollback failed")
		return errors.Join(rollbackErr, reasonErr)
	}
	journalErr := service.repo.RollbackCommitJournal(context.WithoutCancel(ctx), journal.ID, reason)
	return journalErr
}

func (service *Service) failUpload(ctx context.Context, job importJob, originalFilename, stagedPath string, uploadErr error) error {
	errorCode, reason := failureDetails(uploadErr)
	errorField := ""
	var issues ValidationIssues
	var validationErr *ValidationError
	if errors.As(uploadErr, &validationErr) {
		errorField = validationErr.Field
		issues = validationErr.validationIssues()
	}
	cleanupCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), VALIDATION_CLEANUP_TIMEOUT)
	defer cancel()
	return errors.Join(uploadErr, service.storage.RemoveStaged(stagedPath), service.repo.MarkFailed(cleanupCtx, job.ID, originalFilename, errorCode, errorField, reason, issues))
}

func (service *Service) markUploadInterrupted(ctx context.Context, job importJob, originalFilename, stagedPath string, uploadErr error) error {
	cleanupCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), VALIDATION_CLEANUP_TIMEOUT)
	defer cancel()
	var cleanupErr error
	if stagedPath != "" {
		cleanupErr = service.storage.RemoveStaged(stagedPath)
	}
	transitionErr := service.repo.MarkUploadInterrupted(cleanupCtx, job.ID, originalFilename)
	return errors.Join(ErrUploadInterrupted, uploadErr, cleanupErr, transitionErr)
}

func (service *Service) markRejected(ctx context.Context, job importJob, originalFilename string, uploadErr error) error {
	if job.BatchID == "" {
		return nil
	}
	errorCode, reason := failureDetails(uploadErr)
	cleanupCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), VALIDATION_CLEANUP_TIMEOUT)
	defer cancel()
	return service.repo.MarkFailed(cleanupCtx, job.ID, originalFilename, errorCode, "file", reason)
}

func failureDetails(err error) (string, string) {
	var validationErr *ValidationError
	if errors.As(err, &validationErr) {
		return validationErr.Code, validationErr.Reason
	}
	switch {
	case errors.Is(err, ErrUploadTooLarge):
		return "upload_too_large", "file exceeds the configured per-file byte limit"
	case errors.Is(err, ErrBatchTooLarge):
		return "batch_upload_too_large", "batch exceeds the configured byte limit"
	case errors.Is(err, ErrInvalidUpload):
		return "invalid_upload", "upload is invalid"
	case errors.Is(err, ErrInsufficientStorage):
		return "insufficient_storage", "Managed Storage does not have enough capacity for this import and its safety reserve"
	case errors.Is(err, ErrUnsafeStoragePath):
		return "unsafe_storage_path", "Managed Storage path failed containment checks"
	case errors.Is(err, ErrTrackNotFound):
		return "track_not_found", "the Track to replace no longer exists"
	case errors.Is(err, ErrNotManagedTrack):
		return "not_managed_track", "only Managed Tracks can be replaced"
	case errors.Is(err, ErrReplacementConflict):
		return "replacement_preview_changed", "the Track to replace changed during validation"
	case errors.Is(err, context.Canceled), errors.Is(err, context.DeadlineExceeded):
		return string(library.INSPECTION_ERROR_VALIDATION_CANCELLED), "file validation was canceled"
	default:
		return "inspection_failed", "file validation failed"
	}
}

// VALIDATION_PROGRESS_PERSIST_STEP_PERCENT is the minimum advance between two
// persisted validation progress values. Decoders report progress per frame, so
// persisting every percent would issue up to 100 UPDATE statements per file.
const VALIDATION_PROGRESS_PERSIST_STEP_PERCENT = 5

func (service *Service) validationProgressReporter(ctx context.Context, jobID string) library.InspectionProgressReporter {
	lastPersistedPercent := -1
	return func(progress library.InspectionProgress) error {
		service.recordUploadActivity(jobID, 0)
		if progress.Percent <= lastPersistedPercent {
			return nil
		}
		isMilestone := progress.Percent >= 100 || progress.Percent-lastPersistedPercent >= VALIDATION_PROGRESS_PERSIST_STEP_PERCENT
		if lastPersistedPercent >= 0 && !isMilestone {
			return nil
		}
		if err := service.repo.UpdateValidationProgress(ctx, jobID, progress.Percent); err != nil {
			return err
		}
		lastPersistedPercent = progress.Percent
		return nil
	}
}

func safeOriginalFilename(value string) (string, error) {
	if strings.ContainsAny(value, "\x00\r\n") {
		return "", fmt.Errorf("%w: filename contains forbidden characters", ErrInvalidUpload)
	}
	value = strings.TrimSpace(value)
	if strings.ContainsAny(value, "/\\") || filepath.Base(value) != value {
		return "", fmt.Errorf("%w: filename must not contain path segments", ErrInvalidUpload)
	}
	if value == "." || value == "" || len(value) > MAX_ORIGINAL_FILENAME_BYTES {
		return "", fmt.Errorf("%w: filename is missing or too long", ErrInvalidUpload)
	}
	return value, nil
}
