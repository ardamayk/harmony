package playlists

import (
	"context"
	"database/sql"
	"errors"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
)

type fakeTrackAccess struct {
	tracks map[string]library.Track
}

func (f fakeTrackAccess) GetTrack(_ context.Context, trackID string) (library.Track, error) {
	track, ok := f.tracks[trackID]
	if !ok {
		return library.Track{}, library.ErrNotFound
	}
	return track, nil
}

func (f fakeTrackAccess) GetTrackFilePath(_ context.Context, trackID string) (string, error) {
	track, ok := f.tracks[trackID]
	if !ok {
		return "", library.ErrNotFound
	}
	return track.FilePath, nil
}

func setupPlaylistService(t *testing.T) (*Service, *sql.DB) {
	t.Helper()
	db := testutil.OpenMigratedDB(t)
	return NewService(NewRepository(db), library.NewStore(db)), db
}

func setupPlaylistServiceWithTrackAccess(t *testing.T, tracks map[string]library.Track) *Service {
	t.Helper()
	db := testutil.OpenMigratedDB(t)
	for trackID := range tracks {
		testutil.SeedManagedTrack(t, db, testutil.ManagedTrackSpec{TrackID: trackID})
	}
	return NewService(NewRepository(db), fakeTrackAccess{tracks: tracks})
}

func seedPlaylistTrack(t *testing.T, db *sql.DB) string {
	t.Helper()
	_, trackID := testutil.SeedManagedTrack(t, db, testutil.ManagedTrackSpec{Title: "Song", Artist: "Artist", Album: "Album", TrackNo: 1})
	return trackID
}

func TestListPlaylistsCreatesDefaultFavorites(t *testing.T) {
	store, _ := setupPlaylistService(t)

	playlists, err := store.ListPlaylists(context.Background(), "user-1")
	if err != nil {
		t.Fatal(err)
	}

	if len(playlists.Items) != 1 {
		t.Fatalf("items = %d, want 1", len(playlists.Items))
	}
	if playlists.Items[0].Name != DefaultFavoritesName || !playlists.Items[0].IsDefault {
		t.Fatalf("default playlist = %+v", playlists.Items[0])
	}
}

func TestAddAndRemoveTrackAreIdempotent(t *testing.T) {
	store, db := setupPlaylistService(t)
	trackID := seedPlaylistTrack(t, db)

	favorites, err := store.GetDefaultFavorites(context.Background(), "user-1")
	if err != nil {
		t.Fatal(err)
	}

	if _, addErr := store.AddTrack(context.Background(), "user-1", favorites.ID, trackID); addErr != nil {
		t.Fatal(addErr)
	}
	detail, err := store.AddTrack(context.Background(), "user-1", favorites.ID, trackID)
	if err != nil {
		t.Fatal(err)
	}
	if len(detail.Tracks) != 1 || detail.Tracks[0].ID != trackID {
		t.Fatalf("tracks after duplicate add = %+v", detail.Tracks)
	}

	detail, err = store.RemoveTrack(context.Background(), "user-1", favorites.ID, trackID)
	if err != nil {
		t.Fatal(err)
	}
	if len(detail.Tracks) != 0 {
		t.Fatalf("tracks after remove = %d, want 0", len(detail.Tracks))
	}
	detail, err = store.RemoveTrack(context.Background(), "user-1", favorites.ID, trackID)
	if err != nil {
		t.Fatal(err)
	}
	if len(detail.Tracks) != 0 {
		t.Fatalf("tracks after duplicate remove = %d, want 0", len(detail.Tracks))
	}
}

func TestRemoveLastTrackDeletesUserPlaylist(t *testing.T) {
	store, db := setupPlaylistService(t)
	trackID := seedPlaylistTrack(t, db)

	playlist, err := store.CreatePlaylist(context.Background(), "user-1", "Road")
	if err != nil {
		t.Fatal(err)
	}
	if _, addErr := store.AddTrack(context.Background(), "user-1", playlist.ID, trackID); addErr != nil {
		t.Fatal(addErr)
	}

	detail, err := store.RemoveTrack(context.Background(), "user-1", playlist.ID, trackID)
	if err != nil {
		t.Fatal(err)
	}
	if detail.TrackCount != 0 || len(detail.Tracks) != 0 {
		t.Fatalf("removed playlist detail = %+v, want empty", detail)
	}

	playlists, err := store.ListPlaylists(context.Background(), "user-1")
	if err != nil {
		t.Fatal(err)
	}
	if len(playlists.Items) != 1 || !playlists.Items[0].IsDefault {
		t.Fatalf("playlists after empty user playlist cleanup = %+v", playlists.Items)
	}
	if _, err := store.GetPlaylist(context.Background(), "user-1", playlist.ID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("GetPlaylist error = %v, want ErrNotFound", err)
	}
}

func TestCreatePlaylistAndListWithFavoritesPinned(t *testing.T) {
	store, _ := setupPlaylistService(t)

	created, err := store.CreatePlaylist(context.Background(), "user-1", "Road")
	if err != nil {
		t.Fatal(err)
	}
	if created.Name != "Road" || created.IsDefault {
		t.Fatalf("created = %+v", created)
	}

	playlists, err := store.ListPlaylists(context.Background(), "user-1")
	if err != nil {
		t.Fatal(err)
	}
	if len(playlists.Items) != 2 {
		t.Fatalf("items = %d, want 2", len(playlists.Items))
	}
	if playlists.Items[0].Name != DefaultFavoritesName || playlists.Items[1].Name != "Road" {
		t.Fatalf("order = %+v", playlists.Items)
	}
}

func TestStoreUsesTrackAccessInterface(t *testing.T) {
	trackID := "track-1"
	store := setupPlaylistServiceWithTrackAccess(t, map[string]library.Track{
		trackID: {ID: trackID, Title: "Song"},
	})

	favorites, err := store.GetDefaultFavorites(context.Background(), "user-1")
	if err != nil {
		t.Fatal(err)
	}
	detail, err := store.AddTrack(context.Background(), "user-1", favorites.ID, trackID)
	if err != nil {
		t.Fatal(err)
	}

	if len(detail.Tracks) != 1 {
		t.Fatalf("tracks = %d, want 1", len(detail.Tracks))
	}
	if detail.Tracks[0].Title != "Song" {
		t.Fatalf("track = %+v", detail.Tracks[0])
	}
}
