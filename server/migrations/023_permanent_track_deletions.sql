-- +goose Up
CREATE TABLE permanent_track_deletions (
    track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    content_sha256 TEXT NOT NULL CHECK (
        length(content_sha256) = 64 AND
        content_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    artwork_file_path TEXT,
    artwork_content_sha256 TEXT CHECK (
        artwork_content_sha256 IS NULL OR (
            length(artwork_content_sha256) = 64 AND
            artwork_content_sha256 NOT GLOB '*[^0-9a-f]*'
        )
    ),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((artwork_file_path IS NULL) = (artwork_content_sha256 IS NULL))
);

-- +goose Down
DROP TABLE IF EXISTS permanent_track_deletions;
