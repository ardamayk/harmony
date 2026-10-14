-- Writing a committed Managed Track and its commit journal.

-- name: GetArtistIDByName :one
SELECT id FROM artists WHERE name_normalized = ?;

-- name: InsertArtist :exec
INSERT INTO artists (id, name, name_sort, name_normalized) VALUES (?, ?, ?, ?);

-- name: GetAlbumIDByKey :one
SELECT id FROM albums WHERE identity_key = ?;

-- name: ResolveAlbumByKey :one
SELECT albums.id, album_artwork.file_path, album_artwork.content_sha256
FROM albums
LEFT JOIN album_artwork ON album_artwork.album_id = albums.id
WHERE albums.identity_key = ?;

-- name: InsertAlbum :exec
INSERT INTO albums (id, artist_id, title, title_sort, year, release_date, genres, identity_key)
VALUES (?, ?, ?, ?, ?, ?, ?, ?);

-- name: InsertTrack :exec
INSERT INTO tracks (
    id, album_id, title, title_sort, artist_name, track_no, duration_ms, format,
    size_bytes, file_path, file_mtime, genre, sample_rate_hz, bit_depth, disc_no,
    track_total, disc_total, channel_count, bitrate_bps, codec, container,
    replaygain_track_gain_db, replaygain_track_peak, replaygain_album_gain_db,
    replaygain_album_peak, identity_key, lyrics
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);

-- name: InsertTrackSource :exec
INSERT INTO track_sources (id, track_id, source_kind, file_path, content_sha256, source_format, size_bytes)
VALUES (?, ?, 'managed', ?, ?, ?, ?);

-- name: GetGenreIDByName :one
SELECT id FROM genres WHERE name_normalized = ?;

-- name: InsertGenre :exec
INSERT INTO genres (id, name, name_normalized) VALUES (?, ?, ?);

-- name: InsertTrackGenre :exec
INSERT INTO track_genres (track_id, genre_id, position) VALUES (?, ?, ?);

-- name: InsertAlbumArtistCredit :exec
INSERT OR IGNORE INTO album_artists (album_id, artist_id, position) VALUES (?, ?, ?);

-- name: InsertTrackArtistCredit :exec
INSERT INTO track_artists (track_id, artist_id, position) VALUES (?, ?, ?);

-- name: InsertAlbumArtwork :exec
INSERT INTO album_artwork (
    id, album_id, source_track_id, content_sha256, media_type, width, height, encoded_size_bytes, file_path
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?);

-- name: SetTrackPendingCommit :exec
UPDATE tracks SET is_pending_commit = 1 WHERE id = ?;

-- name: PublishPendingTrack :execrows
UPDATE tracks SET is_pending_commit = 0, updated_at = CURRENT_TIMESTAMP WHERE id = ? AND is_pending_commit = 1;

-- name: DeleteArtworkFromSourceTrack :exec
DELETE FROM album_artwork WHERE source_track_id = ?;

-- name: DeletePendingTrack :exec
DELETE FROM tracks WHERE id = ? AND is_pending_commit = 1;

-- name: InsertCommitJournal :exec
INSERT INTO managed_import_commit_journal (
    id, job_id, track_id, phase, staged_file_path, audio_file_path, artwork_file_path,
    audio_sha256, artwork_sha256, artwork_created
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?);

-- name: MarkCommitPlaced :execrows
UPDATE managed_import_commit_journal
SET phase = 'placed', artwork_created = ?, updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND phase = 'prepared';

-- name: MarkCommitArtworkCreated :execrows
UPDATE managed_import_commit_journal SET artwork_created = 1, updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND phase = 'prepared';

-- name: AdvanceCommitJournal :execrows
UPDATE managed_import_commit_journal SET phase = sqlc.arg(to_phase), updated_at = CURRENT_TIMESTAMP
WHERE id = sqlc.arg(id) AND phase = sqlc.arg(from_phase);

-- name: UpdateOpenCommitJournalPhase :execrows
UPDATE managed_import_commit_journal SET phase = ?, updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND phase NOT IN ('completed', 'rolled_back');

-- name: ResetOpenCommitJournal :execrows
UPDATE managed_import_commit_journal SET phase = ?, recovery_reason = ?, updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND phase NOT IN ('completed', 'rolled_back');

-- name: RecordCommitRecoveryReason :execrows
UPDATE managed_import_commit_journal SET recovery_reason = ?, updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND phase NOT IN ('completed', 'rolled_back');

-- name: CompleteCommitJournal :execrows
UPDATE managed_import_commit_journal
SET phase = 'completed', recovery_reason = COALESCE(recovery_reason, 'commit completed'), updated_at = CURRENT_TIMESTAMP
WHERE id = ? AND phase = 'cleaned';

-- name: ListIncompleteCommitJournals :many
SELECT * FROM managed_import_commit_journal
WHERE phase NOT IN ('completed', 'rolled_back')
ORDER BY created_at, id;

-- name: GetCommitJournal :one
SELECT * FROM managed_import_commit_journal WHERE id = ?;

-- name: FindIncompleteCommitJournal :one
SELECT * FROM managed_import_commit_journal WHERE job_id = ? AND phase NOT IN ('completed', 'rolled_back');
