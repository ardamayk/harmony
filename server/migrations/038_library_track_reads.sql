-- +goose Up
-- One read shape for library Tracks: the visible Track joined with its Album
-- title and its managed source, which overrides format, size, and path.
CREATE VIEW library_track_reads AS
SELECT
    t.id,
    t.title,
    t.artist_name,
    t.album_id,
    al.title AS album_title,
    t.track_no,
    COALESCE(t.disc_no, 1) AS disc_no,
    t.track_total,
    t.disc_total,
    t.duration_ms,
    COALESCE(ts.source_format, t.format) AS format,
    COALESCE(ts.size_bytes, t.size_bytes) AS size_bytes,
    COALESCE(t.genre, '') AS genre,
    COALESCE(t.sample_rate_hz, 0) AS sample_rate_hz,
    COALESCE(t.bit_depth, 0) AS bit_depth,
    COALESCE(t.channel_count, 0) AS channel_count,
    COALESCE(t.bitrate_bps, 0) AS bitrate_bps,
    t.codec,
    t.container,
    t.sample_format,
    COALESCE(ts.file_path, t.file_path) AS file_path,
    t.replaygain_track_gain_db,
    t.replaygain_track_peak,
    t.replaygain_album_gain_db,
    t.replaygain_album_peak,
    t.title_sort,
    t.file_mtime,
    ts.content_sha256,
    t.identity_key,
    t.revision,
    t.created_at,
    t.updated_at
FROM visible_tracks t
INNER JOIN albums al ON al.id = t.album_id
LEFT JOIN track_sources ts ON ts.track_id = t.id;

-- +goose Down
DROP VIEW IF EXISTS library_track_reads;
