-- +goose Up
-- Managed Import is the authoritative ingestion path and the legacy startup
-- scanner is retired, so scan job bookkeeping is no longer written or read.
-- The deprecated /api/v1/library/scan/status endpoint answers with a constant
-- idle status and does not depend on this table.
DROP TABLE IF EXISTS scan_jobs;

-- +goose Down
CREATE TABLE IF NOT EXISTS scan_jobs (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL DEFAULT 'idle',
    started_at DATETIME,
    finished_at DATETIME,
    scanned INTEGER NOT NULL DEFAULT 0,
    added INTEGER NOT NULL DEFAULT 0,
    updated INTEGER NOT NULL DEFAULT 0,
    removed INTEGER NOT NULL DEFAULT 0,
    error_message TEXT
);
