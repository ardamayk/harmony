package library

import (
	"context"
	"database/sql"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/testutil"
	"github.com/go-chi/chi/v5"
	_ "modernc.org/sqlite"
)

func setupHandlerFixture(t *testing.T) (*Handler, *sql.DB) {
	t.Helper()
	db := openMemoryDB(t)
	store := NewRepository(db)
	return NewHandler(NewService(store)), db
}

func seedTrack(t *testing.T, db *sql.DB) (albumID, trackID string) {
	t.Helper()
	return testutil.SeedManagedTrack(t, db, testutil.ManagedTrackSpec{
		Title:        "Song",
		Artist:       "Artist",
		Album:        "Album",
		TrackNo:      1,
		Year:         2024,
		DurationMs:   1000,
		Genres:       []string{"Pop"},
		SampleRateHz: 96000,
		BitDepth:     24,
		ReplayGain: testutil.ReplayGainSpec{
			TrackGainDB: float64Pointer(-7.25),
			TrackPeak:   float64Pointer(0.98),
			AlbumGainDB: float64Pointer(-6.5),
			AlbumPeak:   float64Pointer(1.01),
		},
	})
}

func TestPagination(t *testing.T) {
	req := httptest.NewRequest(http.MethodGet, "/?limit=10&offset=5", nil)
	limit, offset := pagination(req)
	if limit != 10 || offset != 5 {
		t.Fatalf("pagination = (%d, %d), want (10, 5)", limit, offset)
	}

	bad := httptest.NewRequest(http.MethodGet, "/?limit=9999&offset=-1", nil)
	limit, offset = pagination(bad)
	if limit != 50 || offset != 0 {
		t.Fatalf("pagination clamp = (%d, %d), want (50, 0)", limit, offset)
	}
}

func TestHandlersGetAlbumNotFound(t *testing.T) {
	h, _ := setupHandlerFixture(t)
	req := httptest.NewRequest(http.MethodGet, "/", nil)
	rec := httptest.NewRecorder()
	rctx := chi.NewRouteContext()
	rctx.URLParams.Add("albumId", "missing")
	req = req.WithContext(context.WithValue(req.Context(), chi.RouteCtxKey, rctx))

	h.GetAlbum(rec, req)
	if rec.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404", rec.Code)
	}
}

func TestHandlersGetAlbumReturnsGenresAndAudioFormat(t *testing.T) {
	h, db := setupHandlerFixture(t)
	albumID, _ := seedTrack(t, db)

	req := httptest.NewRequest(http.MethodGet, "/", nil)
	rec := httptest.NewRecorder()
	rctx := chi.NewRouteContext()
	rctx.URLParams.Add("albumId", albumID)
	req = req.WithContext(context.WithValue(req.Context(), chi.RouteCtxKey, rctx))

	h.GetAlbum(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200", rec.Code)
	}

	responseBody := rec.Body.Bytes()
	var body AlbumDetail
	if err := json.Unmarshal(responseBody, &body); err != nil {
		t.Fatal(err)
	}
	if len(body.Genres) != 1 || body.Genres[0] != "Pop" {
		t.Fatalf("genres = %#v", body.Genres)
	}
	if len(body.Tracks) != 1 {
		t.Fatalf("tracks = %d, want 1", len(body.Tracks))
	}
	if body.Tracks[0].BitDepth != 24 || body.Tracks[0].SampleRateHz != 96000 {
		t.Fatalf("audio format = %+v", body.Tracks[0])
	}
	assertReplayGainMetadata(t, body.Tracks[0].ReplayGain, ReplayGainMetadata{
		TrackGainDB: float64Pointer(-7.25),
		TrackPeak:   float64Pointer(0.98),
		AlbumGainDB: float64Pointer(-6.5),
		AlbumPeak:   float64Pointer(1.01),
	})
	var contractBody struct {
		Tracks []struct {
			ReplayGain map[string]*float64 `json:"replayGain"`
		} `json:"tracks"`
	}
	if err := json.Unmarshal(responseBody, &contractBody); err != nil {
		t.Fatal(err)
	}
	if contractBody.Tracks[0].ReplayGain["trackGainDb"] == nil || contractBody.Tracks[0].ReplayGain["albumPeak"] == nil {
		t.Fatalf("ReplayGain contract = %s", responseBody)
	}
}

func TestHandlersGetAlbumKeepsMatchingTrackNumbersOnDifferentDiscs(t *testing.T) {
	h, db := setupHandlerFixture(t)
	albumID, _ := seedTrack(t, db)

	testutil.SeedManagedTrack(t, db, testutil.ManagedTrackSpec{
		AlbumID: albumID, Title: "Song", Artist: "Artist", Album: "Album",
		TrackNo: 1, DiscNo: 2, Year: 2024, DurationMs: 1000, Genres: []string{"Pop"},
		SampleRateHz: 96000, BitDepth: 24,
	})

	req := httptest.NewRequest(http.MethodGet, "/", nil)
	rec := httptest.NewRecorder()
	rctx := chi.NewRouteContext()
	rctx.URLParams.Add("albumId", albumID)
	req = req.WithContext(context.WithValue(req.Context(), chi.RouteCtxKey, rctx))

	h.GetAlbum(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200", rec.Code)
	}

	var body AlbumDetail
	if err := json.NewDecoder(rec.Body).Decode(&body); err != nil {
		t.Fatal(err)
	}
	if len(body.Tracks) != 2 {
		t.Fatalf("tracks = %d, want 2", len(body.Tracks))
	}
	if body.TrackCount != 2 {
		t.Fatalf("trackCount = %d, want 2", body.TrackCount)
	}
	if body.Tracks[0].DiscNo != 1 || body.Tracks[1].DiscNo != 2 {
		t.Fatalf("disc order = [%d, %d], want [1, 2]", body.Tracks[0].DiscNo, body.Tracks[1].DiscNo)
	}
}
