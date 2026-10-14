package playlists

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/auth"
	"github.com/go-chi/chi/v5"
)

func setupPlaylistHandler(t *testing.T) (*Handler, *Service, string) {
	t.Helper()
	store, db := setupPlaylistService(t)
	trackID := seedPlaylistTrack(t, db)
	return NewHandler(store), store, trackID
}

func withRouteParam(req *http.Request, key, value string) *http.Request {
	rctx := chi.RouteContext(req.Context())
	if rctx == nil {
		rctx = chi.NewRouteContext()
	}
	rctx.URLParams.Add(key, value)
	return req.WithContext(context.WithValue(req.Context(), chi.RouteCtxKey, rctx))
}

func TestHandlersListPlaylistsCreatesFavorites(t *testing.T) {
	h, _, _ := setupPlaylistHandler(t)

	rec := httptest.NewRecorder()
	h.ListPlaylists(rec, httptest.NewRequest(http.MethodGet, "/", nil))

	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d, body %s", rec.Code, rec.Body.String())
	}
	var body PlaylistList
	if err := json.NewDecoder(rec.Body).Decode(&body); err != nil {
		t.Fatal(err)
	}
	if len(body.Items) != 1 || body.Items[0].Name != DefaultFavoritesName {
		t.Fatalf("body = %+v", body)
	}
}

func TestHandlersCreatePlaylist(t *testing.T) {
	h, _, _ := setupPlaylistHandler(t)

	rec := httptest.NewRecorder()
	h.CreatePlaylist(rec, httptest.NewRequest(http.MethodPost, "/", bytes.NewReader([]byte(`{"name":"Road"}`))))

	if rec.Code != http.StatusCreated {
		t.Fatalf("status = %d, body %s", rec.Code, rec.Body.String())
	}
	var body Playlist
	if err := json.NewDecoder(rec.Body).Decode(&body); err != nil {
		t.Fatal(err)
	}
	if body.Name != "Road" {
		t.Fatalf("body = %+v", body)
	}
}

func TestHandlersAddAndRemoveTrack(t *testing.T) {
	h, store, trackID := setupPlaylistHandler(t)
	favorites, err := store.GetDefaultFavorites(context.Background(), auth.DefaultUserID)
	if err != nil {
		t.Fatal(err)
	}

	addReq := withRouteParam(
		httptest.NewRequest(http.MethodPost, "/", bytes.NewReader([]byte(`{"trackId":"`+trackID+`"}`))),
		"playlistId",
		favorites.ID,
	)
	addRec := httptest.NewRecorder()
	h.AddTrack(addRec, addReq)
	if addRec.Code != http.StatusOK {
		t.Fatalf("add status = %d, body %s", addRec.Code, addRec.Body.String())
	}
	var addBody PlaylistDetail
	if err := json.NewDecoder(addRec.Body).Decode(&addBody); err != nil {
		t.Fatal(err)
	}
	if len(addBody.Tracks) != 1 {
		t.Fatalf("tracks = %d, want 1", len(addBody.Tracks))
	}

	removeReq := withRouteParam(httptest.NewRequest(http.MethodDelete, "/", nil), "playlistId", favorites.ID)
	removeReq = withRouteParam(removeReq, "trackId", trackID)
	removeRec := httptest.NewRecorder()
	h.RemoveTrack(removeRec, removeReq)
	if removeRec.Code != http.StatusOK {
		t.Fatalf("remove status = %d, body %s", removeRec.Code, removeRec.Body.String())
	}
	var removeBody PlaylistDetail
	if err := json.NewDecoder(removeRec.Body).Decode(&removeBody); err != nil {
		t.Fatal(err)
	}
	if len(removeBody.Tracks) != 0 {
		t.Fatalf("tracks = %d, want 0", len(removeBody.Tracks))
	}
}
