-- name: EnsureDefaultPlaylist :exec
INSERT OR IGNORE INTO playlists (id, user_id, name, is_default)
VALUES (?, ?, ?, 1);

-- name: InsertPlaylist :exec
INSERT INTO playlists (id, user_id, name, is_default)
VALUES (?, ?, ?, 0);

-- name: ListPlaylists :many
SELECT p.id, p.name, p.is_default, COUNT(pt.track_id) AS track_count
FROM playlists p
LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
WHERE p.user_id = ?
GROUP BY p.id, p.name, p.is_default
ORDER BY p.is_default DESC, p.name COLLATE NOCASE;

-- name: GetPlaylist :one
SELECT p.id, p.name, p.is_default, COUNT(pt.track_id) AS track_count
FROM playlists p
LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
WHERE p.user_id = ? AND p.id = ?
GROUP BY p.id, p.name, p.is_default;

-- name: GetPlaylistByName :one
SELECT p.id, p.name, p.is_default, COUNT(pt.track_id) AS track_count
FROM playlists p
LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
WHERE p.user_id = ? AND p.name = ?
GROUP BY p.id, p.name, p.is_default;

-- name: ListPlaylistTrackIDs :many
SELECT track_id FROM playlist_tracks
WHERE playlist_id = ?
ORDER BY position;

-- name: AppendPlaylistTrack :exec
INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, position)
SELECT ?1, ?2, COALESCE(MAX(position) + 1, 0)
FROM playlist_tracks WHERE playlist_id = ?1;

-- name: DeletePlaylistTrack :exec
DELETE FROM playlist_tracks WHERE playlist_id = ? AND track_id = ?;

-- name: CountPlaylistTracks :one
SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = ?;

-- name: DeleteUserPlaylist :exec
DELETE FROM playlists WHERE id = ? AND user_id = ? AND is_default = 0;
