-- +goose Up
-- Managed Import is the only library model (ADR 0016). The Library Migration
-- that carried legacy rows into Managed Storage has run to completion, so the
-- remaining hidden Legacy Track rows and every legacy-only table go away.

-- Every Legacy Track row goes, hidden by the migration cutover or not: the
-- Music Server no longer locates legacy files, so none of them is playable.
-- Cascades clear track_sources, track_artists, track_genres, playlist_tracks
-- and playback_queue.
DELETE FROM tracks WHERE id IN (
    SELECT track_id FROM track_sources WHERE source_kind = 'legacy'
);
DELETE FROM tracks WHERE id NOT IN (SELECT track_id FROM track_sources);

-- Albums, artists and genres that only legacy rows referenced.
DELETE FROM album_artwork WHERE album_id NOT IN (SELECT DISTINCT album_id FROM tracks);
DELETE FROM album_release_identifiers WHERE album_id NOT IN (SELECT DISTINCT album_id FROM tracks);
DELETE FROM album_artists WHERE album_id NOT IN (SELECT DISTINCT album_id FROM tracks);
DELETE FROM albums WHERE id NOT IN (SELECT DISTINCT album_id FROM tracks);
DELETE FROM artists WHERE id NOT IN (SELECT artist_id FROM albums)
    AND id NOT IN (SELECT artist_id FROM album_artists)
    AND id NOT IN (SELECT artist_id FROM track_artists);
DELETE FROM genres WHERE id NOT IN (SELECT genre_id FROM track_genres);

DROP TRIGGER IF EXISTS validate_legacy_album_artwork_source_insert;
DROP TRIGGER IF EXISTS validate_legacy_album_artwork_source_update;
DROP TRIGGER IF EXISTS clear_legacy_album_artwork_source_track_album_update;
DROP TABLE IF EXISTS legacy_album_artwork_metadata;
DROP TABLE IF EXISTS legacy_album_genres;
DROP TABLE IF EXISTS legacy_track_identities;
DROP TABLE IF EXISTS legacy_album_identities;
DROP TABLE IF EXISTS legacy_artist_identities;
DROP TABLE IF EXISTS legacy_library_backfill_state;
DROP TABLE IF EXISTS legacy_migration_copies;
DROP TABLE IF EXISTS legacy_migration_sources;

-- +goose Down
-- Not reversible. The dropped tables held rows derived from files the Music
-- Server no longer locates, and the previous binary needs all of them (plus
-- their triggers) to start. Restore a pre-028 database backup instead.
