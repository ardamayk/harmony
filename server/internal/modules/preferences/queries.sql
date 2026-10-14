-- name: GetPreferences :one
SELECT theme, layout_json, playback_json FROM user_preferences WHERE user_id = ?;

-- name: UpdatePreferences :exec
UPDATE user_preferences
SET theme = ?, layout_json = ?, playback_json = ?, updated_at = CURRENT_TIMESTAMP
WHERE user_id = ?;
