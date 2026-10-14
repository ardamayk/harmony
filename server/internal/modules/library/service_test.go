package library

import (
	"context"
	"database/sql"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/testutil"
	_ "modernc.org/sqlite"
)

func setupServiceDB(t *testing.T) (*Service, *sql.DB) {
	t.Helper()
	db := openMemoryDB(t)
	t.Cleanup(func() {
		if err := db.Close(); err != nil {
			t.Errorf("close database: %v", err)
		}
	})
	return NewService(NewRepository(db)), db
}

func TestServiceListTracksSearchesAlbumAndGenre(t *testing.T) {
	svc, db := setupServiceDB(t)
	testutil.SeedManagedTrack(t, db, testutil.ManagedTrackSpec{
		Title: "Blue Monday", Artist: "New Order", Album: "Low-Life", TrackNo: 1, DurationMs: 180_000, Genres: []string{"Synthpop"},
	})
	testutil.SeedManagedTrack(t, db, testutil.ManagedTrackSpec{
		Title: "Age of Consent", Artist: "New Order", Album: "Power Corruption and Lies", TrackNo: 1, DurationMs: 300_000, Genres: []string{"Rock"},
	})

	byAlbum, err := svc.ListTracks(context.Background(), 10, 0, "", "Low-Life")
	if err != nil {
		t.Fatal(err)
	}
	if len(byAlbum.Items) != 1 || byAlbum.Items[0].Title != "Blue Monday" {
		t.Fatalf("album search items = %#v, want Blue Monday", byAlbum.Items)
	}

	byGenre, err := svc.ListTracks(context.Background(), 10, 0, "", "Synthpop")
	if err != nil {
		t.Fatal(err)
	}
	if len(byGenre.Items) != 1 || byGenre.Items[0].Title != "Blue Monday" {
		t.Fatalf("genre search items = %#v, want Blue Monday", byGenre.Items)
	}
}
