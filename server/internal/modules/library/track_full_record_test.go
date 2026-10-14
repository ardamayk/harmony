package library

import (
	"context"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/testutil"
)

// Every column of the tracks row, plus the source hash, reaches the API so a
// detail page can show everything the Music Server knows about a Track.
func TestGetTrackExposesEveryStoredColumn(t *testing.T) {
	db := testutil.OpenMigratedDB(t)
	_, trackID := seedTrack(t, db)
	if _, err := db.Exec(`UPDATE tracks SET revision = 3 WHERE id = ?`, trackID); err != nil {
		t.Fatal(err)
	}

	track, err := NewRepository(db).GetTrack(context.Background(), trackID)
	if err != nil {
		t.Fatalf("GetTrack() error = %v", err)
	}

	if track.TitleSort == "" || track.Revision != 3 || track.FilePath == "" {
		t.Fatalf("stored columns missing: %+v", track)
	}
	if track.CreatedAt.IsZero() || track.UpdatedAt.IsZero() || track.FileMtime == 0 {
		t.Fatalf("timestamps missing: created %v updated %v mtime %d", track.CreatedAt, track.UpdatedAt, track.FileMtime)
	}
	if track.ContentSHA256 == "" {
		t.Fatalf("content hash missing: %+v", track)
	}
}
