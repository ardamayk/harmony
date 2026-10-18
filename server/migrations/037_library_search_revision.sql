-- +goose Up
CREATE TABLE library_search_revision (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    revision INTEGER NOT NULL
);
INSERT INTO library_search_revision VALUES (1, 0);

-- Prepared names and all membership/visibility changes invalidate snapshots in
-- the same transaction as the source write. Playback-only writes do not.
-- +goose StatementBegin
CREATE TRIGGER library_search_text_revision_insert AFTER INSERT ON library_search_texts BEGIN
    UPDATE library_search_revision SET revision = revision + 1;
END;
-- +goose StatementEnd
-- +goose StatementBegin
CREATE TRIGGER library_search_text_revision_update AFTER UPDATE ON library_search_texts BEGIN
    UPDATE library_search_revision SET revision = revision + 1;
END;
-- +goose StatementEnd
-- +goose StatementBegin
CREATE TRIGGER library_search_text_revision_delete AFTER DELETE ON library_search_texts BEGIN
    UPDATE library_search_revision SET revision = revision + 1;
END;
-- +goose StatementEnd
-- +goose StatementBegin
CREATE TRIGGER library_search_track_revision AFTER UPDATE OF album_id, missing_at, is_pending_commit ON tracks BEGIN
    UPDATE library_search_revision SET revision = revision + 1;
END;
-- +goose StatementEnd
-- +goose StatementBegin
CREATE TRIGGER library_search_owner_revision AFTER UPDATE OF user_id ON playlists BEGIN
    UPDATE library_search_revision SET revision = revision + 1;
END;
-- +goose StatementEnd

-- +goose StatementBegin
CREATE TRIGGER library_search_track_artist_revision_insert AFTER INSERT ON track_artists BEGIN
    UPDATE library_search_revision SET revision = revision + 1;
END;
-- +goose StatementEnd
-- +goose StatementBegin
CREATE TRIGGER library_search_track_artist_revision_update AFTER UPDATE ON track_artists BEGIN
    UPDATE library_search_revision SET revision = revision + 1;
END;
-- +goose StatementEnd
-- +goose StatementBegin
CREATE TRIGGER library_search_track_artist_revision_delete AFTER DELETE ON track_artists BEGIN
    UPDATE library_search_revision SET revision = revision + 1;
END;
-- +goose StatementEnd
-- +goose StatementBegin
CREATE TRIGGER library_search_album_artist_revision_insert AFTER INSERT ON album_artists BEGIN
    UPDATE library_search_revision SET revision = revision + 1;
END;
-- +goose StatementEnd
-- +goose StatementBegin
CREATE TRIGGER library_search_album_artist_revision_update AFTER UPDATE ON album_artists BEGIN
    UPDATE library_search_revision SET revision = revision + 1;
END;
-- +goose StatementEnd
-- +goose StatementBegin
CREATE TRIGGER library_search_album_artist_revision_delete AFTER DELETE ON album_artists BEGIN
    UPDATE library_search_revision SET revision = revision + 1;
END;
-- +goose StatementEnd
-- +goose StatementBegin
CREATE TRIGGER library_search_track_genre_revision_insert AFTER INSERT ON track_genres BEGIN
    UPDATE library_search_revision SET revision = revision + 1;
END;
-- +goose StatementEnd
-- +goose StatementBegin
CREATE TRIGGER library_search_track_genre_revision_update AFTER UPDATE ON track_genres BEGIN
    UPDATE library_search_revision SET revision = revision + 1;
END;
-- +goose StatementEnd
-- +goose StatementBegin
CREATE TRIGGER library_search_track_genre_revision_delete AFTER DELETE ON track_genres BEGIN
    UPDATE library_search_revision SET revision = revision + 1;
END;
-- +goose StatementEnd

-- +goose Down
DROP TRIGGER library_search_track_genre_revision_delete;
DROP TRIGGER library_search_track_genre_revision_update;
DROP TRIGGER library_search_track_genre_revision_insert;
DROP TRIGGER library_search_album_artist_revision_delete;
DROP TRIGGER library_search_album_artist_revision_update;
DROP TRIGGER library_search_album_artist_revision_insert;
DROP TRIGGER library_search_track_artist_revision_delete;
DROP TRIGGER library_search_track_artist_revision_update;
DROP TRIGGER library_search_track_artist_revision_insert;
DROP TRIGGER library_search_owner_revision;
DROP TRIGGER library_search_track_revision;
DROP TRIGGER library_search_text_revision_delete;
DROP TRIGGER library_search_text_revision_update;
DROP TRIGGER library_search_text_revision_insert;
DROP TABLE library_search_revision;
