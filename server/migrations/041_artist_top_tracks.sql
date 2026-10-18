-- +goose Up
-- An Artist's most popular Track titles on Deezer, best first, as a JSON
-- array; the Artist page shows the ones in the library (ADR 0027). Outcome
-- and checked_at work as for photos and biographies; a found list is
-- refreshed weekly, since popularity moves.
CREATE TABLE IF NOT EXISTS artist_top_tracks (
    artist_id TEXT PRIMARY KEY REFERENCES artists(id) ON DELETE CASCADE,
    outcome TEXT NOT NULL CHECK (outcome IN ('found', 'missing', 'failed')),
    titles TEXT,
    checked_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- +goose Down
DROP TABLE IF EXISTS artist_top_tracks;
