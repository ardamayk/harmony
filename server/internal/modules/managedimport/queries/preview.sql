-- Reads that classify an upload against the existing library and build the
-- Import Preview.

-- name: FindManagedTrackByHash :one
SELECT track_id FROM track_sources WHERE source_kind = 'managed' AND content_sha256 = ?;

-- name: FindTrackByContentHash :one
SELECT tracks.id FROM track_sources JOIN tracks ON tracks.id = track_sources.track_id
WHERE track_sources.content_sha256 = ?;

-- name: FindAlbumArtworkHash :one
SELECT album_artwork.content_sha256
FROM albums JOIN album_artwork ON album_artwork.album_id = albums.id
WHERE albums.identity_key = ?;

-- name: GetAlbumArtworkByKey :one
SELECT album_artwork.content_sha256, album_artwork.media_type, album_artwork.width, album_artwork.height
FROM album_artwork JOIN albums ON albums.id = album_artwork.album_id
WHERE albums.identity_key = ?;

-- name: AlbumRequiresDiscNumber :one
SELECT EXISTS (
    SELECT 1 FROM albums JOIN tracks ON tracks.album_id = albums.id
    WHERE albums.identity_key = ? AND tracks.missing_at IS NULL AND tracks.id != ?
        AND (tracks.disc_no > 1 OR tracks.disc_total > 1)
) AS requires_disc_number;

-- name: AlbumPositionTotalConflict :one
WITH params AS (
    SELECT CAST(sqlc.arg(disc_total) AS INTEGER) AS wanted_disc_total,
        CAST(sqlc.arg(disc_number) AS INTEGER) AS wanted_disc_number,
        CAST(sqlc.arg(track_total) AS INTEGER) AS wanted_track_total
),
matching_tracks AS (
    SELECT tracks.disc_no, tracks.disc_total, tracks.track_no, tracks.track_total
    FROM albums JOIN tracks ON tracks.album_id = albums.id
    WHERE albums.identity_key = sqlc.arg(album_key) AND tracks.missing_at IS NULL
        AND tracks.id != sqlc.arg(excluded_track_id)
)
SELECT
    EXISTS (
        SELECT 1 FROM matching_tracks m, params p
        WHERE p.wanted_disc_total > 0
            AND ((m.disc_total IS NOT NULL AND m.disc_total != p.wanted_disc_total) OR m.disc_no > p.wanted_disc_total)
    ) AS has_disc_conflict,
    EXISTS (
        SELECT 1 FROM matching_tracks m, params p
        WHERE m.disc_no = p.wanted_disc_number AND p.wanted_track_total > 0
            AND ((m.track_total IS NOT NULL AND m.track_total != p.wanted_track_total) OR m.track_no > p.wanted_track_total)
    ) AS has_track_conflict;

-- name: FindTracksAtPosition :many
SELECT tracks.id FROM tracks JOIN albums ON albums.id = tracks.album_id
WHERE tracks.missing_at IS NULL AND tracks.id != ? AND albums.identity_key = ?
    AND COALESCE(tracks.disc_no, 1) = ? AND tracks.track_no = ?
ORDER BY tracks.id;

-- name: GetDuplicateCandidate :one
SELECT tracks.id, tracks.title, albums.title AS album_title, COALESCE(tracks.disc_no, 1) AS disc_no,
    tracks.track_no, tracks.format, tracks.duration_ms, tracks.revision
FROM tracks JOIN albums ON albums.id = tracks.album_id
WHERE tracks.id = ?;

-- name: ListTrackArtistNames :many
SELECT artists.name FROM track_artists JOIN artists ON artists.id = track_artists.artist_id
WHERE track_artists.track_id = ? ORDER BY track_artists.position;

-- name: ListAlbumArtistNames :many
SELECT artists.name FROM album_artists JOIN artists ON artists.id = album_artists.artist_id
WHERE album_artists.album_id = ? ORDER BY album_artists.position;

-- name: ListTrackGenreNames :many
SELECT genres.name FROM track_genres JOIN genres ON genres.id = track_genres.genre_id
WHERE track_genres.track_id = ? ORDER BY track_genres.position;

-- name: MatchAlbums :many
-- An Album matches its exact key or any separate edition created from it.
WITH params AS (SELECT CAST(sqlc.arg(album_key) AS TEXT) AS album_key, CAST(sqlc.arg(edition_prefix) AS TEXT) AS prefix)
SELECT albums.id, albums.identity_key, COALESCE(albums.year, 0) AS year,
    EXISTS(SELECT 1 FROM album_artwork WHERE album_artwork.album_id = albums.id) AS has_artwork
FROM albums, params p
WHERE albums.identity_key = p.album_key OR substr(albums.identity_key, 1, length(p.prefix)) = p.prefix
ORDER BY albums.identity_key, albums.id;

-- name: ListVisibleAlbumTrackIDs :many
SELECT id FROM visible_tracks WHERE album_id = ? ORDER BY disc_no, track_no, id;

-- name: SaveImportArtwork :execrows
INSERT INTO managed_import_artwork (batch_id, id, job_id, album_key, media_type, width, height, content_sha256, data)
SELECT batches.id, sqlc.arg(id), NULLIF(CAST(sqlc.arg(job_id) AS TEXT), ''), sqlc.arg(album_key), sqlc.arg(media_type),
    sqlc.arg(width), sqlc.arg(height), sqlc.arg(content_sha256), sqlc.arg(data)
FROM managed_import_batches batches WHERE batches.id = sqlc.arg(batch_id) AND batches.status = 'uploading'
ON CONFLICT(batch_id, id) DO UPDATE SET media_type = excluded.media_type, width = excluded.width,
    height = excluded.height, content_sha256 = excluded.content_sha256, data = excluded.data;

-- name: ListImportArtworkOptions :many
SELECT id, COALESCE(job_id, '') AS job_id, media_type, content_sha256
FROM managed_import_artwork WHERE batch_id = ? AND album_key = ?
ORDER BY (job_id IS NULL) DESC, id;

-- name: GetImportArtwork :one
SELECT media_type, width, height, content_sha256, data FROM managed_import_artwork WHERE batch_id = ? AND id = ?;
