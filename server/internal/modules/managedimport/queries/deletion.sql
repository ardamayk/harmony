-- Permanent Track Deletion and its crash-recovery journal.

-- name: GetDeletionTrack :one
SELECT tracks.title, tracks.album_id, tracks.revision,
    track_sources.source_kind, track_sources.file_path, track_sources.content_sha256,
    track_sources.size_bytes, track_sources.revision AS source_revision
FROM tracks JOIN track_sources ON track_sources.track_id = tracks.id
WHERE tracks.id = ? AND tracks.missing_at IS NULL;

-- name: ListTrackPlaylists :many
SELECT playlists.id, playlists.name
FROM playlist_tracks JOIN playlists ON playlists.id = playlist_tracks.playlist_id
WHERE playlist_tracks.track_id = ?
ORDER BY playlists.name, playlists.id;

-- name: ListTrackQueues :many
SELECT user_id, COUNT(*) AS item_count FROM playback_queue
WHERE track_id = ? GROUP BY user_id ORDER BY user_id;

-- name: GetFinalAlbumArtwork :one
-- Artwork that leaves with the Album when this Track is its last one.
SELECT album_artwork.file_path, album_artwork.content_sha256 FROM album_artwork
WHERE album_artwork.album_id = sqlc.arg(album_id)
    AND NOT EXISTS (SELECT 1 FROM tracks WHERE tracks.album_id = sqlc.arg(album_id) AND tracks.id != sqlc.arg(track_id));

-- name: InsertPermanentDeletion :exec
INSERT INTO permanent_track_deletions (track_id, file_path, content_sha256, artwork_file_path, artwork_content_sha256)
VALUES (
    sqlc.arg(track_id), sqlc.arg(file_path), sqlc.arg(content_sha256),
    NULLIF(CAST(sqlc.arg(artwork_file_path) AS TEXT), ''), NULLIF(CAST(sqlc.arg(artwork_content_sha256) AS TEXT), '')
);

-- name: HideTrack :exec
UPDATE tracks SET missing_at = CURRENT_TIMESTAMP WHERE id = ?;

-- name: UnhideTrack :exec
UPDATE tracks SET missing_at = NULL WHERE id = ?;

-- name: DeletePermanentDeletion :exec
DELETE FROM permanent_track_deletions WHERE track_id = ?;

-- name: ListPermanentDeletions :many
SELECT track_id, file_path, content_sha256,
    COALESCE(artwork_file_path, '') AS artwork_file_path, COALESCE(artwork_content_sha256, '') AS artwork_content_sha256
FROM permanent_track_deletions ORDER BY created_at, track_id;

-- name: GetPendingDeletion :one
SELECT tracks.album_id, permanent_track_deletions.file_path, permanent_track_deletions.content_sha256,
    COALESCE(permanent_track_deletions.artwork_file_path, '') AS artwork_file_path,
    COALESCE(permanent_track_deletions.artwork_content_sha256, '') AS artwork_content_sha256
FROM permanent_track_deletions JOIN tracks ON tracks.id = permanent_track_deletions.track_id
WHERE tracks.id = ?;

-- name: AdvanceQueueForUser :one
INSERT INTO playback_queue_state (user_id, revision, event_sequence) VALUES (?, 1, 1)
ON CONFLICT(user_id) DO UPDATE SET revision = revision + 1, event_sequence = event_sequence + 1
RETURNING CAST(revision AS TEXT) AS revision, CAST(event_sequence AS TEXT) AS event_sequence;

-- name: DeleteTrackQueueItems :exec
DELETE FROM playback_queue WHERE track_id = ?;

-- name: DeleteTrackPlaylistItems :exec
DELETE FROM playlist_tracks WHERE track_id = ?;

-- name: DeleteTrackJobs :exec
DELETE FROM managed_import_jobs WHERE track_id = ?;

-- name: DeleteTrack :exec
DELETE FROM tracks WHERE id = ?;

-- name: DeleteAlbumIfEmpty :exec
DELETE FROM albums WHERE albums.id = sqlc.arg(album_id)
    AND NOT EXISTS (SELECT 1 FROM tracks WHERE tracks.album_id = sqlc.arg(album_id));

-- name: DeleteArtistIfUnreferenced :exec
DELETE FROM artists WHERE artists.id = sqlc.arg(artist_id)
    AND NOT EXISTS (SELECT 1 FROM track_artists WHERE track_artists.artist_id = sqlc.arg(artist_id))
    AND NOT EXISTS (SELECT 1 FROM album_artists WHERE album_artists.artist_id = sqlc.arg(artist_id));

-- name: FirstOtherAlbumTrack :one
SELECT id FROM tracks WHERE album_id = ? AND id != ? ORDER BY id LIMIT 1;

-- name: DeleteAlbumArtwork :exec
DELETE FROM album_artwork WHERE album_id = ?;

-- name: MoveAlbumArtworkSource :exec
UPDATE album_artwork SET source_track_id = sqlc.arg(new_track_id)
WHERE album_id = sqlc.arg(album_id) AND source_track_id = sqlc.arg(old_track_id);

-- name: ListTrackAndAlbumArtistIDs :many
SELECT artist_id FROM track_artists WHERE track_id = ?
UNION SELECT artist_id FROM album_artists WHERE album_id = ?;

-- name: GetAlbumForDeletion :one
SELECT title, revision FROM albums WHERE id = ?;

-- name: ListAlbumDeletionTrackIDs :many
SELECT id FROM tracks
WHERE album_id = ? AND missing_at IS NULL AND is_pending_commit = 0
ORDER BY COALESCE(disc_no, 1), track_no, title_sort, id;
