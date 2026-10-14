-- Import History: terminal imports retained without their staged audio.

-- name: ListHistory :many
SELECT import_id, started_at, completed_at, result_code, total_count, imported_count,
    rejected_count, failed_count, replaced_count, not_attempted_count, canceled_count
FROM managed_import_history
ORDER BY completed_at DESC, rowid DESC
LIMIT ?;

-- name: ListHistoryFiles :many
SELECT file_id, job_id, safe_filename, started_at, completed_at, content_sha256,
    result_code, created_track_id, replaced_track_id, validation_issues
FROM managed_import_history_files WHERE import_id = ? ORDER BY position;

-- name: GetBatchCreatedAt :one
SELECT created_at FROM managed_import_batches WHERE id = ?;

-- name: ListBatchHistorySources :many
-- Canceled files that never became Jobs are reported as not attempted.
SELECT COALESCE(NULLIF(client_file_id, ''), id) AS file_id, id AS job_id, original_filename AS safe_filename,
    created_at AS started_at, updated_at AS completed_at, content_sha256, status, outcome, error_code, track_id,
    validation_issues, batch_position
FROM managed_import_jobs jobs WHERE jobs.batch_id = sqlc.arg(batch_id)
UNION ALL
SELECT file_id, job_id, safe_filename, started_at, completed_at, content_sha256,
    'failed', 'not_attempted', 'canceled', NULL, '[]', position
FROM managed_import_canceled_files canceled WHERE canceled.batch_id = sqlc.arg(batch_id)
ORDER BY batch_position;

-- name: GetStandaloneHistorySource :one
SELECT COALESCE(NULLIF(client_file_id, ''), id) AS file_id, id AS job_id, original_filename AS safe_filename,
    created_at AS started_at, updated_at AS completed_at, content_sha256, status, outcome, error_code, track_id,
    validation_issues
FROM managed_import_jobs WHERE id = ? AND batch_id IS NULL;

-- name: ClearStandaloneTerminalPayloads :exec
UPDATE managed_import_jobs SET preview_json = NULL, staged_file_path = NULL
WHERE id = ? AND batch_id IS NULL AND status IN ('committed', 'failed');

-- name: InsertHistory :execrows
INSERT OR IGNORE INTO managed_import_history (
    import_id, started_at, completed_at, result_code, total_count, imported_count,
    rejected_count, failed_count, replaced_count, not_attempted_count, canceled_count
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);

-- name: InsertHistoryFile :exec
INSERT INTO managed_import_history_files (
    import_id, file_id, job_id, safe_filename, started_at, completed_at, content_sha256,
    result_code, created_track_id, replaced_track_id, position, validation_issues
) VALUES (
    sqlc.arg(import_id), sqlc.arg(file_id), sqlc.arg(job_id), NULLIF(CAST(sqlc.arg(safe_filename) AS TEXT), ''),
    sqlc.arg(started_at), sqlc.arg(completed_at), NULLIF(CAST(sqlc.arg(content_sha256) AS TEXT), ''),
    sqlc.arg(result_code), NULLIF(CAST(sqlc.arg(created_track_id) AS TEXT), ''),
    NULLIF(CAST(sqlc.arg(replaced_track_id) AS TEXT), ''), sqlc.arg(position), sqlc.arg(validation_issues)
);

-- name: PruneHistory :exec
DELETE FROM managed_import_history WHERE import_id IN (
    SELECT import_id FROM managed_import_history ORDER BY completed_at DESC, rowid DESC LIMIT -1 OFFSET ?
);
