-- +goose Up
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

-- +goose Down
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
