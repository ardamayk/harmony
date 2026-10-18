-- +goose Up
ALTER TABLE playback_queue ADD COLUMN source TEXT NOT NULL DEFAULT '{"kind":"user"}';

-- +goose Down
ALTER TABLE playback_queue DROP COLUMN source;
