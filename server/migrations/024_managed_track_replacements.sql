-- +goose Up
ALTER TABLE managed_import_jobs ADD COLUMN replace_track_id TEXT;

CREATE INDEX idx_managed_import_jobs_replace_track
    ON managed_import_jobs(replace_track_id)
    WHERE replace_track_id IS NOT NULL;

CREATE TABLE managed_track_replacements (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    track_id TEXT NOT NULL,
    phase TEXT NOT NULL CHECK (phase IN (
        'prepared', 'placed', 'verified', 'swapped', 'database_committed', 'completed', 'rolled_back'
    )),
    staged_file_path TEXT NOT NULL,
    pending_audio_path TEXT NOT NULL,
    audio_file_path TEXT NOT NULL,
    previous_audio_path TEXT NOT NULL,
    retired_audio_path TEXT NOT NULL,
    audio_sha256 TEXT NOT NULL CHECK (length(audio_sha256) = 64),
    previous_audio_sha256 TEXT NOT NULL CHECK (length(previous_audio_sha256) = 64),
    artwork_mode TEXT NOT NULL CHECK (artwork_mode IN ('existing', 'create', 'replace')),
    pending_artwork_path TEXT NOT NULL,
    artwork_file_path TEXT NOT NULL,
    previous_artwork_path TEXT NOT NULL,
    retired_artwork_path TEXT NOT NULL,
    artwork_sha256 TEXT NOT NULL CHECK (length(artwork_sha256) = 64),
    previous_artwork_sha256 TEXT NOT NULL,
    artwork_created INTEGER NOT NULL DEFAULT 0 CHECK (artwork_created IN (0, 1)),
    previous_album_id TEXT NOT NULL,
    recovery_reason TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX idx_managed_track_replacements_active_job
    ON managed_track_replacements(job_id)
    WHERE phase NOT IN ('completed', 'rolled_back');

CREATE UNIQUE INDEX idx_managed_track_replacements_active_track
    ON managed_track_replacements(track_id)
    WHERE phase NOT IN ('completed', 'rolled_back');

CREATE INDEX idx_managed_track_replacements_phase
    ON managed_track_replacements(phase);

-- +goose Down
DROP TABLE IF EXISTS managed_track_replacements;
DROP INDEX IF EXISTS idx_managed_import_jobs_replace_track;
ALTER TABLE managed_import_jobs DROP COLUMN replace_track_id;
