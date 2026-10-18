-- +goose Up
CREATE TABLE managed_import_history (
    import_id TEXT PRIMARY KEY,
    started_at DATETIME NOT NULL,
    completed_at DATETIME NOT NULL,
    result_code TEXT NOT NULL CHECK (result_code IN ('completed', 'partially_completed', 'failed', 'canceled')),
    total_count INTEGER NOT NULL CHECK (total_count >= 0),
    imported_count INTEGER NOT NULL CHECK (imported_count >= 0),
    rejected_count INTEGER NOT NULL CHECK (rejected_count >= 0),
    failed_count INTEGER NOT NULL CHECK (failed_count >= 0),
    replaced_count INTEGER NOT NULL CHECK (replaced_count >= 0),
    not_attempted_count INTEGER NOT NULL CHECK (not_attempted_count >= 0),
    canceled_count INTEGER NOT NULL CHECK (canceled_count >= 0),
    CHECK (
        total_count = imported_count + rejected_count + failed_count +
            replaced_count + not_attempted_count + canceled_count
    )
);

CREATE INDEX idx_managed_import_history_completed
    ON managed_import_history(completed_at DESC);

CREATE TABLE managed_import_history_files (
    import_id TEXT NOT NULL REFERENCES managed_import_history(import_id) ON DELETE CASCADE,
    file_id TEXT NOT NULL,
    job_id TEXT NOT NULL,
    safe_filename TEXT,
    started_at DATETIME NOT NULL,
    completed_at DATETIME NOT NULL,
    content_sha256 TEXT CHECK (
        content_sha256 IS NULL OR (
            length(content_sha256) = 64 AND
            content_sha256 NOT GLOB '*[^0-9a-f]*'
        )
    ),
    result_code TEXT NOT NULL,
    created_track_id TEXT,
    replaced_track_id TEXT,
    position INTEGER NOT NULL CHECK (position >= 0),
    PRIMARY KEY (import_id, file_id),
    UNIQUE (import_id, job_id)
);

CREATE INDEX idx_managed_import_history_files_import
    ON managed_import_history_files(import_id, position);

CREATE TABLE managed_import_canceled_files (
    batch_id TEXT NOT NULL REFERENCES managed_import_batches(id) ON DELETE CASCADE,
    file_id TEXT NOT NULL,
    job_id TEXT NOT NULL,
    safe_filename TEXT,
    started_at DATETIME NOT NULL,
    completed_at DATETIME NOT NULL,
    content_sha256 TEXT CHECK (
        content_sha256 IS NULL OR (
            length(content_sha256) = 64 AND
            content_sha256 NOT GLOB '*[^0-9a-f]*'
        )
    ),
    position INTEGER NOT NULL CHECK (position >= 0),
    PRIMARY KEY (batch_id, file_id),
    UNIQUE (batch_id, job_id)
);

WITH batch_counts AS (
    SELECT
        batches.id AS import_id,
        batches.created_at AS started_at,
        batches.updated_at AS completed_at,
        COUNT(jobs.id) AS total_count,
        COALESCE(SUM(CASE WHEN jobs.outcome = 'imported' OR (jobs.outcome IS NULL AND jobs.status = 'committed') THEN 1 ELSE 0 END), 0) AS imported_count,
        COALESCE(SUM(CASE WHEN jobs.outcome = 'rejected' THEN 1 ELSE 0 END), 0) AS rejected_count,
        COALESCE(SUM(CASE WHEN jobs.outcome = 'replaced' THEN 1 ELSE 0 END), 0) AS replaced_count,
        COALESCE(SUM(CASE WHEN jobs.outcome = 'not_attempted' THEN 1 ELSE 0 END), 0) AS not_attempted_count
    FROM managed_import_batches AS batches
    LEFT JOIN managed_import_jobs AS jobs ON jobs.batch_id = batches.id
    WHERE batches.status = 'completed'
    GROUP BY batches.id
), terminal_imports AS (
    SELECT
        import_id,
        started_at,
        completed_at,
        CASE
            WHEN imported_count + replaced_count = total_count THEN 'completed'
            WHEN imported_count + replaced_count > 0 THEN 'partially_completed'
            ELSE 'failed'
        END AS result_code,
        total_count,
        imported_count,
        rejected_count,
        total_count - imported_count - rejected_count - replaced_count - not_attempted_count AS failed_count,
        replaced_count,
        not_attempted_count,
        0 AS canceled_count
    FROM batch_counts
    UNION ALL
    SELECT
        jobs.id,
        jobs.created_at,
        jobs.updated_at,
        CASE WHEN jobs.status = 'committed' THEN 'completed' ELSE 'failed' END,
        1,
        CASE WHEN jobs.status = 'committed' THEN 1 ELSE 0 END,
        0,
        CASE WHEN jobs.status = 'failed' THEN 1 ELSE 0 END,
        0,
        0,
        0
    FROM managed_import_jobs AS jobs
    WHERE jobs.batch_id IS NULL AND jobs.status IN ('committed', 'failed')
)
INSERT INTO managed_import_history (
    import_id, started_at, completed_at, result_code, total_count, imported_count,
    rejected_count, failed_count, replaced_count, not_attempted_count, canceled_count
)
SELECT
    import_id, started_at, completed_at, result_code, total_count, imported_count,
    rejected_count, failed_count, replaced_count, not_attempted_count, canceled_count
FROM terminal_imports
ORDER BY completed_at DESC, import_id DESC
LIMIT 20;

INSERT INTO managed_import_history_files (
    import_id, file_id, job_id, safe_filename, started_at, completed_at, content_sha256,
    result_code, created_track_id, replaced_track_id, position
)
SELECT
    history.import_id,
    COALESCE(NULLIF(jobs.client_file_id, ''), jobs.id),
    jobs.id,
    jobs.original_filename,
    jobs.created_at,
    jobs.updated_at,
    jobs.content_sha256,
    CASE
        WHEN jobs.error_code IS NOT NULL AND (jobs.outcome IS NULL OR jobs.outcome NOT IN ('imported', 'replaced')) THEN jobs.error_code
        WHEN jobs.outcome IS NOT NULL THEN jobs.outcome
        WHEN jobs.status = 'committed' THEN 'imported'
        ELSE jobs.status
    END,
    CASE WHEN jobs.status = 'committed' AND (jobs.outcome IS NULL OR jobs.outcome != 'replaced') THEN jobs.track_id END,
    CASE WHEN jobs.outcome = 'replaced' THEN jobs.track_id END,
    CASE WHEN jobs.batch_id IS NULL THEN 0 ELSE jobs.batch_position END
FROM managed_import_history AS history
INNER JOIN managed_import_jobs AS jobs
    ON jobs.batch_id = history.import_id
    OR (jobs.batch_id IS NULL AND jobs.id = history.import_id);

-- +goose Down
DROP TABLE IF EXISTS managed_import_canceled_files;
DROP INDEX IF EXISTS idx_managed_import_history_files_import;
DROP TABLE IF EXISTS managed_import_history_files;
DROP INDEX IF EXISTS idx_managed_import_history_completed;
DROP TABLE IF EXISTS managed_import_history;
