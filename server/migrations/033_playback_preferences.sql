-- +goose Up
ALTER TABLE user_preferences ADD COLUMN playback_json TEXT NOT NULL DEFAULT '{}';

-- +goose Down
ALTER TABLE user_preferences DROP COLUMN playback_json;
