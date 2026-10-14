package managedimport

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
)

type cancelAtEOFReader struct {
	source io.Reader
	cancel context.CancelFunc
}

func (reader *cancelAtEOFReader) Read(buffer []byte) (int, error) {
	read, err := reader.source.Read(buffer)
	if errors.Is(err, io.EOF) {
		reader.cancel()
	}
	return read, err
}

func TestFinishUncommittedBatchFileDoesNotCleanUpAfterCancellation(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	store := NewRepository(database)
	storage := newStorage(t.TempDir(), StorageLimits{FileBytes: 1024, BatchBytes: 1024}, unlimitedStorageCapacity)
	upload, err := storage.StageUpload(bytes.NewReader([]byte("audio")), 5)
	if err != nil {
		t.Fatalf("stage upload: %v", err)
	}
	service := NewService(store, storage, nil)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	err = service.finishUncommittedBatchFile(ctx, importJob{Job: Job{ID: "job-1"}, StagedFilePath: upload.Path}, OUTCOME_NOT_ATTEMPTED, "", "")
	if err == nil {
		t.Fatal("cleanup succeeded after cancellation")
	}
	if _, err := os.Stat(upload.Path); err != nil {
		t.Fatalf("staged upload was removed after cancellation: %v", err)
	}
}

func TestCleanupInactiveRemovesOnlyExpiredImportBatch(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	store := NewRepository(database)
	storage := newStorage(t.TempDir(), StorageLimits{FileBytes: 1024, BatchBytes: 2048}, unlimitedStorageCapacity)
	service := NewService(store, storage, nil)
	now := time.Date(2026, time.September, 2, 12, 0, 0, 0, time.UTC)
	expiredBatch, expiredUpload := createCleanupBatch(t, store, storage)
	activeBatch, activeUpload := createCleanupBatch(t, store, storage)
	if _, err := database.Exec(`UPDATE managed_import_batches SET updated_at = ? WHERE id = ?`, now.Add(-IMPORT_INACTIVITY_TIMEOUT-time.Second), expiredBatch.ID); err != nil {
		t.Fatalf("expire Managed Import Batch: %v", err)
	}
	if _, err := database.Exec(`UPDATE managed_import_batches SET updated_at = ? WHERE id = ?`, now.Add(-IMPORT_INACTIVITY_TIMEOUT+time.Second), activeBatch.ID); err != nil {
		t.Fatalf("keep Managed Import Batch active: %v", err)
	}

	if err := service.CleanupInactive(context.Background(), now); err != nil {
		t.Fatalf("cleanup inactive Managed Imports: %v", err)
	}
	if _, err := os.Stat(expiredUpload.Path); !os.IsNotExist(err) {
		t.Fatalf("expired staging stat error = %v", err)
	}
	if _, err := store.GetBatch(context.Background(), expiredBatch.ID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("get expired Import Batch error = %v", err)
	}
	if _, err := os.Stat(activeUpload.Path); err != nil {
		t.Fatalf("active staging was removed: %v", err)
	}
	if _, err := store.GetBatch(context.Background(), activeBatch.ID); err != nil {
		t.Fatalf("active Import Batch was removed: %v", err)
	}
}

func TestCleanupRestartRemovesOrphanStaging(t *testing.T) {
	storage := newStorage(t.TempDir(), StorageLimits{FileBytes: 1024, BatchBytes: 1024}, unlimitedStorageCapacity)
	upload, err := storage.StageUpload(bytes.NewReader([]byte("orphan")), 6)
	if err != nil {
		t.Fatalf("stage orphan upload: %v", err)
	}
	service := NewService(NewRepository(testutil.OpenMigratedDB(t)), storage, nil)

	if err := service.CleanupRestart(context.Background()); err != nil {
		t.Fatalf("cleanup restart staging: %v", err)
	}
	if _, err := os.Stat(upload.Path); !os.IsNotExist(err) {
		t.Fatalf("orphan staging stat error = %v", err)
	}
}

func TestCleanupRestartReconcilesRecordsWhenStagingCleanupPartiallyFails(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	store := NewRepository(database)
	storage := newStorage(t.TempDir(), StorageLimits{FileBytes: 1024, BatchBytes: 1024}, unlimitedStorageCapacity)
	batch, _ := createCleanupBatch(t, store, storage)
	blockedPath := filepath.Join(storage.root, ".staging", ".import-blocked.upload")
	if err := os.Mkdir(blockedPath, 0o750); err != nil {
		t.Fatalf("create blocked staging entry: %v", err)
	}
	if err := os.WriteFile(filepath.Join(blockedPath, "child"), []byte("blocked"), 0o600); err != nil {
		t.Fatalf("populate blocked staging entry: %v", err)
	}
	service := NewService(store, storage, nil)

	err := service.CleanupRestart(context.Background())
	if err == nil {
		t.Fatal("restart cleanup unexpectedly succeeded")
	}
	if _, err := store.GetBatch(context.Background(), batch.ID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("get reconciled Import Batch error = %v", err)
	}
}

func TestCancelJobRejectsCompletedBatchFile(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	store := NewRepository(database)
	batch, err := store.CreateBatch(context.Background(), BatchOptions{})
	if err != nil {
		t.Fatalf("create completed batch: %v", err)
	}
	job, err := store.CreateJob(context.Background(), batch.ID, "00000000-0000-4000-8000-000000000001")
	if err != nil {
		t.Fatalf("create completed batch job: %v", err)
	}
	storage := newStorage(t.TempDir(), StorageLimits{FileBytes: 1024, BatchBytes: 1024}, unlimitedStorageCapacity)
	upload, err := storage.StageUpload(bytes.NewReader([]byte("audio")), 5)
	if err != nil {
		t.Fatalf("stage completed batch job: %v", err)
	}
	if _, updateErr := database.Exec(`UPDATE managed_import_jobs SET status = ?, staged_file_path = ? WHERE id = ?`, STATUS_FAILED, upload.Path, job.ID); updateErr != nil {
		t.Fatalf("fail completed batch job: %v", updateErr)
	}
	if _, updateErr := database.Exec(`UPDATE managed_import_batches SET status = ? WHERE id = ?`, BATCH_STATUS_COMPLETED, batch.ID); updateErr != nil {
		t.Fatalf("complete batch: %v", updateErr)
	}
	service := NewService(store, storage, nil)

	err = service.CancelJob(context.Background(), job.ID)
	if !errors.Is(err, ErrInvalidState) {
		t.Fatalf("cancel completed batch job error = %v", err)
	}
	storedBatch, err := store.GetBatch(context.Background(), batch.ID)
	if err != nil {
		t.Fatalf("get completed batch: %v", err)
	}
	if len(storedBatch.Files) != 1 || storedBatch.Files[0].JobID != job.ID {
		t.Fatalf("completed batch files = %+v", storedBatch.Files)
	}
	if _, err := os.Stat(upload.Path); err != nil {
		t.Fatalf("completed batch staging was removed: %v", err)
	}
}

func TestCleanupInactiveExpiresFailedStandaloneJob(t *testing.T) {
	store := NewRepository(testutil.OpenMigratedDB(t))
	job, err := store.CreateJob(context.Background(), "", "")
	if err != nil {
		t.Fatalf("create failed standalone job: %v", err)
	}
	if err := store.MarkFailed(context.Background(), job.ID, "failed.flac", "invalid_upload", "file", "upload failed"); err != nil {
		t.Fatalf("mark standalone job failed: %v", err)
	}
	service := NewService(store, newStorage(t.TempDir(), StorageLimits{FileBytes: 1024, BatchBytes: 1024}, unlimitedStorageCapacity), nil)

	if err := service.CleanupInactive(context.Background(), time.Now().Add(IMPORT_INACTIVITY_TIMEOUT+time.Second)); err != nil {
		t.Fatalf("cleanup failed standalone job: %v", err)
	}
	if _, err := store.GetJob(context.Background(), job.ID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("get expired failed standalone job error = %v", err)
	}
}

func createCleanupBatch(t *testing.T, store *Repository, storage *Storage) (Batch, stagedUpload) {
	t.Helper()
	batch, err := store.CreateBatch(context.Background(), BatchOptions{})
	if err != nil {
		t.Fatalf("create cleanup batch: %v", err)
	}
	job, err := store.CreateJob(context.Background(), batch.ID, "00000000-0000-4000-8000-000000000001")
	if err != nil {
		t.Fatalf("create cleanup job: %v", err)
	}
	upload, err := storage.StageUpload(bytes.NewReader([]byte("audio")), 5)
	if err != nil {
		t.Fatalf("stage cleanup upload: %v", err)
	}
	if _, err := store.sqlDB.Exec(`UPDATE managed_import_jobs SET status = ?, original_filename = 'track.flac', staged_file_path = ?, content_sha256 = ? WHERE id = ?`, STATUS_AWAITING_CONFIRMATION, upload.Path, strings.Repeat("0", 64), job.ID); err != nil {
		t.Fatalf("prepare cleanup job: %v", err)
	}
	return batch, upload
}

func TestFinishUncommittedBatchFileRetainsPathWhenCleanupFails(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	store := NewRepository(database)
	batch, err := store.CreateBatch(context.Background(), BatchOptions{})
	if err != nil {
		t.Fatalf("create batch: %v", err)
	}
	job, err := store.CreateJob(context.Background(), batch.ID, "00000000-0000-4000-8000-000000000001")
	if err != nil {
		t.Fatalf("create job: %v", err)
	}
	unsafePath := t.TempDir() + "/outside.upload"
	if _, err = database.Exec(`UPDATE managed_import_jobs SET staged_file_path = ? WHERE id = ?`, unsafePath, job.ID); err != nil {
		t.Fatalf("set staged path: %v", err)
	}
	if _, err = database.Exec(`UPDATE managed_import_batches SET status = ? WHERE id = ?`, BATCH_STATUS_CONFIRMING, batch.ID); err != nil {
		t.Fatalf("set batch status: %v", err)
	}
	if _, err = database.Exec(`UPDATE managed_import_jobs SET status = ?, selected = 0, original_filename = 'track.flac', content_sha256 = ? WHERE id = ?`, STATUS_AWAITING_CONFIRMATION, strings.Repeat("0", 64), job.ID); err != nil {
		t.Fatalf("set job status: %v", err)
	}
	service := NewService(store, newStorage(t.TempDir(), StorageLimits{FileBytes: 1024, BatchBytes: 1024}, unlimitedStorageCapacity), nil)

	err = service.finishUncommittedBatchFile(context.Background(), importJob{Job: job, BatchID: batch.ID, StagedFilePath: unsafePath}, OUTCOME_NOT_ATTEMPTED, "", "")
	if err == nil {
		t.Fatal("cleanup unexpectedly succeeded")
	}
	storedJob, err := store.GetJob(context.Background(), job.ID)
	if err != nil {
		t.Fatalf("get job: %v", err)
	}
	if storedJob.StagedFilePath != unsafePath || storedJob.Outcome != "" {
		t.Fatalf("job after failed cleanup = %+v", storedJob)
	}
}

func TestRejectExactDuplicateRetainsStagingUntilFailureIsPersisted(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	store := NewRepository(database)
	job, err := store.CreateJob(context.Background(), "", "")
	if err != nil {
		t.Fatalf("create job: %v", err)
	}
	storage := newStorage(t.TempDir(), StorageLimits{FileBytes: 1024, BatchBytes: 1024}, unlimitedStorageCapacity)
	upload, err := storage.StageUpload(bytes.NewReader([]byte("audio")), 5)
	if err != nil {
		t.Fatalf("stage upload: %v", err)
	}
	if _, err = database.Exec(`
		UPDATE managed_import_jobs
		SET status = ?, original_filename = 'duplicate.flac', staged_file_path = ?, content_sha256 = ?
		WHERE id = ?`, STATUS_AWAITING_CONFIRMATION, upload.Path, strings.Repeat("0", 64), job.ID); err != nil {
		t.Fatalf("prepare duplicate job: %v", err)
	}
	if _, err = database.Exec(`
		CREATE TRIGGER reject_exact_duplicate_transition
		BEFORE UPDATE ON managed_import_jobs
		WHEN NEW.status = 'failed' AND NEW.error_code = 'exact_duplicate'
		BEGIN
			SELECT RAISE(ABORT, 'forced duplicate persistence failure');
		END`); err != nil {
		t.Fatalf("create failure trigger: %v", err)
	}
	service := NewService(store, storage, nil)
	duplicateJob, err := store.GetJob(context.Background(), job.ID)
	if err != nil {
		t.Fatalf("get duplicate job: %v", err)
	}

	err = service.rejectExactDuplicate(context.Background(), duplicateJob)
	if !errors.Is(err, ErrExactDuplicate) {
		t.Fatalf("reject exact duplicate error = %v", err)
	}
	if _, err = os.Stat(upload.Path); err != nil {
		t.Fatalf("staged duplicate removed before failure was persisted: %v", err)
	}
	storedJob, err := store.GetJob(context.Background(), job.ID)
	if err != nil {
		t.Fatalf("get unpersisted duplicate job: %v", err)
	}
	if storedJob.Status != STATUS_AWAITING_CONFIRMATION || storedJob.StagedFilePath != upload.Path {
		t.Fatalf("job after persistence failure = %+v", storedJob)
	}

	if _, err = database.Exec(`DROP TRIGGER reject_exact_duplicate_transition`); err != nil {
		t.Fatalf("drop failure trigger: %v", err)
	}
	err = service.rejectExactDuplicate(context.Background(), storedJob)
	if !errors.Is(err, ErrExactDuplicate) {
		t.Fatalf("retry exact duplicate error = %v", err)
	}
	if _, err = os.Stat(upload.Path); !os.IsNotExist(err) {
		t.Fatalf("staged duplicate still exists after persisted rejection: %v", err)
	}
	storedJob, err = store.GetJob(context.Background(), job.ID)
	if err != nil {
		t.Fatalf("get rejected duplicate job: %v", err)
	}
	if storedJob.Status != STATUS_FAILED || storedJob.ErrorCode != ERROR_CODE_EXACT_DUPLICATE || storedJob.StagedFilePath != "" {
		t.Fatalf("persisted duplicate job = %+v", storedJob)
	}
}

func TestBatchExactDuplicateCleanupFailureRemainsRetryable(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	store := NewRepository(database)
	storage := newStorage(t.TempDir(), StorageLimits{FileBytes: 1024, BatchBytes: 1024}, unlimitedStorageCapacity)
	batch, upload := createCleanupBatch(t, store, storage)
	if _, err := database.Exec(`UPDATE managed_import_batches SET status = ? WHERE id = ?`, BATCH_STATUS_CONFIRMING, batch.ID); err != nil {
		t.Fatalf("start Batch confirmation: %v", err)
	}
	jobs, err := store.ListBatchJobs(context.Background(), batch.ID)
	if err != nil {
		t.Fatalf("list Batch jobs: %v", err)
	}
	job := jobs[0]
	if err = store.MarkConfirmationExactDuplicate(context.Background(), job.ID); err != nil {
		t.Fatalf("mark confirmation Exact Duplicate: %v", err)
	}
	if err = os.Remove(upload.Path); err != nil {
		t.Fatalf("replace staged file: %v", err)
	}
	if err = os.Mkdir(upload.Path, 0o750); err != nil {
		t.Fatalf("create staged directory: %v", err)
	}
	if err = os.WriteFile(filepath.Join(upload.Path, "retained"), []byte("x"), 0o600); err != nil {
		t.Fatalf("make staged directory non-empty: %v", err)
	}
	service := NewService(store, storage, nil)
	cleanupErr := service.finishExactDuplicateCleanup(context.Background(), job)
	if cleanupErr == nil || service.handleBatchConfirmationError(context.Background(), job, cleanupErr) == nil {
		t.Fatal("failed Exact Duplicate cleanup was treated as completed")
	}
	storedBatch, err := store.GetBatch(context.Background(), batch.ID)
	if err != nil {
		t.Fatalf("get Batch after cleanup failure: %v", err)
	}
	storedJob, err := store.GetJob(context.Background(), job.ID)
	if err != nil {
		t.Fatalf("get job after cleanup failure: %v", err)
	}
	if storedBatch.Status != BATCH_STATUS_CONFIRMING || storedJob.Outcome != "" || storedJob.StagedFilePath != upload.Path {
		t.Fatalf("cleanup failure state: batch=%+v job=%+v", storedBatch, storedJob)
	}
}

func TestUploadKeepsBatchJobRetryableWhenFinalReservationIsCanceled(t *testing.T) {
	service, store, job := newBatchUploadService(t)
	ctx, cancel := context.WithCancel(context.Background())
	body := &cancelAtEOFReader{source: bytes.NewReader([]byte("audio")), cancel: cancel}

	_, err := service.Upload(ctx, job.ID, "track.flac", body, 5)
	assertRetryableBatchJob(t, store, job.ID, err)
}

func TestRecoverPreviewFailureKeepsCanceledBatchJobRetryable(t *testing.T) {
	service, store, job := newBatchUploadService(t)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	_, err := service.recoverPreviewFailure(ctx, job, "track.flac", "", context.Canceled, library.MediaInspection{})
	assertRetryableBatchJob(t, store, job.ID, err)
}

func newBatchUploadService(t *testing.T) (*Service, *Repository, importJob) {
	t.Helper()
	store := NewRepository(testutil.OpenMigratedDB(t))
	batch, err := store.CreateBatch(context.Background(), BatchOptions{})
	if err != nil {
		t.Fatalf("create batch: %v", err)
	}
	createdJob, err := store.CreateJob(context.Background(), batch.ID, "00000000-0000-4000-8000-000000000001")
	if err != nil {
		t.Fatalf("create job: %v", err)
	}
	job, err := store.GetJob(context.Background(), createdJob.ID)
	if err != nil {
		t.Fatalf("get job: %v", err)
	}
	storage := newStorage(t.TempDir(), StorageLimits{FileBytes: 1024, BatchBytes: 1024}, unlimitedStorageCapacity)
	return NewService(store, storage, nil), store, job
}

func assertRetryableBatchJob(t *testing.T, store *Repository, jobID string, uploadErr error) {
	t.Helper()
	if !errors.Is(uploadErr, ErrUploadInterrupted) {
		t.Fatalf("upload error = %v", uploadErr)
	}
	job, err := store.GetJob(context.Background(), jobID)
	if err != nil {
		t.Fatalf("get job: %v", err)
	}
	if job.Status != STATUS_UPLOADING || job.ErrorCode != UPLOAD_INTERRUPTED_ERROR_CODE {
		t.Fatalf("batch job after interruption = %+v", job)
	}
}

func TestCancelBatchReportsUnsafeCleanupAndRetainsRecords(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	store := NewRepository(database)
	batch, err := store.CreateBatch(context.Background(), BatchOptions{})
	if err != nil {
		t.Fatalf("create unsafe cleanup batch: %v", err)
	}
	job, err := store.CreateJob(context.Background(), batch.ID, "00000000-0000-4000-8000-000000000001")
	if err != nil {
		t.Fatalf("create unsafe cleanup job: %v", err)
	}
	unsafePath := filepath.Join(t.TempDir(), "outside.upload")
	if _, updateErr := database.Exec(`UPDATE managed_import_jobs SET status = ?, original_filename = 'track.flac', staged_file_path = ?, content_sha256 = ? WHERE id = ?`, STATUS_AWAITING_CONFIRMATION, unsafePath, strings.Repeat("0", 64), job.ID); updateErr != nil {
		t.Fatalf("set unsafe cleanup path: %v", updateErr)
	}
	service := NewService(store, newStorage(t.TempDir(), StorageLimits{FileBytes: 1024, BatchBytes: 1024}, unlimitedStorageCapacity), nil)

	err = service.CancelBatch(context.Background(), batch.ID)
	if !errors.Is(err, ErrUnsafeStoragePath) {
		t.Fatalf("cancel unsafe Import Batch error = %v", err)
	}
	storedBatch, err := store.GetBatch(context.Background(), batch.ID)
	if err != nil {
		t.Fatalf("unsafe Import Batch records were removed: %v", err)
	}
	if len(storedBatch.Files) != 1 {
		t.Fatalf("unsafe Import Batch files = %+v", storedBatch.Files)
	}
}
