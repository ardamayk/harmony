-- name: ListStations :many
SELECT * FROM radio_stations
WHERE user_id = ?
ORDER BY is_favorite DESC, position ASC, name COLLATE NOCASE;

-- name: GetStation :one
SELECT * FROM radio_stations WHERE user_id = ? AND id = ?;

-- name: GetStationByExternalID :one
SELECT * FROM radio_stations WHERE user_id = ? AND source = ? AND external_id = ?;

-- name: NextStationPosition :one
SELECT CAST(COALESCE(MAX(position) + 1, 0) AS INTEGER) AS next_position
FROM radio_stations WHERE user_id = ?;

-- name: InsertStation :exec
INSERT INTO radio_stations (
    id, user_id, name, stream_url, homepage_url, favicon_url, country, language, tags, codec, bitrate,
    source, external_id, is_favorite, position
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);

-- name: UpdateStation :exec
UPDATE radio_stations
SET name = ?, stream_url = ?, homepage_url = ?, favicon_url = ?, country = ?, language = ?,
    tags = ?, codec = ?, bitrate = ?, is_favorite = ?, position = ?, updated_at = CURRENT_TIMESTAMP
WHERE user_id = ? AND id = ?;

-- name: DeleteStation :execrows
DELETE FROM radio_stations WHERE user_id = ? AND id = ?;

-- name: UpdateNowPlaying :exec
UPDATE radio_stations
SET last_now_playing_title = ?, last_now_playing_artist = ?, last_now_playing_raw = ?,
    last_now_playing_updated_at = ?, updated_at = CURRENT_TIMESTAMP
WHERE id = ?;
