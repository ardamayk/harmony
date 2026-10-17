-- +goose Up
-- Scaled-down Album covers, generated lazily on request so lists do not load
-- full-size artwork. source_sha256 identifies the cover each variant was made
-- from, so a replaced cover is regenerated without any cross-module hook.
CREATE TABLE IF NOT EXISTS album_cover_variants (
    album_id TEXT NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    size INTEGER NOT NULL,
    source_sha256 TEXT NOT NULL,
    media_type TEXT NOT NULL,
    data BLOB NOT NULL,
    generated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (album_id, size)
);

-- +goose Down
DROP TABLE IF EXISTS album_cover_variants;
