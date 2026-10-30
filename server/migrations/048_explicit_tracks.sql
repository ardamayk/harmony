-- +goose Up
-- Tracks Deezer marks as explicit, matched by title within their Album. A Track
-- is listed here or is not known to be explicit.
CREATE TABLE IF NOT EXISTS explicit_tracks (
    track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE
);

-- When each Album was last looked up for them. Outcome and checked_at work as
-- for Artist photos; a found Album is kept for good.
CREATE TABLE IF NOT EXISTS album_explicit_checks (
    album_id TEXT PRIMARY KEY REFERENCES albums(id) ON DELETE CASCADE,
    outcome TEXT NOT NULL CHECK (outcome IN ('found', 'missing', 'failed')),
    checked_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- +goose Down
DROP TABLE IF EXISTS album_explicit_checks;
DROP TABLE IF EXISTS explicit_tracks;
