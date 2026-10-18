-- +goose Up
-- Artist photos (Deezer) and biographies (Wikipedia), looked up online on
-- first request (ADR 0027). A row without data records that the lookup found
-- nothing ('missing') or could not run ('failed'), so it is not repeated on
-- every request; checked_at says when to try again.
CREATE TABLE IF NOT EXISTS artist_photos (
    artist_id TEXT PRIMARY KEY REFERENCES artists(id) ON DELETE CASCADE,
    outcome TEXT NOT NULL CHECK (outcome IN ('found', 'missing', 'failed')),
    media_type TEXT,
    data BLOB,
    checked_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS artist_biographies (
    artist_id TEXT PRIMARY KEY REFERENCES artists(id) ON DELETE CASCADE,
    outcome TEXT NOT NULL CHECK (outcome IN ('found', 'missing', 'failed')),
    text TEXT,
    source_url TEXT,
    checked_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- +goose Down
DROP TABLE IF EXISTS artist_biographies;
DROP TABLE IF EXISTS artist_photos;
