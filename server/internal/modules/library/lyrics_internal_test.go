package library

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/ardam/navidrome-replacement/server/internal/testutil"
)

func TestStoreGetTrackLyrics(t *testing.T) {
	db := openMemoryDB(t)
	store := NewRepository(db)
	_, trackID := testutil.SeedManagedTrack(t, db, testutil.ManagedTrackSpec{Title: "Song", Artist: "Artist", Album: "Album"})

	lyrics, err := store.GetTrackLyrics(context.Background(), trackID)
	if err != nil || lyrics != "" {
		t.Fatalf("lyrics before update = %q, %v", lyrics, err)
	}
	if _, err = db.ExecContext(context.Background(), `UPDATE tracks SET lyrics = ? WHERE id = ?`, "Line A\nLine B", trackID); err != nil {
		t.Fatal(err)
	}
	lyrics, err = store.GetTrackLyrics(context.Background(), trackID)
	if err != nil || lyrics != "Line A\nLine B" {
		t.Fatalf("lyrics after update = %q, %v", lyrics, err)
	}
	if _, err := store.GetTrackLyrics(context.Background(), "missing"); err != ErrNotFound {
		t.Fatalf("missing Track error = %v, want ErrNotFound", err)
	}
}

func TestHandlersGetTrackLyrics(t *testing.T) {
	h, db := setupHandlerFixture(t)
	_, trackID := seedTrack(t, db)
	if _, err := db.ExecContext(context.Background(), `UPDATE tracks SET lyrics = ? WHERE id = ?`, "Hello", trackID); err != nil {
		t.Fatal(err)
	}

	rec := httptest.NewRecorder()
	h.GetTrackLyrics(rec, requestWithTrackID(trackID))
	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200", rec.Code)
	}
	var body TrackLyrics
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if body.TrackID != trackID || body.Lyrics != "Hello" {
		t.Fatalf("body = %+v", body)
	}

	rec = httptest.NewRecorder()
	h.GetTrackLyrics(rec, requestWithTrackID("missing"))
	if rec.Code != http.StatusNotFound {
		t.Fatalf("missing status = %d, want 404", rec.Code)
	}
}

func requestWithTrackID(trackID string) *http.Request {
	req := httptest.NewRequest(http.MethodGet, "/", nil)
	rctx := chi.NewRouteContext()
	rctx.URLParams.Add("trackId", trackID)
	return req.WithContext(context.WithValue(req.Context(), chi.RouteCtxKey, rctx))
}
