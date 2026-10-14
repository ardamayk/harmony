package radio

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/testutil"
	"github.com/go-chi/chi/v5"
)

type fakeSearcher struct {
	result SearchResult
}

func (f fakeSearcher) Search(_ *http.Request) (SearchResultList, error) {
	return SearchResultList{Items: []SearchResult{f.result}, Total: 1}, nil
}

func (f fakeSearcher) LookupStation(_ *http.Request, stationUUID string) (SearchResult, error) {
	result := f.result
	result.StationUUID = stationUUID
	return result, nil
}

func (f fakeSearcher) Countries(_ context.Context) (CatalogOptionList, error) {
	return CatalogOptionList{
		Items: []CatalogOption{{Name: "Switzerland", Code: "CH", StationCount: 10}},
		Total: 1,
	}, nil
}

func (f fakeSearcher) Tags(_ context.Context) (CatalogOptionList, error) {
	return CatalogOptionList{
		Items: []CatalogOption{{Name: "jazz", StationCount: 7}},
		Total: 1,
	}, nil
}

type fakeStreamer struct {
	station Station
}

func (f *fakeStreamer) Stream(w http.ResponseWriter, _ *http.Request, station Station) {
	f.station = station
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte("preview"))
}

func setupHandlers(t *testing.T) (*Handler, *Service) {
	t.Helper()
	db := testutil.OpenMigratedDB(t)
	store := newTestService(db)
	return NewHandler(store, nil, nil), store
}

func withStationID(req *http.Request, stationID string) *http.Request {
	rctx := chi.NewRouteContext()
	rctx.URLParams.Add("stationId", stationID)
	return req.WithContext(context.WithValue(req.Context(), chi.RouteCtxKey, rctx))
}

func TestHandlersCreateListPatchDeleteStation(t *testing.T) {
	h, _ := setupHandlers(t)

	createReq := httptest.NewRequest(http.MethodPost, "/", strings.NewReader(`{
		"name": "Radio Paradise",
		"streamUrl": "https://stream.radioparadise.com/mp3-192",
		"tags": ["rock"]
	}`))
	createRec := httptest.NewRecorder()
	h.CreateStation(createRec, createReq)
	if createRec.Code != http.StatusCreated {
		t.Fatalf("create status = %d, body = %s", createRec.Code, createRec.Body.String())
	}
	var created Station
	if err := json.NewDecoder(createRec.Body).Decode(&created); err != nil {
		t.Fatal(err)
	}

	listRec := httptest.NewRecorder()
	h.ListStations(listRec, httptest.NewRequest(http.MethodGet, "/", nil))
	if listRec.Code != http.StatusOK {
		t.Fatalf("list status = %d", listRec.Code)
	}
	var list StationList
	if err := json.NewDecoder(listRec.Body).Decode(&list); err != nil {
		t.Fatal(err)
	}
	if list.Total != 1 || list.Items[0].ID != created.ID {
		t.Fatalf("list = %+v", list)
	}

	patchReq := withStationID(httptest.NewRequest(http.MethodPatch, "/", strings.NewReader(`{"isFavorite": true}`)), created.ID)
	patchRec := httptest.NewRecorder()
	h.PatchStation(patchRec, patchReq)
	if patchRec.Code != http.StatusOK {
		t.Fatalf("patch status = %d", patchRec.Code)
	}
	var patched Station
	if err := json.NewDecoder(patchRec.Body).Decode(&patched); err != nil {
		t.Fatal(err)
	}
	if !patched.IsFavorite {
		t.Fatalf("patched = %+v", patched)
	}

	deleteReq := withStationID(httptest.NewRequest(http.MethodDelete, "/", nil), created.ID)
	deleteRec := httptest.NewRecorder()
	h.DeleteStation(deleteRec, deleteReq)
	if deleteRec.Code != http.StatusNoContent {
		t.Fatalf("delete status = %d", deleteRec.Code)
	}
}

func TestHandlersSearchAndImportStation(t *testing.T) {
	db := testutil.OpenMigratedDB(t)
	store := newTestService(db)
	searcher := fakeSearcher{result: SearchResult{
		StationUUID: "abc",
		Name:        "Radio Paradise",
		StreamURL:   "https://stream.radioparadise.com/mp3-192",
		HomepageURL: "https://radioparadise.com",
		Tags:        []string{"rock"},
		Codec:       "MP3",
		Bitrate:     192,
	}}
	h := NewHandler(store, searcher, nil)

	searchRec := httptest.NewRecorder()
	h.SearchStations(searchRec, httptest.NewRequest(http.MethodGet, "/?q=paradise", nil))
	if searchRec.Code != http.StatusOK {
		t.Fatalf("search status = %d", searchRec.Code)
	}

	importRec := httptest.NewRecorder()
	h.ImportStation(importRec, httptest.NewRequest(http.MethodPost, "/", strings.NewReader(`{"stationUuid": "abc"}`)))
	if importRec.Code != http.StatusCreated {
		t.Fatalf("import status = %d, body = %s", importRec.Code, importRec.Body.String())
	}
	var station Station
	if err := json.NewDecoder(importRec.Body).Decode(&station); err != nil {
		t.Fatal(err)
	}
	if station.Source != RadioBrowserSource || station.ExternalID != "abc" {
		t.Fatalf("station = %+v", station)
	}
}

func TestHandlersListCatalogOptions(t *testing.T) {
	h := NewHandler(nil, fakeSearcher{}, nil)

	countriesRec := httptest.NewRecorder()
	h.ListCatalogCountries(countriesRec, httptest.NewRequest(http.MethodGet, "/", nil))
	if countriesRec.Code != http.StatusOK {
		t.Fatalf("countries status = %d", countriesRec.Code)
	}
	var countries CatalogOptionList
	if err := json.NewDecoder(countriesRec.Body).Decode(&countries); err != nil {
		t.Fatal(err)
	}
	if countries.Total != 1 || countries.Items[0].Code != "CH" {
		t.Fatalf("countries = %+v", countries)
	}

	tagsRec := httptest.NewRecorder()
	h.ListCatalogTags(tagsRec, httptest.NewRequest(http.MethodGet, "/", nil))
	if tagsRec.Code != http.StatusOK {
		t.Fatalf("tags status = %d", tagsRec.Code)
	}
	var tags CatalogOptionList
	if err := json.NewDecoder(tagsRec.Body).Decode(&tags); err != nil {
		t.Fatal(err)
	}
	if tags.Total != 1 || tags.Items[0].Name != "jazz" {
		t.Fatalf("tags = %+v", tags)
	}
}

func TestHandlersPreviewCatalogStationStreamsWithoutImport(t *testing.T) {
	db := testutil.OpenMigratedDB(t)
	store := newTestService(db)
	streamer := &fakeStreamer{}
	searcher := fakeSearcher{result: SearchResult{
		StationUUID: "abc",
		Name:        "Radio Swiss Jazz",
		StreamURL:   "https://stream.example.com/jazz",
		Country:     "Switzerland",
		Language:    "italian",
		Tags:        []string{"jazz", "public radio"},
		Codec:       "AAC",
		Bitrate:     128,
	}}
	h := NewHandler(store, searcher, streamer)

	rec := httptest.NewRecorder()
	h.PreviewStation(rec, httptest.NewRequest(http.MethodPost, "/", strings.NewReader(`{"stationUuid": "abc"}`)))
	if rec.Code != http.StatusOK {
		t.Fatalf("preview status = %d, body = %s", rec.Code, rec.Body.String())
	}
	if streamer.station.ID != "preview:abc" || streamer.station.Source != RadioBrowserSource {
		t.Fatalf("preview station = %+v", streamer.station)
	}

	list, err := store.ListStations(context.Background(), "00000000-0000-0000-0000-000000000001")
	if err != nil {
		t.Fatal(err)
	}
	if list.Total != 0 {
		t.Fatalf("preview imported station, list = %+v", list)
	}
}

func TestHandlersGetStationReturnsMetadata(t *testing.T) {
	h, store := setupHandlers(t)
	station, err := store.CreateStation(context.Background(), "00000000-0000-0000-0000-000000000001", StationCreate{
		Name:        "Radio Paradise",
		StreamURL:   "https://stream.radioparadise.com/mp3-192",
		HomepageURL: "https://radioparadise.com",
		FaviconURL:  "https://radioparadise.com/favicon.ico",
		Country:     "United States",
		Language:    "english",
		Tags:        []string{"rock", "eclectic"},
		Codec:       "MP3",
		Bitrate:     192,
		Source:      RadioBrowserSource,
		ExternalID:  "rp-main",
	})
	if err != nil {
		t.Fatal(err)
	}

	rec := httptest.NewRecorder()
	h.GetStation(rec, withStationID(httptest.NewRequest(http.MethodGet, "/", nil), station.ID))
	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", rec.Code, rec.Body.String())
	}
	var body Station
	if err := json.NewDecoder(rec.Body).Decode(&body); err != nil {
		t.Fatal(err)
	}
	if body.ID != station.ID || body.HomepageURL != "https://radioparadise.com" || body.Bitrate != 192 {
		t.Fatalf("station = %+v", body)
	}
}

func TestHandlersGetStationNotFound(t *testing.T) {
	h, _ := setupHandlers(t)
	rec := httptest.NewRecorder()
	h.GetStation(rec, withStationID(httptest.NewRequest(http.MethodGet, "/", nil), "missing"))
	if rec.Code != http.StatusNotFound {
		t.Fatalf("status = %d", rec.Code)
	}
}

func TestHandlersGetNowPlaying(t *testing.T) {
	h, store := setupHandlers(t)
	station, err := store.CreateStation(context.Background(), "00000000-0000-0000-0000-000000000001", StationCreate{
		Name:      "Radio Paradise",
		StreamURL: "https://stream.radioparadise.com/mp3-192",
	})
	if err != nil {
		t.Fatal(err)
	}
	now := NowPlaying{Title: "Title", Artist: "Artist", Raw: "Artist - Title"}
	if err := store.UpdateNowPlaying(context.Background(), station.ID, now); err != nil {
		t.Fatal(err)
	}

	rec := httptest.NewRecorder()
	h.GetNowPlaying(rec, withStationID(httptest.NewRequest(http.MethodGet, "/", nil), station.ID))
	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d", rec.Code)
	}
	var body NowPlaying
	if err := json.NewDecoder(rec.Body).Decode(&body); err != nil {
		t.Fatal(err)
	}
	if body.Title != "Title" || body.Artist != "Artist" {
		t.Fatalf("now playing = %+v", body)
	}
}
