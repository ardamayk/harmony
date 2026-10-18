-- +goose Up
-- Deezer's genres for an Album whose tags name none, shown on its page and
-- nowhere else (ADR 0027). genres is a JSON array of names. Outcome and
-- checked_at work as for Artist photos; a found list is kept for good.
CREATE TABLE IF NOT EXISTS album_online_genres (
    album_id TEXT PRIMARY KEY REFERENCES albums(id) ON DELETE CASCADE,
    outcome TEXT NOT NULL CHECK (outcome IN ('found', 'missing', 'failed')),
    genres TEXT,
    checked_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- +goose Down
DROP TABLE IF EXISTS album_online_genres;
