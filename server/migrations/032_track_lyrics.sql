-- +goose Up
ALTER TABLE tracks ADD COLUMN lyrics TEXT NOT NULL DEFAULT '';

-- +goose Down
ALTER TABLE tracks DROP COLUMN lyrics;
