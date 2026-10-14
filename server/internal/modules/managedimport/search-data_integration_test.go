package managedimport_test

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"github.com/ardam/navidrome-replacement/server/internal/api/gen"
	"net/http"
	"net/url"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/modules/managedimport"
	"github.com/ardam/navidrome-replacement/server/internal/searchdata"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
)

func TestSearchDataFollowsHTTPImportReplacementAndDeletion(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	router := newTrackReplacementRouter(t, database, t.TempDir())
	assertSearchTrack(t, router, "beyonce inspection", "")
	original := replaceFixtureTag(t, readStrictFLACFixture(t), "ARTIST=Test Artist", "ARTIST=Beyoncé")
	trackID := importOneFLAC(t, router, original, "original.flac")
	assertPreparedField(t, database, trackID, "track_artist", "Beyoncé", "beyonce")
	assertSearchTrack(t, router, "beyonce inspection", trackID)
	assertSearchTrack(t, router, "sebnem inspection", "")
	assertStreamedBytes(t, router, trackID, original)
	replacement := replaceFixtureTag(t, original, "ARTIST=Beyoncé", "ARTIST=Şebnem")
	job := createTrackReplacementJob(t, router, trackID)
	preview := uploadFLACToJob(t, router, job.ID, replacement, "replacement.flac")
	assertPreparedField(t, database, trackID, "track_artist", "Beyoncé", "beyonce")
	assertSearchTrack(t, router, "beyonce inspection", trackID)
	assertSearchTrack(t, router, "sebnem inspection", "")
	if preview.Replacement == nil {
		t.Fatal("missing replacement preview")
	}
	confirmed := serveReplacementConfirmation(router, job.ID, preview.Revision, preview.Replacement.ConfirmationToken, true)
	if confirmed.Code != http.StatusOK {
		t.Fatalf("replacement: %d %s", confirmed.Code, confirmed.Body)
	}
	assertPreparedField(t, database, trackID, "track_artist", "Şebnem", "sebnem")
	assertSearchTrack(t, router, "sebnem inspection", trackID)
	assertSearchTrack(t, router, "beyonce inspection", "")
	tracks := listTracks(t, router)
	if len(tracks.Items) != 1 || tracks.Items[0].ID != trackID || len(tracks.Items[0].Artists) != 1 || tracks.Items[0].Artists[0].Name != "Şebnem" {
		t.Fatalf("source metadata: %+v", tracks)
	}
	assertStreamedBytes(t, router, trackID, replacement)
	deleteSearchTrack(t, router, trackID)
	assertSearchTrack(t, router, "sebnem inspection", "")
	if _, err := searchdata.NewRepository(database).GetDocument(context.Background(), "track", trackID, "user-1"); !errors.Is(err, sql.ErrNoRows) {
		t.Fatalf("deleted search record: %v", err)
	}
}

func assertPreparedField(t *testing.T, database *sql.DB, trackID, name, original, folded string) {
	t.Helper()
	document, err := searchdata.NewRepository(database).GetDocument(context.Background(), "track", trackID, "user-1")
	if err != nil {
		t.Fatal(err)
	}
	for _, field := range document.Fields {
		if field.Name != name {
			continue
		}
		if field.Text.Original != original || field.Text.Folded != folded {
			t.Fatalf("prepared field: %+v", field)
		}
		return
	}
	t.Fatalf("missing field %s", name)
}

func deleteSearchTrack(t *testing.T, router http.Handler, trackID string) {
	t.Helper()
	response := performTrackDeletionRequest(t, router, http.MethodGet, "/api/v1/library/tracks/"+trackID+"/deletion", nil, false)
	if response.Code != http.StatusOK {
		t.Fatalf("deletion preview: %d %s", response.Code, response.Body)
	}
	var preview managedimport.TrackDeletionPreview
	testutil.DecodeJSON(t, response, &preview)
	body, err := json.Marshal(managedimport.TrackDeletionConfirmation{ConfirmationToken: preview.ConfirmationToken})
	if err != nil {
		t.Fatal(err)
	}
	response = performTrackDeletionRequest(t, router, http.MethodDelete, "/api/v1/library/tracks/"+trackID, body, true)
	if response.Code != http.StatusOK {
		t.Fatalf("deletion: %d %s", response.Code, response.Body)
	}
}

func TestSearchPreparationFailureRejectsHTTPReplacement(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	router := newTrackReplacementRouter(t, database, t.TempDir())
	original := readStrictFLACFixture(t)
	trackID := importOneFLAC(t, router, original, "original.flac")
	replacement := replaceFixtureTag(t, original, "ARTIST=Test Artist", "ARTIST=Şebnem")
	job := createTrackReplacementJob(t, router, trackID)
	preview := uploadFLACToJob(t, router, job.ID, replacement, "replacement.flac")
	if preview.Replacement == nil {
		t.Fatal("missing replacement preview")
	}
	if _, err := database.Exec(`CREATE TRIGGER reject_search_write BEFORE INSERT ON library_search_texts WHEN NEW.kind = 'track' BEGIN SELECT RAISE(ABORT, 'injected search preparation failure'); END`); err != nil {
		t.Fatal(err)
	}
	response := serveReplacementConfirmation(router, job.ID, preview.Revision, preview.Replacement.ConfirmationToken, true)
	if response.Code < http.StatusBadRequest {
		t.Fatalf("preparation failure reported success: %d %s", response.Code, response.Body)
	}
	assertPreparedField(t, database, trackID, "track_artist", "Test Artist", "test artist")
	assertStreamedBytes(t, router, trackID, original)
	assertSearchTrack(t, router, "Test Artist inspection", trackID)
	assertSearchTrack(t, router, "sebnem inspection", "")
}

func assertSearchTrack(t *testing.T, router http.Handler, query, trackID string) {
	t.Helper()
	response := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/library/search?q="+url.QueryEscape(query), nil, nil)
	if response.Code != http.StatusOK {
		t.Fatalf("search %q: %d %s", query, response.Code, response.Body)
	}
	var result gen.LibrarySearchResponse
	testutil.DecodeJSON(t, response, &result)
	tracks := result.Tracks
	if result.BestMatch != nil && result.BestMatch.Type == "track" {
		tracks = append(tracks, *result.BestMatch)
	}
	if trackID == "" {
		if len(tracks) != 0 {
			t.Fatalf("search %q unexpectedly returned Tracks: %+v", query, tracks)
		}
		return
	}
	for _, track := range tracks {
		if track.Id == trackID {
			return
		}
	}
	t.Fatalf("search %q missing Track %s: %+v", query, trackID, result)
}
