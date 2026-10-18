-- +goose Up
ALTER TABLE tracks ADD COLUMN is_pending_commit INTEGER NOT NULL DEFAULT 0 CHECK (is_pending_commit IN (0, 1));

CREATE VIEW visible_tracks AS
    SELECT * FROM tracks
    WHERE missing_at IS NULL AND is_pending_commit = 0;

CREATE VIEW visible_album_artwork AS
    SELECT artwork.*
    FROM album_artwork artwork
    JOIN tracks source_track ON source_track.id = artwork.source_track_id
    WHERE source_track.is_pending_commit = 0;

CREATE TABLE managed_import_commit_journal (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    track_id TEXT NOT NULL,
    phase TEXT NOT NULL CHECK (phase IN (
        'prepared', 'placed', 'verified', 'database_committed', 'cleaned', 'completed', 'rolled_back'
    )),
    staged_file_path TEXT NOT NULL,
    audio_file_path TEXT NOT NULL,
    artwork_file_path TEXT NOT NULL,
    audio_sha256 TEXT NOT NULL CHECK (length(audio_sha256) = 64),
    artwork_sha256 TEXT NOT NULL CHECK (length(artwork_sha256) = 64),
    artwork_created INTEGER NOT NULL DEFAULT 0 CHECK (artwork_created IN (0, 1)),
    recovery_reason TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX idx_managed_import_commit_journal_active_job
    ON managed_import_commit_journal(job_id)
    WHERE phase NOT IN ('completed', 'rolled_back');

CREATE INDEX idx_managed_import_commit_journal_phase
    ON managed_import_commit_journal(phase);

-- +goose Down
DROP TABLE IF EXISTS managed_import_commit_journal;
DROP VIEW IF EXISTS visible_album_artwork;
DROP VIEW IF EXISTS visible_tracks;
ALTER TABLE tracks DROP COLUMN is_pending_commit;
