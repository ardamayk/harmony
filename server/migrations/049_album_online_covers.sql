-- +goose Up
-- A sharper copy of an Album's cover from Deezer, for a cover too small to fill
-- the Album page (ADR 0027). source_sha256 names the Album's own cover it
-- stands in for, so replacing that cover drops it. Outcome and checked_at work
-- as for Artist photos.
CREATE TABLE IF NOT EXISTS album_online_covers (
    album_id TEXT PRIMARY KEY REFERENCES albums(id) ON DELETE CASCADE,
    source_sha256 TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('found', 'missing', 'failed')),
    media_type TEXT,
    data BLOB,
    checked_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- +goose Down
DROP TABLE IF EXISTS album_online_covers;
