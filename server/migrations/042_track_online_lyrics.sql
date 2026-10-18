-- +goose Up
-- Lyrics looked up on LRCLIB for Tracks whose files carry none (ADR 0029).
-- A row without lyrics records that the lookup found nothing ('missing') or
-- could not run ('failed'); checked_at says when to try again. An
-- instrumental Track is 'found' with empty lyrics.
CREATE TABLE IF NOT EXISTS track_online_lyrics (
    track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    outcome TEXT NOT NULL CHECK (outcome IN ('found', 'missing', 'failed')),
    lyrics TEXT,
    checked_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- +goose Down
DROP TABLE IF EXISTS track_online_lyrics;
