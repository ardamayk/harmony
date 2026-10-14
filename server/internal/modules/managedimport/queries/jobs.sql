-- Managed Import Jobs and Batches. Status literals match the constants in
-- types.go and the CHECK constraints in the migrations.

-- name: InsertStandaloneJob :exec
INSERT INTO managed_import_jobs (id, status, revision, client_file_id)
VALUES (sqlc.arg(id), 'uploading', 1, NULLIF(CAST(sqlc.arg(client_file_id) AS TEXT), ''));

-- name: InsertReplacementJob :exec
INSERT INTO managed_import_jobs (id, status, revision, replace_track_id)
VALUES (?, 'uploading', 1, ?);

-- name: InsertBatchJob :execrows
-- Adds the file at the next Batch position unless the Batch stopped uploading
-- or the client already canceled this file.
INSERT INTO managed_import_jobs (id, status, revision, batch_id, batch_position, client_file_id)
SELECT sqlc.arg(id), 'uploading', 1, batches.id, (
    SELECT COALESCE(MAX(position), 0) + 1 FROM (
        SELECT jobs.batch_position AS position FROM managed_import_jobs jobs WHERE jobs.batch_id = sqlc.arg(batch_id)
        UNION ALL
        SELECT canceled.position FROM managed_import_canceled_files canceled WHERE canceled.batch_id = sqlc.arg(batch_id)
    )
), NULLIF(CAST(sqlc.arg(client_file_id) AS TEXT), '')
FROM managed_import_batches batches
WHERE batches.id = sqlc.arg(batch_id) AND batches.status = 'uploading' AND NOT EXISTS (
    SELECT 1 FROM managed_import_canceled_files canceled
    WHERE canceled.batch_id = sqlc.arg(batch_id) AND canceled.file_id = NULLIF(CAST(sqlc.arg(client_file_id) AS TEXT), '')
);

-- name: InsertBatch :exec
INSERT INTO managed_import_batches (id, status, revision) VALUES (?, 'uploading', 1);

-- name: GetJob :one
SELECT id, status, revision, validation_progress, batch_id, client_file_id, original_filename, staged_file_path,
    content_sha256, error_code, track_id, preview_json, error_field, error_reason, outcome, selected,
    replace_track_id, validation_issues, import_plan_json
FROM managed_import_jobs WHERE id = ?;

-- name: GetBatch :one
SELECT id, status, revision FROM managed_import_batches WHERE id = ?;

-- name: ListBatchJobIDs :many
SELECT id FROM managed_import_jobs WHERE batch_id = ? ORDER BY batch_position;

-- name: ReviseBatch :exec
UPDATE managed_import_batches SET revision = revision + 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?;

-- name: ReviseJobBatch :exec
UPDATE managed_import_batches SET revision = revision + 1, updated_at = CURRENT_TIMESTAMP
WHERE id = (SELECT batch_id FROM managed_import_jobs WHERE managed_import_jobs.id = sqlc.arg(job_id));

-- name: ReviseUploadingJobBatch :exec
UPDATE managed_import_batches SET revision = revision + 1, updated_at = CURRENT_TIMESTAMP
WHERE id = (SELECT batch_id FROM managed_import_jobs WHERE managed_import_jobs.id = sqlc.arg(job_id))
    AND status = 'uploading';

-- name: HeartbeatBatch :execrows
UPDATE managed_import_batches SET updated_at = CURRENT_TIMESTAMP WHERE id = ? AND status <> 'completed';

-- name: DeleteBatchJobs :exec
DELETE FROM managed_import_jobs WHERE batch_id = ?;

-- name: DeleteUncompletedBatch :execrows
DELETE FROM managed_import_batches WHERE id = ? AND status != 'completed';

-- name: GetCancelableJob :one
SELECT jobs.batch_id, jobs.status
FROM managed_import_jobs jobs
LEFT JOIN managed_import_batches batches ON batches.id = jobs.batch_id
WHERE jobs.id = ? AND jobs.status != 'committed' AND (jobs.batch_id IS NULL OR batches.status != 'completed');

-- name: TouchJob :exec
UPDATE managed_import_jobs SET updated_at = ? WHERE id = ?;

-- name: RetainCanceledBatchFile :exec
INSERT INTO managed_import_canceled_files (
    batch_id, file_id, job_id, safe_filename, started_at, completed_at, content_sha256, position
)
SELECT batch_id, COALESCE(NULLIF(client_file_id, ''), id), id, original_filename,
    created_at, sqlc.arg(completed_at), content_sha256, batch_position
FROM managed_import_jobs WHERE managed_import_jobs.id = sqlc.arg(job_id);

-- name: DeleteJob :exec
DELETE FROM managed_import_jobs WHERE id = ?;

-- name: ListUncommittedBatchIDs :many
SELECT id FROM managed_import_batches WHERE status != 'completed';

-- name: ListUncommittedBatchIDsBefore :many
SELECT id FROM managed_import_batches WHERE status != 'completed' AND updated_at <= ?;

-- name: ListUncommittedStandaloneJobIDs :many
SELECT id FROM managed_import_jobs WHERE batch_id IS NULL AND status != 'committed';

-- name: ListUncommittedStandaloneJobIDsBefore :many
SELECT id FROM managed_import_jobs WHERE batch_id IS NULL AND status != 'committed' AND updated_at <= ?;

-- name: IsBatchUncommittedBefore :one
SELECT EXISTS(
    SELECT 1 FROM managed_import_batches WHERE id = ? AND status != 'completed' AND updated_at <= ?
) AS is_eligible;

-- name: IsStandaloneJobUncommittedBefore :one
SELECT EXISTS(
    SELECT 1 FROM managed_import_jobs
    WHERE id = ? AND batch_id IS NULL AND status != 'committed' AND updated_at <= ?
) AS is_eligible;

-- name: UpdateValidationProgress :execrows
UPDATE managed_import_jobs
SET validation_progress = sqlc.arg(progress), updated_at = CURRENT_TIMESTAMP
WHERE id = sqlc.arg(id) AND status = 'uploading' AND validation_progress < sqlc.arg(progress);

-- name: ReserveBatchUpload :execrows
UPDATE managed_import_jobs
SET upload_size_bytes = sqlc.arg(upload_size), error_code = NULL, error_field = NULL, error_reason = NULL,
    updated_at = CURRENT_TIMESTAMP
WHERE managed_import_jobs.id = sqlc.arg(id) AND managed_import_jobs.status = 'uploading' AND managed_import_jobs.batch_id IS NOT NULL AND sqlc.arg(upload_size) + COALESCE((
    SELECT SUM(sibling.upload_size_bytes) FROM managed_import_jobs AS sibling
    WHERE sibling.batch_id = managed_import_jobs.batch_id AND sibling.id != managed_import_jobs.id
), 0) <= sqlc.arg(batch_limit);

-- name: MarkPreview :execrows
UPDATE managed_import_jobs
SET status = 'awaiting_confirmation', revision = revision + 1, original_filename = sqlc.arg(original_filename),
    staged_file_path = sqlc.arg(staged_file_path), content_sha256 = sqlc.arg(content_sha256),
    preview_json = sqlc.arg(preview_json), upload_size_bytes = sqlc.arg(upload_size), error_code = NULL,
    error_field = NULL, error_reason = NULL, validation_issues = '[]', outcome = NULL, selected = 1,
    validation_progress = 100, updated_at = CURRENT_TIMESTAMP
WHERE managed_import_jobs.id = sqlc.arg(id) AND managed_import_jobs.status = 'uploading' AND (
    managed_import_jobs.batch_id IS NULL OR sqlc.arg(upload_size) + COALESCE((
        SELECT SUM(sibling.upload_size_bytes) FROM managed_import_jobs AS sibling
        WHERE sibling.batch_id = managed_import_jobs.batch_id AND sibling.id != managed_import_jobs.id
    ), 0) <= sqlc.arg(batch_limit)
);

-- name: MarkExactDuplicate :execrows
UPDATE managed_import_jobs
SET status = 'failed', revision = revision + 1, original_filename = sqlc.arg(original_filename),
    content_sha256 = sqlc.arg(content_sha256), track_id = sqlc.arg(track_id), preview_json = sqlc.arg(preview_json),
    error_code = 'exact_duplicate', error_field = 'file', error_reason = 'file bytes match an existing Track',
    outcome = CASE WHEN batch_id IS NULL THEN NULL ELSE 'rejected' END, selected = 0,
    staged_file_path = sqlc.arg(staged_file_path), upload_size_bytes = sqlc.arg(upload_size),
    validation_progress = 100, updated_at = CURRENT_TIMESTAMP
WHERE id = sqlc.arg(id) AND status = 'uploading';

-- name: ClearStagedFile :execrows
UPDATE managed_import_jobs SET staged_file_path = NULL, upload_size_bytes = 0,
    status = CASE WHEN error_code = 'exact_duplicate' THEN 'failed' ELSE status END,
    outcome = CASE WHEN error_code = 'exact_duplicate' AND batch_id IS NOT NULL THEN 'rejected' ELSE outcome END,
    selected = CASE WHEN error_code = 'exact_duplicate' THEN 0 ELSE selected END,
    updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND staged_file_path IS NOT NULL;

-- name: MarkConfirmationExactDuplicate :execrows
UPDATE managed_import_jobs
SET status = 'failed', error_code = 'exact_duplicate', error_field = 'file',
    error_reason = 'file bytes match an existing Track', updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND status = 'awaiting_confirmation';

-- name: RefreshDuplicatePreview :execrows
UPDATE managed_import_jobs SET preview_json = ?, revision = revision + 1, selected = 0, updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND status = 'awaiting_confirmation';

-- name: MarkUploadInterrupted :execrows
UPDATE managed_import_jobs
SET revision = revision + 1,
    original_filename = COALESCE(NULLIF(CAST(sqlc.arg(original_filename) AS TEXT), ''), original_filename),
    error_code = 'upload_interrupted', error_field = 'file',
    error_reason = 'upload stream was interrupted; retry this file',
    outcome = NULL, selected = 0, staged_file_path = NULL, content_sha256 = NULL,
    preview_json = NULL, upload_size_bytes = 0, validation_progress = 0, updated_at = CURRENT_TIMESTAMP
WHERE id = sqlc.arg(id) AND status = 'uploading' AND batch_id IS NOT NULL;

-- name: MarkFailed :execrows
UPDATE managed_import_jobs
SET status = 'failed',
    original_filename = COALESCE(NULLIF(CAST(sqlc.arg(original_filename) AS TEXT), ''), original_filename),
    error_code = sqlc.arg(error_code), error_field = sqlc.arg(error_field), error_reason = sqlc.arg(error_reason),
    validation_issues = sqlc.arg(validation_issues),
    outcome = CASE WHEN batch_id IS NULL THEN NULL ELSE 'rejected' END,
    selected = 0, staged_file_path = NULL, updated_at = CURRENT_TIMESTAMP
WHERE id = sqlc.arg(id) AND status IN ('uploading', 'awaiting_confirmation');

-- name: CountUnresolvedBatchJobs :one
SELECT COUNT(*) FROM managed_import_jobs
WHERE batch_id = ? AND status = 'uploading' AND COALESCE(error_code, '') <> 'upload_interrupted';

-- name: StartBatchConfirmation :execrows
UPDATE managed_import_batches SET status = 'confirming', revision = revision + 1, updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND status = 'uploading' AND revision = ?;

-- name: ClearBatchSelection :exec
UPDATE managed_import_jobs SET selected = 0 WHERE batch_id = ?;

-- name: SelectBatchJob :execrows
UPDATE managed_import_jobs SET selected = 1 WHERE id = ? AND batch_id = ? AND status = 'awaiting_confirmation';

-- name: MarkBatchFileOutcome :execrows
UPDATE managed_import_jobs
SET status = sqlc.arg(status), outcome = sqlc.arg(outcome),
    error_code = NULLIF(CAST(sqlc.arg(error_code) AS TEXT), ''),
    error_reason = NULLIF(CAST(sqlc.arg(error_reason) AS TEXT), ''),
    staged_file_path = NULL, updated_at = CURRENT_TIMESTAMP
WHERE id = sqlc.arg(id)
    AND (status = 'awaiting_confirmation' OR (status = 'uploading' AND error_code = 'upload_interrupted'))
    AND outcome IS NULL;

-- name: CompleteBatch :execrows
UPDATE managed_import_batches SET status = 'completed', revision = revision + 1, updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND status = 'confirming';

-- name: ClearBatchPayloads :exec
UPDATE managed_import_jobs SET preview_json = NULL, import_plan_json = NULL WHERE batch_id = ?;

-- name: DeleteBatchArtwork :exec
DELETE FROM managed_import_artwork WHERE batch_id = ?;

-- name: ListAwaitingPreviewPaths :many
SELECT staged_file_path FROM managed_import_jobs
WHERE status = 'awaiting_confirmation' AND id != ? AND staged_file_path IS NOT NULL;

-- name: GetUploadingBatchRevision :one
SELECT revision FROM managed_import_batches WHERE id = ? AND status = 'uploading';

-- name: SaveImportPlan :execrows
UPDATE managed_import_jobs
SET import_plan_json = sqlc.arg(import_plan_json), replace_track_id = NULLIF(CAST(sqlc.arg(replace_track_id) AS TEXT), '')
WHERE id = sqlc.arg(id) AND batch_id = sqlc.arg(batch_id) AND status = 'awaiting_confirmation';

-- name: MarkJobCommitted :execrows
UPDATE managed_import_jobs
SET status = 'committed', revision = revision + 1, track_id = ?, staged_file_path = NULL,
    outcome = CASE WHEN batch_id IS NULL THEN NULL ELSE 'imported' END, updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND status = 'awaiting_confirmation' AND revision = ?;

-- name: MarkJobReplaced :execrows
UPDATE managed_import_jobs
SET status = 'committed', revision = revision + 1, track_id = replace_track_id, outcome = 'replaced',
    staged_file_path = NULL, updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND status = 'awaiting_confirmation' AND revision = ?;

-- name: SetJobReplaceTarget :exec
UPDATE managed_import_jobs SET replace_track_id = ? WHERE id = ?;

-- name: ContentHashExists :one
SELECT EXISTS(SELECT 1 FROM track_sources WHERE content_sha256 = ?) AS hash_exists;
