-- +goose Up
UPDATE user_preferences
SET theme = '{"mode":"system","preset":"noir-red"}'
WHERE theme = '{"mode":"system","preset":"earthly"}';

-- +goose Down
UPDATE user_preferences
SET theme = '{"mode":"system","preset":"earthly"}'
WHERE theme = '{"mode":"system","preset":"noir-red"}';
