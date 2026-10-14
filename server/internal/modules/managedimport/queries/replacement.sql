-- Track Replacement: a new file takes over an existing Managed Track ID.

-- name: GetReplacementTarget :one
SELECT tracks.title, tracks.album_id, tracks.revision, tracks.track_no, tracks.track_total, tracks.disc_no,
    tracks.disc_total, tracks.duration_ms, tracks.sample_rate_hz, tracks.bit_depth, tracks.channel_count,
    tracks.bitrate_bps, tracks.codec, tracks.container, albums.title AS album_title, albums.year,
    albums.identity_key AS album_key, track_sources.source_kind, track_sources.file_path,
    track_sources.content_sha256, track_sources.size_bytes, track_sources.source_format,
    track_sources.revision AS source_revision, album_artwork.file_path AS artwork_path,
    album_artwork.content_sha256 AS artwork_sha256, album_artwork.media_type AS artwork_type,
    NOT EXISTS (
        SELECT 1 FROM tracks siblings
        WHERE siblings.album_id = tracks.album_id AND siblings.id != tracks.id AND siblings.missing_at IS NULL
    ) AS is_sole_track
FROM tracks
JOIN albums ON albums.id = tracks.album_id
JOIN track_sources ON track_sources.track_id = tracks.id
LEFT JOIN album_artwork ON album_artwork.album_id = tracks.album_id
WHERE tracks.id = ? AND tracks.missing_at IS NULL AND tracks.is_pending_commit = 0;

-- name: AlbumKeyExists :one
SELECT EXISTS (SELECT 1 FROM albums WHERE identity_key = ?) AS album_exists;

-- name: ArtistNameExists :one
SELECT EXISTS (SELECT 1 FROM artists WHERE name_normalized = ?) AS artist_exists;

-- name: GenreNameExists :one
SELECT EXISTS (SELECT 1 FROM genres WHERE name_normalized = ?) AS genre_exists;

-- name: ListReplacementArtistReferences :many
SELECT DISTINCT artists.id, artists.name, artists.name_normalized FROM artists
WHERE artists.id IN (
    SELECT track_artists.artist_id FROM track_artists WHERE track_artists.track_id = sqlc.arg(track_id)
    UNION SELECT album_artists.artist_id FROM album_artists
    WHERE album_artists.album_id = sqlc.arg(album_id) AND CAST(sqlc.arg(includes_album) AS INTEGER)
)
ORDER BY artists.name;

-- name: IsArtistReferencedElsewhere :one
SELECT EXISTS (
        SELECT 1 FROM track_artists
        WHERE track_artists.artist_id = sqlc.arg(artist_id) AND track_artists.track_id != sqlc.arg(track_id)
    ) OR EXISTS (
        SELECT 1 FROM album_artists
        WHERE album_artists.artist_id = sqlc.arg(artist_id)
            AND NOT (CAST(sqlc.arg(excludes_album) AS INTEGER) AND album_artists.album_id = sqlc.arg(album_id))
    ) AS is_referenced;

-- name: InsertReplacementJournal :exec
INSERT INTO managed_track_replacements (
    id, job_id, track_id, phase, staged_file_path, pending_audio_path, audio_file_path,
    previous_audio_path, retired_audio_path, audio_sha256, previous_audio_sha256, artwork_mode,
    pending_artwork_path, artwork_file_path, previous_artwork_path, retired_artwork_path,
    artwork_sha256, previous_artwork_sha256, artwork_created, previous_album_id
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);

-- name: AdvanceReplacementJournal :execrows
UPDATE managed_track_replacements SET phase = sqlc.arg(to_phase), updated_at = CURRENT_TIMESTAMP
WHERE id = sqlc.arg(id) AND phase = sqlc.arg(from_phase);

-- name: MarkReplacementArtworkCreated :execrows
UPDATE managed_track_replacements SET artwork_created = 1, updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND phase = 'prepared';

-- name: RollbackReplacementJournal :execrows
UPDATE managed_track_replacements SET phase = 'rolled_back', recovery_reason = ?, updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND phase NOT IN ('database_committed', 'completed', 'rolled_back');

-- name: RecordReplacementRecoveryReason :execrows
UPDATE managed_track_replacements SET recovery_reason = ?, updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND phase NOT IN ('completed', 'rolled_back');

-- name: CompleteReplacementJournal :execrows
UPDATE managed_track_replacements
SET phase = 'completed', recovery_reason = COALESCE(recovery_reason, 'replacement completed'), updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND phase = 'database_committed';

-- name: ListIncompleteReplacementJournals :many
SELECT * FROM managed_track_replacements WHERE phase NOT IN ('completed', 'rolled_back') ORDER BY created_at, id;

-- name: FindIncompleteReplacementJournal :one
SELECT * FROM managed_track_replacements WHERE job_id = ? AND phase NOT IN ('completed', 'rolled_back');

-- name: GetReplacementJournal :one
SELECT * FROM managed_track_replacements WHERE id = ?;

-- name: LatestReplacementJournalForJob :one
SELECT * FROM managed_track_replacements WHERE job_id = ? ORDER BY created_at DESC, id DESC LIMIT 1;

-- name: UpdateReplacedTrack :execrows
UPDATE tracks SET album_id = ?, title = ?, title_sort = ?, artist_name = ?, track_no = ?, duration_ms = ?,
    format = ?, size_bytes = ?, file_path = ?, file_mtime = ?, genre = ?, sample_rate_hz = ?, bit_depth = ?,
    disc_no = ?, track_total = ?, disc_total = ?, channel_count = ?, bitrate_bps = ?, codec = ?, container = ?,
    replaygain_track_gain_db = ?, replaygain_track_peak = ?, replaygain_album_gain_db = ?,
    replaygain_album_peak = ?, identity_key = ?, revision = revision + 1, updated_at = CURRENT_TIMESTAMP
WHERE id = sqlc.arg(track_id) AND revision = sqlc.arg(expected_revision) AND missing_at IS NULL;

-- name: UpdateReplacedTrackSource :execrows
UPDATE track_sources SET file_path = ?, content_sha256 = ?, source_format = ?, size_bytes = ?,
    revision = revision + 1, updated_at = CURRENT_TIMESTAMP
WHERE track_id = sqlc.arg(track_id) AND revision = sqlc.arg(expected_revision) AND source_kind = 'managed';

-- name: DeleteTrackArtists :exec
DELETE FROM track_artists WHERE track_id = ?;

-- name: DeleteTrackGenres :exec
DELETE FROM track_genres WHERE track_id = ?;

-- name: ReplaceAlbumArtwork :execrows
UPDATE album_artwork SET source_track_id = ?, content_sha256 = ?, media_type = ?, width = ?, height = ?,
    encoded_size_bytes = ?, file_path = ?, revision = revision + 1, updated_at = CURRENT_TIMESTAMP
WHERE album_id = ?;
