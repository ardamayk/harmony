package managedimport

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/ardam/navidrome-replacement/server/internal/modules/managedimport/db"
	"github.com/google/uuid"
)

func (r *Repository) CreateJob(ctx context.Context, batchID, clientFileID string) (Job, error) {
	job := Job{ID: uuid.NewString(), Status: STATUS_UPLOADING, Revision: 1}
	if batchID == "" {
		if err := r.q.InsertStandaloneJob(ctx, db.InsertStandaloneJobParams{ID: job.ID, ClientFileID: clientFileID}); err != nil {
			return Job{}, fmt.Errorf("create Managed Import Job: %w", err)
		}
		return job, nil
	}
	err := r.inTx(ctx, "Managed Import Batch file creation", func(q *db.Queries) error {
		inserted, err := q.InsertBatchJob(ctx, db.InsertBatchJobParams{ID: job.ID, BatchID: &batchID, ClientFileID: clientFileID})
		if err != nil {
			return fmt.Errorf("create Managed Import Job: %w", err)
		}
		if inserted != 1 {
			return ErrInvalidState
		}
		if err := q.ReviseBatch(ctx, batchID); err != nil {
			return fmt.Errorf("revise Managed Import Batch: %w", err)
		}
		return nil
	})
	if err != nil {
		return Job{}, err
	}
	return job, nil
}

func (r *Repository) CreateReplacementJob(ctx context.Context, trackID string) (Job, error) {
	job := Job{ID: uuid.NewString(), Status: STATUS_UPLOADING, Revision: 1, ReplacesTrackID: trackID}
	if err := r.q.InsertReplacementJob(ctx, db.InsertReplacementJobParams{ID: job.ID, ReplaceTrackID: &trackID}); err != nil {
		return Job{}, fmt.Errorf("create Track Replacement job: %w", err)
	}
	return job, nil
}

func (r *Repository) CreateBatch(ctx context.Context, _ BatchOptions) (Batch, error) {
	batch := Batch{ID: uuid.NewString(), Status: BATCH_STATUS_UPLOADING, Revision: 1, Files: []BatchFile{}, Albums: []AlbumPreview{}}
	if err := r.q.InsertBatch(ctx, batch.ID); err != nil {
		return Batch{}, fmt.Errorf("create Managed Import Batch: %w", err)
	}
	return batch, nil
}

func (r *Repository) GetJob(ctx context.Context, jobID string) (importJob, error) {
	return getImportJob(ctx, r.q, jobID)
}

func getImportJob(ctx context.Context, q *db.Queries, jobID string) (importJob, error) {
	row, err := q.GetJob(ctx, jobID)
	if errors.Is(err, sql.ErrNoRows) {
		return importJob{}, ErrNotFound
	}
	if err != nil {
		return importJob{}, fmt.Errorf("get Managed Import Job %q: %w", jobID, err)
	}
	job := importJob{
		Job: Job{
			ID:                 row.ID,
			Status:             ImportStatus(row.Status),
			Revision:           int(row.Revision),
			ValidationProgress: int(row.ValidationProgress),
			ErrorCode:          deref(row.ErrorCode),
			TrackID:            deref(row.TrackID),
			ReplacesTrackID:    deref(row.ReplaceTrackID),
		},
		ImportPlanJSON:   deref(row.ImportPlanJson),
		BatchID:          deref(row.BatchID),
		ClientFileID:     deref(row.ClientFileID),
		OriginalFilename: deref(row.OriginalFilename),
		StagedFilePath:   deref(row.StagedFilePath),
		ContentSHA256:    deref(row.ContentSha256),
		PreviewJSON:      deref(row.PreviewJson),
		ErrorField:       deref(row.ErrorField),
		ErrorReason:      deref(row.ErrorReason),
		Outcome:          ImportOutcome(deref(row.Outcome)),
		Selected:         row.Selected != 0,
		ReplaceTrackID:   deref(row.ReplaceTrackID),
	}
	job.Issues, err = decodeValidationIssues(row.ValidationIssues)
	if err != nil {
		return importJob{}, fmt.Errorf("get Managed Import Job %q: %w", jobID, err)
	}
	return job, nil
}

func (r *Repository) GetBatch(ctx context.Context, batchID string) (Batch, error) {
	row, err := r.q.GetBatch(ctx, batchID)
	if errors.Is(err, sql.ErrNoRows) {
		return Batch{}, ErrNotFound
	}
	if err != nil {
		return Batch{}, fmt.Errorf("get Managed Import Batch %q: %w", batchID, err)
	}
	batch := Batch{ID: row.ID, Status: BatchStatus(row.Status), Revision: int(row.Revision), Albums: []AlbumPreview{}}
	jobs, err := r.ListBatchJobs(ctx, batchID)
	if err != nil {
		return Batch{}, err
	}
	batch.Files = make([]BatchFile, 0, len(jobs))
	for _, job := range jobs {
		file, err := batchFileFromJob(job)
		if err != nil {
			return Batch{}, err
		}
		batch.Files = append(batch.Files, file)
	}
	return batch, nil
}

func (r *Repository) GetBatchStatus(ctx context.Context, batchID string) (BatchStatus, error) {
	row, err := r.q.GetBatch(ctx, batchID)
	if errors.Is(err, sql.ErrNoRows) {
		return "", ErrNotFound
	}
	if err != nil {
		return "", fmt.Errorf("get Managed Import Batch %q status: %w", batchID, err)
	}
	return BatchStatus(row.Status), nil
}

func (r *Repository) ListBatchJobs(ctx context.Context, batchID string) ([]importJob, error) {
	// The ID list is fully read before loading files: a pending SQLite writer
	// must not wait for a read cursor whose next query waits for that writer.
	ids, err := r.q.ListBatchJobIDs(ctx, &batchID)
	if err != nil {
		return nil, fmt.Errorf("list Managed Import Batch files: %w", err)
	}
	jobs := make([]importJob, 0, len(ids))
	for _, jobID := range ids {
		job, err := r.GetJob(ctx, jobID)
		if err != nil {
			return nil, err
		}
		jobs = append(jobs, job)
	}
	return jobs, nil
}

func (r *Repository) HeartbeatBatch(ctx context.Context, batchID string) error {
	refreshed, err := r.q.HeartbeatBatch(ctx, batchID)
	if err != nil {
		return fmt.Errorf("refresh Managed Import Batch %q liveness: %w", batchID, err)
	}
	if refreshed == 0 {
		return ErrNotFound
	}
	return nil
}

func (r *Repository) DeleteBatch(ctx context.Context, batchID string) error {
	return r.inTx(ctx, "Managed Import Batch cancellation", func(q *db.Queries) error {
		if err := archiveBatchHistory(ctx, q, batchID, HISTORY_RESULT_CANCELED); err != nil {
			return err
		}
		if err := q.DeleteBatchJobs(ctx, &batchID); err != nil {
			return fmt.Errorf("delete canceled Managed Import Batch files: %w", err)
		}
		deleted, err := q.DeleteUncompletedBatch(ctx, batchID)
		if err != nil {
			return fmt.Errorf("delete canceled Managed Import Batch: %w", err)
		}
		return requireOne(deleted, nil)
	})
}

func (r *Repository) DeleteJob(ctx context.Context, jobID string) error {
	return r.inTx(ctx, "Managed Import Job cancellation", func(q *db.Queries) error {
		job, err := q.GetCancelableJob(ctx, jobID)
		if errors.Is(err, sql.ErrNoRows) {
			return ErrInvalidState
		}
		if err != nil {
			return fmt.Errorf("resolve canceled Managed Import Job: %w", err)
		}
		now := time.Now().UTC()
		if job.BatchID == nil && ImportStatus(job.Status) != STATUS_FAILED {
			if err := q.TouchJob(ctx, db.TouchJobParams{UpdatedAt: now, ID: jobID}); err != nil {
				return fmt.Errorf("timestamp canceled standalone Managed Import Job: %w", err)
			}
			if err := archiveStandaloneHistory(ctx, q, jobID, HISTORY_RESULT_CANCELED); err != nil {
				return err
			}
		}
		if job.BatchID != nil {
			if err := q.RetainCanceledBatchFile(ctx, db.RetainCanceledBatchFileParams{CompletedAt: now, JobID: jobID}); err != nil {
				return fmt.Errorf("retain canceled Managed Import Batch file: %w", err)
			}
			if err := q.ReviseBatch(ctx, *job.BatchID); err != nil {
				return fmt.Errorf("revise canceled Managed Import Batch file: %w", err)
			}
		}
		if err := q.DeleteJob(ctx, jobID); err != nil {
			return fmt.Errorf("delete canceled Managed Import Job: %w", err)
		}
		return nil
	})
}

func (r *Repository) ListUncommittedBatchIDs(ctx context.Context, updatedBefore *time.Time) ([]string, error) {
	if updatedBefore != nil {
		return r.q.ListUncommittedBatchIDsBefore(ctx, *updatedBefore)
	}
	return r.q.ListUncommittedBatchIDs(ctx)
}

func (r *Repository) ListUncommittedStandaloneJobIDs(ctx context.Context, updatedBefore *time.Time) ([]string, error) {
	if updatedBefore != nil {
		return r.q.ListUncommittedStandaloneJobIDsBefore(ctx, *updatedBefore)
	}
	return r.q.ListUncommittedStandaloneJobIDs(ctx)
}

func (r *Repository) IsBatchUncommittedBefore(ctx context.Context, batchID string, updatedBefore time.Time) (bool, error) {
	isEligible, err := r.q.IsBatchUncommittedBefore(ctx, db.IsBatchUncommittedBeforeParams{ID: batchID, UpdatedAt: updatedBefore})
	if err != nil {
		return false, fmt.Errorf("check inactive Managed Import Batch %q: %w", batchID, err)
	}
	return isEligible, nil
}

func (r *Repository) IsStandaloneJobUncommittedBefore(ctx context.Context, jobID string, updatedBefore time.Time) (bool, error) {
	isEligible, err := r.q.IsStandaloneJobUncommittedBefore(ctx, db.IsStandaloneJobUncommittedBeforeParams{ID: jobID, UpdatedAt: updatedBefore})
	if err != nil {
		return false, fmt.Errorf("check inactive Managed Import Job %q: %w", jobID, err)
	}
	return isEligible, nil
}

func (r *Repository) UpdateValidationProgress(ctx context.Context, jobID string, progress int) error {
	if progress < 0 || progress > 100 {
		return fmt.Errorf("managed import validation progress must be between 0 and 100: got %d", progress)
	}
	updated, err := r.q.UpdateValidationProgress(ctx, db.UpdateValidationProgressParams{Progress: int64(progress), ID: jobID})
	if err != nil {
		return fmt.Errorf("update Managed Import validation progress: %w", err)
	}
	if updated == 0 {
		job, getErr := r.GetJob(ctx, jobID)
		if getErr != nil {
			return getErr
		}
		if job.Status != STATUS_UPLOADING {
			return ErrInvalidState
		}
	}
	return nil
}

func (r *Repository) ReserveBatchUpload(ctx context.Context, jobID string, uploadSize, batchLimit int64) error {
	reserved, err := r.q.ReserveBatchUpload(ctx, db.ReserveBatchUploadParams{UploadSize: uploadSize, ID: jobID, BatchLimit: batchLimit})
	if err != nil {
		return fmt.Errorf("reserve Managed Import Batch upload bytes: %w", err)
	}
	if reserved == 1 {
		return nil
	}
	job, getErr := r.GetJob(ctx, jobID)
	if getErr != nil {
		return getErr
	}
	if job.Status != STATUS_UPLOADING || job.BatchID == "" {
		return ErrInvalidState
	}
	return ErrBatchTooLarge
}

func (r *Repository) MarkPreview(ctx context.Context, jobID, originalFilename, stagedFilePath, contentSHA256, previewJSON string, uploadSize, batchLimit int64) (importJob, error) {
	err := r.inTx(ctx, "Import Preview transition", func(q *db.Queries) error {
		marked, err := q.MarkPreview(ctx, db.MarkPreviewParams{
			OriginalFilename: &originalFilename,
			StagedFilePath:   &stagedFilePath,
			ContentSha256:    &contentSHA256,
			PreviewJson:      &previewJSON,
			UploadSize:       uploadSize,
			ID:               jobID,
			BatchLimit:       batchLimit,
		})
		if err != nil {
			return fmt.Errorf("mark Import Preview ready: %w", err)
		}
		if marked != 1 {
			job, getErr := getImportJob(ctx, q, jobID)
			if getErr != nil {
				return getErr
			}
			if job.Status == STATUS_UPLOADING && job.BatchID != "" {
				return ErrBatchTooLarge
			}
			return ErrInvalidState
		}
		if err := q.ReviseJobBatch(ctx, jobID); err != nil {
			return fmt.Errorf("revise Managed Import Batch preview: %w", err)
		}
		return nil
	})
	if err != nil {
		return importJob{}, err
	}
	return r.GetJob(ctx, jobID)
}

func (r *Repository) MarkExactDuplicate(ctx context.Context, jobID, originalFilename, stagedPath, contentSHA256, previewJSON, trackID string, uploadSize int64) error {
	return r.inTx(ctx, "Exact Duplicate transition", func(q *db.Queries) error {
		marked, err := q.MarkExactDuplicate(ctx, db.MarkExactDuplicateParams{
			OriginalFilename: &originalFilename,
			ContentSha256:    &contentSHA256,
			TrackID:          &trackID,
			PreviewJson:      &previewJSON,
			StagedFilePath:   &stagedPath,
			UploadSize:       uploadSize,
			ID:               jobID,
		})
		if err = requireOne(marked, err); err != nil {
			return fmt.Errorf("mark Exact Duplicate: %w", err)
		}
		if err := q.ReviseUploadingJobBatch(ctx, jobID); err != nil {
			return fmt.Errorf("revise Exact Duplicate Batch file: %w", err)
		}
		return nil
	})
}

func (r *Repository) ClearStagedFile(ctx context.Context, jobID string) error {
	return r.inTx(ctx, "Managed Import staging cleanup", func(q *db.Queries) error {
		if err := requireOne(q.ClearStagedFile(ctx, jobID)); err != nil {
			return fmt.Errorf("clear Managed Import staging path: %w", err)
		}
		return archiveStandaloneHistory(ctx, q, jobID, HISTORY_RESULT_FAILED)
	})
}

func (r *Repository) MarkConfirmationExactDuplicate(ctx context.Context, jobID string) error {
	if err := requireOne(r.q.MarkConfirmationExactDuplicate(ctx, jobID)); err != nil {
		return fmt.Errorf("mark confirmation Exact Duplicate: %w", err)
	}
	return nil
}

func (r *Repository) RefreshDuplicatePreview(ctx context.Context, jobID, previewJSON string) error {
	return r.inTx(ctx, "duplicate preview refresh", func(q *db.Queries) error {
		if err := requireOne(q.RefreshDuplicatePreview(ctx, db.RefreshDuplicatePreviewParams{PreviewJson: &previewJSON, ID: jobID})); err != nil {
			return fmt.Errorf("refresh duplicate preview: %w", err)
		}
		if err := q.ReviseUploadingJobBatch(ctx, jobID); err != nil {
			return fmt.Errorf("revise refreshed duplicate batch: %w", err)
		}
		return nil
	})
}

func (r *Repository) MarkUploadInterrupted(ctx context.Context, jobID, originalFilename string) error {
	return r.inTx(ctx, "interrupted Managed Import transition", func(q *db.Queries) error {
		if err := requireOne(q.MarkUploadInterrupted(ctx, db.MarkUploadInterruptedParams{OriginalFilename: originalFilename, ID: jobID})); err != nil {
			return fmt.Errorf("mark Managed Import upload interrupted: %w", err)
		}
		if err := q.ReviseJobBatch(ctx, jobID); err != nil {
			return fmt.Errorf("revise interrupted Managed Import Batch file: %w", err)
		}
		return nil
	})
}

func (r *Repository) MarkFailed(ctx context.Context, jobID, originalFilename, errorCode, errorField, errorReason string, issueLists ...ValidationIssues) error {
	var issues ValidationIssues
	if len(issueLists) > 0 {
		issues = issueLists[0]
	}
	encodedIssues, err := issues.encode()
	if err != nil {
		return fmt.Errorf("mark Managed Import failed: %w", err)
	}
	return r.inTx(ctx, "failed Managed Import transition", func(q *db.Queries) error {
		marked, err := q.MarkFailed(ctx, db.MarkFailedParams{
			OriginalFilename: originalFilename,
			ErrorCode:        &errorCode,
			ErrorField:       &errorField,
			ErrorReason:      &errorReason,
			ValidationIssues: encodedIssues,
			ID:               jobID,
		})
		if err = requireOne(marked, err); err != nil {
			return fmt.Errorf("mark Managed Import failed: %w", err)
		}
		if err := q.ReviseUploadingJobBatch(ctx, jobID); err != nil {
			return fmt.Errorf("revise rejected Managed Import Batch file: %w", err)
		}
		return archiveStandaloneHistory(ctx, q, jobID, HISTORY_RESULT_FAILED)
	})
}

func (r *Repository) StartBatchConfirmation(ctx context.Context, batchID string, revision int, selectedIDs map[string]bool) error {
	return r.inTx(ctx, "Managed Import Batch confirmation", func(q *db.Queries) error {
		unresolved, err := q.CountUnresolvedBatchJobs(ctx, &batchID)
		if err != nil {
			return fmt.Errorf("count unresolved Managed Import Batch files: %w", err)
		}
		if unresolved > 0 {
			return fmt.Errorf("%w: Managed Import Batch has unresolved files", ErrInvalidState)
		}
		if err := transitionBatchConfirmation(ctx, q, batchID, revision); err != nil {
			return err
		}
		if err := q.ClearBatchSelection(ctx, &batchID); err != nil {
			return fmt.Errorf("clear Managed Import Batch selection: %w", err)
		}
		for jobID := range selectedIDs {
			if err := requireOne(q.SelectBatchJob(ctx, db.SelectBatchJobParams{ID: jobID, BatchID: &batchID})); err != nil {
				if errors.Is(err, ErrInvalidState) {
					return fmt.Errorf("%w: selected file %q is not accepted", ErrInvalidUpload, jobID)
				}
				return fmt.Errorf("select Managed Import Batch file: %w", err)
			}
		}
		return nil
	})
}

func transitionBatchConfirmation(ctx context.Context, q *db.Queries, batchID string, revision int) error {
	started, err := q.StartBatchConfirmation(ctx, db.StartBatchConfirmationParams{ID: batchID, Revision: int64(revision)})
	if err != nil {
		return fmt.Errorf("start Managed Import Batch confirmation: %w", err)
	}
	if started == 1 {
		return nil
	}
	current, err := q.GetBatch(ctx, batchID)
	if errors.Is(err, sql.ErrNoRows) {
		return ErrNotFound
	}
	if err != nil {
		return fmt.Errorf("inspect Managed Import Batch confirmation conflict: %w", err)
	}
	if int(current.Revision) != revision {
		return ErrRevisionConflict
	}
	return ErrInvalidState
}

func (r *Repository) MarkBatchFileOutcome(ctx context.Context, jobID string, outcome ImportOutcome, errorCode, errorReason string) error {
	status := STATUS_FAILED
	if outcome == OUTCOME_IMPORTED || outcome == OUTCOME_REPLACED {
		status = STATUS_COMMITTED
	}
	marked, err := r.q.MarkBatchFileOutcome(ctx, db.MarkBatchFileOutcomeParams{
		Status:      string(status),
		Outcome:     ptr(string(outcome)),
		ErrorCode:   errorCode,
		ErrorReason: errorReason,
		ID:          jobID,
	})
	if err = requireOne(marked, err); err != nil {
		return fmt.Errorf("record Managed Import Batch file outcome: %w", err)
	}
	return nil
}

func (r *Repository) CompleteBatch(ctx context.Context, batchID string) error {
	return r.inTx(ctx, "Managed Import Batch completion", func(q *db.Queries) error {
		completed, err := q.CompleteBatch(ctx, batchID)
		if err != nil {
			return fmt.Errorf("complete Managed Import Batch: %w", err)
		}
		if err := requireOne(completed, nil); err != nil {
			return err
		}
		if err := archiveBatchHistory(ctx, q, batchID, HISTORY_RESULT_COMPLETED); err != nil {
			return err
		}
		if err := q.ClearBatchPayloads(ctx, &batchID); err != nil {
			return fmt.Errorf("clear completed Import Preview payloads: %w", err)
		}
		if err := q.DeleteBatchArtwork(ctx, batchID); err != nil {
			return fmt.Errorf("clear completed import artwork: %w", err)
		}
		return nil
	})
}

func (r *Repository) AwaitingPreviewPaths(ctx context.Context, excludedJobID string) ([]string, error) {
	rows, err := r.q.ListAwaitingPreviewPaths(ctx, excludedJobID)
	if err != nil {
		return nil, fmt.Errorf("list awaiting Import Preview files: %w", err)
	}
	var paths []string
	for _, path := range rows {
		paths = append(paths, deref(path))
	}
	return paths, nil
}

// saveImportPlans records each file's plan only while the Batch is still at the
// revision the plans were computed from.
func (r *Repository) saveImportPlans(ctx context.Context, batch Batch, plans map[string]importPlan) error {
	return r.inTx(ctx, "import planning", func(q *db.Queries) error {
		revision, err := q.GetUploadingBatchRevision(ctx, batch.ID)
		if err != nil {
			return err
		}
		if int(revision) != batch.Revision {
			return ErrRevisionConflict
		}
		for jobID, plan := range plans {
			encoded, err := json.Marshal(plan)
			if err != nil {
				return err
			}
			saved, err := q.SaveImportPlan(ctx, db.SaveImportPlanParams{
				ImportPlanJson: ptr(string(encoded)),
				ReplaceTrackID: plan.TargetID,
				ID:             jobID,
				BatchID:        &batch.ID,
			})
			if err = requireOne(saved, err); err != nil {
				return err
			}
		}
		return nil
	})
}

func (r *Repository) SetJobReplaceTarget(ctx context.Context, jobID, trackID string) error {
	return r.q.SetJobReplaceTarget(ctx, db.SetJobReplaceTargetParams{ReplaceTrackID: &trackID, ID: jobID})
}

func (r *Repository) ContentHashExists(ctx context.Context, contentSHA256 string) (bool, error) {
	exists, err := r.q.ContentHashExists(ctx, &contentSHA256)
	if err != nil {
		return false, fmt.Errorf("check skipped import hash: %w", err)
	}
	return exists, nil
}
