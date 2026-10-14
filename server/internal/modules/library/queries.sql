-- Artists are listed through their Album Artist credits only. Track-level
-- credits (featured Artists) deliberately do not surface an Artist.

-- name: CountArtists :one
WITH active_artist_albums AS (
    SELECT album_artists.artist_id, album_artists.album_id FROM album_artists
    WHERE EXISTS (SELECT 1 FROM visible_tracks WHERE visible_tracks.album_id = album_artists.album_id)
)
SELECT COUNT(DISTINCT a.id) FROM artists a
INNER JOIN active_artist_albums credits ON credits.artist_id = a.id
WHERE CAST(sqlc.arg(q) AS TEXT) = '' OR a.name LIKE '%' || CAST(sqlc.arg(q) AS TEXT) || '%';

-- name: ListArtists :many
-- LIKE is case-insensitive for ASCII, so exact-name precedence is too.
WITH params AS (SELECT CAST(sqlc.arg(q) AS TEXT) AS q),
active_artist_albums AS (
    SELECT album_artists.artist_id, album_artists.album_id FROM album_artists
    WHERE EXISTS (SELECT 1 FROM visible_tracks WHERE visible_tracks.album_id = album_artists.album_id)
)
SELECT a.id, a.name, COUNT(credits.album_id) AS album_count
FROM params p, artists a
INNER JOIN active_artist_albums credits ON credits.artist_id = a.id
WHERE p.q = '' OR a.name LIKE '%' || p.q || '%'
GROUP BY a.id, a.name
ORDER BY (p.q != '' AND a.name = p.q COLLATE NOCASE) DESC, a.name_sort, a.id
LIMIT sqlc.arg(row_limit) OFFSET sqlc.arg(row_offset);

-- Only Album Artist credits place an Album under an Artist; a guest credited
-- on a single Track does not own the Album.

-- name: CountAlbums :one
SELECT COUNT(DISTINCT al.id) FROM albums al
INNER JOIN visible_tracks t ON t.album_id = al.id
WHERE (CAST(sqlc.arg(artist_id) AS TEXT) = '' OR EXISTS (
        SELECT 1 FROM album_artists credit WHERE credit.album_id = al.id AND credit.artist_id = CAST(sqlc.arg(artist_id) AS TEXT)))
  AND (CAST(sqlc.arg(q) AS TEXT) = '' OR al.title LIKE '%' || CAST(sqlc.arg(q) AS TEXT) || '%' OR EXISTS (
        SELECT 1 FROM album_artists credit
        INNER JOIN artists credited ON credited.id = credit.artist_id
        WHERE credit.album_id = al.id AND credited.name LIKE '%' || CAST(sqlc.arg(q) AS TEXT) || '%'));

-- name: ListAlbums :many
-- An Artist's Albums are newest first; the full list is alphabetical.
WITH params AS (SELECT CAST(sqlc.arg(artist_id) AS TEXT) AS artist_id, CAST(sqlc.arg(q) AS TEXT) AS q)
SELECT al.id, al.title,
    COALESCE(primary_artist.id, legacy_artist.id) AS artist_id,
    COALESCE(primary_artist.name, legacy_artist.name) AS artist_name,
    al.year, al.release_date, al.genres, COUNT(t.id) AS track_count
FROM params p, albums al
INNER JOIN artists legacy_artist ON legacy_artist.id = al.artist_id
LEFT JOIN album_artists primary_credit ON primary_credit.album_id = al.id AND primary_credit.position = 0
LEFT JOIN artists primary_artist ON primary_artist.id = primary_credit.artist_id
INNER JOIN visible_tracks t ON t.album_id = al.id
WHERE (p.artist_id = '' OR EXISTS (
        SELECT 1 FROM album_artists credit WHERE credit.album_id = al.id AND credit.artist_id = p.artist_id))
  AND (p.q = '' OR al.title LIKE '%' || p.q || '%' OR EXISTS (
        SELECT 1 FROM album_artists credit
        INNER JOIN artists credited ON credited.id = credit.artist_id
        WHERE credit.album_id = al.id AND credited.name LIKE '%' || p.q || '%'))
GROUP BY al.id, al.title, primary_artist.id, primary_artist.name,
    legacy_artist.id, legacy_artist.name, al.year, al.release_date, al.genres
ORDER BY
    CASE WHEN p.artist_id != '' THEN al.year IS NULL END,
    CASE WHEN p.artist_id != '' THEN al.year END DESC,
    al.title_sort
LIMIT sqlc.arg(row_limit) OFFSET sqlc.arg(row_offset);

-- name: GetAlbum :one
SELECT al.id, al.title,
    COALESCE(primary_artist.id, legacy_artist.id) AS artist_id,
    COALESCE(primary_artist.name, legacy_artist.name) AS artist_name,
    al.year, al.release_date, al.genres
FROM albums al
INNER JOIN artists legacy_artist ON legacy_artist.id = al.artist_id
LEFT JOIN album_artists primary_credit ON primary_credit.album_id = al.id AND primary_credit.position = 0
LEFT JOIN artists primary_artist ON primary_artist.id = primary_credit.artist_id
WHERE al.id = ?;

-- name: ListAlbumTracks :many
SELECT * FROM library_track_reads
WHERE album_id = ?
ORDER BY disc_no, COALESCE(track_no, 9999), title_sort, id;

-- name: CountTracks :one
SELECT COUNT(*) FROM library_track_reads t
WHERE (CAST(sqlc.arg(artist_id) AS TEXT) = '' OR EXISTS (
        SELECT 1 FROM track_artists credit WHERE credit.track_id = t.id AND credit.artist_id = CAST(sqlc.arg(artist_id) AS TEXT)
    ) OR EXISTS (
        SELECT 1 FROM album_artists credit WHERE credit.album_id = t.album_id AND credit.artist_id = CAST(sqlc.arg(artist_id) AS TEXT)))
  AND (CAST(sqlc.arg(q) AS TEXT) = '' OR t.title LIKE '%' || CAST(sqlc.arg(q) AS TEXT) || '%' OR t.album_title LIKE '%' || CAST(sqlc.arg(q) AS TEXT) || '%' OR EXISTS (
        SELECT 1 FROM track_artists credit
        INNER JOIN artists credited ON credited.id = credit.artist_id
        WHERE credit.track_id = t.id AND credited.name LIKE '%' || CAST(sqlc.arg(q) AS TEXT) || '%'
    ) OR EXISTS (
        SELECT 1 FROM track_genres relation
        INNER JOIN genres genre ON genre.id = relation.genre_id
        WHERE relation.track_id = t.id AND genre.name LIKE '%' || CAST(sqlc.arg(q) AS TEXT) || '%'));

-- name: ListTracks :many
SELECT t.* FROM library_track_reads t
WHERE (CAST(sqlc.arg(artist_id) AS TEXT) = '' OR EXISTS (
        SELECT 1 FROM track_artists credit WHERE credit.track_id = t.id AND credit.artist_id = CAST(sqlc.arg(artist_id) AS TEXT)
    ) OR EXISTS (
        SELECT 1 FROM album_artists credit WHERE credit.album_id = t.album_id AND credit.artist_id = CAST(sqlc.arg(artist_id) AS TEXT)))
  AND (CAST(sqlc.arg(q) AS TEXT) = '' OR t.title LIKE '%' || CAST(sqlc.arg(q) AS TEXT) || '%' OR t.album_title LIKE '%' || CAST(sqlc.arg(q) AS TEXT) || '%' OR EXISTS (
        SELECT 1 FROM track_artists credit
        INNER JOIN artists credited ON credited.id = credit.artist_id
        WHERE credit.track_id = t.id AND credited.name LIKE '%' || CAST(sqlc.arg(q) AS TEXT) || '%'
    ) OR EXISTS (
        SELECT 1 FROM track_genres relation
        INNER JOIN genres genre ON genre.id = relation.genre_id
        WHERE relation.track_id = t.id AND genre.name LIKE '%' || CAST(sqlc.arg(q) AS TEXT) || '%'))
ORDER BY t.title_sort
LIMIT sqlc.arg(row_limit) OFFSET sqlc.arg(row_offset);

-- name: GetTrack :one
SELECT * FROM library_track_reads WHERE id = ?;

-- name: GetTrackLyrics :one
SELECT lyrics FROM tracks WHERE id = ?;

-- name: GetTrackPlaybackSource :one
SELECT file_path, duration_ms FROM library_track_reads WHERE id = ?;

-- name: GetVisibleAlbumArtwork :one
SELECT media_type, file_path FROM visible_album_artwork WHERE album_id = ?;

-- name: GetEmbeddedAlbumCover :one
SELECT cover_mime, cover_data FROM albums WHERE id = ?;

-- name: ListAlbumArtistCredits :many
SELECT credits.album_id, artists.id, artists.name
FROM album_artists credits
INNER JOIN artists ON artists.id = credits.artist_id
WHERE credits.album_id IN (sqlc.slice(album_ids))
ORDER BY credits.album_id, credits.position;

-- name: ListTrackArtistCredits :many
SELECT credits.track_id, artists.id, artists.name
FROM track_artists credits
INNER JOIN artists ON artists.id = credits.artist_id
WHERE credits.track_id IN (sqlc.slice(track_ids))
ORDER BY credits.track_id, credits.position;

-- name: ListAlbumGenres :many
SELECT tracks.album_id, genres.id, genres.name
FROM visible_tracks tracks
INNER JOIN track_genres relations ON relations.track_id = tracks.id
INNER JOIN genres ON genres.id = relations.genre_id
WHERE tracks.album_id IN (sqlc.slice(album_ids))
GROUP BY tracks.album_id, genres.id, genres.name
ORDER BY tracks.album_id, MIN(relations.position), genres.name;

-- name: ListTrackGenres :many
SELECT relations.track_id, genres.id, genres.name
FROM track_genres relations
INNER JOIN genres ON genres.id = relations.genre_id
WHERE relations.track_id IN (sqlc.slice(track_ids))
ORDER BY relations.track_id, relations.position;

-- name: ListAlbumReleaseIdentifiers :many
SELECT album_id, scheme, value FROM album_release_identifiers
WHERE album_id IN (sqlc.slice(album_ids))
ORDER BY album_id, scheme;

-- name: ListAlbumArtworkMetadata :many
SELECT album_id, source_track_id, content_sha256, media_type, width, height, encoded_size_bytes
FROM visible_album_artwork
WHERE album_id IN (sqlc.slice(album_ids));

-- name: GetTrackWaveform :one
SELECT peaks, source_size_bytes, source_modified_at FROM track_waveforms WHERE track_id = ?;

-- name: PutTrackWaveform :exec
INSERT INTO track_waveforms (track_id, peak_count, peaks, source_size_bytes, source_modified_at, generated_at)
VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
ON CONFLICT(track_id) DO UPDATE SET
    peak_count = excluded.peak_count,
    peaks = excluded.peaks,
    source_size_bytes = excluded.source_size_bytes,
    source_modified_at = excluded.source_modified_at,
    generated_at = CURRENT_TIMESTAMP;
