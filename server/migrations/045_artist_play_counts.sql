-- +goose Up
-- Last.fm play counts of an Artist's most played Tracks, looked up only
-- when the operator set a Last.fm API key (ADR 0030). counts is a JSON
-- object from a Track's folded title (no case or extra spaces, versions
-- kept) to its play count. Outcome and checked_at work as for photos; a found list is
-- refreshed weekly.
CREATE TABLE IF NOT EXISTS artist_play_counts (
    artist_id TEXT PRIMARY KEY REFERENCES artists(id) ON DELETE CASCADE,
    outcome TEXT NOT NULL CHECK (outcome IN ('found', 'missing', 'failed')),
    counts TEXT,
    checked_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- +goose Down
DROP TABLE IF EXISTS artist_play_counts;
