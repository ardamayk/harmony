-- +goose Up
CREATE TABLE legacy_migration_copies (
    source_track_id TEXT PRIMARY KEY REFERENCES tracks(id),
    pending_track_id TEXT NOT NULL UNIQUE,
    pending_album_id TEXT NOT NULL,
    pending_album_artist_id TEXT NOT NULL,
    source_file_path TEXT NOT NULL UNIQUE,
    pending_audio_path TEXT NOT NULL UNIQUE,
    pending_artwork_path TEXT NOT NULL,
    source_sha256 TEXT NOT NULL CHECK (
        length(source_sha256) = 64 AND source_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    pending_sha256 TEXT NOT NULL CHECK (
        length(pending_sha256) = 64 AND pending_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    artwork_sha256 TEXT NOT NULL CHECK (
        length(artwork_sha256) = 64 AND artwork_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    inspection_json TEXT NOT NULL CHECK (json_valid(inspection_json)),
    status TEXT NOT NULL CHECK (status IN ('prepared', 'verified', 'failed')),
    recovery_reason TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (source_sha256 = pending_sha256)
);

CREATE INDEX idx_legacy_migration_copies_status
    ON legacy_migration_copies(status);

-- +goose Down
DROP TABLE IF EXISTS legacy_migration_copies;
