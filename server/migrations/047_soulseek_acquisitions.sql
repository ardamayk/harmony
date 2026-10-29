-- +goose Up
-- Soulseek Acquisitions (ADR 0036): one row per acquisition of an
-- Acquisition Version, keyed by user. sources_json is the version's ordered
-- source list snapshotted at start, each source with its remote audio files
-- and cover images; source_index is the source being tried, and equals the
-- source count once every source was tried. progress_at_ms is when the
-- current source last made progress, for the stall rule. batch_id is the
-- acquisition's server-owned Managed Import Batch.
CREATE TABLE soulseek_acquisitions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    query TEXT NOT NULL,
    scope TEXT NOT NULL CHECK (scope IN ('album', 'track')),
    version_key TEXT NOT NULL,
    quality_json TEXT NOT NULL,
    sources_json TEXT NOT NULL,
    source_index INTEGER NOT NULL DEFAULT 0 CHECK (source_index >= 0),
    state TEXT NOT NULL CHECK (
        state IN ('downloading', 'importing', 'review', 'completed', 'failed', 'cancelled')
    ),
    batch_id TEXT,
    failure_reason TEXT,
    progress_at_ms INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_soulseek_acquisitions_user_state
    ON soulseek_acquisitions(user_id, state);

-- One remote file of an acquisition: an audio file, or the source folder's
-- cover image (kind artwork). name is the file name without its folder, by
-- which a later source's file replaces it; source_index, remote_filename and
-- size describe the source it is currently or was last taken from.
CREATE TABLE soulseek_acquisition_files (
    acquisition_id TEXT NOT NULL REFERENCES soulseek_acquisitions(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    kind TEXT NOT NULL CHECK (kind IN ('audio', 'artwork')),
    name TEXT NOT NULL,
    source_index INTEGER NOT NULL CHECK (source_index >= 0),
    remote_filename TEXT NOT NULL,
    size INTEGER NOT NULL CHECK (size >= 0),
    transfer_id TEXT,
    state TEXT NOT NULL CHECK (state IN ('queued', 'downloading', 'downloaded', 'staged', 'failed')),
    bytes_transferred INTEGER NOT NULL DEFAULT 0 CHECK (bytes_transferred >= 0),
    job_id TEXT,
    outcome TEXT CHECK (outcome IS NULL OR outcome IN ('previewed', 'rejected', 'attached', 'skipped')),
    PRIMARY KEY (acquisition_id, position)
);

-- +goose Down
DROP TABLE IF EXISTS soulseek_acquisition_files;
DROP INDEX IF EXISTS idx_soulseek_acquisitions_user_state;
DROP TABLE IF EXISTS soulseek_acquisitions;
