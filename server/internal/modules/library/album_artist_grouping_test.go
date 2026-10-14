package library_test

import (
	"context"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
)

// A featured Track credited to a guest performer still belongs to the Album
// Artist's album: the guest appears on the Track, not as an album owner.
func TestAlbumArtistGroupsFeaturedTrackUnderAlbumArtist(t *testing.T) {
	db := testutil.OpenMigratedDB(t)
	albumID, _ := testutil.SeedManagedTrack(t, db, testutil.ManagedTrackSpec{
		Title:       "Snow On The Beach (feat. Lana Del Rey)",
		Artist:      "Lana Del Rey",
		AlbumArtist: "Taylor Swift",
		Album:       "Midnights",
		TrackNo:     4,
		Year:        2022,
		DurationMs:  256000,
		Genres:      []string{"Synthpop"},
	})

	album, err := library.NewRepository(db).GetAlbum(context.Background(), albumID)
	if err != nil {
		t.Fatal(err)
	}
	if album.ArtistName != "Taylor Swift" {
		t.Fatalf("album artist = %q, want Taylor Swift", album.ArtistName)
	}
	if len(album.Tracks) != 1 || len(album.Tracks[0].Artists) != 1 || album.Tracks[0].Artists[0].Name != "Lana Del Rey" {
		t.Fatalf("track credits = %#v, want Lana Del Rey only", album.Tracks)
	}
	var albumCount int
	if err := db.QueryRow(`SELECT COUNT(*) FROM albums`).Scan(&albumCount); err != nil {
		t.Fatal(err)
	}
	if albumCount != 1 {
		t.Fatalf("albums = %d, want 1 (guest must not get an album)", albumCount)
	}
}
