package managedimport_test

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/modules/managedimport"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
)

func TestTagImportAcceptsOptionalGenreAndArtwork(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	fixture := withoutFrontCover(t, readStrictFLACFixture(t))
	fixture = replaceFixtureTag(t, fixture, "GENRE=Electronic", "XENRE=Electronic")
	trackID := importOneFLAC(t, router, fixture, "no-cover.flac")
	if trackID == "" {
		t.Fatal("import returned no Track")
	}
	tracks := listTracks(t, router)
	if len(tracks.Items) != 1 {
		t.Fatalf("library = %+v", tracks)
	}
}

func TestTagImportRejectsMissingTitle(t *testing.T) {
	fixture := replaceFixtureTag(t, readStrictFLACFixture(t), "TITLE=  Inspection   Fixture  ", "XITLE=  Inspection   Fixture  ")
	response, _ := uploadManagedImportFixture(t, fixture)
	if response.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d: %s", response.Code, response.Body)
	}
}

func TestTagImportBatchSelectsOneAlternativeAndExternalCover(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	batch := createTagBatch(t, router)
	first := withoutFrontCover(t, readStrictFLACFixture(t))
	second := replaceFixtureTag(t, first, "ARTIST=Test Artist", "ARTIST=New Artist ")
	firstID := createBatchImportJob(t, router, batch.ID)
	secondID := createBatchImportJob(t, router, batch.ID)
	uploadFLACToJob(t, router, firstID, first, "first.flac")
	uploadFLACToJob(t, router, secondID, second, "second.flac")
	batch = getImportBatch(t, router, batch.ID)
	if len(batch.Albums) != 1 {
		t.Fatalf("Albums = %+v", batch.Albums)
	}
	conflict := confirmImportBatch(t, router, batch, []string{firstID, secondID})
	if conflict.Code != http.StatusBadRequest {
		t.Fatalf("alternatives status = %d: %s", conflict.Code, conflict.Body)
	}
	upload := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/import-batches/"+batch.ID+"/albums/"+batch.Albums[0].Key+"/artwork", bytes.NewReader(encodeAlternatePNG(t)), nil)
	if upload.Code != http.StatusOK {
		t.Fatalf("artwork upload = %d: %s", upload.Code, upload.Body)
	}
	var artwork managedimport.ArtworkOption
	testutil.DecodeJSON(t, upload, &artwork)
	confirmation := managedimport.BatchConfirmation{Revision: batch.Revision, SelectedFileIDs: []string{secondID}, AlbumDecisions: []managedimport.AlbumDecision{{AlbumKey: batch.Albums[0].Key, ArtworkMode: "selected", ArtworkID: artwork.ID}}}
	result := confirmTagBatch(t, router, batch.ID, confirmation)
	if result.Code != http.StatusOK {
		t.Fatalf("confirm = %d: %s", result.Code, result.Body)
	}
	tracks := listTracks(t, router)
	if len(tracks.Items) != 1 {
		t.Fatalf("Tracks = %+v", tracks)
	}
	var committed managedimport.Batch
	testutil.DecodeJSON(t, result, &committed)
	if committed.Files[1].Outcome != managedimport.OUTCOME_IMPORTED {
		t.Fatalf("result = %+v", committed)
	}
	removed := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/import-batches/"+batch.ID+"/artwork/"+artwork.ID, nil, nil)
	if removed.Code != http.StatusNotFound {
		t.Fatalf("staged artwork survived completion: %d", removed.Code)
	}
}

func createTagBatch(t *testing.T, router http.Handler) managedimport.Batch {
	t.Helper()
	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches", nil, nil)
	if response.Code != http.StatusCreated {
		t.Fatalf("create batch = %d: %s", response.Code, response.Body)
	}
	var batch managedimport.Batch
	testutil.DecodeJSON(t, response, &batch)
	return batch
}

func confirmTagBatch(t *testing.T, router http.Handler, batchID string, confirmation managedimport.BatchConfirmation) *httptest.ResponseRecorder {
	t.Helper()
	body, err := json.Marshal(confirmation)
	if err != nil {
		t.Fatal(err)
	}
	return testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches/"+batchID+"/confirm", bytes.NewReader(body), map[string]string{"Content-Type": "application/json"})
}

func TestTagImportBatchReplacesOnlyAfterExplicitConfirmation(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	original := withoutFrontCover(t, readStrictFLACFixture(t))
	trackID := importOneFLAC(t, router, original, "original.flac")
	batch := createTagBatch(t, router)
	replacement := replaceFixtureTag(t, original, "ARTIST=Test Artist", "ARTIST=New Artist ")
	jobID := createBatchImportJob(t, router, batch.ID)
	preview := uploadFLACToJob(t, router, jobID, replacement, "replacement.flac")
	if preview.DuplicateClassification != managedimport.DUPLICATE_NONE || len(preview.MatchingTracks) != 1 {
		t.Fatalf("preview = %+v", preview)
	}
	batch = getImportBatch(t, router, batch.ID)
	rejected := confirmImportBatch(t, router, batch, []string{jobID})
	if rejected.Code != http.StatusConflict {
		t.Fatalf("unconfirmed replacement = %d: %s", rejected.Code, rejected.Body)
	}
	result := confirmTagBatch(t, router, batch.ID, managedimport.BatchConfirmation{Revision: batch.Revision, SelectedFileIDs: []string{jobID}, DuplicateDecisions: []managedimport.DuplicateDecision{{JobID: jobID, Action: managedimport.DUPLICATE_ACTION_REPLACE_EXISTING, TrackID: trackID, TargetRevision: 1}}})
	if result.Code != http.StatusOK {
		t.Fatalf("replacement = %d: %s", result.Code, result.Body)
	}
	var completed managedimport.Batch
	testutil.DecodeJSON(t, result, &completed)
	if completed.Files[0].Outcome != managedimport.OUTCOME_REPLACED || completed.Files[0].TrackID != trackID {
		t.Fatalf("replacement result = %+v", completed)
	}
	tracks := listTracks(t, router)
	if len(tracks.Items) != 1 || tracks.Items[0].ID != trackID {
		t.Fatalf("replacement changed identity: %+v", tracks)
	}
}

func TestTagImportGroupsYearsAndArtistsButSeparatesDeluxe(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	first := replaceFixtureTag(t, readStrictFLACFixture(t), "ALBUM=Strict Import Tests", "ALBUM=Album X")
	importOneFLAC(t, router, first, "first.flac")
	next := replaceFixtureTag(t, secondTrackFixture(first), "DATE=2026", "DATE=2001")
	next = replaceFixtureTag(t, next, "ARTIST=Test Artist", "ARTIST=New Artist ")
	importOneFLAC(t, router, next, "second.flac")
	deluxe := replaceFixtureTag(t, readStrictFLACFixture(t), "ALBUM=Strict Import Tests", "ALBUM=Album X (Deluxe)")
	importOneFLAC(t, router, deluxe, "deluxe.flac")
	tracks := listTracks(t, router)
	if len(tracks.Items) != 3 {
		t.Fatalf("Tracks = %+v", tracks)
	}
	response := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/library/albums", nil, nil)
	var albums struct {
		Items []struct {
			ID         string `json:"id"`
			Title      string `json:"title"`
			TrackCount int    `json:"trackCount"`
		} `json:"items"`
	}
	testutil.DecodeJSON(t, response, &albums)
	if len(albums.Items) != 2 {
		t.Fatalf("Albums = %+v", albums)
	}
	for _, album := range albums.Items {
		if album.Title == "Album X" && album.TrackCount != 2 {
			t.Fatalf("year/Artist split Album: %+v", album)
		}
	}
}

func TestTagImportRequiresCoverChoiceAndPreservesExistingCover(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	router := newManagedImportTestRouterWithDatabase(t, database, t.TempDir())
	batch := createTagBatch(t, router)
	original := readStrictFLACFixture(t)
	firstID := createBatchImportJob(t, router, batch.ID)
	secondID := createBatchImportJob(t, router, batch.ID)
	uploadFLACToJob(t, router, firstID, original, "first.flac")
	uploadFLACToJob(t, router, secondID, replaceFrontCover(t, secondTrackFixture(original), encodeAlternatePNG(t)), "second.flac")
	batch = getImportBatch(t, router, batch.ID)
	if len(batch.Albums[0].Artworks) != 2 {
		t.Fatalf("cover options: %+v", batch.Albums)
	}
	rejected := confirmImportBatch(t, router, batch, []string{firstID, secondID})
	if rejected.Code != http.StatusBadRequest {
		t.Fatalf("ambiguous covers: %d %s", rejected.Code, rejected.Body)
	}
	selectedCover := batch.Albums[0].Artworks[0]
	result := confirmTagBatch(t, router, batch.ID, managedimport.BatchConfirmation{Revision: batch.Revision, SelectedFileIDs: []string{firstID, secondID}, AlbumDecisions: []managedimport.AlbumDecision{{AlbumKey: batch.Albums[0].Key, ArtworkMode: "selected", ArtworkID: selectedCover.ID}}})
	if result.Code != http.StatusOK {
		t.Fatalf("selected cover: %d %s", result.Code, result.Body)
	}
	var hash string
	if err := database.QueryRow(`SELECT content_sha256 FROM album_artwork`).Scan(&hash); err != nil || hash != selectedCover.ContentSHA256 {
		t.Fatalf("cover hash: %q %v", hash, err)
	}
	importOneFLAC(t, router, thirdTrackFixture(original), "third.flac")
	if err := database.QueryRow(`SELECT content_sha256 FROM album_artwork`).Scan(&hash); err != nil || hash != selectedCover.ContentSHA256 {
		t.Fatalf("existing cover changed: %q %v", hash, err)
	}
}

func TestTagImportArtworkValidationAndCancellation(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	batch := createTagBatch(t, router)
	jobID := createBatchImportJob(t, router, batch.ID)
	uploadFLACToJob(t, router, jobID, withoutFrontCover(t, readStrictFLACFixture(t)), "no-cover.flac")
	batch = getImportBatch(t, router, batch.ID)
	path := "/api/v1/import-batches/" + batch.ID + "/albums/" + batch.Albums[0].Key + "/artwork"
	invalid := testutil.ServeRequest(t, router, http.MethodPut, path, bytes.NewReader([]byte("not a PNG")), map[string]string{"Content-Type": "image/png"})
	if invalid.Code != http.StatusUnprocessableEntity {
		t.Fatalf("forged image: %d %s", invalid.Code, invalid.Body)
	}
	uploaded := testutil.ServeRequest(t, router, http.MethodPut, path, bytes.NewReader(encodeAlternatePNG(t)), nil)
	if uploaded.Code != http.StatusOK {
		t.Fatalf("valid image: %d %s", uploaded.Code, uploaded.Body)
	}
	var artwork managedimport.ArtworkOption
	testutil.DecodeJSON(t, uploaded, &artwork)
	canceled := testutil.ServeRequest(t, router, http.MethodDelete, "/api/v1/import-batches/"+batch.ID, nil, nil)
	if canceled.Code != http.StatusNoContent {
		t.Fatalf("cancel: %d %s", canceled.Code, canceled.Body)
	}
	image := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/import-batches/"+batch.ID+"/artwork/"+artwork.ID, nil, nil)
	if image.Code != http.StatusNotFound {
		t.Fatalf("canceled image survived: %d", image.Code)
	}
}

func TestTagImportRejectsStaleReplacementVersion(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	router := newTrackReplacementRouter(t, database, t.TempDir())
	original := withoutFrontCover(t, readStrictFLACFixture(t))
	trackID := importOneFLAC(t, router, original, "original.flac")
	batch := createTagBatch(t, router)
	jobID := createBatchImportJob(t, router, batch.ID)
	uploadFLACToJob(t, router, jobID, replaceFixtureTag(t, original, "ARTIST=Test Artist", "ARTIST=New Artist "), "replacement.flac")
	batch = getImportBatch(t, router, batch.ID)
	revision := batch.Albums[0].ExistingAlbums[0].Tracks[0].Revision
	if _, err := database.Exec(`UPDATE tracks SET revision = revision + 1 WHERE id = ?`, trackID); err != nil {
		t.Fatal(err)
	}
	result := confirmTagBatch(t, router, batch.ID, managedimport.BatchConfirmation{Revision: batch.Revision, SelectedFileIDs: []string{jobID}, DuplicateDecisions: []managedimport.DuplicateDecision{{JobID: jobID, TrackID: trackID, TargetRevision: revision, Action: managedimport.DUPLICATE_ACTION_REPLACE_EXISTING}}})
	if result.Code != http.StatusConflict {
		t.Fatalf("stale replacement: %d %s", result.Code, result.Body)
	}
	assertStreamedBytes(t, router, trackID, original)
}

func TestTagImportCanSeparateEditionWithDifferentTrackTotal(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	original := readStrictFLACFixture(t)
	importOneFLAC(t, router, original, "original.flac")
	batch := createTagBatch(t, router)
	jobID := createBatchImportJob(t, router, batch.ID)
	uploadFLACToJob(t, router, jobID, replaceFixtureTag(t, original, "TRACKNUMBER=3/9", "TRACKNUMBER=3/8"), "edition.flac")
	batch = getImportBatch(t, router, batch.ID)
	result := confirmTagBatch(t, router, batch.ID, managedimport.BatchConfirmation{Revision: batch.Revision, SelectedFileIDs: []string{jobID}, AlbumDecisions: []managedimport.AlbumDecision{{AlbumKey: batch.Albums[0].Key, CreateSeparate: true}}})
	if result.Code != http.StatusOK {
		t.Fatalf("separate edition: %d %s", result.Code, result.Body)
	}
	if len(listTracks(t, router).Items) != 2 {
		t.Fatal("separate edition was not imported")
	}
}

func TestTagImportSeparateMultiDiscRequiresExplicitDiscNumbers(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	original := readStrictFLACFixture(t)
	importOneFLAC(t, router, original, "original.flac")
	batch := createTagBatch(t, router)
	firstID := createBatchImportJob(t, router, batch.ID)
	secondID := createBatchImportJob(t, router, batch.ID)
	uploadFLACToJob(t, router, firstID, replaceFixtureTag(t, original, "DISCNUMBER=1/1", "XISCNUMBER=1/1"), "missing-disc.flac")
	uploadFLACToJob(t, router, secondID, replaceFixtureTag(t, original, "DISCNUMBER=1/1", "DISCNUMBER=2/2"), "second-disc.flac")
	batch = getImportBatch(t, router, batch.ID)
	result := confirmTagBatch(t, router, batch.ID, managedimport.BatchConfirmation{Revision: batch.Revision, SelectedFileIDs: []string{firstID, secondID}, AlbumDecisions: []managedimport.AlbumDecision{{AlbumKey: batch.Albums[0].Key, CreateSeparate: true}}})
	if result.Code != http.StatusUnprocessableEntity {
		t.Fatalf("missing disc: %d %s", result.Code, result.Body)
	}
	if len(listTracks(t, router).Items) != 1 {
		t.Fatal("invalid multi-disc plan partially committed")
	}
}

func TestTagImportUploadingEmbeddedCoverBytesRemainsSelectable(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	original := readStrictFLACFixture(t)
	batch := createTagBatch(t, router)
	jobID := createBatchImportJob(t, router, batch.ID)
	uploadFLACToJob(t, router, jobID, original, "original.flac")
	batch = getImportBatch(t, router, batch.ID)
	response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/import-batches/"+batch.ID+"/albums/"+batch.Albums[0].Key+"/artwork", bytes.NewReader(embeddedFrontCover(t, original)), nil)
	if response.Code != http.StatusOK {
		t.Fatalf("same cover upload: %d %s", response.Code, response.Body)
	}
	var artwork managedimport.ArtworkOption
	testutil.DecodeJSON(t, response, &artwork)
	batch = getImportBatch(t, router, batch.ID)
	if len(batch.Albums[0].Artworks) != 1 || batch.Albums[0].Artworks[0].ID != artwork.ID {
		t.Fatalf("deduplicated cover selection: %+v", batch.Albums[0].Artworks)
	}
	result := confirmTagBatch(t, router, batch.ID, managedimport.BatchConfirmation{Revision: batch.Revision, SelectedFileIDs: []string{jobID}, AlbumDecisions: []managedimport.AlbumDecision{{AlbumKey: batch.Albums[0].Key, ArtworkMode: "selected", ArtworkID: artwork.ID}}})
	if result.Code != http.StatusOK {
		t.Fatalf("same cover confirmation: %d %s", result.Code, result.Body)
	}
}
