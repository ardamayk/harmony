package managedimport_test

import (
	"bytes"
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/binary"
	"errors"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/ardam/navidrome-replacement/server/internal/config"
	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/ardam/navidrome-replacement/server/internal/modules/managedimport"
	"github.com/ardam/navidrome-replacement/server/internal/modules/playback"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
	"github.com/go-chi/chi/v5"
	"github.com/google/uuid"
	flacmeta "github.com/mewkiz/flac/meta"
)

const (
	FLAC_METADATA_BLOCK_LAST_FLAG            byte = 0x80
	FLAC_METADATA_BLOCK_TYPE_MASK            byte = 0x7f
	FLAC_PICTURE_FIELD_SIZE_BYTES                 = 4
	FLAC_PICTURE_DIMENSION_FIELDS_SIZE_BYTES      = 16
	FLAC_METADATA_LENGTH_HIGH_BYTE_OFFSET         = 1
	FLAC_METADATA_LENGTH_MIDDLE_BYTE_OFFSET       = 2
	FLAC_METADATA_LENGTH_LOW_BYTE_OFFSET          = 3
	FLAC_METADATA_LENGTH_HIGH_SHIFT               = 16
	FLAC_METADATA_LENGTH_MIDDLE_SHIFT             = 8
)

func TestManagedImportCommitsOneStrictFLACThroughLibraryPlayback(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	managedStoragePath := t.TempDir()
	configuration := config.Config{
		ManagedStoragePath: managedStoragePath,
	}
	libraryModule := library.NewModule(database)
	importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	playbackModule := playback.NewModule(database, libraryModule.TrackAccess())
	router := chi.NewRouter()
	importModule.RegisterRoutes(router)
	libraryModule.RegisterRoutes(router)
	playbackModule.RegisterRoutes(router)
	fixture := readStrictFLACFixture(t)

	jobResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", nil, nil)
	if jobResponse.Code != http.StatusCreated {
		t.Fatalf("create Import Job status = %d, body = %s", jobResponse.Code, jobResponse.Body.String())
	}
	var job struct {
		ID       string `json:"id"`
		Status   string `json:"status"`
		Revision int    `json:"revision"`
	}
	testutil.DecodeJSON(t, jobResponse, &job)
	if job.ID == "" || job.Status != "uploading" || job.Revision != 1 {
		t.Fatalf("created Import Job = %+v", job)
	}

	uploadResponse := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+job.ID+"/file", bytes.NewReader(fixture), map[string]string{
		"Content-Type":      "audio/flac",
		"X-Import-Filename": "strict-import.flac",
	})
	if uploadResponse.Code != http.StatusOK {
		t.Fatalf("upload status = %d, body = %s", uploadResponse.Code, uploadResponse.Body.String())
	}
	var preview struct {
		JobID    string `json:"jobId"`
		Status   string `json:"status"`
		Revision int    `json:"revision"`
		File     struct {
			Title            string   `json:"title"`
			Artists          []string `json:"artists"`
			AlbumArtists     []string `json:"albumArtists"`
			Album            string   `json:"album"`
			Genres           []string `json:"genres"`
			TrackNo          int      `json:"trackNo"`
			DiscNo           int      `json:"discNo"`
			Format           string   `json:"format"`
			Container        string   `json:"container"`
			Codec            string   `json:"codec"`
			DurationMs       int      `json:"durationMs"`
			SampleRateHz     int      `json:"sampleRateHz"`
			ChannelCount     int      `json:"channelCount"`
			BitDepth         int      `json:"bitDepth"`
			BitrateKbps      int      `json:"bitrateKbps"`
			ArtworkMediaType string   `json:"artworkMediaType"`
		} `json:"file"`
	}
	testutil.DecodeJSON(t, uploadResponse, &preview)
	if preview.JobID != job.ID || preview.Status != "awaiting_confirmation" || preview.Revision != 2 {
		t.Fatalf("Import Preview = %+v", preview)
	}
	if preview.File.Title != "Inspection Fixture" || preview.File.Album != "Strict Import Tests" || preview.File.TrackNo != 3 || preview.File.DiscNo != 1 {
		t.Fatalf("Import Preview file = %+v", preview.File)
	}
	if strings.Join(preview.File.Artists, ",") != "Test Artist" || strings.Join(preview.File.AlbumArtists, ",") != "Test Album Artist" || strings.Join(preview.File.Genres, ",") != "Electronic" {
		t.Fatalf("Import Preview relationships = %+v", preview.File)
	}
	if preview.File.Format != "flac" || preview.File.Container != "flac" || preview.File.Codec != "flac" || preview.File.ArtworkMediaType != "image/png" {
		t.Fatalf("Import Preview media = %+v", preview.File)
	}
	assertStagedStorage(t, managedStoragePath, job.ID, fixture)
	if preview.File.DurationMs <= 0 || preview.File.SampleRateHz <= 0 || preview.File.ChannelCount <= 0 || preview.File.BitDepth <= 0 || preview.File.BitrateKbps <= 0 {
		t.Fatalf("Import Preview technical audio properties = %+v", preview.File)
	}

	tracksBeforeConfirm := listTracks(t, router)
	if len(tracksBeforeConfirm.Items) != 0 {
		t.Fatalf("Tracks before confirmation = %+v", tracksBeforeConfirm.Items)
	}
	staleResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports/"+job.ID+"/confirm", strings.NewReader(`{"revision":1}`), map[string]string{"Content-Type": "application/json"})
	if staleResponse.Code != http.StatusConflict {
		t.Fatalf("stale confirmation status = %d, body = %s", staleResponse.Code, staleResponse.Body.String())
	}

	confirmationBody := strings.NewReader(`{"revision":2}`)
	confirmResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports/"+job.ID+"/confirm", confirmationBody, map[string]string{"Content-Type": "application/json"})
	if confirmResponse.Code != http.StatusOK {
		t.Fatalf("confirm status = %d, body = %s", confirmResponse.Code, confirmResponse.Body.String())
	}
	var result struct {
		JobID    string `json:"jobId"`
		Status   string `json:"status"`
		Revision int    `json:"revision"`
		TrackID  string `json:"trackId"`
	}
	testutil.DecodeJSON(t, confirmResponse, &result)
	if result.JobID != job.ID || result.Status != "committed" || result.Revision != 3 || result.TrackID == "" {
		t.Fatalf("Managed Import result = %+v", result)
	}

	tracksAfterConfirm := listTracks(t, router)
	if len(tracksAfterConfirm.Items) != 1 || tracksAfterConfirm.Items[0].ID != result.TrackID || tracksAfterConfirm.Items[0].Title != "Inspection Fixture" {
		t.Fatalf("Tracks after confirmation = %+v", tracksAfterConfirm.Items)
	}
	committedTrack := tracksAfterConfirm.Items[0]
	if committedTrack.AlbumTitle != "Strict Import Tests" || committedTrack.DiscNo != 1 || len(committedTrack.Artists) != 1 || committedTrack.Artists[0].Name != "Test Artist" || len(committedTrack.Genres) != 1 || committedTrack.Genres[0].Name != "Electronic" {
		t.Fatalf("normalized Managed Track = %+v", committedTrack)
	}
	if committedTrack.Container != "flac" || committedTrack.Codec != "flac" || committedTrack.DurationMs <= 0 || committedTrack.SampleRateHz <= 0 || committedTrack.ChannelCount <= 0 || committedTrack.BitDepth <= 0 || committedTrack.BitrateBps <= 0 {
		t.Fatalf("persisted Managed Track technical properties = %+v", committedTrack)
	}
	idempotentResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports/"+job.ID+"/confirm", strings.NewReader(`{"revision":2}`), map[string]string{"Content-Type": "application/json"})
	if idempotentResponse.Code != http.StatusOK {
		t.Fatalf("idempotent confirm status = %d, body = %s", idempotentResponse.Code, idempotentResponse.Body.String())
	}
	var idempotentResult struct {
		TrackID string `json:"trackId"`
	}
	testutil.DecodeJSON(t, idempotentResponse, &idempotentResult)
	if idempotentResult.TrackID != result.TrackID || len(listTracks(t, router).Items) != 1 {
		t.Fatalf("idempotent result = %+v", idempotentResult)
	}
	wrongRevisionResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports/"+job.ID+"/confirm", strings.NewReader(`{"revision":1}`), map[string]string{"Content-Type": "application/json"})
	testutil.AssertErrorCode(t, wrongRevisionResponse, http.StatusConflict, managedimport.ERROR_CODE_REVISION_CONFLICT)
	assertNormalizedAlbum(t, router, committedTrack.AlbumID, result.TrackID)
	streamResponse := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/tracks/"+result.TrackID+"/stream", nil, nil)
	if streamResponse.Code != http.StatusOK {
		t.Fatalf("stream status = %d, body = %s", streamResponse.Code, streamResponse.Body.String())
	}
	if !bytes.Equal(streamResponse.Body.Bytes(), fixture) {
		t.Fatal("streamed Managed Track bytes differ from uploaded FLAC")
	}

	assertCanonicalStorage(t, managedStoragePath, fixture)
}

func TestManagedImportSerializesConcurrentConfirmationOfOnePreview(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	importModule.RegisterRoutes(router)
	jobID, revision := uploadFLACForPreview(t, router, readStrictFLACFixture(t), "concurrent.flac")

	const confirmationCount = 8
	responses := make(chan *httptest.ResponseRecorder, confirmationCount)
	start := make(chan struct{})
	var waitGroup sync.WaitGroup
	for range confirmationCount {
		waitGroup.Add(1)
		go func() {
			defer waitGroup.Done()
			<-start
			responses <- serveImportConfirmation(router, jobID, revision)
		}()
	}
	close(start)
	waitGroup.Wait()
	close(responses)

	trackID := ""
	for response := range responses {
		if response.Code != http.StatusOK {
			t.Fatalf("concurrent confirmation status = %d, body = %s", response.Code, response.Body.String())
		}
		var result managedimport.Result
		testutil.DecodeJSON(t, response, &result)
		if trackID == "" {
			trackID = result.TrackID
		}
		if result.TrackID != trackID {
			t.Fatalf("concurrent confirmation Track ID = %q, want %q", result.TrackID, trackID)
		}
	}
	var trackCount int
	if err := database.QueryRow(`SELECT COUNT(*) FROM tracks`).Scan(&trackCount); err != nil || trackCount != 1 {
		t.Fatalf("concurrently confirmed Track count = %d, err = %v", trackCount, err)
	}
}

func TestManagedImportClassifiesExactDuplicateDuringPreview(t *testing.T) {
	managedStoragePath := t.TempDir()
	router := newManagedImportTestRouter(t, managedStoragePath)
	existingTrackID := importOneFLAC(t, router, readStrictFLACFixture(t), "existing.flac")

	response := uploadFixtureThroughRouter(t, router, readStrictFLACFixture(t))
	if response.Code != http.StatusOK {
		t.Fatalf("exact duplicate preview status = %d, body = %s", response.Code, response.Body.String())
	}
	var preview managedimport.Preview
	testutil.DecodeJSON(t, response, &preview)
	if preview.DuplicateClassification != managedimport.DUPLICATE_EXACT || len(preview.DuplicateCandidates) != 1 {
		t.Fatalf("exact duplicate preview = %+v", preview)
	}
	if preview.DuplicateCandidates[0].TrackID != existingTrackID {
		t.Fatalf("exact duplicate Track = %q, want %q", preview.DuplicateCandidates[0].TrackID, existingTrackID)
	}
	if tracks := listTracks(t, router); len(tracks.Items) != 1 {
		t.Fatalf("Tracks after exact duplicate preview = %+v", tracks.Items)
	}
	stagedFiles, err := filepath.Glob(filepath.Join(managedStoragePath, ".staging", "*.upload"))
	if err != nil || len(stagedFiles) != 0 {
		t.Fatalf("Exact Duplicate staging files = %v, err = %v", stagedFiles, err)
	}
}

func TestManagedImportClassifiesOnlySameEditionPositionAsPossibleDuplicate(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	existingTrackID := importOneFLAC(t, router, readStrictFLACFixture(t), "existing.flac")
	fixture := replaceFixtureTag(t, readStrictFLACFixture(t),
		"TITLE=  Inspection   Fixture  ", "TITLE=Inspection       Fixture")

	response := uploadFixtureThroughRouter(t, router, fixture)
	if response.Code != http.StatusOK {
		t.Fatalf("possible duplicate preview status = %d, body = %s", response.Code, response.Body.String())
	}
	var preview managedimport.Preview
	testutil.DecodeJSON(t, response, &preview)
	if preview.DuplicateClassification != managedimport.DUPLICATE_NONE || len(preview.MatchingTracks) != 1 {
		t.Fatalf("possible duplicate preview = %+v", preview)
	}
	if preview.MatchingTracks[0].TrackID != existingTrackID {
		t.Fatalf("possible duplicate Track = %q, want %q", preview.MatchingTracks[0].TrackID, existingTrackID)
	}
}

func TestManagedImportDoesNotClassifyDifferentEditionAsDuplicate(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	importOneFLAC(t, router, readStrictFLACFixture(t), "existing.flac")
	fixture := replaceFixtureTag(t, readStrictFLACFixture(t), "DATE=2026", "DATE=2027")

	response := uploadFixtureThroughRouter(t, router, fixture)
	if response.Code != http.StatusOK {
		t.Fatalf("different edition preview status = %d, body = %s", response.Code, response.Body.String())
	}
	var preview managedimport.Preview
	testutil.DecodeJSON(t, response, &preview)
	if preview.DuplicateClassification != managedimport.DUPLICATE_NONE || len(preview.DuplicateCandidates) != 0 {
		t.Fatalf("different edition duplicate classification = %+v", preview)
	}
}

func TestManagedImportBatchRequiresPossibleDuplicateDecision(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	importOneFLAC(t, router, readStrictFLACFixture(t), "existing.flac")
	batchID := testutil.CreateResourceID(t, router, "/api/v1/import-batches")
	jobID := createBatchImportJob(t, router, batchID)
	fixture := replaceFixtureTag(t, readStrictFLACFixture(t),
		"TITLE=  Inspection   Fixture  ", "TITLE=Inspection       Fixture")
	response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(fixture), map[string]string{
		"Content-Type": "audio/flac", "X-Import-Filename": "candidate.flac",
	})
	if response.Code != http.StatusOK {
		t.Fatalf("Possible Duplicate upload status = %d, body = %s", response.Code, response.Body.String())
	}
	batch := getImportBatch(t, router, batchID)
	body := strings.NewReader(fmt.Sprintf(`{"revision":%d,"selectedFileIds":[%q]}`, batch.Revision, jobID))
	response = testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches/"+batchID+"/confirm", body, map[string]string{"Content-Type": "application/json"})
	testutil.AssertErrorCode(t, response, http.StatusConflict, managedimport.ERROR_CODE_REVISION_CONFLICT)
	if tracks := listTracks(t, router); len(tracks.Items) != 1 {
		t.Fatalf("Tracks after missing duplicate decision = %+v", tracks.Items)
	}
}

func TestManagedImportBatchImportsPossibleDuplicateAsSeparateEdition(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	router := newManagedImportTestRouterWithDatabase(t, database, t.TempDir())
	importOneFLAC(t, router, readStrictFLACFixture(t), "existing.flac")
	batchID := testutil.CreateResourceID(t, router, "/api/v1/import-batches")
	jobID := createBatchImportJob(t, router, batchID)
	secondJobID := createBatchImportJob(t, router, batchID)
	thirdJobID := createBatchImportJob(t, router, batchID)
	fixture := replaceFixtureTag(t, readStrictFLACFixture(t),
		"TITLE=  Inspection   Fixture  ", "TITLE=Inspection       Fixture")
	secondExisting := secondTrackFixture(readStrictFLACFixture(t))
	importOneFLAC(t, router, secondExisting, "second-existing.flac")
	secondFixture := replaceFixtureTag(t, secondExisting, "encoder=Lavf63.1.101", "encoder=Lavf63.1.102")
	thirdFixture := thirdTrackFixture(readStrictFLACFixture(t))
	for _, upload := range []struct {
		jobID, filename string
		fixture         []byte
	}{
		{jobID, "candidate.flac", fixture},
		{secondJobID, "second-candidate.flac", secondFixture},
		{thirdJobID, "third.flac", thirdFixture},
	} {
		response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+upload.jobID+"/file", bytes.NewReader(upload.fixture), map[string]string{
			"Content-Type": "audio/flac", "X-Import-Filename": upload.filename,
		})
		if response.Code != http.StatusOK {
			t.Fatalf("Possible Duplicate upload status = %d, body = %s", response.Code, response.Body.String())
		}
	}
	batch := getImportBatch(t, router, batchID)
	body := strings.NewReader(fmt.Sprintf(`{"revision":%d,"selectedFileIds":[%q,%q,%q],"albumDecisions":[{"albumKey":%q,"createSeparate":true}]}`, batch.Revision, jobID, secondJobID, thirdJobID, batch.Albums[0].Key))
	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches/"+batchID+"/confirm", body, map[string]string{"Content-Type": "application/json"})
	if response.Code != http.StatusOK {
		t.Fatalf("separate-edition confirmation status = %d, body = %s", response.Code, response.Body.String())
	}
	var trackCount, albumCount int
	if err := database.QueryRow(`SELECT COUNT(*) FROM tracks`).Scan(&trackCount); err != nil || trackCount != 5 {
		t.Fatalf("separate-edition Track count = %d, err = %v", trackCount, err)
	}
	if err := database.QueryRow(`SELECT COUNT(*) FROM albums`).Scan(&albumCount); err != nil || albumCount != 2 {
		t.Fatalf("separate-edition Album count = %d, err = %v", albumCount, err)
	}
	var editionTrackCount int
	if err := database.QueryRow(`
		SELECT COUNT(*) FROM tracks WHERE album_id = (
			SELECT album_id FROM tracks WHERE title = 'Inspection Third!!'
		)`).Scan(&editionTrackCount); err != nil || editionTrackCount != 3 {
		t.Fatalf("shared separate-edition Track count = %d, err = %v", editionTrackCount, err)
	}
}

func TestManagedImportBatchKeepsLatePossibleDuplicateReviewable(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	batchID := testutil.CreateResourceID(t, router, "/api/v1/import-batches")
	jobID := createBatchImportJob(t, router, batchID)
	firstFixture := replaceFixtureTag(t, readStrictFLACFixture(t), "TITLE=  Inspection   Fixture  ", "TITLE=Inspection       Fixture")
	secondFixture := replaceFixtureTag(t, firstFixture, "encoder=Lavf63.1.101", "encoder=Lavf63.1.102")
	uploadFLACToJob(t, router, jobID, firstFixture, "first.flac")
	importOneFLAC(t, router, secondFixture, "second.flac")
	batch := getImportBatch(t, router, batchID)
	body := strings.NewReader(fmt.Sprintf(`{"revision":%d,"selectedFileIds":[%q]}`, batch.Revision, jobID))
	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches/"+batchID+"/confirm", body, map[string]string{"Content-Type": "application/json"})
	testutil.AssertErrorCode(t, response, http.StatusConflict, managedimport.ERROR_CODE_REVISION_CONFLICT)
	batch = getImportBatch(t, router, batchID)
	if batch.Status != managedimport.BATCH_STATUS_UPLOADING || len(batch.Files) != 1 || batch.Files[0].Preview == nil || batch.Files[0].Preview.DuplicateClassification != managedimport.DUPLICATE_NONE {
		t.Fatalf("late duplicate batch = %+v", batch)
	}
}

func TestManagedImportBatchReviewsDuplicatesWithinSameBatch(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	batchID := testutil.CreateResourceID(t, router, "/api/v1/import-batches")
	firstJobID := createBatchImportJob(t, router, batchID)
	secondJobID := createBatchImportJob(t, router, batchID)
	firstFixture := replaceFixtureTag(t, readStrictFLACFixture(t), "TITLE=  Inspection   Fixture  ", "TITLE=Inspection       Fixture")
	secondFixture := replaceFixtureTag(t, firstFixture, "encoder=Lavf63.1.101", "encoder=Lavf63.1.102")
	uploadFLACToJob(t, router, firstJobID, firstFixture, "first.flac")
	uploadFLACToJob(t, router, secondJobID, secondFixture, "second.flac")
	batch := getImportBatch(t, router, batchID)
	body := strings.NewReader(fmt.Sprintf(`{"revision":%d,"selectedFileIds":[%q,%q]}`, batch.Revision, firstJobID, secondJobID))
	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches/"+batchID+"/confirm", body, map[string]string{"Content-Type": "application/json"})
	testutil.AssertErrorCode(t, response, http.StatusBadRequest, "invalid_upload")
	batch = getImportBatch(t, router, batchID)
	if batch.Status != managedimport.BATCH_STATUS_UPLOADING || batch.Files[1].Preview == nil || batch.Files[1].Preview.DuplicateClassification != managedimport.DUPLICATE_NONE {
		t.Fatalf("same-batch duplicate review = %+v", batch)
	}
}

func TestManagedImportClassifiesExactHashFromMissingTrack(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	router := newManagedImportTestRouterWithDatabase(t, database, t.TempDir())
	trackID := importOneFLAC(t, router, readStrictFLACFixture(t), "existing.flac")
	if _, err := database.Exec(`UPDATE tracks SET missing_at = CURRENT_TIMESTAMP WHERE id = ?`, trackID); err != nil {
		t.Fatalf("mark Track missing: %v", err)
	}
	response := uploadFixtureThroughRouter(t, router, readStrictFLACFixture(t))
	var preview managedimport.Preview
	testutil.DecodeJSON(t, response, &preview)
	if preview.DuplicateClassification != managedimport.DUPLICATE_EXACT {
		t.Fatalf("missing Track duplicate classification = %q", preview.DuplicateClassification)
	}
}

func TestManagedImportRetainsStagingPathWhenExactCleanupFails(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	storagePath := t.TempDir()
	router := newManagedImportTestRouterWithDatabase(t, database, storagePath)
	fixture := readStrictFLACFixture(t)
	importOneFLAC(t, router, fixture, "existing.flac")
	inspector := &blockingCompletedInspector{completedPath: make(chan string), release: make(chan struct{})}
	service := managedimport.NewService(managedimport.NewRepository(database), managedimport.NewStorage(storagePath, managedimport.StorageLimits{FileBytes: 1 << 20, BatchBytes: 2 << 20}), inspector)
	job, err := service.CreateJob(context.Background(), "", "")
	if err != nil {
		t.Fatalf("create duplicate job: %v", err)
	}
	uploadDone := make(chan error, 1)
	go func() {
		_, uploadErr := service.Upload(context.Background(), job.ID, "duplicate.flac", bytes.NewReader(fixture), int64(len(fixture)))
		uploadDone <- uploadErr
	}()
	stagedPath := <-inspector.completedPath
	if err := os.Remove(stagedPath); err != nil {
		t.Fatalf("replace staged file: %v", err)
	}
	if err := os.Mkdir(stagedPath, 0o750); err != nil {
		t.Fatalf("create staged directory: %v", err)
	}
	if err := os.WriteFile(filepath.Join(stagedPath, "retained"), []byte("x"), 0o600); err != nil {
		t.Fatalf("make staged directory non-empty: %v", err)
	}
	close(inspector.release)
	if uploadErr := <-uploadDone; uploadErr == nil {
		t.Fatal("Exact Duplicate cleanup unexpectedly succeeded")
	}
	var retainedPath sql.NullString
	if err := database.QueryRow(`SELECT staged_file_path FROM managed_import_jobs WHERE id = ?`, job.ID).Scan(&retainedPath); err != nil || !retainedPath.Valid {
		t.Fatalf("retained staging path = %+v, err = %v", retainedPath, err)
	}
}

func TestManagedImportStandaloneRequiresPossibleDuplicateDecision(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	importOneFLAC(t, router, readStrictFLACFixture(t), "existing.flac")
	fixture := replaceFixtureTag(t, readStrictFLACFixture(t),
		"TITLE=  Inspection   Fixture  ", "TITLE=Inspection       Fixture")
	jobID, revision := uploadFLACForPreview(t, router, fixture, "candidate.flac")
	response := serveImportConfirmation(router, jobID, revision)
	testutil.AssertErrorCode(t, response, http.StatusBadRequest, "invalid_upload")
}

func TestManagedImportRechecksPossibleDuplicateAtConfirmation(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	firstFixture := replaceFixtureTag(t, readStrictFLACFixture(t),
		"TITLE=  Inspection   Fixture  ", "TITLE=Inspection       Fixture")
	secondFixture := replaceFixtureTag(t, firstFixture, "encoder=Lavf63.1.101", "encoder=Lavf63.1.102")
	jobID, revision := uploadFLACForPreview(t, router, firstFixture, "first.flac")
	importOneFLAC(t, router, secondFixture, "second.flac")
	response := serveImportConfirmation(router, jobID, revision)
	testutil.AssertErrorCode(t, response, http.StatusBadRequest, "invalid_upload")
	if tracks := listTracks(t, router); len(tracks.Items) != 1 {
		t.Fatalf("Tracks after commit-time Possible Duplicate = %+v", tracks.Items)
	}
}

func TestManagedImportConcurrentExactByteImportsReturnDeterministicDuplicate(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	firstModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	firstRouter := chi.NewRouter()
	firstModule.RegisterRoutes(firstRouter)
	secondModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	secondRouter := chi.NewRouter()
	secondModule.RegisterRoutes(secondRouter)
	fixture := readStrictFLACFixture(t)
	firstJobID, firstRevision := uploadFLACForPreview(t, firstRouter, fixture, "first.flac")
	secondJobID, secondRevision := uploadFLACForPreview(t, secondRouter, fixture, "second.flac")

	type confirmationResult struct {
		response *httptest.ResponseRecorder
		jobID    string
	}
	results := make(chan confirmationResult, 2)
	start := make(chan struct{})
	for _, confirmation := range []struct {
		jobID    string
		revision int
		router   http.Handler
	}{
		{jobID: firstJobID, revision: firstRevision, router: firstRouter},
		{jobID: secondJobID, revision: secondRevision, router: secondRouter},
	} {
		confirmation := confirmation
		go func() {
			<-start
			response := serveImportConfirmation(confirmation.router, confirmation.jobID, confirmation.revision)
			results <- confirmationResult{response: response, jobID: confirmation.jobID}
		}()
	}
	close(start)

	committedCount := 0
	duplicateCount := 0
	duplicateJobID := ""
	for range 2 {
		result := <-results
		switch result.response.Code {
		case http.StatusOK:
			committedCount++
		case http.StatusConflict:
			duplicateCount++
			duplicateJobID = result.jobID
			testutil.AssertErrorCode(t, result.response, http.StatusConflict, managedimport.ERROR_CODE_EXACT_DUPLICATE)
		default:
			t.Fatalf("exact-byte confirmation for job %q status = %d, body = %s", result.jobID, result.response.Code, result.response.Body.String())
		}
	}
	if committedCount != 1 || duplicateCount != 1 {
		t.Fatalf("exact-byte confirmation results = %d committed, %d duplicate", committedCount, duplicateCount)
	}
	wrongRevisionResponse := serveImportConfirmation(firstRouter, duplicateJobID, 1)
	testutil.AssertErrorCode(t, wrongRevisionResponse, http.StatusConflict, managedimport.ERROR_CODE_REVISION_CONFLICT)
	var trackCount int
	if err := database.QueryRow(`SELECT COUNT(*) FROM tracks`).Scan(&trackCount); err != nil || trackCount != 1 {
		t.Fatalf("exact-byte Track count = %d, err = %v", trackCount, err)
	}
}

func TestManagedImportBatchReportsPerFilePartialResults(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	managedStoragePath := t.TempDir()
	configuration := config.Config{ManagedStoragePath: managedStoragePath}
	libraryModule := library.NewModule(database)
	importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	importModule.RegisterRoutes(router)
	libraryModule.RegisterRoutes(router)
	fixture := readStrictFLACFixture(t)

	batchResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches", nil, nil)
	if batchResponse.Code != http.StatusCreated {
		t.Fatalf("create Import Batch status = %d, body = %s", batchResponse.Code, batchResponse.Body.String())
	}
	var batch managedimport.Batch
	testutil.DecodeJSON(t, batchResponse, &batch)

	jobIDs := make([]string, 4)
	clientFileIDs := make([]string, len(jobIDs))
	for index := range jobIDs {
		clientFileIDs[index] = uuid.NewString()
		body := strings.NewReader(fmt.Sprintf(`{"batchId":%q,"clientFileId":%q}`, batch.ID, clientFileIDs[index]))
		response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", body, map[string]string{"Content-Type": "application/json"})
		if response.Code != http.StatusCreated {
			t.Fatalf("create batch file %d status = %d, body = %s", index, response.Code, response.Body.String())
		}
		var job managedimport.Job
		testutil.DecodeJSON(t, response, &job)
		jobIDs[index] = job.ID
	}

	for index, jobID := range jobIDs {
		fileBytes := fixture
		if index == 2 {
			fileBytes = []byte("not audio")
		}
		response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(fileBytes), map[string]string{
			"Content-Type": "audio/flac", "X-Import-Filename": fmt.Sprintf("track-%d.flac", index+1),
		})
		if index == 2 && response.Code != http.StatusUnprocessableEntity {
			t.Fatalf("rejected file status = %d, body = %s", response.Code, response.Body.String())
		}
		if index != 2 && response.Code != http.StatusOK {
			t.Fatalf("accepted file %d status = %d, body = %s", index, response.Code, response.Body.String())
		}
	}

	previewResponse := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/import-batches/"+batch.ID, nil, nil)
	testutil.DecodeJSON(t, previewResponse, &batch)
	if batch.Status != managedimport.BATCH_STATUS_UPLOADING || len(batch.Files) != 4 {
		t.Fatalf("Import Batch preview = %+v", batch)
	}
	if batch.Files[0].ClientFileID != clientFileIDs[0] {
		t.Fatalf("Batch client file ID = %q, want %q", batch.Files[0].ClientFileID, clientFileIDs[0])
	}
	if batch.Files[0].State != managedimport.BATCH_FILE_ACCEPTED || batch.Files[2].State != managedimport.BATCH_FILE_REJECTED {
		t.Fatalf("Import Batch file states = %+v", batch.Files)
	}

	confirmation := strings.NewReader(fmt.Sprintf(`{"revision":%d,"selectedFileIds":[%q,%q]}`, batch.Revision, jobIDs[0], jobIDs[1]))
	confirmResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches/"+batch.ID+"/confirm", confirmation, map[string]string{"Content-Type": "application/json"})
	if confirmResponse.Code != http.StatusOK {
		t.Fatalf("confirm Import Batch status = %d, body = %s", confirmResponse.Code, confirmResponse.Body.String())
	}
	testutil.DecodeJSON(t, confirmResponse, &batch)
	if batch.Status != managedimport.BATCH_STATUS_COMPLETED {
		t.Fatalf("completed Import Batch = %+v", batch)
	}
	outcomes := []managedimport.ImportOutcome{batch.Files[0].Outcome, batch.Files[1].Outcome, batch.Files[2].Outcome, batch.Files[3].Outcome}
	wantOutcomes := []managedimport.ImportOutcome{managedimport.OUTCOME_IMPORTED, managedimport.OUTCOME_REJECTED, managedimport.OUTCOME_REJECTED, managedimport.OUTCOME_NOT_ATTEMPTED}
	if !reflect.DeepEqual(outcomes, wantOutcomes) {
		t.Fatalf("Import Batch outcomes = %v, want %v", outcomes, wantOutcomes)
	}
	if batch.Files[1].ErrorCode != managedimport.ERROR_CODE_EXACT_DUPLICATE {
		t.Fatalf("exact-byte Batch failure code = %q", batch.Files[1].ErrorCode)
	}
	if tracks := listTracks(t, router); len(tracks.Items) != 1 || tracks.Items[0].ID != batch.Files[0].TrackID {
		t.Fatalf("Tracks after partial Import Batch = %+v", tracks.Items)
	}
	stagedFiles, err := filepath.Glob(filepath.Join(managedStoragePath, ".staging", "*.upload"))
	if err != nil {
		t.Fatalf("list completed Import Batch staging: %v", err)
	}
	if len(stagedFiles) != 0 {
		t.Fatalf("completed Import Batch left staging files: %v", stagedFiles)
	}
}

func TestManagedImportBatchReportsExactByteLoserAsRejectedDuplicate(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	importModule.RegisterRoutes(router)
	batchID := testutil.CreateResourceID(t, router, "/api/v1/import-batches")
	fixture := readStrictFLACFixture(t)
	jobIDs := []string{
		createBatchImportJob(t, router, batchID),
		createBatchImportJob(t, router, batchID),
	}
	for index, jobID := range jobIDs {
		response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(fixture), map[string]string{
			"Content-Type":      "audio/flac",
			"X-Import-Filename": fmt.Sprintf("duplicate-%d.flac", index),
		})
		if response.Code != http.StatusOK {
			t.Fatalf("upload exact-byte Batch file %d status = %d, body = %s", index, response.Code, response.Body.String())
		}
	}

	previewBatch := getImportBatch(t, router, batchID)
	response := confirmImportBatch(t, router, previewBatch, jobIDs)
	if response.Code != http.StatusOK {
		t.Fatalf("confirm exact-byte Batch status = %d, body = %s", response.Code, response.Body.String())
	}
	var batch managedimport.Batch
	testutil.DecodeJSON(t, response, &batch)
	if len(batch.Files) != 2 {
		t.Fatalf("exact-byte Batch files = %+v", batch.Files)
	}
	if batch.Files[0].Outcome != managedimport.OUTCOME_IMPORTED {
		t.Fatalf("first exact-byte outcome = %q", batch.Files[0].Outcome)
	}
	if batch.Files[1].Outcome != managedimport.OUTCOME_REJECTED || batch.Files[1].ErrorCode != managedimport.ERROR_CODE_EXACT_DUPLICATE {
		t.Fatalf("second exact-byte result = %+v", batch.Files[1])
	}
	repeatedResponse := confirmImportBatch(t, router, previewBatch, jobIDs)
	if repeatedResponse.Code != http.StatusOK {
		t.Fatalf("repeat exact-byte Batch status = %d, body = %s", repeatedResponse.Code, repeatedResponse.Body.String())
	}
	previewBatch.Revision--
	staleResponse := confirmImportBatch(t, router, previewBatch, jobIDs)
	testutil.AssertErrorCode(t, staleResponse, http.StatusConflict, managedimport.ERROR_CODE_REVISION_CONFLICT)
	var trackCount int
	if err := database.QueryRow(`SELECT COUNT(*) FROM tracks`).Scan(&trackCount); err != nil || trackCount != 1 {
		t.Fatalf("exact-byte Batch Track count = %d, err = %v", trackCount, err)
	}
}

func TestManagedImportBatchEnforcesCumulativeByteLimit(t *testing.T) {
	fixture := readStrictFLACFixture(t)
	configuration := config.Config{
		ManagedStoragePath:           t.TempDir(),
		ManagedImportFileLimitBytes:  int64(len(fixture) + 1),
		ManagedImportBatchLimitBytes: int64(len(fixture) + 1),
	}
	router := newConfiguredManagedImportRouter(t, configuration)
	batchID := testutil.CreateResourceID(t, router, "/api/v1/import-batches")
	jobIDs := make([]string, 2)
	for index := range jobIDs {
		body := strings.NewReader(fmt.Sprintf(`{"batchId":%q,"clientFileId":%q}`, batchID, uuid.NewString()))
		response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", body, map[string]string{"Content-Type": "application/json"})
		if response.Code != http.StatusCreated {
			t.Fatalf("create cumulative-limit batch file status = %d, body = %s", response.Code, response.Body.String())
		}
		var job managedimport.Job
		testutil.DecodeJSON(t, response, &job)
		jobIDs[index] = job.ID
	}
	for index, jobID := range jobIDs {
		response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(fixture), map[string]string{
			"Content-Type": "audio/flac", "X-Import-Filename": fmt.Sprintf("track-%d.flac", index+1),
		})
		if index == 0 && response.Code != http.StatusOK {
			t.Fatalf("first batch upload status = %d, body = %s", response.Code, response.Body.String())
		}
		if index == 1 {
			testutil.AssertErrorCode(t, response, http.StatusRequestEntityTooLarge, "batch_upload_too_large")
		}
	}
	response := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/import-batches/"+batchID, nil, nil)
	var batch managedimport.Batch
	testutil.DecodeJSON(t, response, &batch)
	if batch.Files[0].State != managedimport.BATCH_FILE_ACCEPTED || batch.Files[1].State != managedimport.BATCH_FILE_REJECTED {
		t.Fatalf("cumulative limit Import Batch = %+v", batch.Files)
	}
}

func TestManagedImportBatchAllowsMultipleChunkedUploadsWithinActualLimit(t *testing.T) {
	fixture := readStrictFLACFixture(t)
	configuration := config.Config{
		ManagedStoragePath:           t.TempDir(),
		ManagedImportFileLimitBytes:  int64(len(fixture) * 4),
		ManagedImportBatchLimitBytes: int64(len(fixture)*2 + 1),
	}
	router := newConfiguredManagedImportRouter(t, configuration)
	batchID := testutil.CreateResourceID(t, router, "/api/v1/import-batches")
	for index := 0; index < 2; index++ {
		createBody := strings.NewReader(fmt.Sprintf(`{"batchId":%q,"clientFileId":%q}`, batchID, uuid.NewString()))
		jobResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", createBody, map[string]string{"Content-Type": "application/json"})
		var job managedimport.Job
		testutil.DecodeJSON(t, jobResponse, &job)
		request := httptest.NewRequest(http.MethodPut, "/api/v1/imports/"+job.ID+"/file", bytes.NewReader(fixture))
		request.ContentLength = -1
		request.TransferEncoding = []string{"chunked"}
		request.Header.Set("Content-Type", "audio/flac")
		request.Header.Set("X-Import-Filename", fmt.Sprintf("chunked-%d.flac", index))
		response := httptest.NewRecorder()
		router.ServeHTTP(response, request)
		if response.Code != http.StatusOK {
			t.Fatalf("chunked upload %d status = %d, body = %s", index, response.Code, response.Body.String())
		}
	}
}

func TestManagedImportBatchCountsRejectedUploadBytes(t *testing.T) {
	fixture := readStrictFLACFixture(t)
	rejectedBytes := []byte("not audio")
	configuration := config.Config{
		ManagedStoragePath:           t.TempDir(),
		ManagedImportFileLimitBytes:  int64(len(fixture) + 1),
		ManagedImportBatchLimitBytes: int64(len(fixture) + len(rejectedBytes) - 1),
	}
	router := newConfiguredManagedImportRouter(t, configuration)
	batchID := testutil.CreateResourceID(t, router, "/api/v1/import-batches")
	rejectedJobID := createBatchImportJob(t, router, batchID)
	acceptedJobID := createBatchImportJob(t, router, batchID)

	response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+rejectedJobID+"/file", bytes.NewReader(rejectedBytes), map[string]string{
		"Content-Type": "audio/wav", "X-Import-Filename": "rejected.wav",
	})
	testutil.AssertErrorCode(t, response, http.StatusUnprocessableEntity, "unsupported_format")
	response = testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+acceptedJobID+"/file", bytes.NewReader(fixture), map[string]string{
		"Content-Type": "audio/flac", "X-Import-Filename": "accepted.flac",
	})
	testutil.AssertErrorCode(t, response, http.StatusRequestEntityTooLarge, "batch_upload_too_large")
}

func TestManagedImportBatchReservesConcurrentUploadBytesAtomically(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	store := managedimport.NewRepository(database)
	ctx := context.Background()
	batch, err := store.CreateBatch(ctx, managedimport.BatchOptions{})
	if err != nil {
		t.Fatalf("create reservation test batch: %v", err)
	}
	jobIDs := make([]string, 2)
	for index := range jobIDs {
		job, createErr := store.CreateJob(ctx, batch.ID, "")
		if createErr != nil {
			t.Fatalf("create reservation test job: %v", createErr)
		}
		jobIDs[index] = job.ID
	}

	const reservationBytes int64 = 10
	results := make(chan error, len(jobIDs))
	start := make(chan struct{})
	for _, jobID := range jobIDs {
		go func() {
			<-start
			results <- store.ReserveBatchUpload(ctx, jobID, reservationBytes, reservationBytes)
		}()
	}
	close(start)
	var successCount, limitCount int
	for range jobIDs {
		switch result := <-results; {
		case result == nil:
			successCount++
		case errors.Is(result, managedimport.ErrBatchTooLarge):
			limitCount++
		default:
			t.Fatalf("reserve concurrent batch upload: %v", result)
		}
	}
	if successCount != 1 || limitCount != 1 {
		t.Fatalf("concurrent reservations: success = %d, limited = %d", successCount, limitCount)
	}
}

func TestManagedImportBatchRejectsConfirmationWhileFileIsUnresolved(t *testing.T) {
	router := newConfiguredManagedImportRouter(t, config.Config{ManagedStoragePath: t.TempDir()})
	batchID := testutil.CreateResourceID(t, router, "/api/v1/import-batches")
	body := strings.NewReader(fmt.Sprintf(`{"batchId":%q,"clientFileId":%q}`, batchID, uuid.NewString()))
	testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", body, map[string]string{"Content-Type": "application/json"})

	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches/"+batchID+"/confirm", strings.NewReader(`{"revision":2,"selectedFileIds":[]}`), map[string]string{"Content-Type": "application/json"})
	testutil.AssertErrorCode(t, response, http.StatusConflict, "import_state_conflict")

	response = testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/import-batches/"+batchID, nil, nil)
	var batch managedimport.Batch
	testutil.DecodeJSON(t, response, &batch)
	if batch.Status != managedimport.BATCH_STATUS_UPLOADING {
		t.Fatalf("unresolved Import Batch status = %q", batch.Status)
	}
}

func TestManagedImportBatchResumesConfirmationAfterInterruption(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	importModule.RegisterRoutes(router)
	batchID := testutil.CreateResourceID(t, router, "/api/v1/import-batches")
	body := strings.NewReader(fmt.Sprintf(`{"batchId":%q,"clientFileId":%q}`, batchID, uuid.NewString()))
	jobResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", body, map[string]string{"Content-Type": "application/json"})
	var job managedimport.Job
	testutil.DecodeJSON(t, jobResponse, &job)
	uploadResponse := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+job.ID+"/file", bytes.NewReader(readStrictFLACFixture(t)), map[string]string{
		"Content-Type": "audio/flac", "X-Import-Filename": "resume.flac",
	})
	if uploadResponse.Code != http.StatusOK {
		t.Fatalf("upload resumable batch file status = %d, body = %s", uploadResponse.Code, uploadResponse.Body.String())
	}
	previewBatch := getImportBatch(t, router, batchID)
	if _, err := database.Exec(`UPDATE managed_import_batches SET status = 'confirming', revision = revision + 1 WHERE id = ?`, batchID); err != nil {
		t.Fatalf("simulate interrupted confirmation: %v", err)
	}

	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches/"+batchID+"/confirm", strings.NewReader(fmt.Sprintf(`{"revision":%d,"selectedFileIds":[%q]}`, previewBatch.Revision, job.ID)), map[string]string{"Content-Type": "application/json"})
	if response.Code != http.StatusOK {
		t.Fatalf("resume Import Batch status = %d, body = %s", response.Code, response.Body.String())
	}
	var batch managedimport.Batch
	testutil.DecodeJSON(t, response, &batch)
	if batch.Status != managedimport.BATCH_STATUS_COMPLETED || batch.Files[0].Outcome != managedimport.OUTCOME_IMPORTED {
		t.Fatalf("resumed Import Batch = %+v", batch)
	}
}

func TestManagedImportBatchRejectsDirectFileConfirmation(t *testing.T) {
	router := newConfiguredManagedImportRouter(t, config.Config{ManagedStoragePath: t.TempDir()})
	batchID := testutil.CreateResourceID(t, router, "/api/v1/import-batches")
	jobID := createBatchImportJob(t, router, batchID)
	uploadResponse := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(readStrictFLACFixture(t)), map[string]string{
		"Content-Type": "audio/flac", "X-Import-Filename": "batch-owned.flac",
	})
	if uploadResponse.Code != http.StatusOK {
		t.Fatalf("upload batch-owned file status = %d, body = %s", uploadResponse.Code, uploadResponse.Body.String())
	}

	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports/"+jobID+"/confirm", strings.NewReader(`{"revision":2}`), map[string]string{"Content-Type": "application/json"})
	testutil.AssertErrorCode(t, response, http.StatusConflict, "import_state_conflict")

	batch := getImportBatch(t, router, batchID)
	response = confirmImportBatch(t, router, batch, []string{jobID})
	if response.Code != http.StatusOK {
		t.Fatalf("confirm batch after direct rejection status = %d, body = %s", response.Code, response.Body.String())
	}
}

func TestManagedImportBatchSerializesConcurrentConfirmation(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	importModule.RegisterRoutes(router)
	batchID := testutil.CreateResourceID(t, router, "/api/v1/import-batches")
	jobID := createBatchImportJob(t, router, batchID)
	uploadResponse := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(readStrictFLACFixture(t)), map[string]string{
		"Content-Type": "audio/flac", "X-Import-Filename": "concurrent.flac",
	})
	if uploadResponse.Code != http.StatusOK {
		t.Fatalf("upload concurrently confirmed file status = %d, body = %s", uploadResponse.Code, uploadResponse.Body.String())
	}
	batch := getImportBatch(t, router, batchID)

	const confirmationCount = 8
	results := make(chan *httptest.ResponseRecorder, confirmationCount)
	start := make(chan struct{})
	for range confirmationCount {
		go func() {
			<-start
			results <- confirmImportBatch(t, router, batch, []string{jobID})
		}()
	}
	close(start)
	for range confirmationCount {
		response := <-results
		if response.Code != http.StatusOK {
			t.Fatalf("concurrent batch confirmation status = %d, body = %s", response.Code, response.Body.String())
		}
	}
	completedBatch := getImportBatch(t, router, batchID)
	if completedBatch.Status != managedimport.BATCH_STATUS_COMPLETED || completedBatch.Files[0].Outcome != managedimport.OUTCOME_IMPORTED {
		t.Fatalf("concurrently confirmed Import Batch = %+v", completedBatch)
	}
	var trackCount int
	if err := database.QueryRow(`SELECT COUNT(*) FROM tracks`).Scan(&trackCount); err != nil || trackCount != 1 {
		t.Fatalf("concurrently confirmed Track count = %d, err = %v", trackCount, err)
	}
}

func TestManagedImportBatchRetriesInterruptedUploadInSameJob(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	managedStoragePath := t.TempDir()
	configuration := config.Config{ManagedStoragePath: managedStoragePath}
	router := chi.NewRouter()
	managedimport.NewModule(database, configuration, library.NewMediaInspector()).RegisterRoutes(router)
	batchID := testutil.CreateResourceID(t, router, "/api/v1/import-batches")
	jobID := createBatchImportJob(t, router, batchID)
	fixture := readStrictFLACFixture(t)
	request := httptest.NewRequest(http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(fixture[:128]))
	request.ContentLength = int64(len(fixture))
	request.Header.Set("Content-Type", "audio/flac")
	request.Header.Set("X-Import-Filename", "interrupted.flac")
	response := httptest.NewRecorder()

	router.ServeHTTP(response, request)

	testutil.AssertErrorCode(t, response, http.StatusRequestTimeout, "upload_interrupted")
	batch := getImportBatch(t, router, batchID)
	if len(batch.Files) != 1 || batch.Files[0].JobID != jobID || batch.Files[0].State != managedimport.BATCH_FILE_UNRESOLVED || batch.Files[0].Status != managedimport.STATUS_UPLOADING {
		t.Fatalf("interrupted Import Batch file = %+v", batch.Files)
	}
	var uploadSize int64
	if err := database.QueryRow(`SELECT upload_size_bytes FROM managed_import_jobs WHERE id = ?`, jobID).Scan(&uploadSize); err != nil || uploadSize != 0 {
		t.Fatalf("interrupted upload reservation = %d, err = %v", uploadSize, err)
	}
	stagedFiles, err := filepath.Glob(filepath.Join(managedStoragePath, ".staging", "*.upload"))
	if err != nil || len(stagedFiles) != 0 {
		t.Fatalf("interrupted staged files = %v, err = %v", stagedFiles, err)
	}

	retryResponse := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(fixture), map[string]string{
		"Content-Type":      "audio/flac",
		"X-Import-Filename": "interrupted.flac",
	})
	if retryResponse.Code != http.StatusOK {
		t.Fatalf("retry interrupted upload status = %d, body = %s", retryResponse.Code, retryResponse.Body.String())
	}
	batch = getImportBatch(t, router, batchID)
	if len(batch.Files) != 1 || batch.Files[0].JobID != jobID || batch.Files[0].State != managedimport.BATCH_FILE_ACCEPTED || !batch.Files[0].Selected || batch.Files[0].ErrorCode != "" || batch.Files[0].ErrorReason != "" {
		t.Fatalf("retried Import Batch file = %+v", batch.Files)
	}
}

func TestManagedImportInterruptedUploadPreservesSiblingStaging(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	storage := managedimport.NewStorage(t.TempDir(), managedimport.StorageLimits{FileBytes: 1 << 20, BatchBytes: 2 << 20})
	service := managedimport.NewService(managedimport.NewRepository(database), storage, library.NewMediaInspector())
	batch, err := service.CreateBatch(context.Background(), managedimport.BatchOptions{})
	if err != nil {
		t.Fatalf("create interrupted upload batch: %v", err)
	}
	sibling, err := service.CreateJob(context.Background(), batch.ID, uuid.NewString())
	if err != nil {
		t.Fatalf("create sibling upload job: %v", err)
	}
	fixture := readStrictFLACFixture(t)
	if _, uploadErr := service.Upload(context.Background(), sibling.ID, "sibling.flac", bytes.NewReader(fixture), int64(len(fixture))); uploadErr != nil {
		t.Fatalf("upload sibling batch file: %v", uploadErr)
	}
	var siblingStagedPath string
	if queryErr := database.QueryRow(`SELECT staged_file_path FROM managed_import_jobs WHERE id = ?`, sibling.ID).Scan(&siblingStagedPath); queryErr != nil {
		t.Fatalf("read sibling staging path: %v", queryErr)
	}
	interrupted, err := service.CreateJob(context.Background(), batch.ID, uuid.NewString())
	if err != nil {
		t.Fatalf("create interrupted upload job: %v", err)
	}
	_, err = service.Upload(context.Background(), interrupted.ID, "interrupted.flac", bytes.NewReader(fixture[:128]), int64(len(fixture)))
	if !errors.Is(err, managedimport.ErrUploadInterrupted) {
		t.Fatalf("interrupted upload error = %v", err)
	}
	if _, err := os.Stat(siblingStagedPath); err != nil {
		t.Fatalf("interrupted upload affected sibling staging: %v", err)
	}
}

func TestManagedImportBatchCancellationRemovesUncommittedStaging(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	managedStoragePath := t.TempDir()
	configuration := config.Config{ManagedStoragePath: managedStoragePath}
	module := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	module.RegisterRoutes(router)
	batchResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches", nil, nil)
	var batch managedimport.Batch
	testutil.DecodeJSON(t, batchResponse, &batch)
	fixture := readStrictFLACFixture(t)
	for range 2 {
		jobID := createBatchImportJob(t, router, batch.ID)
		uploadResponse := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(fixture), map[string]string{
			"Content-Type":      "audio/flac",
			"X-Import-Filename": "cancel-me.flac",
		})
		if uploadResponse.Code != http.StatusOK {
			t.Fatalf("upload cancellable batch file status = %d, body = %s", uploadResponse.Code, uploadResponse.Body.String())
		}
	}

	response := testutil.ServeRequest(t, router, http.MethodDelete, "/api/v1/import-batches/"+batch.ID, nil, nil)
	if response.Code != http.StatusNoContent {
		t.Fatalf("cancel Import Batch status = %d, body = %s", response.Code, response.Body.String())
	}
	stagedFiles, err := filepath.Glob(filepath.Join(managedStoragePath, ".staging", "*.upload"))
	if err != nil {
		t.Fatalf("list canceled Import Batch staging: %v", err)
	}
	if len(stagedFiles) != 0 {
		t.Fatalf("canceled Import Batch left staging files: %v", stagedFiles)
	}
	getResponse := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/import-batches/"+batch.ID, nil, nil)
	if getResponse.Code != http.StatusNotFound {
		t.Fatalf("get canceled Import Batch status = %d, body = %s", getResponse.Code, getResponse.Body.String())
	}
	assertNoLibraryEntities(t, database)
}

func TestManagedImportJobCancellationRemovesOneBatchFile(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	managedStoragePath := t.TempDir()
	module := managedimport.NewModule(database, config.Config{ManagedStoragePath: managedStoragePath}, library.NewMediaInspector())
	router := chi.NewRouter()
	module.RegisterRoutes(router)
	batchResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches", nil, nil)
	var batch managedimport.Batch
	testutil.DecodeJSON(t, batchResponse, &batch)
	jobID := createBatchImportJob(t, router, batch.ID)
	fixture := readStrictFLACFixture(t)
	uploadResponse := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(fixture), map[string]string{
		"Content-Type":      "audio/flac",
		"X-Import-Filename": "cancel-file.flac",
	})
	if uploadResponse.Code != http.StatusOK {
		t.Fatalf("upload cancellable Import Job status = %d, body = %s", uploadResponse.Code, uploadResponse.Body.String())
	}

	response := testutil.ServeRequest(t, router, http.MethodDelete, "/api/v1/imports/"+jobID, nil, nil)
	if response.Code != http.StatusNoContent {
		t.Fatalf("cancel batch-owned Import Job status = %d, body = %s", response.Code, response.Body.String())
	}
	updatedBatch := getImportBatch(t, router, batch.ID)
	if len(updatedBatch.Files) != 0 || updatedBatch.Revision <= batch.Revision {
		t.Fatalf("Import Batch after file cancellation = %+v", updatedBatch)
	}
	stagedFiles, err := filepath.Glob(filepath.Join(managedStoragePath, ".staging", "*.upload"))
	if err != nil {
		t.Fatalf("list canceled Import Job staging: %v", err)
	}
	if len(stagedFiles) != 0 {
		t.Fatalf("canceled Import Job left staging files: %v", stagedFiles)
	}
}

func TestManagedImportBatchLeavesCanceledConfirmationResumable(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	storage := managedimport.NewStorage(t.TempDir(), managedimport.StorageLimits{FileBytes: 1 << 20, BatchBytes: 2 << 20})
	ctx, cancel := context.WithCancel(context.Background())
	inspector := &cancelOnConfirmationInspector{delegate: library.NewMediaInspector(), cancel: cancel}
	service := managedimport.NewService(managedimport.NewRepository(database), storage, inspector)
	batch, err := service.CreateBatch(context.Background(), managedimport.BatchOptions{})
	if err != nil {
		t.Fatalf("create canceled confirmation batch: %v", err)
	}
	job, err := service.CreateJob(context.Background(), batch.ID, uuid.NewString())
	if err != nil {
		t.Fatalf("create canceled confirmation job: %v", err)
	}
	preview, err := service.Upload(context.Background(), job.ID, "resumable.flac", bytes.NewReader(readStrictFLACFixture(t)), int64(len(readStrictFLACFixture(t))))
	if err != nil {
		t.Fatalf("upload resumable confirmation job: %v", err)
	}
	batch, err = service.GetBatch(context.Background(), batch.ID)
	if err != nil {
		t.Fatalf("get resumable confirmation batch: %v", err)
	}
	_, err = service.ConfirmBatch(ctx, batch.ID, managedimport.BatchConfirmation{Revision: batch.Revision, SelectedFileIDs: []string{preview.JobID}})
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("canceled confirmation error = %v", err)
	}

	completedBatch, err := service.ConfirmBatch(context.Background(), batch.ID, managedimport.BatchConfirmation{Revision: batch.Revision, SelectedFileIDs: []string{preview.JobID}})
	if err != nil {
		t.Fatalf("resume canceled confirmation: %v", err)
	}
	if completedBatch.Status != managedimport.BATCH_STATUS_COMPLETED || completedBatch.Files[0].Outcome != managedimport.OUTCOME_IMPORTED {
		t.Fatalf("resumed canceled confirmation batch = %+v", completedBatch)
	}
}

func TestManagedImportCommitsOGGFormatsWithoutChangingSourceBytes(t *testing.T) {
	tests := []struct {
		name       string
		filename   string
		mediaType  string
		format     string
		codec      string
		sampleRate int
	}{
		{name: "Vorbis", filename: "strict-import.ogg", mediaType: "audio/ogg", format: "ogg", codec: "vorbis", sampleRate: 44100},
		{name: "Opus", filename: "strict-import.opus", mediaType: "audio/opus", format: "opus", codec: "opus", sampleRate: 48000},
	}
	for _, testCase := range tests {
		t.Run(testCase.name, func(t *testing.T) {
			database := testutil.OpenMigratedDB(t)
			managedStoragePath := t.TempDir()
			configuration := config.Config{ManagedStoragePath: managedStoragePath}
			libraryModule := library.NewModule(database)
			importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
			playbackModule := playback.NewModule(database, libraryModule.TrackAccess())
			router := chi.NewRouter()
			importModule.RegisterRoutes(router)
			libraryModule.RegisterRoutes(router)
			playbackModule.RegisterRoutes(router)
			fixture, err := os.ReadFile(filepath.Join("..", "library", "testdata", testCase.filename))
			if err != nil {
				t.Fatalf("read %s fixture: %v", testCase.name, err)
			}

			jobResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", nil, nil)
			var job managedimport.Job
			testutil.DecodeJSON(t, jobResponse, &job)
			uploadResponse := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+job.ID+"/file", bytes.NewReader(fixture), map[string]string{
				"Content-Type": testCase.mediaType, "X-Import-Filename": testCase.filename,
			})
			if uploadResponse.Code != http.StatusOK {
				t.Fatalf("upload status = %d, body = %s", uploadResponse.Code, uploadResponse.Body.String())
			}
			if strings.Contains(uploadResponse.Body.String(), `"bitDepth"`) {
				t.Fatalf("lossy Import Preview includes inapplicable bitDepth: %s", uploadResponse.Body.String())
			}
			var preview managedimport.Preview
			testutil.DecodeJSON(t, uploadResponse, &preview)
			if preview.File.Format != testCase.format || preview.File.Container != "ogg" || preview.File.Codec != testCase.codec || preview.File.SampleRateHz != testCase.sampleRate || preview.File.BitDepth != 0 || preview.File.BitrateKbps <= 0 {
				t.Fatalf("Import Preview technical properties = %+v", preview.File)
			}
			if !reflect.DeepEqual(preview.File.Artists, []string{"First Artist", "Second Artist"}) || !reflect.DeepEqual(preview.File.Genres, []string{"Electronic", "Ambient"}) || preview.File.ArtworkMediaType != "image/png" {
				t.Fatalf("Import Preview relationships/artwork = %+v", preview.File)
			}

			confirmResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports/"+job.ID+"/confirm", strings.NewReader(`{"revision":2}`), map[string]string{"Content-Type": "application/json"})
			if confirmResponse.Code != http.StatusOK {
				t.Fatalf("confirm status = %d, body = %s", confirmResponse.Code, confirmResponse.Body.String())
			}
			var result managedimport.Result
			testutil.DecodeJSON(t, confirmResponse, &result)
			tracks := listTracks(t, router)
			if len(tracks.Items) != 1 || len(tracks.Items[0].Artists) != 2 || len(tracks.Items[0].Genres) != 2 || tracks.Items[0].BitDepth != 0 || tracks.Items[0].BitrateBps <= 0 {
				t.Fatalf("persisted Managed Track = %+v", tracks.Items)
			}
			var bitDepth sql.NullInt64
			if err = database.QueryRow(`SELECT bit_depth FROM tracks WHERE id = ?`, result.TrackID).Scan(&bitDepth); err != nil || bitDepth.Valid {
				t.Fatalf("lossy Track bit depth = %+v, error = %v", bitDepth, err)
			}
			streamResponse := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/tracks/"+result.TrackID+"/stream", nil, nil)
			if streamResponse.Code != http.StatusOK || !bytes.Equal(streamResponse.Body.Bytes(), fixture) {
				t.Fatalf("streamed source differs: status = %d", streamResponse.Code)
			}
			assertCanonicalSource(t, managedStoragePath, "."+testCase.format, fixture)
		})
	}
}

func assertCanonicalSource(t *testing.T, managedStoragePath, extension string, fixture []byte) {
	t.Helper()
	var audioPath string
	err := filepath.WalkDir(managedStoragePath, func(path string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if !entry.IsDir() && filepath.Ext(path) == extension {
			audioPath = path
		}
		return nil
	})
	if err != nil || audioPath == "" {
		t.Fatalf("find canonical %s source: path = %q, error = %v", extension, audioPath, err)
	}
	stored, err := os.ReadFile(audioPath)
	if err != nil || !bytes.Equal(stored, fixture) {
		t.Fatalf("canonical %s bytes differ: %v", extension, err)
	}
}

func TestManagedImportStreamsUploadIntoServerOwnedStaging(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	managedStoragePath := t.TempDir()
	configuration := config.Config{ManagedStoragePath: managedStoragePath}
	importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	importModule.RegisterRoutes(router)
	jobResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", nil, nil)
	var job struct {
		ID string `json:"id"`
	}
	testutil.DecodeJSON(t, jobResponse, &job)
	reader := &stagingObservingReader{
		data:               readStrictFLACFixture(t),
		managedStoragePath: managedStoragePath,
	}

	response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+job.ID+"/file", reader, map[string]string{
		"Content-Type":      "audio/flac",
		"X-Import-Filename": "strict-import.flac",
	})

	if response.Code != http.StatusOK {
		t.Fatalf("streaming upload status = %d, body = %s", response.Code, response.Body.String())
	}
	if !reader.observedStagedBytes {
		t.Fatal("upload body was consumed before bytes appeared in server-owned staging")
	}
}

func TestManagedImportEnforcesFileLimitWithoutContentLength(t *testing.T) {
	assertManagedImportUploadLimit(t, 1024, 4096, -1, "upload_too_large")
}

func TestManagedImportEnforcesFileLimitWhenContentLengthIsFalse(t *testing.T) {
	assertManagedImportUploadLimit(t, 1024, 4096, 512, "upload_too_large")
}

func TestManagedImportEnforcesBatchLimitWhileStreaming(t *testing.T) {
	assertManagedImportUploadLimit(t, 4096, 1024, -1, "batch_upload_too_large")
}

func assertManagedImportUploadLimit(t *testing.T, fileLimit, batchLimit, contentLength int64, expectedCode string) {
	t.Helper()
	router := newConfiguredManagedImportRouter(t, config.Config{
		ManagedStoragePath:           t.TempDir(),
		ManagedStorageReserveBytes:   0,
		ManagedImportFileLimitBytes:  fileLimit,
		ManagedImportBatchLimitBytes: batchLimit,
	})
	jobID := testutil.CreateResourceID(t, router, "/api/v1/imports")
	request := httptest.NewRequest(http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(make([]byte, 2048)))
	request.ContentLength = contentLength
	request.Header.Set("Content-Type", "audio/flac")
	request.Header.Set("X-Import-Filename", "oversized.flac")
	response := httptest.NewRecorder()

	router.ServeHTTP(response, request)

	testutil.AssertErrorCode(t, response, http.StatusRequestEntityTooLarge, expectedCode)
}

func TestManagedImportRejectsClientFilenamePathSegments(t *testing.T) {
	router := newConfiguredManagedImportRouter(t, config.Config{ManagedStoragePath: t.TempDir()})
	jobID := testutil.CreateResourceID(t, router, "/api/v1/imports")
	response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(readStrictFLACFixture(t)), map[string]string{
		"Content-Type":      "audio/flac",
		"X-Import-Filename": "../strict-import.flac",
	})

	testutil.AssertErrorCode(t, response, http.StatusBadRequest, "invalid_upload")
}

func TestManagedImportRejectsInvalidFLACWithoutLibraryMutation(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	managedStoragePath := t.TempDir()
	configuration := config.Config{ManagedStoragePath: managedStoragePath}
	importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	libraryModule := library.NewModule(database)
	router := chi.NewRouter()
	importModule.RegisterRoutes(router)
	libraryModule.RegisterRoutes(router)
	jobResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", nil, nil)
	var job struct {
		ID string `json:"id"`
	}
	testutil.DecodeJSON(t, jobResponse, &job)
	fixture := readStrictFLACFixture(t)

	response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+job.ID+"/file", bytes.NewReader(fixture[:len(fixture)-8]), map[string]string{
		"Content-Type":      "audio/flac",
		"X-Import-Filename": "truncated.flac",
	})

	if response.Code != http.StatusUnprocessableEntity {
		t.Fatalf("invalid FLAC status = %d, body = %s", response.Code, response.Body.String())
	}
	var failure struct {
		Code string `json:"code"`
	}
	testutil.DecodeJSON(t, response, &failure)
	if failure.Code != "audio_decode_failed" {
		t.Fatalf("invalid FLAC error code = %q", failure.Code)
	}
	if len(listTracks(t, router).Items) != 0 {
		t.Fatal("invalid FLAC mutated the normal library")
	}
	stagedFiles, err := filepath.Glob(filepath.Join(managedStoragePath, ".staging", "*.upload"))
	if err != nil {
		t.Fatalf("list staged files: %v", err)
	}
	if len(stagedFiles) != 0 {
		t.Fatalf("invalid FLAC left staged files: %v", stagedFiles)
	}
}

func TestManagedImportExposesValidationProgressAndCancellation(t *testing.T) {
	inspector := &cancellingInspector{reported: make(chan struct{})}
	router := newManagedImportRouter(t, inspector)
	jobID := createManagedImportJob(t, router)
	cancel, uploadResponse, uploadDone := startManagedImportUpload(t, router, jobID)
	waitForSignal(t, inspector.reported, "validation progress was not reported")
	activeJob := getManagedImportJob(t, router, jobID)
	if activeJob.Status != "uploading" || activeJob.ValidationProgress != 40 || activeJob.ErrorCode != "" {
		t.Fatalf("active Managed Import Job = %+v", activeJob)
	}

	cancel()
	waitForSignal(t, uploadDone, "validation did not stop after cancellation")
	if uploadResponse.Code != http.StatusUnprocessableEntity {
		t.Fatalf("cancelled validation status = %d, body = %s", uploadResponse.Code, uploadResponse.Body.String())
	}
	failedJob := getManagedImportJob(t, router, jobID)
	if failedJob.Status != "failed" || failedJob.ValidationProgress != 40 || failedJob.ErrorCode != "validation_cancelled" {
		t.Fatalf("cancelled Managed Import Job = %+v", failedJob)
	}
}

func TestManagedImportRecordsCancellationAtPreviewBoundary(t *testing.T) {
	inspector := &completionCancellingInspector{}
	router := newManagedImportRouter(t, inspector)
	jobID := createManagedImportJob(t, router)
	ctx, cancel := context.WithCancel(context.Background())
	inspector.cancel = cancel
	request := httptest.NewRequest(http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(readStrictFLACFixture(t))).WithContext(ctx)
	request.Header.Set("Content-Type", "audio/flac")
	request.Header.Set("X-Import-Filename", "strict-import.flac")
	response := httptest.NewRecorder()

	router.ServeHTTP(response, request)

	job := getManagedImportJob(t, router, jobID)
	if job.Status != "failed" || job.ValidationProgress != 100 || job.ErrorCode != "validation_cancelled" {
		t.Fatalf("preview-boundary cancellation result = %+v", job)
	}
}

func TestManagedImportSerializesUploadsForTheSameJob(t *testing.T) {
	inspector := &blockingInspector{
		started: make(chan struct{}, 2),
		release: make(chan struct{}),
	}
	router := newManagedImportRouter(t, inspector)
	jobID := createManagedImportJob(t, router)
	_, firstResponse, firstDone := startManagedImportUpload(t, router, jobID)
	waitForSignal(t, inspector.started, "first upload did not reach inspection")
	_, secondResponse, secondDone := startManagedImportUpload(t, router, jobID)
	select {
	case <-inspector.started:
		t.Fatal("second upload inspected the same job concurrently")
	case <-time.After(100 * time.Millisecond):
	}
	close(inspector.release)
	waitForSignal(t, firstDone, "first upload did not finish")
	waitForSignal(t, secondDone, "second upload did not finish")
	if firstResponse.Code != http.StatusOK || secondResponse.Code != http.StatusConflict {
		t.Fatalf("serialized upload statuses = (%d, %d)", firstResponse.Code, secondResponse.Code)
	}
}

func TestManagedImportBatchCancellationCleansPreviewCompletedDuringCancellation(t *testing.T) {
	inspector := &blockingInspector{
		started: make(chan struct{}, 1),
		release: make(chan struct{}),
	}
	database := testutil.OpenMigratedDB(t)
	managedStoragePath := t.TempDir()
	module := managedimport.NewModule(database, config.Config{ManagedStoragePath: managedStoragePath}, inspector)
	router := chi.NewRouter()
	module.RegisterRoutes(router)
	batchResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches", nil, nil)
	var batch managedimport.Batch
	testutil.DecodeJSON(t, batchResponse, &batch)
	jobID := createBatchImportJob(t, router, batch.ID)
	_, uploadResponse, uploadDone := startManagedImportUpload(t, router, jobID)
	waitForSignal(t, inspector.started, "upload did not reach inspection")
	cancelResponse := httptest.NewRecorder()
	cancelDone := make(chan struct{})
	go func() {
		defer close(cancelDone)
		request := httptest.NewRequest(http.MethodDelete, "/api/v1/import-batches/"+batch.ID, nil)
		router.ServeHTTP(cancelResponse, request)
	}()
	waitForSignal(t, cancelDone, "batch cancellation did not stop active upload")
	waitForSignal(t, uploadDone, "canceled upload did not finish")
	close(inspector.release)
	if uploadResponse.Code != http.StatusRequestTimeout || cancelResponse.Code != http.StatusNoContent {
		t.Fatalf("upload/cancel statuses = (%d, %d)", uploadResponse.Code, cancelResponse.Code)
	}
	stagedFiles, err := filepath.Glob(filepath.Join(managedStoragePath, ".staging", "*.upload"))
	if err != nil {
		t.Fatalf("list cancellation-race staging: %v", err)
	}
	if len(stagedFiles) != 0 {
		t.Fatalf("batch cancellation race left staging files: %v", stagedFiles)
	}
}

func TestInactiveCleanupCancelsStalledUploadAfterFifteenMinutes(t *testing.T) {
	inspector := &blockingInspector{
		started: make(chan struct{}, 1),
		release: make(chan struct{}),
	}
	database := testutil.OpenMigratedDB(t)
	managedStoragePath := t.TempDir()
	service := managedimport.NewService(
		managedimport.NewRepository(database),
		managedimport.NewStorage(managedStoragePath, managedimport.StorageLimits{FileBytes: 1 << 20, BatchBytes: 2 << 20}),
		inspector,
	)
	batch, err := service.CreateBatch(context.Background(), managedimport.BatchOptions{})
	if err != nil {
		t.Fatalf("create stalled upload batch: %v", err)
	}
	job, err := service.CreateJob(context.Background(), batch.ID, uuid.NewString())
	if err != nil {
		t.Fatalf("create stalled upload job: %v", err)
	}
	uploadDone := make(chan error, 1)
	fixture := readStrictFLACFixture(t)
	go func() {
		_, uploadErr := service.Upload(context.Background(), job.ID, "stalled.flac", bytes.NewReader(fixture), int64(len(fixture)))
		uploadDone <- uploadErr
	}()
	waitForSignal(t, inspector.started, "stalled upload did not reach inspection")

	if cleanupErr := service.CleanupInactive(context.Background(), time.Now().Add(16*time.Minute)); cleanupErr != nil {
		t.Fatalf("cleanup stalled Managed Import upload: %v", cleanupErr)
	}
	close(inspector.release)
	select {
	case uploadErr := <-uploadDone:
		if !errors.Is(uploadErr, context.Canceled) {
			t.Fatalf("stalled upload error = %v", uploadErr)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("stalled upload did not stop")
	}
	if _, getErr := service.GetBatch(context.Background(), batch.ID); !errors.Is(getErr, managedimport.ErrNotFound) {
		t.Fatalf("get expired stalled Import Batch error = %v", getErr)
	}
	stagedFiles, err := filepath.Glob(filepath.Join(managedStoragePath, ".staging", "*.upload"))
	if err != nil {
		t.Fatalf("list stalled upload staging: %v", err)
	}
	if len(stagedFiles) != 0 {
		t.Fatalf("stalled upload left staging files: %v", stagedFiles)
	}
}

func TestManagedImportCancellationWaitsForSuccessfulConfirmation(t *testing.T) {
	inspector := &blockingConfirmationInspector{
		started: make(chan struct{}, 1),
		release: make(chan struct{}),
	}
	database := testutil.OpenMigratedDB(t)
	service := managedimport.NewService(
		managedimport.NewRepository(database),
		managedimport.NewStorage(t.TempDir(), managedimport.StorageLimits{FileBytes: 1 << 20, BatchBytes: 2 << 20}),
		inspector,
	)
	job, err := service.CreateJob(context.Background(), "", "")
	if err != nil {
		t.Fatalf("create confirm-race job: %v", err)
	}
	fixture := readStrictFLACFixture(t)
	preview, err := service.Upload(context.Background(), job.ID, "confirm-race.flac", bytes.NewReader(fixture), int64(len(fixture)))
	if err != nil {
		t.Fatalf("upload confirm-race job: %v", err)
	}
	confirmationDone := make(chan error, 1)
	go func() {
		_, confirmErr := service.Confirm(context.Background(), job.ID, preview.Revision)
		confirmationDone <- confirmErr
	}()
	waitForSignal(t, inspector.started, "confirmation did not reach inspection")
	cancellationDone := make(chan error, 1)
	go func() { cancellationDone <- service.CancelJob(context.Background(), job.ID) }()
	select {
	case cancelErr := <-cancellationDone:
		t.Fatalf("cancellation finished during confirmation: %v", cancelErr)
	case <-time.After(100 * time.Millisecond):
	}

	close(inspector.release)
	if confirmErr := <-confirmationDone; confirmErr != nil {
		t.Fatalf("confirm during cancellation race: %v", confirmErr)
	}
	if cancelErr := <-cancellationDone; !errors.Is(cancelErr, managedimport.ErrInvalidState) {
		t.Fatalf("cancel committed Managed Import error = %v", cancelErr)
	}
	var trackCount int
	if err := database.QueryRow(`SELECT COUNT(*) FROM tracks`).Scan(&trackCount); err != nil || trackCount != 1 {
		t.Fatalf("confirmed Track count = %d, err = %v", trackCount, err)
	}
}

func TestManagedImportDetectsFLACWithoutTrustingFilenameOrContentType(t *testing.T) {
	router := newManagedImportRouter(t, library.NewMediaInspector())
	jobID := createManagedImportJob(t, router)

	response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(readStrictFLACFixture(t)), map[string]string{
		"Content-Type":      "audio/mpeg",
		"X-Import-Filename": "misleading.mp3",
	})

	if response.Code != http.StatusOK || !strings.Contains(response.Body.String(), `"container":"flac"`) || !strings.Contains(response.Body.String(), `"codec":"flac"`) {
		t.Fatalf("detected FLAC response = %d %s", response.Code, response.Body.String())
	}
}

type managedImportJobResult struct {
	Status             string `json:"status"`
	ValidationProgress int    `json:"validationProgress"`
	ErrorCode          string `json:"errorCode"`
}

func getManagedImportJob(t *testing.T, router http.Handler, jobID string) managedImportJobResult {
	t.Helper()
	response := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/imports/"+jobID, nil, nil)
	if response.Code != http.StatusOK {
		t.Fatalf("get Managed Import Job status = %d, body = %s", response.Code, response.Body.String())
	}
	var result managedImportJobResult
	testutil.DecodeJSON(t, response, &result)
	return result
}

type cancellingInspector struct {
	reported chan struct{}
}

type completionCancellingInspector struct {
	cancel context.CancelFunc
}

type blockingInspector struct {
	started chan struct{}
	release chan struct{}
}

type blockingConfirmationInspector struct {
	mutex   sync.Mutex
	calls   int
	started chan struct{}
	release chan struct{}
}

type blockingCompletedInspector struct {
	completedPath chan string
	release       chan struct{}
}

func (inspector *blockingCompletedInspector) Inspect(ctx context.Context, path string, reportProgress library.InspectionProgressReporter) (library.MediaInspection, error) {
	inspection, err := library.NewMediaInspector().Inspect(ctx, path, reportProgress)
	if err != nil {
		return library.MediaInspection{}, err
	}
	inspector.completedPath <- path
	select {
	case <-ctx.Done():
		return library.MediaInspection{}, ctx.Err()
	case <-inspector.release:
		return inspection, nil
	}
}

func (inspector *blockingConfirmationInspector) Inspect(ctx context.Context, path string, reportProgress library.InspectionProgressReporter) (library.MediaInspection, error) {
	inspector.mutex.Lock()
	inspector.calls++
	call := inspector.calls
	inspector.mutex.Unlock()
	if call > 1 {
		inspector.started <- struct{}{}
		select {
		case <-ctx.Done():
			return library.MediaInspection{}, ctx.Err()
		case <-inspector.release:
		}
	}
	return library.NewMediaInspector().Inspect(ctx, path, reportProgress)
}

func (inspector *blockingInspector) Inspect(ctx context.Context, path string, reportProgress library.InspectionProgressReporter) (library.MediaInspection, error) {
	inspector.started <- struct{}{}
	select {
	case <-ctx.Done():
		return library.MediaInspection{}, ctx.Err()
	case <-inspector.release:
		return library.NewMediaInspector().Inspect(ctx, path, reportProgress)
	}
}

func (inspector *completionCancellingInspector) Inspect(ctx context.Context, path string, reportProgress library.InspectionProgressReporter) (library.MediaInspection, error) {
	inspection, err := library.NewMediaInspector().Inspect(ctx, path, reportProgress)
	if err != nil {
		return library.MediaInspection{}, err
	}
	inspector.cancel()
	return inspection, nil
}

func createManagedImportJob(t *testing.T, router http.Handler) string {
	t.Helper()
	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", nil, nil)
	var job struct {
		ID string `json:"id"`
	}
	testutil.DecodeJSON(t, response, &job)
	return job.ID
}

func newManagedImportRouter(t *testing.T, inspector library.MediaInspector) http.Handler {
	t.Helper()
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	router := chi.NewRouter()
	managedimport.NewModule(database, configuration, inspector).RegisterRoutes(router)
	return router
}

func startManagedImportUpload(t *testing.T, router http.Handler, jobID string) (context.CancelFunc, *httptest.ResponseRecorder, <-chan struct{}) {
	t.Helper()
	ctx, cancel := context.WithCancel(context.Background())
	request := httptest.NewRequest(http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(readStrictFLACFixture(t))).WithContext(ctx)
	request.Header.Set("Content-Type", "audio/flac")
	request.Header.Set("X-Import-Filename", "strict-import.flac")
	response := httptest.NewRecorder()
	done := make(chan struct{})
	go func() {
		defer close(done)
		router.ServeHTTP(response, request)
	}()
	return cancel, response, done
}

func waitForSignal(t *testing.T, signal <-chan struct{}, failureMessage string) {
	t.Helper()
	select {
	case <-signal:
	case <-time.After(2 * time.Second):
		t.Fatal(failureMessage)
	}
}

func (inspector *cancellingInspector) Inspect(ctx context.Context, _ string, reportProgress library.InspectionProgressReporter) (library.MediaInspection, error) {
	if err := reportProgress(library.InspectionProgress{DecodedSamples: 40, TotalSamples: 100, Percent: 40}); err != nil {
		return library.MediaInspection{}, err
	}
	close(inspector.reported)
	<-ctx.Done()
	return library.MediaInspection{}, &library.InspectionError{
		Code:  library.INSPECTION_ERROR_VALIDATION_CANCELLED,
		Field: "validation",
		Err:   ctx.Err(),
	}
}

func TestManagedImportAcceptsMissingEmbeddedArtwork(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	firstTrackID := importOneFLAC(t, router, readStrictFLACFixture(t), "first.flac")
	jobID := createImportJob(t, router)
	fixture := withoutFrontCover(t, secondTrackFixture(readStrictFLACFixture(t)))

	response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(fixture), map[string]string{
		"Content-Type":      "audio/flac",
		"X-Import-Filename": "missing-cover.flac",
	})

	if response.Code != http.StatusOK {
		t.Fatalf("optional artwork status = %d: %s", response.Code, response.Body.String())
	}
	tracks := listTracks(t, router)
	if len(tracks.Items) != 1 || tracks.Items[0].ID != firstTrackID {
		t.Fatalf("Tracks after missing artwork = %+v", tracks.Items)
	}
}

func TestManagedImportPreservesExistingAlbumArtwork(t *testing.T) {
	managedStoragePath := t.TempDir()
	router := newManagedImportTestRouter(t, managedStoragePath)
	firstFixture := readStrictFLACFixture(t)
	importOneFLAC(t, router, firstFixture, "first.flac")
	secondFixture := replaceFrontCover(t, secondTrackFixture(firstFixture), encodeAlternatePNG(t))
	jobID, revision := uploadFLACForPreview(t, router, secondFixture, "second.flac")

	confirmResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports/"+jobID+"/confirm", strings.NewReader(fmt.Sprintf(`{"revision":%d}`, revision)), map[string]string{"Content-Type": "application/json"})

	if confirmResponse.Code != http.StatusOK {
		t.Fatalf("preserved artwork status = %d: %s", confirmResponse.Code, confirmResponse.Body.String())
	}
	tracks := listTracks(t, router)
	if len(tracks.Items) != 2 {
		t.Fatalf("Tracks = %+v", tracks.Items)
	}
	assertCanonicalFileCounts(t, managedStoragePath, 2, 1)
}

func TestManagedImportRejectsMissingAndEmptyIdentityMetadataWithoutLibraryEntities(t *testing.T) {
	testCases := []struct {
		name        string
		tag         string
		fixtureTag  string
		replacement string
	}{
		{name: "missing TITLE", tag: "TITLE", fixtureTag: "TITLE=  Inspection   Fixture  ", replacement: "XITLE=  Inspection   Fixture  "},
		{name: "empty TITLE", tag: "TITLE", fixtureTag: "TITLE=  Inspection   Fixture  "},
		{name: "missing ARTIST", tag: "ARTIST", fixtureTag: "ARTIST=Test Artist", replacement: "XRTIST=Test Artist"},
		{name: "empty ARTIST", tag: "ARTIST", fixtureTag: "ARTIST=Test Artist"},
		{name: "missing ALBUMARTIST", tag: "ALBUMARTIST", fixtureTag: "ALBUMARTIST=Test Album Artist", replacement: "XLBUMARTIST=Test Album Artist"},
		{name: "empty ALBUMARTIST", tag: "ALBUMARTIST", fixtureTag: "ALBUMARTIST=Test Album Artist"},
		{name: "missing ALBUM", tag: "ALBUM", fixtureTag: "ALBUM=Strict Import Tests", replacement: "XLBUM=Strict Import Tests"},
		{name: "empty ALBUM", tag: "ALBUM", fixtureTag: "ALBUM=Strict Import Tests"},
		{name: "missing TRACKNUMBER", tag: "TRACKNUMBER", fixtureTag: "TRACKNUMBER=3/9", replacement: "XRACKNUMBER=3/9"},
		{name: "empty TRACKNUMBER", tag: "TRACKNUMBER", fixtureTag: "TRACKNUMBER=3/9"},
	}

	for _, testCase := range testCases {
		t.Run(testCase.name, func(t *testing.T) {
			database := testutil.OpenMigratedDB(t)
			configuration := config.Config{ManagedStoragePath: t.TempDir()}
			importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
			router := chi.NewRouter()
			importModule.RegisterRoutes(router)
			fixture := replaceFixtureTag(t, readStrictFLACFixture(t), testCase.fixtureTag, testCase.replacement)

			jobResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", nil, nil)
			var job struct {
				ID string `json:"id"`
			}
			testutil.DecodeJSON(t, jobResponse, &job)
			response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+job.ID+"/file", bytes.NewReader(fixture), map[string]string{
				"Content-Type":      "audio/flac",
				"X-Import-Filename": "invalid-identity.flac",
			})

			if response.Code != http.StatusUnprocessableEntity {
				t.Fatalf("invalid identity status = %d, body = %s", response.Code, response.Body.String())
			}
			var failure struct {
				Code  string `json:"code"`
				Field string `json:"field"`
			}
			testutil.DecodeJSON(t, response, &failure)
			if failure.Code != "invalid_metadata" || failure.Field != testCase.tag {
				t.Fatalf("invalid identity error = %+v", failure)
			}
			assertNoLibraryEntities(t, database)
		})
	}
}

func TestManagedImportRequiresDiscNumberForTaggedMultiDiscFile(t *testing.T) {
	fixture := replaceFixtureTag(t, readStrictFLACFixture(t), "DISCNUMBER=1/1", "XISCNUMBER=1/1")
	fixture = replaceFixtureTag(t, fixture, "encoder=Lavf63.1.101", "TOTALDISCS=2")
	response, database := uploadManagedImportFixture(t, fixture)

	assertValidationFailure(t, response, "invalid_metadata", "DISCNUMBER")
	assertNoLibraryEntities(t, database)
}

func TestManagedImportUsesEffectiveDiscOneForSingleDiscFile(t *testing.T) {
	fixture := replaceFixtureTag(t, readStrictFLACFixture(t), "DISCNUMBER=1/1", "XISCNUMBER=1/1")
	fixture = replaceFixtureTag(t, fixture, "encoder=Lavf63.1.101", "TOTALDISCS=1")
	response, database := uploadManagedImportFixture(t, fixture)

	if response.Code != http.StatusOK {
		t.Fatalf("single-disc upload status = %d, body = %s", response.Code, response.Body.String())
	}
	var preview struct {
		File struct {
			DiscNo    int `json:"discNo"`
			DiscTotal int `json:"discTotal"`
		} `json:"file"`
	}
	testutil.DecodeJSON(t, response, &preview)
	if preview.File.DiscNo != 1 || preview.File.DiscTotal != 1 {
		t.Fatalf("single-disc position = %d/%d", preview.File.DiscNo, preview.File.DiscTotal)
	}
	assertNoLibraryEntities(t, database)
}

func TestManagedImportReadsSeparateTrackTotal(t *testing.T) {
	fixture := replaceFixtureTag(t, readStrictFLACFixture(t), "TRACKNUMBER=3/9", "TRACKNUMBER=3")
	fixture = replaceFixtureTag(t, fixture, "encoder=Lavf63.1.101", "TOTALTRACKS=9")
	response, database := uploadManagedImportFixture(t, fixture)

	if response.Code != http.StatusOK {
		t.Fatalf("separate Track total upload status = %d, body = %s", response.Code, response.Body.String())
	}
	var preview struct {
		File struct {
			TrackNo    int `json:"trackNo"`
			TrackTotal int `json:"trackTotal"`
		} `json:"file"`
	}
	testutil.DecodeJSON(t, response, &preview)
	if preview.File.TrackNo != 3 || preview.File.TrackTotal != 9 {
		t.Fatalf("Track position = %d/%d", preview.File.TrackNo, preview.File.TrackTotal)
	}
	assertNoLibraryEntities(t, database)
}

func TestManagedImportReadsSeparateDiscTotal(t *testing.T) {
	fixture := replaceFixtureTag(t, readStrictFLACFixture(t), "DISCNUMBER=1/1", "DISCNUMBER=1")
	fixture = replaceFixtureTag(t, fixture, "encoder=Lavf63.1.101", "TOTALDISCS=2")
	response, database := uploadManagedImportFixture(t, fixture)

	if response.Code != http.StatusOK {
		t.Fatalf("separate Disc total upload status = %d, body = %s", response.Code, response.Body.String())
	}
	var preview struct {
		File struct {
			DiscNo    int `json:"discNo"`
			DiscTotal int `json:"discTotal"`
		} `json:"file"`
	}
	testutil.DecodeJSON(t, response, &preview)
	if preview.File.DiscNo != 1 || preview.File.DiscTotal != 2 {
		t.Fatalf("Disc position = %d/%d", preview.File.DiscNo, preview.File.DiscTotal)
	}
	assertNoLibraryEntities(t, database)
}

func TestManagedImportRejectsInvalidOptionalTotals(t *testing.T) {
	testCases := []struct {
		name  string
		tag   string
		value string
	}{
		{name: "Track total below Track number", tag: "TOTALTRACKS", value: "TOTALTRACKS=2"},
		{name: "Track total conflicts with inline total", tag: "TOTALTRACKS", value: "TOTALTRACKS=8"},
		{name: "Disc total conflicts with inline total", tag: "TOTALDISCS", value: "TOTALDISCS=2"},
		{name: "Disc total is not positive", tag: "TOTALDISCS", value: "TOTALDISCS=0"},
		{name: "Disc total exceeds supported range", tag: "TOTALDISCS", value: "TOTALDISCS=10000"},
	}

	for _, testCase := range testCases {
		t.Run(testCase.name, func(t *testing.T) {
			fixture := replaceFixtureTag(t, readStrictFLACFixture(t), "encoder=Lavf63.1.101", testCase.value)
			response, database := uploadManagedImportFixture(t, fixture)

			assertValidationFailure(t, response, "invalid_metadata", testCase.tag)
			assertNoLibraryEntities(t, database)
		})
	}
}

func TestManagedImportRequiresDiscNumberForExistingMultiDiscAlbum(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	libraryModule := library.NewModule(database)
	router := chi.NewRouter()
	importModule.RegisterRoutes(router)
	libraryModule.RegisterRoutes(router)
	multiDiscFixture := replaceFixtureTag(t, readStrictFLACFixture(t), "DISCNUMBER=1/1", "DISCNUMBER=2/2")
	importOneFLAC(t, router, multiDiscFixture, "disc-two.flac")
	untaggedFixture := replaceFixtureTag(t, readStrictFLACFixture(t), "DISCNUMBER=1/1", "XISCNUMBER=1/1")

	jobResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", nil, nil)
	var job struct {
		ID string `json:"id"`
	}
	testutil.DecodeJSON(t, jobResponse, &job)
	response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+job.ID+"/file", bytes.NewReader(untaggedFixture), map[string]string{
		"Content-Type":      "audio/flac",
		"X-Import-Filename": "missing-disc-number.flac",
	})

	assertValidationFailure(t, response, "invalid_metadata", "DISCNUMBER")
	if tracks := listTracks(t, router); len(tracks.Items) != 1 || tracks.Items[0].DiscNo != 2 {
		t.Fatalf("existing multi-disc Album Tracks = %+v", tracks.Items)
	}
}

func TestManagedImportRequiresDiscNumberForAwaitingMultiDiscSibling(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	importModule.RegisterRoutes(router)
	multiDiscFixture := replaceFixtureTag(t, readStrictFLACFixture(t), "DISCNUMBER=1/1", "DISCNUMBER=2/2")
	firstResponse := uploadFixtureThroughRouter(t, router, multiDiscFixture)
	if firstResponse.Code != http.StatusOK {
		t.Fatalf("multi-disc sibling upload status = %d, body = %s", firstResponse.Code, firstResponse.Body.String())
	}
	untaggedFixture := replaceFixtureTag(t, readStrictFLACFixture(t), "DISCNUMBER=1/1", "XISCNUMBER=1/1")
	secondResponse := uploadFixtureThroughRouter(t, router, untaggedFixture)

	assertValidationFailure(t, secondResponse, "invalid_metadata", "DISCNUMBER")
	assertNoLibraryEntities(t, database)
}

func TestManagedImportSeparatesDisplayValuesFromUnicodeComparisonKeys(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	libraryModule := library.NewModule(database)
	router := chi.NewRouter()
	importModule.RegisterRoutes(router)
	libraryModule.RegisterRoutes(router)
	firstFixture := replaceFixtureTag(t, readStrictFLACFixture(t), "ALBUMARTIST=Test Album Artist", "ALBUMARTIST=Straße Artist")
	importOneFLAC(t, router, firstFixture, "first-case.flac")
	secondFixture := replaceFixtureTag(t, readStrictFLACFixture(t), "ALBUMARTIST=Test Album Artist", "ALBUMARTIST=STRASSE ARTIST")
	secondFixture = replaceFixtureTag(t, secondFixture, "TITLE=  Inspection   Fixture  ", "TITLE=  Inspection   Second!  ")
	secondFixture = replaceFixtureTag(t, secondFixture, "TRACKNUMBER=3/9", "TRACKNUMBER=4/9")
	importOneFLAC(t, router, secondFixture, "second-case.flac")

	tracks := listTracks(t, router)
	if len(tracks.Items) != 2 || tracks.Items[0].AlbumID != tracks.Items[1].AlbumID {
		t.Fatalf("Unicode case-folded Albums = %+v", tracks.Items)
	}
	response := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/library/albums/"+tracks.Items[0].AlbumID, nil, nil)
	var album struct {
		AlbumArtists []struct {
			Name string `json:"name"`
		} `json:"albumArtists"`
	}
	testutil.DecodeJSON(t, response, &album)
	if len(album.AlbumArtists) != 1 || album.AlbumArtists[0].Name != "Straße Artist" {
		t.Fatalf("Album Artist display value = %+v", album.AlbumArtists)
	}
}

func TestManagedImportRejectsControlCharactersInIdentityValues(t *testing.T) {
	fixture := replaceFixtureTag(t, readStrictFLACFixture(t), "ARTIST=Test Artist", "ARTIST=Test\x1fArtist")
	response, database := uploadManagedImportFixture(t, fixture)

	assertValidationFailure(t, response, "invalid_metadata", "ARTIST")
	assertNoLibraryEntities(t, database)
}

func TestManagedImportRejectsControlWhitespaceAtIdentityValueEdges(t *testing.T) {
	testCases := []struct {
		name  string
		value string
	}{
		{name: "Leading tab", value: "ARTIST=\tTest Artis"},
		{name: "Trailing newline", value: "ARTIST=Test Artis\n"},
	}

	for _, testCase := range testCases {
		t.Run(testCase.name, func(t *testing.T) {
			fixture := replaceFixtureTag(t, readStrictFLACFixture(t), "ARTIST=Test Artist", testCase.value)
			response, database := uploadManagedImportFixture(t, fixture)

			assertValidationFailure(t, response, "invalid_metadata", "ARTIST")
			assertNoLibraryEntities(t, database)
		})
	}
}

func TestManagedImportRejectsBidirectionalControlsInIdentityValues(t *testing.T) {
	fixture := replaceFixtureTag(t, readStrictFLACFixture(t), "ARTIST=Test Artist", "ARTIST=A\u202e Artist")
	response, database := uploadManagedImportFixture(t, fixture)

	assertValidationFailure(t, response, "invalid_metadata", "ARTIST")
	assertNoLibraryEntities(t, database)
}

func TestManagedImportRejectsInvalidUTF8InIdentityValues(t *testing.T) {
	fixture := replaceFixtureTag(t, readStrictFLACFixture(t), "ARTIST=Test Artist", "ARTIST=\xffest Artist")
	response, database := uploadManagedImportFixture(t, fixture)

	assertValidationFailure(t, response, "invalid_metadata", "ARTIST")
	assertNoLibraryEntities(t, database)
}

func TestManagedImportValidationErrorsExplainHowToRepairMetadata(t *testing.T) {
	fixture := replaceFixtureTag(t, readStrictFLACFixture(t), "TITLE=  Inspection   Fixture  ", "XITLE=  Inspection   Fixture  ")
	response, _ := uploadManagedImportFixture(t, fixture)
	var failure struct {
		Message string `json:"message"`
		Reason  string `json:"reason"`
	}
	testutil.DecodeJSON(t, response, &failure)

	if failure.Reason != "required tag is missing" || !strings.Contains(failure.Message, failure.Reason) {
		t.Fatalf("actionable validation error = %+v", failure)
	}
}

func TestManagedImportValidationErrorsDoNotExposeParserDetails(t *testing.T) {
	fixture := readStrictFLACFixture(t)
	response, _ := uploadManagedImportFixture(t, fixture[:len(fixture)-8])
	var failure struct {
		Code   string `json:"code"`
		Field  string `json:"field"`
		Reason string `json:"reason"`
	}
	testutil.DecodeJSON(t, response, &failure)

	if failure.Code != "audio_decode_failed" || failure.Field != "audio" || failure.Reason != "audio stream failed full decode" {
		t.Fatalf("safe validation error = %+v", failure)
	}
}

func TestManagedImportRejectsTotalsConflictingWithExistingAlbum(t *testing.T) {
	testCases := []struct {
		name             string
		firstPosition    string
		secondPosition   string
		expectedField    string
		positionTagValue string
	}{
		{name: "Disc total", firstPosition: "DISCNUMBER=1/2", secondPosition: "DISCNUMBER=2/3", expectedField: "TOTALDISCS", positionTagValue: "DISCNUMBER=1/1"},
		{name: "Disc total below existing position", firstPosition: "DISCNUMBER=2", secondPosition: "DISCNUMBER=1/1", expectedField: "TOTALDISCS", positionTagValue: "DISCNUMBER=1/1"},
		{name: "Track total on same disc", firstPosition: "TRACKNUMBER=2/9", secondPosition: "TRACKNUMBER=4/8", expectedField: "TOTALTRACKS", positionTagValue: "TRACKNUMBER=3/9"},
		{name: "Track total below existing position", firstPosition: "TRACKNUMBER=10", secondPosition: "TRACKNUMBER=1/9", expectedField: "TOTALTRACKS", positionTagValue: "TRACKNUMBER=3/9"},
	}

	for _, testCase := range testCases {
		t.Run(testCase.name, func(t *testing.T) {
			database := testutil.OpenMigratedDB(t)
			configuration := config.Config{ManagedStoragePath: t.TempDir()}
			importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
			libraryModule := library.NewModule(database)
			router := chi.NewRouter()
			importModule.RegisterRoutes(router)
			libraryModule.RegisterRoutes(router)
			firstFixture := replaceFixtureTag(t, readStrictFLACFixture(t), testCase.positionTagValue, testCase.firstPosition)
			importOneFLAC(t, router, firstFixture, "first-total.flac")
			secondFixture := readStrictFLACFixture(t)
			if testCase.secondPosition != testCase.positionTagValue {
				secondFixture = replaceFixtureTag(t, secondFixture, testCase.positionTagValue, testCase.secondPosition)
			}
			secondFixture = replaceFixtureTag(t, secondFixture, "TITLE=  Inspection   Fixture  ", "TITLE=  Inspection   Second!  ")

			jobResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", nil, nil)
			var job struct {
				ID string `json:"id"`
			}
			testutil.DecodeJSON(t, jobResponse, &job)
			response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+job.ID+"/file", bytes.NewReader(secondFixture), map[string]string{
				"Content-Type":      "audio/flac",
				"X-Import-Filename": "conflicting-total.flac",
			})

			assertValidationFailure(t, response, "invalid_metadata", testCase.expectedField)
			if tracks := listTracks(t, router); len(tracks.Items) != 1 {
				t.Fatalf("Tracks after conflicting total = %+v", tracks.Items)
			}
		})
	}
}

func TestManagedImportReusesMatchingNormalizedAlbumArtwork(t *testing.T) {
	router := newManagedImportTestRouter(t, t.TempDir())
	firstFixture := readStrictFLACFixture(t)
	firstTrackID := importOneFLAC(t, router, firstFixture, "first.flac")
	secondFixture := secondTrackFixture(firstFixture)

	secondTrackID := importOneFLAC(t, router, secondFixture, "second.flac")

	if secondTrackID == firstTrackID {
		t.Fatalf("second Managed Track reused Track ID %q", firstTrackID)
	}
	tracks := listTracks(t, router)
	if len(tracks.Items) != 2 || tracks.Items[0].AlbumID != tracks.Items[1].AlbumID {
		t.Fatalf("Tracks in matching normalized Album = %+v", tracks.Items)
	}
	assertNormalizedAlbum(t, router, tracks.Items[0].AlbumID, firstTrackID)
}

func TestManagedImportSerializesConcurrentCommitsForOneAlbum(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	managedStoragePath := t.TempDir()
	configuration := config.Config{ManagedStoragePath: managedStoragePath}
	importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	importModule.RegisterRoutes(router)
	fixture := readStrictFLACFixture(t)
	firstTrackID := importOneFLAC(t, router, fixture, "first.flac")
	var artworkPath string
	if err := database.QueryRow(`SELECT album_artwork.file_path
		FROM tracks JOIN album_artwork ON album_artwork.album_id = tracks.album_id
		WHERE tracks.id = ?`, firstTrackID).Scan(&artworkPath); err != nil {
		t.Fatalf("resolve first Album Artwork: %v", err)
	}
	if _, err := database.Exec(`DELETE FROM album_artwork WHERE file_path = ?`, artworkPath); err != nil {
		t.Fatalf("remove first Album Artwork record: %v", err)
	}
	if err := os.Remove(artworkPath); err != nil {
		t.Fatalf("remove first canonical Album Artwork: %v", err)
	}
	secondJobID, secondRevision := uploadFLACForPreview(t, router, secondTrackFixture(fixture), "second.flac")
	thirdJobID, thirdRevision := uploadFLACForPreview(t, router, thirdTrackFixture(fixture), "third.flac")
	type confirmationResult struct {
		status int
		body   string
	}
	results := make(chan confirmationResult, 2)
	start := make(chan struct{})
	for _, confirmation := range []struct {
		jobID    string
		revision int
	}{
		{jobID: secondJobID, revision: secondRevision},
		{jobID: thirdJobID, revision: thirdRevision},
	} {
		confirmation := confirmation
		go func() {
			<-start
			request := httptest.NewRequest(http.MethodPost, "/api/v1/imports/"+confirmation.jobID+"/confirm", strings.NewReader(fmt.Sprintf(`{"revision":%d}`, confirmation.revision)))
			request.Header.Set("Content-Type", "application/json")
			response := httptest.NewRecorder()
			router.ServeHTTP(response, request)
			results <- confirmationResult{status: response.Code, body: response.Body.String()}
		}()
	}
	close(start)
	for range 2 {
		result := <-results
		if result.status != http.StatusOK {
			t.Fatalf("concurrent confirmation status = %d, body = %s", result.status, result.body)
		}
	}
	if _, err := os.Stat(artworkPath); err != nil {
		t.Fatalf("stat canonical Album Artwork after concurrent confirmations: %v", err)
	}
	var artworkCount int
	if err := database.QueryRow(`SELECT COUNT(*) FROM album_artwork WHERE file_path = ?`, artworkPath).Scan(&artworkCount); err != nil {
		t.Fatalf("count committed Album Artwork records: %v", err)
	}
	if artworkCount != 1 {
		t.Fatalf("committed Album Artwork records = %d, want 1", artworkCount)
	}
}

func TestManagedImportRejectsExistingAlbumOutsideCanonicalLayout(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	managedStoragePath := t.TempDir()
	configuration := config.Config{ManagedStoragePath: managedStoragePath}
	importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	importModule.RegisterRoutes(router)
	fixture := readStrictFLACFixture(t)
	firstTrackID := importOneFLAC(t, router, fixture, "first.flac")
	moveAlbumToLegacyPath(t, database, managedStoragePath, firstTrackID)
	secondFixture := bytes.Replace(fixture, []byte("TITLE=  Inspection   Fixture  "), []byte("TITLE=  Inspection   Second!  "), 1)
	secondFixture = bytes.Replace(secondFixture, []byte("TRACKNUMBER=3/9"), []byte("TRACKNUMBER=4/9"), 1)
	jobID, revision := uploadFLACForPreview(t, router, secondFixture, "second.flac")

	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports/"+jobID+"/confirm", strings.NewReader(fmt.Sprintf(`{"revision":%d}`, revision)), map[string]string{
		"Content-Type": "application/json",
	})

	testutil.AssertErrorCode(t, response, http.StatusConflict, "unsafe_storage_path")
}

func importOneFLAC(t *testing.T, router http.Handler, fixture []byte, filename string) string {
	t.Helper()
	jobID, revision := uploadFLACForPreview(t, router, fixture, filename)
	confirmation := strings.NewReader(fmt.Sprintf(`{"revision":%d}`, revision))
	confirmResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports/"+jobID+"/confirm", confirmation, map[string]string{"Content-Type": "application/json"})
	if confirmResponse.Code != http.StatusOK {
		t.Fatalf("confirm %q status = %d, body = %s", filename, confirmResponse.Code, confirmResponse.Body.String())
	}
	var result struct {
		TrackID string `json:"trackId"`
	}
	testutil.DecodeJSON(t, confirmResponse, &result)
	return result.TrackID
}

func uploadFLACForPreview(t *testing.T, router http.Handler, fixture []byte, filename string) (string, int) {
	t.Helper()
	jobID := createImportJob(t, router)
	uploadResponse := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(fixture), map[string]string{
		"Content-Type":      "audio/flac",
		"X-Import-Filename": filename,
	})
	if uploadResponse.Code != http.StatusOK {
		t.Fatalf("upload %q status = %d, body = %s", filename, uploadResponse.Code, uploadResponse.Body.String())
	}
	var preview struct {
		Revision int `json:"revision"`
	}
	testutil.DecodeJSON(t, uploadResponse, &preview)
	return jobID, preview.Revision
}

func uploadFLACToJob(t *testing.T, router http.Handler, jobID string, fixture []byte, filename string) managedimport.Preview {
	t.Helper()
	response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(fixture), map[string]string{
		"Content-Type": "audio/flac", "X-Import-Filename": filename,
	})
	if response.Code != http.StatusOK {
		t.Fatalf("upload %q status = %d, body = %s", filename, response.Code, response.Body.String())
	}
	var preview managedimport.Preview
	testutil.DecodeJSON(t, response, &preview)
	return preview
}

func moveAlbumToLegacyPath(t *testing.T, database *sql.DB, root, trackID string) {
	t.Helper()
	var audioPath, artworkPath string
	err := database.QueryRow(`SELECT tracks.file_path, album_artwork.file_path
		FROM tracks JOIN album_artwork ON album_artwork.album_id = tracks.album_id
		WHERE tracks.id = ?`, trackID).Scan(&audioPath, &artworkPath)
	if err != nil {
		t.Fatalf("resolve committed Album paths: %v", err)
	}
	legacyAlbumPath := filepath.Join(root, "library", "test-album-artist", filepath.Base(filepath.Dir(audioPath)))
	if err := os.MkdirAll(filepath.Dir(legacyAlbumPath), 0o750); err != nil {
		t.Fatalf("create legacy Artist path: %v", err)
	}
	if err := os.Rename(filepath.Dir(audioPath), legacyAlbumPath); err != nil {
		t.Fatalf("move Album to legacy path: %v", err)
	}
	updateLegacyAlbumPaths(t, database, trackID, audioPath, artworkPath, legacyAlbumPath)
}

func updateLegacyAlbumPaths(t *testing.T, database *sql.DB, trackID, audioPath, artworkPath, legacyAlbumPath string) {
	t.Helper()
	newAudioPath := filepath.Join(legacyAlbumPath, filepath.Base(audioPath))
	newArtworkPath := filepath.Join(legacyAlbumPath, filepath.Base(artworkPath))
	updates := []struct {
		query string
		args  []any
	}{
		{query: `UPDATE tracks SET file_path = ? WHERE id = ?`, args: []any{newAudioPath, trackID}},
		{query: `UPDATE track_sources SET file_path = ? WHERE track_id = ?`, args: []any{newAudioPath, trackID}},
		{query: `UPDATE album_artwork SET file_path = ? WHERE file_path = ?`, args: []any{newArtworkPath, artworkPath}},
	}
	for _, update := range updates {
		if _, err := database.Exec(update.query, update.args...); err != nil {
			t.Fatalf("update legacy Album path: %v", err)
		}
	}
}

func createImportJob(t *testing.T, router http.Handler) string {
	t.Helper()
	jobResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", nil, nil)
	var job struct {
		ID string `json:"id"`
	}
	testutil.DecodeJSON(t, jobResponse, &job)
	return job.ID
}

func createBatchImportJob(t *testing.T, router http.Handler, batchID string) string {
	t.Helper()
	body := strings.NewReader(fmt.Sprintf(`{"batchId":%q,"clientFileId":%q}`, batchID, uuid.NewString()))
	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", body, map[string]string{"Content-Type": "application/json"})
	if response.Code != http.StatusCreated {
		t.Fatalf("create batch Import Job status = %d, body = %s", response.Code, response.Body.String())
	}
	var job managedimport.Job
	testutil.DecodeJSON(t, response, &job)
	return job.ID
}

func getImportBatch(t *testing.T, router http.Handler, batchID string) managedimport.Batch {
	t.Helper()
	response := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/import-batches/"+batchID, nil, nil)
	if response.Code != http.StatusOK {
		t.Fatalf("get Import Batch status = %d, body = %s", response.Code, response.Body.String())
	}
	var batch managedimport.Batch
	testutil.DecodeJSON(t, response, &batch)
	return batch
}

func confirmImportBatch(t *testing.T, router http.Handler, batch managedimport.Batch, selectedFileIDs []string) *httptest.ResponseRecorder {
	t.Helper()
	selectedJSON := make([]string, len(selectedFileIDs))
	for index, jobID := range selectedFileIDs {
		selectedJSON[index] = fmt.Sprintf("%q", jobID)
	}
	body := strings.NewReader(fmt.Sprintf(`{"revision":%d,"selectedFileIds":[%s]}`, batch.Revision, strings.Join(selectedJSON, ",")))
	return testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches/"+batch.ID+"/confirm", body, map[string]string{"Content-Type": "application/json"})
}

type cancelOnConfirmationInspector struct {
	delegate library.MediaInspector
	cancel   context.CancelFunc
	calls    int
}

func (inspector *cancelOnConfirmationInspector) Inspect(ctx context.Context, path string, reportProgress library.InspectionProgressReporter) (library.MediaInspection, error) {
	inspector.calls++
	if inspector.calls == 2 {
		inspector.cancel()
	}
	return inspector.delegate.Inspect(ctx, path, reportProgress)
}

func newManagedImportTestRouter(t *testing.T, managedStoragePath string) http.Handler {
	t.Helper()
	database := testutil.OpenMigratedDB(t)
	return newManagedImportTestRouterWithDatabase(t, database, managedStoragePath)
}

func newManagedImportTestRouterWithDatabase(t *testing.T, database *sql.DB, managedStoragePath string) http.Handler {
	t.Helper()
	configuration := config.Config{ManagedStoragePath: managedStoragePath}
	importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	libraryModule := library.NewModule(database)
	router := chi.NewRouter()
	importModule.RegisterRoutes(router)
	libraryModule.RegisterRoutes(router)
	return router
}

func secondTrackFixture(fixture []byte) []byte {
	result := bytes.Replace(fixture, []byte("TITLE=  Inspection   Fixture  "), []byte("TITLE=  Inspection   Second!  "), 1)
	return bytes.Replace(result, []byte("TRACKNUMBER=3/9"), []byte("TRACKNUMBER=4/9"), 1)
}

func thirdTrackFixture(fixture []byte) []byte {
	result := bytes.Replace(fixture, []byte("TITLE=  Inspection   Fixture  "), []byte("TITLE=  Inspection   Third!!  "), 1)
	return bytes.Replace(result, []byte("TRACKNUMBER=3/9"), []byte("TRACKNUMBER=5/9"), 1)
}

type stagingObservingReader struct {
	data                []byte
	offset              int
	managedStoragePath  string
	observedStagedBytes bool
}

func (reader *stagingObservingReader) Read(buffer []byte) (int, error) {
	if reader.offset > 0 && !reader.observedStagedBytes {
		matches, err := filepath.Glob(filepath.Join(reader.managedStoragePath, ".staging", "*.upload"))
		if err != nil {
			return 0, err
		}
		for _, path := range matches {
			info, statErr := os.Stat(path)
			if statErr != nil {
				return 0, statErr
			}
			if info.Size() > 0 {
				reader.observedStagedBytes = true
				break
			}
		}
		if !reader.observedStagedBytes {
			return 0, errors.New("server-owned staging has no bytes while upload is still being read")
		}
	}
	if reader.offset == len(reader.data) {
		return 0, io.EOF
	}
	chunkSize := min(256, len(reader.data)-reader.offset, len(buffer))
	copy(buffer, reader.data[reader.offset:reader.offset+chunkSize])
	reader.offset += chunkSize
	return chunkSize, nil
}

type trackList struct {
	Items []struct {
		ID           string `json:"id"`
		Title        string `json:"title"`
		AlbumID      string `json:"albumId"`
		AlbumTitle   string `json:"albumTitle"`
		DiscNo       int    `json:"discNo"`
		DurationMs   int    `json:"durationMs"`
		Container    string `json:"container"`
		Codec        string `json:"codec"`
		SampleRateHz int    `json:"sampleRateHz"`
		ChannelCount int    `json:"channelCount"`
		BitDepth     int    `json:"bitDepth"`
		BitrateBps   int    `json:"bitrateBps"`
		ReplayGain   struct {
			TrackGainDB *float64 `json:"trackGainDb"`
			TrackPeak   *float64 `json:"trackPeak"`
			AlbumGainDB *float64 `json:"albumGainDb"`
			AlbumPeak   *float64 `json:"albumPeak"`
		} `json:"replayGain"`
		Artists []struct {
			Name string `json:"name"`
		} `json:"artists"`
		Genres []struct {
			Name string `json:"name"`
		} `json:"genres"`
	} `json:"items"`
}

type albumArtworkResponse struct {
	ContentSHA256 string `json:"contentSha256"`
	MediaType     string `json:"mediaType"`
	Width         int    `json:"width"`
	Height        int    `json:"height"`
	SizeBytes     int    `json:"sizeBytes"`
	SourceTrackID string `json:"sourceTrackId"`
}

func serveImportConfirmation(router http.Handler, jobID string, revision int) *httptest.ResponseRecorder {
	body := strings.NewReader(fmt.Sprintf(`{"revision":%d}`, revision))
	request := httptest.NewRequest(http.MethodPost, "/api/v1/imports/"+jobID+"/confirm", body)
	request.Header.Set("Content-Type", "application/json")
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)
	return response
}

func assertNormalizedAlbum(t *testing.T, router http.Handler, albumID, sourceTrackID string) {
	t.Helper()
	response := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/library/albums/"+albumID, nil, nil)
	if response.Code != http.StatusOK {
		t.Fatalf("get imported Album status = %d, body = %s", response.Code, response.Body.String())
	}
	var album struct {
		Title        string `json:"title"`
		ReleaseDate  string `json:"releaseDate"`
		AlbumArtists []struct {
			Name string `json:"name"`
		} `json:"albumArtists"`
		GenreItems []struct {
			Name string `json:"name"`
		} `json:"genreItems"`
		Artwork *albumArtworkResponse `json:"artwork"`
	}
	testutil.DecodeJSON(t, response, &album)
	if album.Title != "Strict Import Tests" || album.ReleaseDate != "2026" || len(album.AlbumArtists) != 1 || album.AlbumArtists[0].Name != "Test Album Artist" {
		t.Fatalf("normalized imported Album = %+v", album)
	}
	if len(album.GenreItems) != 1 || album.GenreItems[0].Name != "Electronic" {
		t.Fatalf("imported Album Genre/Artwork = %+v", album)
	}
	assertAlbumArtworkMetadata(t, album.Artwork, sourceTrackID)
	coverResponse := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/library/albums/"+albumID+"/cover", nil, nil)
	if coverResponse.Code != http.StatusOK || coverResponse.Header().Get("Content-Type") != "image/png" || coverResponse.Body.Len() == 0 {
		t.Fatalf("imported Album Artwork response = %d %q (%d bytes)", coverResponse.Code, coverResponse.Header().Get("Content-Type"), coverResponse.Body.Len())
	}
}

func assertAlbumArtworkMetadata(t *testing.T, artwork *albumArtworkResponse, sourceTrackID string) {
	t.Helper()
	if artwork == nil || artwork.MediaType != "image/png" {
		t.Fatalf("persisted Album Artwork = %+v", artwork)
	}
	if artwork.ContentSHA256 != "d153c3ba2710fc0a3e364b3533812a538287d75bdfe407bd2f6e7c4c2358e85e" || artwork.Width != 32 || artwork.Height != 32 || artwork.SizeBytes <= 0 || artwork.SourceTrackID != sourceTrackID {
		t.Fatalf("persisted Album Artwork metadata = %+v", artwork)
	}
}

func listTracks(t *testing.T, router http.Handler) trackList {
	t.Helper()
	response := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/library/tracks", nil, nil)
	if response.Code != http.StatusOK {
		t.Fatalf("list Tracks status = %d, body = %s", response.Code, response.Body.String())
	}
	var result trackList
	testutil.DecodeJSON(t, response, &result)
	return result
}

func assertCanonicalStorage(t *testing.T, managedStoragePath string, fixture []byte) {
	t.Helper()
	var audioPath string
	var artworkPath string
	err := filepath.WalkDir(managedStoragePath, func(path string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if entry.IsDir() {
			return nil
		}
		switch filepath.Ext(path) {
		case ".flac":
			audioPath = path
		case ".png":
			artworkPath = path
		}
		return nil
	})
	if err != nil {
		t.Fatalf("walk Managed Storage: %v", err)
	}
	if audioPath == "" || artworkPath == "" {
		t.Fatalf("canonical audio/artwork paths = %q / %q", audioPath, artworkPath)
	}
	if filepath.Base(audioPath) == "strict-import.flac" || !strings.Contains(audioPath, "strict-import-tests") || !strings.Contains(audioPath, "inspection-fixture") {
		t.Fatalf("audio path is not canonical: %q", audioPath)
	}
	relativePath, err := filepath.Rel(managedStoragePath, audioPath)
	if err != nil {
		t.Fatalf("resolve canonical relative path: %v", err)
	}
	parts := strings.Split(relativePath, string(filepath.Separator))
	if len(parts) != 4 || parts[0] != "library" {
		t.Fatalf("canonical path parts = %v", parts)
	}
	assertCanonicalID(t, parts[1], "test-album-artist-")
	assertCanonicalID(t, parts[2], "strict-import-tests-")
	audioStem := strings.TrimSuffix(parts[3], ".flac")
	assertCanonicalID(t, audioStem, "01-03-inspection-fixture-")
	stored, err := os.ReadFile(audioPath)
	if err != nil {
		t.Fatalf("read canonical audio: %v", err)
	}
	if !bytes.Equal(stored, fixture) {
		t.Fatal("canonical audio bytes differ from uploaded FLAC")
	}
}

func assertStagedStorage(t *testing.T, managedStoragePath, jobID string, fixture []byte) {
	t.Helper()
	stagingPath := filepath.Join(managedStoragePath, ".staging")
	stagingInfo, err := os.Stat(stagingPath)
	if err != nil {
		t.Fatalf("stat staging directory: %v", err)
	}
	if stagingInfo.Mode().Perm() != 0o700 {
		t.Fatalf("staging directory mode = %o", stagingInfo.Mode().Perm())
	}
	entries, err := os.ReadDir(stagingPath)
	if err != nil || len(entries) != 1 {
		t.Fatalf("staging entries = %v, error = %v", entries, err)
	}
	entry := entries[0]
	if entry.Name() == jobID+".upload" || !strings.HasPrefix(entry.Name(), ".import-") {
		t.Fatalf("staging filename is not an independent server-generated name: %q", entry.Name())
	}
	info, err := entry.Info()
	if err != nil {
		t.Fatalf("stat staged file: %v", err)
	}
	if info.Mode().Perm() != 0o600 {
		t.Fatalf("staged file mode = %o", info.Mode().Perm())
	}
	stagedBytes, err := os.ReadFile(filepath.Join(stagingPath, entry.Name()))
	if err != nil {
		t.Fatalf("read staged file: %v", err)
	}
	if sha256.Sum256(stagedBytes) != sha256.Sum256(fixture) {
		t.Fatal("staged file SHA-256 differs from uploaded bytes")
	}
}

func assertCanonicalID(t *testing.T, value, prefix string) {
	t.Helper()
	stableID := strings.TrimPrefix(value, prefix)
	if stableID == value {
		t.Fatalf("canonical component %q does not start with %q", value, prefix)
	}
	if _, err := uuid.Parse(stableID); err != nil {
		t.Fatalf("canonical component %q has invalid stable ID: %v", value, err)
	}
}

func assertCanonicalFileCounts(t *testing.T, managedStoragePath string, expectedAudio, expectedArtwork int) {
	t.Helper()
	var audioCount int
	var artworkCount int
	err := filepath.WalkDir(managedStoragePath, func(path string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if entry.IsDir() {
			return nil
		}
		switch filepath.Ext(path) {
		case ".flac":
			audioCount++
		case ".jpg", ".png", ".webp":
			artworkCount++
		}
		return nil
	})
	if err != nil {
		t.Fatalf("walk Managed Storage: %v", err)
	}
	if audioCount != expectedAudio || artworkCount != expectedArtwork {
		t.Fatalf("canonical audio/artwork counts = %d/%d, want %d/%d", audioCount, artworkCount, expectedAudio, expectedArtwork)
	}
}

func withoutFrontCover(t *testing.T, fixture []byte) []byte {
	t.Helper()
	result := append([]byte(nil), fixture...)
	_, bodyOffset, _ := findFLACPictureBlock(t, result)
	binary.BigEndian.PutUint32(result[bodyOffset:bodyOffset+FLAC_PICTURE_FIELD_SIZE_BYTES], library.FLAC_PICTURE_TYPE_BACK_COVER)
	return result
}

func replaceFrontCover(t *testing.T, fixture, artwork []byte) []byte {
	t.Helper()
	headerOffset, bodyOffset, bodyEnd := findFLACPictureBlock(t, fixture)
	newBody := replaceFLACPictureData(t, fixture[bodyOffset:bodyEnd], artwork)
	newBlock := encodeFLACMetadataBlock(fixture[headerOffset], newBody)
	result := append([]byte(nil), fixture[:headerOffset]...)
	result = append(result, newBlock...)
	return append(result, fixture[bodyEnd:]...)
}

func findFLACPictureBlock(t *testing.T, fixture []byte) (int, int, int) {
	t.Helper()
	offset := library.FLAC_SIGNATURE_SIZE_BYTES
	for offset+library.FLAC_METADATA_BLOCK_HEADER_SIZE_BYTES <= len(fixture) {
		headerOffset := offset
		header := fixture[headerOffset]
		length := flacMetadataBlockLength(fixture, headerOffset)
		bodyOffset := headerOffset + library.FLAC_METADATA_BLOCK_HEADER_SIZE_BYTES
		bodyEnd := bodyOffset + length
		if bodyEnd > len(fixture) {
			t.Fatal("strict FLAC fixture metadata is truncated")
		}
		if header&FLAC_METADATA_BLOCK_TYPE_MASK == byte(flacmeta.TypePicture) {
			return headerOffset, bodyOffset, bodyEnd
		}
		offset = bodyEnd
		if header&FLAC_METADATA_BLOCK_LAST_FLAG != 0 {
			break
		}
	}
	t.Fatal("strict FLAC fixture has no Picture block")
	return 0, 0, 0
}

func flacMetadataBlockLength(fixture []byte, headerOffset int) int {
	high := int(fixture[headerOffset+FLAC_METADATA_LENGTH_HIGH_BYTE_OFFSET]) << FLAC_METADATA_LENGTH_HIGH_SHIFT
	middle := int(fixture[headerOffset+FLAC_METADATA_LENGTH_MIDDLE_BYTE_OFFSET]) << FLAC_METADATA_LENGTH_MIDDLE_SHIFT
	return high | middle | int(fixture[headerOffset+FLAC_METADATA_LENGTH_LOW_BYTE_OFFSET])
}

func replaceFLACPictureData(t *testing.T, body, artwork []byte) []byte {
	t.Helper()
	dataLengthOffset := pictureDataLengthOffset(t, body)
	newBody := append([]byte(nil), body[:dataLengthOffset]...)
	lengthBytes := make([]byte, FLAC_PICTURE_FIELD_SIZE_BYTES)
	binary.BigEndian.PutUint32(lengthBytes, uint32(len(artwork)))
	newBody = append(newBody, lengthBytes...)
	return append(newBody, artwork...)
}

func encodeFLACMetadataBlock(header byte, body []byte) []byte {
	length := len(body)
	result := []byte{
		header,
		byte(length >> FLAC_METADATA_LENGTH_HIGH_SHIFT),
		byte(length >> FLAC_METADATA_LENGTH_MIDDLE_SHIFT),
		byte(length),
	}
	return append(result, body...)
}

func pictureDataLengthOffset(t *testing.T, body []byte) int {
	t.Helper()
	minimumHeaderSize := FLAC_PICTURE_FIELD_SIZE_BYTES * 2
	if len(body) < minimumHeaderSize {
		t.Fatal("FLAC Picture block is truncated")
	}
	offset := FLAC_PICTURE_FIELD_SIZE_BYTES
	mimeLength := int(binary.BigEndian.Uint32(body[offset : offset+FLAC_PICTURE_FIELD_SIZE_BYTES]))
	offset += FLAC_PICTURE_FIELD_SIZE_BYTES + mimeLength
	if offset+FLAC_PICTURE_FIELD_SIZE_BYTES > len(body) {
		t.Fatal("FLAC Picture MIME is truncated")
	}
	descriptionLength := int(binary.BigEndian.Uint32(body[offset : offset+FLAC_PICTURE_FIELD_SIZE_BYTES]))
	offset += FLAC_PICTURE_FIELD_SIZE_BYTES + descriptionLength + FLAC_PICTURE_DIMENSION_FIELDS_SIZE_BYTES
	if offset+FLAC_PICTURE_FIELD_SIZE_BYTES > len(body) {
		t.Fatal("FLAC Picture fields are truncated")
	}
	return offset
}

func encodeAlternatePNG(t *testing.T) []byte {
	t.Helper()
	artwork := image.NewRGBA(image.Rect(0, 0, 2, 2))
	artwork.Set(0, 0, color.RGBA{R: 255, G: 64, A: 255})
	artwork.Set(1, 1, color.RGBA{B: 255, A: 255})
	var output bytes.Buffer
	if err := png.Encode(&output, artwork); err != nil {
		t.Fatalf("encode alternate PNG: %v", err)
	}
	return output.Bytes()
}

func readStrictFLACFixture(t *testing.T) []byte {
	t.Helper()
	fixture, err := os.ReadFile(filepath.Join("..", "library", "testdata", "strict-import.flac"))
	if err != nil {
		t.Fatalf("read strict FLAC fixture: %v", err)
	}
	return fixture
}

func replaceFixtureTag(t *testing.T, fixture []byte, oldValue string, newValue string) []byte {
	t.Helper()
	if newValue == "" {
		separatorIndex := strings.Index(oldValue, "=")
		newValue = oldValue[:separatorIndex+1] + strings.Repeat(" ", len(oldValue)-separatorIndex-1)
	}
	if len(newValue) < len(oldValue) {
		newValue += strings.Repeat(" ", len(oldValue)-len(newValue))
	}
	if len(oldValue) != len(newValue) {
		t.Fatalf("fixture tag replacement lengths differ: %d != %d", len(oldValue), len(newValue))
	}
	updated := bytes.Replace(fixture, []byte(oldValue), []byte(newValue), 1)
	if bytes.Equal(updated, fixture) {
		t.Fatalf("fixture tag %q not found", oldValue)
	}
	return updated
}

func uploadManagedImportFixture(t *testing.T, fixture []byte) (*httptest.ResponseRecorder, *sql.DB) {
	t.Helper()
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	importModule := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	importModule.RegisterRoutes(router)
	return uploadFixtureThroughRouter(t, router, fixture), database
}

func uploadFixtureThroughRouter(t *testing.T, router http.Handler, fixture []byte) *httptest.ResponseRecorder {
	t.Helper()
	jobResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", nil, nil)
	var job struct {
		ID string `json:"id"`
	}
	testutil.DecodeJSON(t, jobResponse, &job)
	response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+job.ID+"/file", bytes.NewReader(fixture), map[string]string{
		"Content-Type":      "audio/flac",
		"X-Import-Filename": "identity-test.flac",
	})
	return response
}

func assertValidationFailure(t *testing.T, response *httptest.ResponseRecorder, expectedCode, expectedField string) {
	t.Helper()
	if response.Code != http.StatusUnprocessableEntity {
		t.Fatalf("validation status = %d, body = %s", response.Code, response.Body.String())
	}
	var failure struct {
		Code  string `json:"code"`
		Field string `json:"field"`
	}
	testutil.DecodeJSON(t, response, &failure)
	if failure.Code != expectedCode || failure.Field != expectedField {
		t.Fatalf("validation error = %+v", failure)
	}
}

func assertNoLibraryEntities(t *testing.T, database interface {
	QueryRow(query string, args ...any) *sql.Row
}) {
	t.Helper()
	for _, tableName := range []string{"artists", "albums", "genres", "album_artwork", "tracks"} {
		var count int
		if err := database.QueryRow("SELECT COUNT(*) FROM " + tableName).Scan(&count); err != nil {
			t.Fatalf("count %s: %v", tableName, err)
		}
		if count != 0 {
			t.Fatalf("%s row count = %d, want 0", tableName, count)
		}
	}
}

func newConfiguredManagedImportRouter(t *testing.T, configuration config.Config) http.Handler {
	t.Helper()
	database := testutil.OpenMigratedDB(t)
	module := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	module.RegisterRoutes(router)
	return router
}
