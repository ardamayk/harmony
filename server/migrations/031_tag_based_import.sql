-- +goose Up
DROP TABLE musicbrainz_recording_cache;
DROP TABLE acoustid_lookup_cache;
ALTER TABLE managed_import_history_files DROP COLUMN musicbrainz_recording_id;
ALTER TABLE managed_import_history_files DROP COLUMN acoustid_score;
ALTER TABLE managed_import_history_files DROP COLUMN metadata_source;
DROP INDEX idx_managed_import_jobs_musicbrainz_recording_id;
ALTER TABLE managed_import_jobs DROP COLUMN musicbrainz_recording_id;
ALTER TABLE managed_import_jobs DROP COLUMN identification_json;
ALTER TABLE managed_import_batches DROP COLUMN recording_identification;
DROP INDEX idx_tracks_musicbrainz_recording_id;
ALTER TABLE tracks DROP COLUMN musicbrainz_changed_fields;
ALTER TABLE tracks DROP COLUMN metadata_source;
ALTER TABLE tracks DROP COLUMN acoustid_score;
ALTER TABLE tracks DROP COLUMN isrc;
ALTER TABLE tracks DROP COLUMN musicbrainz_recording_id;

CREATE TABLE managed_import_commit_journal_new (
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
    artwork_sha256 TEXT NOT NULL CHECK (artwork_sha256 = '' OR length(artwork_sha256) = 64),
    artwork_created INTEGER NOT NULL DEFAULT 0 CHECK (artwork_created IN (0, 1)),
    recovery_reason TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO managed_import_commit_journal_new SELECT * FROM managed_import_commit_journal;
DROP TABLE managed_import_commit_journal;
ALTER TABLE managed_import_commit_journal_new RENAME TO managed_import_commit_journal;


CREATE UNIQUE INDEX idx_managed_import_commit_journal_active_job
    ON managed_import_commit_journal(job_id)
    WHERE phase NOT IN ('completed', 'rolled_back');

CREATE INDEX idx_managed_import_commit_journal_phase
    ON managed_import_commit_journal(phase);


CREATE TABLE managed_track_replacements_new (
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
    artwork_mode TEXT NOT NULL CHECK (artwork_mode IN ('none', 'existing', 'create', 'replace')),
    pending_artwork_path TEXT NOT NULL,
    artwork_file_path TEXT NOT NULL,
    previous_artwork_path TEXT NOT NULL,
    retired_artwork_path TEXT NOT NULL,
    artwork_sha256 TEXT NOT NULL CHECK (artwork_sha256 = '' OR length(artwork_sha256) = 64),
    previous_artwork_sha256 TEXT NOT NULL,
    artwork_created INTEGER NOT NULL DEFAULT 0 CHECK (artwork_created IN (0, 1)),
    previous_album_id TEXT NOT NULL,
    recovery_reason TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO managed_track_replacements_new SELECT * FROM managed_track_replacements;
DROP TABLE managed_track_replacements;
ALTER TABLE managed_track_replacements_new RENAME TO managed_track_replacements;


CREATE UNIQUE INDEX idx_managed_track_replacements_active_job
    ON managed_track_replacements(job_id)
    WHERE phase NOT IN ('completed', 'rolled_back');

CREATE UNIQUE INDEX idx_managed_track_replacements_active_track
    ON managed_track_replacements(track_id)
    WHERE phase NOT IN ('completed', 'rolled_back');

CREATE INDEX idx_managed_track_replacements_phase
    ON managed_track_replacements(phase);


ALTER TABLE managed_import_jobs ADD COLUMN import_plan_json TEXT;
CREATE TABLE managed_import_artwork (
    batch_id TEXT NOT NULL REFERENCES managed_import_batches(id) ON DELETE CASCADE,
    id TEXT NOT NULL,
    job_id TEXT REFERENCES managed_import_jobs(id) ON DELETE CASCADE,
    album_key TEXT NOT NULL,
    media_type TEXT NOT NULL,
    width INTEGER NOT NULL,
    height INTEGER NOT NULL,
    content_sha256 TEXT NOT NULL CHECK(length(content_sha256) = 64),
    data BLOB NOT NULL CHECK(length(data) <= 20971520),
    PRIMARY KEY(batch_id, id)
);

-- +goose Down
DROP TABLE managed_import_artwork;
ALTER TABLE managed_import_jobs DROP COLUMN import_plan_json;
-- Recording Identification (ADR 0017): Tracks remember which MusicBrainz
-- Recording they were identified as and how, Import Batches carry the
-- per-batch switch, and outbound AcoustID / MusicBrainz answers are cached
-- indefinitely so a re-upload never repeats a request.
ALTER TABLE tracks ADD COLUMN musicbrainz_recording_id TEXT;
ALTER TABLE tracks ADD COLUMN isrc TEXT;
ALTER TABLE tracks ADD COLUMN acoustid_score REAL CHECK (acoustid_score IS NULL OR (acoustid_score >= 0 AND acoustid_score <= 1));
ALTER TABLE tracks ADD COLUMN metadata_source TEXT NOT NULL DEFAULT 'file_tags' CHECK (metadata_source IN ('file_tags', 'musicbrainz'));
ALTER TABLE tracks ADD COLUMN musicbrainz_changed_fields TEXT NOT NULL DEFAULT '[]';
CREATE INDEX idx_tracks_musicbrainz_recording_id
    ON tracks(musicbrainz_recording_id)
    WHERE musicbrainz_recording_id IS NOT NULL;

ALTER TABLE managed_import_batches ADD COLUMN recording_identification INTEGER NOT NULL DEFAULT 0 CHECK (recording_identification IN (0, 1));

ALTER TABLE managed_import_jobs ADD COLUMN identification_json TEXT;
ALTER TABLE managed_import_jobs ADD COLUMN musicbrainz_recording_id TEXT;
CREATE INDEX idx_managed_import_jobs_musicbrainz_recording_id
    ON managed_import_jobs(musicbrainz_recording_id)
    WHERE musicbrainz_recording_id IS NOT NULL;

ALTER TABLE managed_import_history_files ADD COLUMN metadata_source TEXT NOT NULL DEFAULT 'file_tags' CHECK (metadata_source IN ('file_tags', 'musicbrainz'));
ALTER TABLE managed_import_history_files ADD COLUMN acoustid_score REAL;
ALTER TABLE managed_import_history_files ADD COLUMN musicbrainz_recording_id TEXT;

CREATE TABLE acoustid_lookup_cache (
    fingerprint_sha256 TEXT PRIMARY KEY CHECK (length(fingerprint_sha256) = 64),
    duration_seconds INTEGER NOT NULL CHECK (duration_seconds > 0),
    results_json TEXT NOT NULL,
    fetched_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE musicbrainz_recording_cache (
    recording_id TEXT PRIMARY KEY,
    recording_json TEXT NOT NULL,
    fetched_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
