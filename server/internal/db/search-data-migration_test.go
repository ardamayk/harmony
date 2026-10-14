package db_test

import (
	"context"
	"path/filepath"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/db"
	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/ardam/navidrome-replacement/server/internal/searchdata"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
	"github.com/pressly/goose/v3"
)

func TestSearchDataUpgradeBackfillsExistingLibrary(t *testing.T) {
	databasePath := filepath.Join(t.TempDir(), "library.db")
	database := openDatabasePathAtVersion(t, databasePath, 35)
	albumID, trackID := testutil.SeedManagedTrack(t, database, testutil.ManagedTrackSpec{Title: "Don't Stop (Live)", Artist: "Beyoncé", Album: "AC/DC"})
	if err := database.Close(); err != nil {
		t.Fatal(err)
	}
	database, err := db.OpenAndMigrate(context.Background(), databasePath, migrationsDir(t))
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if closeErr := database.Close(); closeErr != nil {
			t.Error(closeErr)
		}
	}()
	document, err := searchdata.NewRepository(database).GetDocument(context.Background(), "track", trackID, "user-1")
	if err != nil {
		t.Fatal(err)
	}
	if document.Fields[0].Text.Folded != "dont stop live" {
		t.Fatalf("backfilled title: %+v", document)
	}
	track, err := library.NewStore(database).GetTrack(context.Background(), trackID)
	if err != nil {
		t.Fatal(err)
	}
	if track.Title != "Don't Stop (Live)" || track.AlbumID != albumID || track.Artists[0].Name != "Beyoncé" {
		t.Fatalf("source metadata changed: %+v", track)
	}
	if err := goose.Down(database, migrationsDir(t)); err != nil {
		t.Fatal(err)
	}
	if err := goose.Up(database, migrationsDir(t)); err != nil {
		t.Fatal(err)
	}
	if _, err := searchdata.NewRepository(database).GetDocument(context.Background(), "track", trackID, "user-1"); err != nil {
		t.Fatalf("reapplied migration: %v", err)
	}
}
