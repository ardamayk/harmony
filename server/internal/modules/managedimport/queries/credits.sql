-- Offline Artist credit repair (cmd/repair-artist-credits).

-- name: HasPendingFileOperations :one
SELECT EXISTS(SELECT 1 FROM managed_import_commit_journal WHERE phase NOT IN ('completed', 'rolled_back'))
    OR EXISTS(SELECT 1 FROM managed_track_replacements WHERE phase NOT IN ('completed', 'rolled_back'))
    OR EXISTS(SELECT 1 FROM permanent_track_deletions) AS has_pending;

-- name: ListRepairTracks :many
SELECT t.id, t.album_id, t.artist_name, t.file_path, t.revision, t.size_bytes,
    CAST(t.missing_at IS NOT NULL OR t.is_pending_commit = 1 AS INTEGER) AS hidden,
    COALESCE(s.id, '') AS source_id, COALESCE(s.source_kind, '') AS source_kind,
    COALESCE(s.file_path, '') AS source_path, COALESCE(s.content_sha256, '') AS source_hash,
    COALESCE(s.revision, 0) AS source_revision, COALESCE(s.size_bytes, 0) AS source_size,
    CAST((SELECT json_group_array(json_object('ID', id, 'Name', name, 'Position', position)) FROM (
        SELECT a.id, a.name, c.position FROM track_artists c JOIN artists a ON a.id = c.artist_id
        WHERE c.track_id = t.id ORDER BY c.position
    )) AS TEXT) AS credits
FROM tracks t LEFT JOIN track_sources s ON s.track_id = t.id
ORDER BY t.id;

-- name: ListRepairAlbums :many
SELECT b.id, b.title, b.artist_id, COALESCE(b.identity_key, '') AS identity_key, b.revision,
    CAST((SELECT json_group_array(json_object('ID', id, 'Name', name, 'Position', position)) FROM (
        SELECT a.id, a.name, c.position FROM album_artists c JOIN artists a ON a.id = c.artist_id
        WHERE c.album_id = b.id ORDER BY c.position
    )) AS TEXT) AS credits
FROM albums b
ORDER BY b.id;

-- name: SetTrackArtistName :exec
UPDATE tracks SET artist_name = ?, revision = revision + 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?;

-- name: DeleteAlbumArtists :exec
DELETE FROM album_artists WHERE album_id = ?;

-- name: SetAlbumArtistIdentity :exec
UPDATE albums SET artist_id = ?, identity_key = ?, revision = revision + 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?;

-- name: IsAlbumPrimaryArtist :one
SELECT EXISTS(SELECT 1 FROM albums WHERE artist_id = ?) AS is_referenced;
