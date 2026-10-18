-- +goose Up
CREATE TABLE library_search_texts (
    kind TEXT NOT NULL CHECK (kind IN ('track','album','artist','genre','playlist')),
    id TEXT NOT NULL,
    text_json TEXT NOT NULL CHECK (json_valid(text_json)),
    rules_version INTEGER NOT NULL,
    PRIMARY KEY (kind, id)
);

CREATE VIEW library_search_sources AS
SELECT 'track' AS kind, id, title AS name FROM tracks
UNION ALL
SELECT 'album' AS kind, id, title AS name FROM albums
UNION ALL
SELECT 'artist' AS kind, id, name AS name FROM artists
UNION ALL
SELECT 'genre' AS kind, id, name AS name FROM genres
UNION ALL
SELECT 'playlist' AS kind, id, name AS name FROM playlists;

-- +goose StatementBegin
CREATE TRIGGER library_search_track_insert AFTER INSERT ON tracks
BEGIN

    INSERT INTO library_search_texts VALUES ('track', NEW.id, library_search_text(NEW.title, 'track'), library_search_version());
END;
-- +goose StatementEnd

-- +goose StatementBegin
CREATE TRIGGER library_search_track_update AFTER UPDATE OF title, id ON tracks
BEGIN
    DELETE FROM library_search_texts WHERE kind = 'track' AND id = OLD.id;
    INSERT INTO library_search_texts VALUES ('track', NEW.id, library_search_text(NEW.title, 'track'), library_search_version());
END;
-- +goose StatementEnd

-- +goose StatementBegin
CREATE TRIGGER library_search_track_delete AFTER DELETE ON tracks
BEGIN
    DELETE FROM library_search_texts WHERE kind = 'track' AND id = OLD.id;
END;
-- +goose StatementEnd

-- +goose StatementBegin
CREATE TRIGGER library_search_album_insert AFTER INSERT ON albums
BEGIN

    INSERT INTO library_search_texts VALUES ('album', NEW.id, library_search_text(NEW.title, 'album'), library_search_version());
END;
-- +goose StatementEnd

-- +goose StatementBegin
CREATE TRIGGER library_search_album_update AFTER UPDATE OF title, id ON albums
BEGIN
    DELETE FROM library_search_texts WHERE kind = 'album' AND id = OLD.id;
    INSERT INTO library_search_texts VALUES ('album', NEW.id, library_search_text(NEW.title, 'album'), library_search_version());
END;
-- +goose StatementEnd

-- +goose StatementBegin
CREATE TRIGGER library_search_album_delete AFTER DELETE ON albums
BEGIN
    DELETE FROM library_search_texts WHERE kind = 'album' AND id = OLD.id;
END;
-- +goose StatementEnd

-- +goose StatementBegin
CREATE TRIGGER library_search_artist_insert AFTER INSERT ON artists
BEGIN

    INSERT INTO library_search_texts VALUES ('artist', NEW.id, library_search_text(NEW.name, 'artist'), library_search_version());
END;
-- +goose StatementEnd

-- +goose StatementBegin
CREATE TRIGGER library_search_artist_update AFTER UPDATE OF name, id ON artists
BEGIN
    DELETE FROM library_search_texts WHERE kind = 'artist' AND id = OLD.id;
    INSERT INTO library_search_texts VALUES ('artist', NEW.id, library_search_text(NEW.name, 'artist'), library_search_version());
END;
-- +goose StatementEnd

-- +goose StatementBegin
CREATE TRIGGER library_search_artist_delete AFTER DELETE ON artists
BEGIN
    DELETE FROM library_search_texts WHERE kind = 'artist' AND id = OLD.id;
END;
-- +goose StatementEnd

-- +goose StatementBegin
CREATE TRIGGER library_search_genre_insert AFTER INSERT ON genres
BEGIN

    INSERT INTO library_search_texts VALUES ('genre', NEW.id, library_search_text(NEW.name, 'genre'), library_search_version());
END;
-- +goose StatementEnd

-- +goose StatementBegin
CREATE TRIGGER library_search_genre_update AFTER UPDATE OF name, id ON genres
BEGIN
    DELETE FROM library_search_texts WHERE kind = 'genre' AND id = OLD.id;
    INSERT INTO library_search_texts VALUES ('genre', NEW.id, library_search_text(NEW.name, 'genre'), library_search_version());
END;
-- +goose StatementEnd

-- +goose StatementBegin
CREATE TRIGGER library_search_genre_delete AFTER DELETE ON genres
BEGIN
    DELETE FROM library_search_texts WHERE kind = 'genre' AND id = OLD.id;
END;
-- +goose StatementEnd

-- +goose StatementBegin
CREATE TRIGGER library_search_playlist_insert AFTER INSERT ON playlists
BEGIN

    INSERT INTO library_search_texts VALUES ('playlist', NEW.id, library_search_text(NEW.name, 'playlist'), library_search_version());
END;
-- +goose StatementEnd

-- +goose StatementBegin
CREATE TRIGGER library_search_playlist_update AFTER UPDATE OF name, id ON playlists
BEGIN
    DELETE FROM library_search_texts WHERE kind = 'playlist' AND id = OLD.id;
    INSERT INTO library_search_texts VALUES ('playlist', NEW.id, library_search_text(NEW.name, 'playlist'), library_search_version());
END;
-- +goose StatementEnd

-- +goose StatementBegin
CREATE TRIGGER library_search_playlist_delete AFTER DELETE ON playlists
BEGIN
    DELETE FROM library_search_texts WHERE kind = 'playlist' AND id = OLD.id;
END;
-- +goose StatementEnd

INSERT INTO library_search_texts
SELECT kind, id, library_search_text(name, kind), library_search_version() FROM library_search_sources;

CREATE VIEW library_search_entities AS
SELECT 'track' AS kind, id, '' AS user_id FROM visible_tracks
UNION ALL
SELECT 'album', id, '' FROM albums WHERE EXISTS (SELECT 1 FROM visible_tracks WHERE album_id = albums.id)
UNION ALL
SELECT 'artist', id, '' FROM artists WHERE EXISTS (
    SELECT 1 FROM track_artists JOIN visible_tracks ON visible_tracks.id = track_artists.track_id WHERE artist_id = artists.id
) OR EXISTS (
    SELECT 1 FROM album_artists JOIN visible_tracks ON visible_tracks.album_id = album_artists.album_id WHERE artist_id = artists.id
)
UNION ALL
SELECT 'genre', id, '' FROM genres WHERE EXISTS (
    SELECT 1 FROM track_genres JOIN visible_tracks ON visible_tracks.id = track_genres.track_id WHERE genre_id = genres.id
)
UNION ALL
SELECT 'playlist', id, user_id FROM playlists;

CREATE VIEW library_search_field_sources AS
SELECT kind, id, 'primary' AS field, kind AS related_kind, id AS related_id, 0 AS position FROM library_search_entities
UNION ALL
SELECT 'track', id, 'album', 'album', album_id, 0 FROM visible_tracks
UNION ALL
SELECT 'track', track_id, 'track_artist', 'artist', artist_id, position FROM track_artists
UNION ALL
SELECT 'track', tracks.id, 'album_artist', 'artist', credits.artist_id, credits.position
FROM visible_tracks tracks JOIN album_artists credits ON credits.album_id = tracks.album_id
UNION ALL
SELECT 'track', track_id, 'genre', 'genre', genre_id, position FROM track_genres
UNION ALL
SELECT 'album', album_id, 'album_artist', 'artist', artist_id, position FROM album_artists;

CREATE VIEW library_search_fields AS
SELECT source.kind, source.id, entity.user_id, source.field, source.related_id, source.position, prepared.text_json
FROM library_search_field_sources source
JOIN library_search_entities entity ON entity.kind = source.kind AND entity.id = source.id
JOIN library_search_texts prepared ON prepared.kind = source.related_kind AND prepared.id = source.related_id;

-- +goose Down
DROP VIEW library_search_fields;
DROP VIEW library_search_field_sources;
DROP VIEW library_search_entities;

DROP TRIGGER library_search_track_insert;

DROP TRIGGER library_search_track_update;

DROP TRIGGER library_search_track_delete;

DROP TRIGGER library_search_album_insert;

DROP TRIGGER library_search_album_update;

DROP TRIGGER library_search_album_delete;

DROP TRIGGER library_search_artist_insert;

DROP TRIGGER library_search_artist_update;

DROP TRIGGER library_search_artist_delete;

DROP TRIGGER library_search_genre_insert;

DROP TRIGGER library_search_genre_update;

DROP TRIGGER library_search_genre_delete;

DROP TRIGGER library_search_playlist_insert;

DROP TRIGGER library_search_playlist_update;

DROP TRIGGER library_search_playlist_delete;

DROP VIEW library_search_sources;
DROP TABLE library_search_texts;
