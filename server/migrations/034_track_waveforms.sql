-- +goose Up
-- Cached waveform peaks per Track, generated lazily from the managed file.
-- source_size_bytes and source_modified_at identify the file the peaks were
-- read from, so a replaced file is regenerated without any cross-module hook.
CREATE TABLE IF NOT EXISTS track_waveforms (
    track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    peak_count INTEGER NOT NULL,
    peaks BLOB NOT NULL,
    source_size_bytes INTEGER NOT NULL,
    source_modified_at INTEGER NOT NULL,
    generated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- +goose Down
DROP TABLE IF EXISTS track_waveforms;
