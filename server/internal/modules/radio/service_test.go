package radio

import (
	"context"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/testutil"
)

func TestStoreCreatesAndListsUserStations(t *testing.T) {
	db := testutil.OpenMigratedDB(t)
	store := newTestService(db)
	ctx := context.Background()

	first, err := store.CreateStation(ctx, "user-1", StationCreate{
		Name:      "Radio Paradise",
		StreamURL: "https://stream.radioparadise.com/mp3-192",
		Tags:      []string{"rock", "eclectic"},
		Codec:     "MP3",
		Bitrate:   192,
	})
	if err != nil {
		t.Fatal(err)
	}
	if _, createErr := store.CreateStation(ctx, "user-2", StationCreate{
		Name:      "Other User Station",
		StreamURL: "https://example.com/other.mp3",
	}); createErr != nil {
		t.Fatal(createErr)
	}
	second, err := store.CreateStation(ctx, "user-1", StationCreate{
		Name:       "FIP",
		StreamURL:  "https://example.com/fip.mp3",
		IsFavorite: true,
	})
	if err != nil {
		t.Fatal(err)
	}

	list, err := store.ListStations(ctx, "user-1")
	if err != nil {
		t.Fatal(err)
	}
	if list.Total != 2 || len(list.Items) != 2 {
		t.Fatalf("list = %+v, want two user-1 stations", list)
	}
	if list.Items[0].ID != second.ID || !list.Items[0].IsFavorite {
		t.Fatalf("favorite station should sort first: %+v", list.Items)
	}
	if list.Items[1].ID != first.ID || list.Items[1].Position != 0 {
		t.Fatalf("first station position mismatch: %+v", list.Items[1])
	}
	if got := list.Items[1].Tags; len(got) != 2 || got[0] != "rock" || got[1] != "eclectic" {
		t.Fatalf("tags = %#v", got)
	}
}

func TestStoreUpdatesAndDeletesStation(t *testing.T) {
	db := testutil.OpenMigratedDB(t)
	store := newTestService(db)
	ctx := context.Background()

	station, err := store.CreateStation(ctx, "user-1", StationCreate{
		Name:      "Radio Paradise",
		StreamURL: "https://stream.radioparadise.com/mp3-192",
	})
	if err != nil {
		t.Fatal(err)
	}

	name := "Radio Paradise Main Mix"
	isFavorite := true
	updated, err := store.UpdateStation(ctx, "user-1", station.ID, StationPatch{
		Name:       &name,
		IsFavorite: &isFavorite,
	})
	if err != nil {
		t.Fatal(err)
	}
	if updated.Name != name || !updated.IsFavorite {
		t.Fatalf("updated = %+v", updated)
	}

	if err := store.DeleteStation(ctx, "user-1", station.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := store.GetStation(ctx, "user-1", station.ID); err != ErrNotFound {
		t.Fatalf("err = %v, want ErrNotFound", err)
	}
}

func TestStoreUpdatesStationMetadata(t *testing.T) {
	db := testutil.OpenMigratedDB(t)
	store := newTestService(db)
	ctx := context.Background()

	station, err := store.CreateStation(ctx, "user-1", StationCreate{
		Name:      "Radio Paradise",
		StreamURL: "https://stream.radioparadise.com/mp3-192",
	})
	if err != nil {
		t.Fatal(err)
	}

	homepage := "https://example.com"
	favicon := "https://example.com/favicon.ico"
	country := "France"
	language := "french"
	codec := "AAC"
	bitrate := 128
	updated, err := store.UpdateStation(ctx, "user-1", station.ID, StationPatch{
		HomepageURL: &homepage,
		FaviconURL:  &favicon,
		Country:     &country,
		Language:    &language,
		Tags:        []string{"jazz", "public"},
		Codec:       &codec,
		Bitrate:     &bitrate,
	})
	if err != nil {
		t.Fatal(err)
	}
	if updated.HomepageURL != homepage || updated.Codec != codec || updated.Bitrate != bitrate {
		t.Fatalf("updated = %+v", updated)
	}
	if got := updated.Tags; len(got) != 2 || got[0] != "jazz" || got[1] != "public" {
		t.Fatalf("tags = %#v", got)
	}
}

func TestStoreRejectsInvalidStationURLsOnCreate(t *testing.T) {
	store := newTestService(testutil.OpenMigratedDB(t))
	testCases := []struct {
		name  string
		input StationCreate
	}{
		{name: "stream URL scheme", input: StationCreate{Name: "Station", StreamURL: "file:///tmp/audio.mp3"}},
		{name: "stream URL host", input: StationCreate{Name: "Station", StreamURL: "https://"}},
		{name: "homepage URL scheme", input: StationCreate{Name: "Station", StreamURL: "https://radio.example/live", HomepageURL: "javascript:alert(1)"}},
		{name: "homepage URL format", input: StationCreate{Name: "Station", StreamURL: "https://radio.example/live", HomepageURL: "not a URI"}},
		{name: "favicon URL scheme", input: StationCreate{Name: "Station", StreamURL: "https://radio.example/live", FaviconURL: "data:image/png;base64,AAAA"}},
		{name: "favicon URL format", input: StationCreate{Name: "Station", StreamURL: "https://radio.example/live", FaviconURL: "://missing-scheme"}},
	}

	for _, testCase := range testCases {
		t.Run(testCase.name, func(t *testing.T) {
			if _, err := store.CreateStation(context.Background(), "user-1", testCase.input); err == nil {
				t.Fatal("CreateStation() error = nil, want URL validation error")
			}
		})
	}
}

func TestStoreRejectsInvalidStationURLsOnUpdate(t *testing.T) {
	store := newTestService(testutil.OpenMigratedDB(t))
	station, err := store.CreateStation(context.Background(), "user-1", StationCreate{
		Name:        "Station",
		StreamURL:   "https://radio.example/live",
		HomepageURL: "https://radio.example",
		FaviconURL:  "https://radio.example/favicon.ico",
	})
	if err != nil {
		t.Fatal(err)
	}
	unsafeStreamURL := "http://127.0.0.1/private"
	unsafeHomepageURL := "file:///tmp/homepage.html"
	unsafeFaviconURL := "javascript:alert(1)"
	testCases := []struct {
		name  string
		patch StationPatch
	}{
		{name: "stream URL", patch: StationPatch{StreamURL: &unsafeStreamURL}},
		{name: "homepage URL", patch: StationPatch{HomepageURL: &unsafeHomepageURL}},
		{name: "favicon URL", patch: StationPatch{FaviconURL: &unsafeFaviconURL}},
	}

	for _, testCase := range testCases {
		t.Run(testCase.name, func(t *testing.T) {
			if _, err := store.UpdateStation(context.Background(), "user-1", station.ID, testCase.patch); err == nil {
				t.Fatal("UpdateStation() error = nil, want URL validation error")
			}
		})
	}
}
