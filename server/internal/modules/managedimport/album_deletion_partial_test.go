package managedimport

import (
	"context"
	"errors"
	"os"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/testutil"
)

// A failure part-way through an Album deletion stops the run: Tracks already
// deleted stay deleted, the failing Track and every later one stay intact, and
// the result names both lists.
func TestAlbumDeletionStopsAtFirstFailureAndReportsBothLists(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	root := t.TempDir()
	first := SeedManagedAlbumTrackForTest(t, database, root, "album-1", "artist-1", "track-1", 1, "First")
	second := SeedManagedAlbumTrackForTest(t, database, root, "album-1", "artist-1", "track-2", 2, "Second")
	third := SeedManagedAlbumTrackForTest(t, database, root, "album-1", "artist-1", "track-3", 3, "Third")
	storage := newStorage(root, StorageLimits{FileBytes: 1 << 20, BatchBytes: 1 << 20}, unlimitedStorageCapacity)
	service := NewService(NewRepository(database), storage, nil)
	service.albumDeletionTrackHook = func(trackID string) error {
		if trackID == "track-2" {
			return errors.New("injected failure before the second Track")
		}
		return nil
	}

	preview, err := service.PreviewAlbumDeletion(context.Background(), "album-1")
	if err != nil {
		t.Fatal(err)
	}
	result, err := service.DeleteAlbum(context.Background(), AlbumDeletionRequest{AlbumID: "album-1", ConfirmationToken: preview.ConfirmationToken})
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Deleted) != 1 || result.Deleted[0].TrackID != "track-1" || result.DeletedFiles != 1 {
		t.Fatalf("deleted = %+v (files %d)", result.Deleted, result.DeletedFiles)
	}
	if result.StoppedAt == nil || result.StoppedAt.TrackID != "track-2" || result.StoppedAt.Reason != "injected failure before the second Track" {
		t.Fatalf("stoppedAt = %+v", result.StoppedAt)
	}
	if _, err := os.Stat(first); !os.IsNotExist(err) {
		t.Fatalf("first file should be gone: %v", err)
	}
	for _, path := range []string{second, third} {
		if _, err := os.Stat(path); err != nil {
			t.Fatalf("file %q should remain after the failure: %v", path, err)
		}
	}
	var remaining int
	if err := database.QueryRow(`SELECT COUNT(*) FROM tracks WHERE missing_at IS NULL`).Scan(&remaining); err != nil {
		t.Fatal(err)
	}
	if remaining != 2 {
		t.Fatalf("remaining tracks = %d, want 2", remaining)
	}
}
