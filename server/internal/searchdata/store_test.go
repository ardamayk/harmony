package searchdata_test

import (
	"context"
	"database/sql"
	"errors"
	"reflect"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/searchdata"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
)

func execute(t *testing.T, database *sql.DB, statement string, arguments ...any) {
	t.Helper()
	if _, err := database.Exec(statement, arguments...); err != nil {
		t.Fatal(err)
	}
}

func seedLibrary(t *testing.T, database *sql.DB) {
	t.Helper()
	execute(t, database, `
 INSERT INTO artists(id,name,name_sort) VALUES ('artist','Beyoncé','beyoncé'), ('guest','Şebnem','şebnem');
 INSERT INTO albums(id,artist_id,title,title_sort) VALUES ('album','artist','AC/DC','ac/dc');
 INSERT INTO album_artists VALUES ('album','artist',0);
 INSERT INTO tracks(id,album_id,title,title_sort,artist_name,format,file_path) VALUES ('track','album','Song (2011 Remaster)','song','Şebnem','flac','/managed/song.flac');
 INSERT INTO track_artists VALUES ('track','guest',0);
 INSERT INTO genres(id,name,name_normalized) VALUES ('genre','R&B','r&b');
 INSERT INTO track_genres VALUES ('track','genre',0);
 INSERT INTO playlists(id,user_id,name) VALUES ('playlist','listener','Favorites');`)
}

func TestDocumentsReflectStoredNamesAndStructuredCredits(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	seedLibrary(t, database)
	document, err := searchdata.NewRepository(database).GetDocument(context.Background(), "track", "track", "listener")
	if err != nil {
		t.Fatal(err)
	}
	expected := map[string]string{"primary": "song 2011 remaster", "album": "ac dc", "track_artist": "sebnem", "album_artist": "beyonce", "genre": "r b"}
	if len(document.Fields) != len(expected) {
		t.Fatalf("fields: %+v", document.Fields)
	}
	for _, field := range document.Fields {
		if field.Text.Folded != expected[field.Name] {
			t.Fatalf("field: %+v", field)
		}
	}
	if document.Fields[0].Text.Original != "Song (2011 Remaster)" {
		t.Fatalf("original title lost: %+v", document)
	}
	if len(document.Fields[0].Text.Versions) != 1 || document.Fields[0].Text.Versions[0] != "remaster" {
		t.Fatalf("version: %+v", document)
	}
	for _, kind := range []string{"album", "artist", "genre", "playlist"} {
		if _, readErr := searchdata.NewRepository(database).GetDocument(context.Background(), kind, kind, "listener"); readErr != nil {
			t.Fatalf("%s: %v", kind, readErr)
		}
	}
	_, err = searchdata.NewRepository(database).GetDocument(context.Background(), "playlist", "playlist", "other")
	if !errors.Is(err, sql.ErrNoRows) {
		t.Fatalf("playlist ownership: %v", err)
	}
}

func TestDocumentsFollowMetadataRelationshipsAndVisibility(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	seedLibrary(t, database)
	store := searchdata.NewRepository(database)
	ctx := context.Background()
	execute(t, database, `UPDATE artists SET name = 'Beyonce' WHERE id = 'artist'`)
	document, err := store.GetDocument(ctx, "track", "track", "listener")
	if err != nil {
		t.Fatal(err)
	}
	for _, field := range document.Fields {
		if field.Name == "album_artist" && field.Text.Original != "Beyonce" {
			t.Fatalf("stale credit: %+v", field)
		}
	}
	execute(t, database, `DELETE FROM track_artists WHERE track_id = 'track'; INSERT INTO track_artists VALUES ('track','artist',0)`)
	document, err = store.GetDocument(ctx, "track", "track", "listener")
	if err != nil {
		t.Fatal(err)
	}
	for _, field := range document.Fields {
		if field.Name == "track_artist" && field.RelatedID != "artist" {
			t.Fatalf("stale relationship: %+v", field)
		}
	}
	for _, column := range []string{"is_pending_commit", "missing_at"} {
		execute(t, database, "UPDATE tracks SET "+column+" = 1 WHERE id = 'track'")
		for _, kind := range []string{"track", "album", "artist", "genre"} {
			if _, readErr := store.GetDocument(ctx, kind, kind, "listener"); !errors.Is(readErr, sql.ErrNoRows) {
				t.Fatalf("hidden %s: %v", kind, readErr)
			}
		}
		reset := "NULL"
		if column == "is_pending_commit" {
			reset = "0"
		}
		execute(t, database, "UPDATE tracks SET "+column+" = "+reset+" WHERE id = 'track'")
	}
	execute(t, database, `DELETE FROM tracks WHERE id = 'track'`)
	if _, readErr := store.GetDocument(ctx, "track", "track", "listener"); !errors.Is(readErr, sql.ErrNoRows) {
		t.Fatalf("deleted Track: %v", readErr)
	}
	document, err = store.GetDocument(ctx, "playlist", "playlist", "listener")
	if err != nil || len(document.Fields) != 1 {
		t.Fatalf("Playlist must only carry its name: %+v, %v", document, err)
	}
}

func TestStoredVersionRecognition(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	seedLibrary(t, database)
	cases := []struct{ title, version string }{
		{"Song (Radio) (Edit)", ""}, {"Live Forever", ""}, {"Song (Something Else)", ""}, {"Song (Live)", "live"},
		{"Song - Remix", "remix"}, {"Song (Acoustic)", "acoustic"}, {"Song (Instrumental)", "instrumental"},
		{"Song - Demo", "demo"}, {"Song (2011 Remaster)", "remaster"}, {"Song - Remastered 2011", "remaster"},
		{"Song (Radio Edit)", "radio edit"},
	}
	for _, testCase := range cases {
		t.Run(testCase.title, func(t *testing.T) {
			execute(t, database, `UPDATE tracks SET title = ? WHERE id = 'track'`, testCase.title)
			document, err := searchdata.NewRepository(database).GetDocument(context.Background(), "track", "track", "listener")
			if err != nil {
				t.Fatal(err)
			}
			versions := document.Fields[0].Text.Versions
			if testCase.version == "" && len(versions) != 0 || testCase.version != "" && (len(versions) != 1 || versions[0] != testCase.version) {
				t.Fatalf("versions: %v", versions)
			}
			if document.Fields[0].Text.Original != testCase.title {
				t.Fatal("stored display title changed")
			}
		})
	}
}

func TestRebuildPreservesSourceDataAndRollsBackOnFailure(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	seedLibrary(t, database)
	store := searchdata.NewRepository(database)
	ctx := context.Background()
	before, err := store.GetDocument(ctx, "track", "track", "listener")
	if err != nil {
		t.Fatal(err)
	}
	execute(t, database, `CREATE TRIGGER reject_search_rebuild BEFORE INSERT ON library_search_texts BEGIN SELECT RAISE(ABORT, 'injected search write failure'); END`)
	if rebuildErr := store.Rebuild(ctx); rebuildErr == nil {
		t.Fatal("rebuild must report preparation failure")
	}
	after, err := store.GetDocument(ctx, "track", "track", "listener")
	if err != nil || !reflect.DeepEqual(before, after) {
		t.Fatalf("failed rebuild changed previous data: %+v %v", after, err)
	}
	if _, writeErr := database.Exec(`UPDATE tracks SET title = 'New title' WHERE id = 'track'`); writeErr == nil {
		t.Fatal("metadata write must fail when search preparation fails")
	}
	var title string
	if readErr := database.QueryRow(`SELECT title FROM tracks WHERE id = 'track'`).Scan(&title); readErr != nil {
		t.Fatal(readErr)
	}
	if title != "Song (2011 Remaster)" {
		t.Fatalf("failed metadata write committed: %s", title)
	}
	execute(t, database, `DROP TRIGGER reject_search_rebuild; DELETE FROM library_search_texts`)
	if prepareErr := store.EnsureCurrent(ctx); prepareErr != nil {
		t.Fatal(prepareErr)
	}
	after, err = store.GetDocument(ctx, "track", "track", "listener")
	if err != nil || !reflect.DeepEqual(before, after) {
		t.Fatalf("backfill: %+v %v", after, err)
	}
	execute(t, database, `UPDATE library_search_texts SET rules_version = 0, text_json = '{}'`)
	if prepareErr := store.EnsureCurrent(ctx); prepareErr != nil {
		t.Fatal(prepareErr)
	}
	after, err = store.GetDocument(ctx, "track", "track", "listener")
	if err != nil || !reflect.DeepEqual(before, after) {
		t.Fatalf("rule upgrade: %+v %v", after, err)
	}
}
