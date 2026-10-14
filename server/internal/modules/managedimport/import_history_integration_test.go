package managedimport_test

import (
	"bytes"
	"crypto/sha256"
	"errors"
	"fmt"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/ardam/navidrome-replacement/server/internal/config"
	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/ardam/navidrome-replacement/server/internal/modules/managedimport"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
	"github.com/go-chi/chi/v5"
)

func TestImportHistoryReportsPartialBatchWithoutStagingOrRawMetadata(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	module := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	module.RegisterRoutes(router)
	fixture := readStrictFLACFixture(t)

	batch := createHistoryTestBatch(t, router)
	acceptedFileID := "00000000-0000-4000-8000-000000000001"
	rejectedFileID := "00000000-0000-4000-8000-000000000002"
	acceptedJob := createHistoryTestJob(t, router, batch.ID, acceptedFileID)
	rejectedJob := createHistoryTestJob(t, router, batch.ID, rejectedFileID)
	uploadHistoryTestFile(t, router, acceptedJob.ID, "strict-import.flac", fixture, http.StatusOK)
	uploadHistoryTestFile(t, router, rejectedJob.ID, "broken.flac", []byte("not audio"), http.StatusUnprocessableEntity)

	batchResponse := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/import-batches/"+batch.ID, nil, nil)
	testutil.DecodeJSON(t, batchResponse, &batch)
	confirmation := strings.NewReader(fmt.Sprintf(`{"revision":%d,"selectedFileIds":[%q]}`, batch.Revision, acceptedJob.ID))
	confirmResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches/"+batch.ID+"/confirm", confirmation, map[string]string{"Content-Type": "application/json"})
	if confirmResponse.Code != http.StatusOK {
		t.Fatalf("confirm Import Batch status = %d, body = %s", confirmResponse.Code, confirmResponse.Body.String())
	}

	history := getImportHistory(t, router)
	if len(history.Items) != 1 {
		t.Fatalf("Import History items = %d, want 1", len(history.Items))
	}
	item := history.Items[0]
	if item.ImportID != batch.ID || item.ResultCode != "partially_completed" || item.Counts.Total != 2 || item.Counts.Imported != 1 || item.Counts.Rejected != 1 {
		t.Fatalf("partial Import History = %+v", item)
	}
	if item.StartedAt.IsZero() || item.CompletedAt.IsZero() || item.CompletedAt.Before(item.StartedAt) {
		t.Fatalf("Import History timestamps = %s to %s", item.StartedAt, item.CompletedAt)
	}
	if len(item.Files) != 2 {
		t.Fatalf("Import History files = %d, want 2", len(item.Files))
	}
	accepted := item.Files[0]
	wantHash := fmt.Sprintf("%x", sha256.Sum256(fixture))
	if accepted.FileID != acceptedFileID || accepted.JobID != acceptedJob.ID || accepted.SafeFilename != "strict-import.flac" || accepted.ContentSHA256 != wantHash || accepted.ResultCode != "imported" || accepted.CreatedTrackID == "" {
		t.Fatalf("accepted Import History file = %+v", accepted)
	}
	if item.Files[1].FileID != rejectedFileID || item.Files[1].ResultCode == "" || item.Files[1].CreatedTrackID != "" || item.Files[1].ReplacedTrackID != "" {
		t.Fatalf("rejected Import History file = %+v", item.Files[1])
	}
	var retainedPreviewCount int
	if err := database.QueryRow(`SELECT COUNT(*) FROM managed_import_jobs WHERE preview_json IS NOT NULL OR staged_file_path IS NOT NULL`).Scan(&retainedPreviewCount); err != nil {
		t.Fatalf("count retained Managed Import payloads: %v", err)
	}
	if retainedPreviewCount != 0 {
		t.Fatalf("retained Managed Import payload count = %d, want 0", retainedPreviewCount)
	}
}

func TestImportHistoryReportsCancellationAndPrunesOldestResult(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	module := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	module.RegisterRoutes(router)

	importIDs := make([]string, managedimport.IMPORT_HISTORY_LIMIT+1)
	for index := range importIDs {
		batch := createHistoryTestBatch(t, router)
		importIDs[index] = batch.ID
		fileID := fmt.Sprintf("00000000-0000-4000-8000-%012d", index+1)
		job := createHistoryTestJob(t, router, batch.ID, fileID)
		if index == len(importIDs)-1 {
			uploadHistoryTestFile(t, router, job.ID, "cancel-me.flac", readStrictFLACFixture(t), http.StatusOK)
		}
		response := testutil.ServeRequest(t, router, http.MethodDelete, "/api/v1/import-batches/"+batch.ID, nil, nil)
		if response.Code != http.StatusNoContent {
			t.Fatalf("cancel Import Batch %d status = %d, body = %s", index, response.Code, response.Body.String())
		}
	}

	history := getImportHistory(t, router)
	if len(history.Items) != managedimport.IMPORT_HISTORY_LIMIT {
		t.Fatalf("Import History items = %d, want %d", len(history.Items), managedimport.IMPORT_HISTORY_LIMIT)
	}
	for _, item := range history.Items {
		if item.ImportID == importIDs[0] {
			t.Fatalf("oldest Import History result %q was not pruned", importIDs[0])
		}
	}
	latest := history.Items[0]
	if latest.ImportID != importIDs[len(importIDs)-1] || latest.ResultCode != "canceled" || latest.Counts.Total != 1 || latest.Counts.Canceled != 1 {
		t.Fatalf("canceled Import History = %+v", latest)
	}
	if len(latest.Files) != 1 || latest.Files[0].SafeFilename != "cancel-me.flac" || latest.Files[0].ResultCode != "canceled" || latest.Files[0].ContentSHA256 == "" {
		t.Fatalf("canceled Import History file = %+v", latest.Files)
	}
}

func TestImportHistoryArchivesStandaloneCommitWithoutRawMetadata(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	module := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	module.RegisterRoutes(router)

	jobID, revision := uploadFLACForPreview(t, router, readStrictFLACFixture(t), "standalone.flac")
	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports/"+jobID+"/confirm", strings.NewReader(fmt.Sprintf(`{"revision":%d}`, revision)), map[string]string{"Content-Type": "application/json"})
	if response.Code != http.StatusOK {
		t.Fatalf("confirm standalone Import status = %d, body = %s", response.Code, response.Body.String())
	}

	history := getImportHistory(t, router)
	if len(history.Items) != 1 || history.Items[0].ImportID != jobID || history.Items[0].ResultCode != "completed" || history.Items[0].Counts.Imported != 1 {
		t.Fatalf("standalone Import History = %+v", history)
	}
	if len(history.Items[0].Files) != 1 || history.Items[0].Files[0].CreatedTrackID == "" {
		t.Fatalf("standalone Import History files = %+v", history.Items[0].Files)
	}
	var retainedPreviewCount int
	if err := database.QueryRow(`SELECT COUNT(*) FROM managed_import_jobs WHERE id = ? AND preview_json IS NOT NULL`, jobID).Scan(&retainedPreviewCount); err != nil {
		t.Fatalf("count standalone Import Preview payloads: %v", err)
	}
	if retainedPreviewCount != 0 {
		t.Fatalf("retained standalone Import Preview payload count = %d, want 0", retainedPreviewCount)
	}
}

func TestImportHistoryRetainsCanceledBatchFile(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	module := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	module.RegisterRoutes(router)

	batch := createHistoryTestBatch(t, router)
	fileID := "00000000-0000-4000-8000-000000000020"
	job := createHistoryTestJob(t, router, batch.ID, fileID)
	cancelResponse := testutil.ServeRequest(t, router, http.MethodDelete, "/api/v1/imports/"+job.ID, nil, nil)
	if cancelResponse.Code != http.StatusNoContent {
		t.Fatalf("cancel Import Batch file status = %d, body = %s", cancelResponse.Code, cancelResponse.Body.String())
	}
	time.Sleep(10 * time.Millisecond)
	batchResponse := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/import-batches/"+batch.ID, nil, nil)
	testutil.DecodeJSON(t, batchResponse, &batch)
	confirmation := strings.NewReader(fmt.Sprintf(`{"revision":%d,"selectedFileIds":[]}`, batch.Revision))
	confirmResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches/"+batch.ID+"/confirm", confirmation, map[string]string{"Content-Type": "application/json"})
	if confirmResponse.Code != http.StatusOK {
		t.Fatalf("confirm batch with canceled file status = %d, body = %s", confirmResponse.Code, confirmResponse.Body.String())
	}

	history := getImportHistory(t, router)
	if len(history.Items) != 1 || history.Items[0].Counts.Total != 1 || history.Items[0].Counts.Canceled != 1 {
		t.Fatalf("canceled Batch file history = %+v", history)
	}
	if len(history.Items[0].Files) != 1 || history.Items[0].Files[0].FileID != fileID || history.Items[0].Files[0].ResultCode != "canceled" {
		t.Fatalf("canceled Batch file result = %+v", history.Items[0].Files)
	}
	if !history.Items[0].Files[0].CompletedAt.Before(history.Items[0].CompletedAt) {
		t.Fatalf("canceled file completion = %s, batch completion = %s", history.Items[0].Files[0].CompletedAt, history.Items[0].CompletedAt)
	}
}

func TestImportHistoryPreservesTerminalFileOutcomesWhenBatchIsCanceled(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	module := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	module.RegisterRoutes(router)

	batch := createHistoryTestBatch(t, router)
	rejectedFileID := "00000000-0000-4000-8000-000000000030"
	unresolvedFileID := "00000000-0000-4000-8000-000000000031"
	rejectedJob := createHistoryTestJob(t, router, batch.ID, rejectedFileID)
	createHistoryTestJob(t, router, batch.ID, unresolvedFileID)
	uploadHistoryTestFile(t, router, rejectedJob.ID, "broken.flac", []byte("not audio"), http.StatusUnprocessableEntity)
	cancelResponse := testutil.ServeRequest(t, router, http.MethodDelete, "/api/v1/import-batches/"+batch.ID, nil, nil)
	if cancelResponse.Code != http.StatusNoContent {
		t.Fatalf("cancel partially terminal Import Batch status = %d, body = %s", cancelResponse.Code, cancelResponse.Body.String())
	}

	history := getImportHistory(t, router)
	item := history.Items[0]
	if item.ResultCode != "canceled" || item.Counts.Rejected != 1 || item.Counts.Canceled != 1 {
		t.Fatalf("partially terminal canceled history = %+v", item)
	}
	if item.Files[0].FileID != rejectedFileID || item.Files[0].ResultCode == "canceled" {
		t.Fatalf("rejected result rewritten by cancellation = %+v", item.Files[0])
	}
	if item.Files[1].FileID != unresolvedFileID || item.Files[1].ResultCode != "canceled" {
		t.Fatalf("unresolved cancellation result = %+v", item.Files[1])
	}
}

func TestCancelRejectedOnlyBatchClosesWithoutConflict(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	configuration := config.Config{ManagedStoragePath: t.TempDir()}
	module := managedimport.NewModule(database, configuration, library.NewMediaInspector())
	router := chi.NewRouter()
	module.RegisterRoutes(router)

	batch := createHistoryTestBatch(t, router)
	rejectedJob := createHistoryTestJob(t, router, batch.ID, "00000000-0000-4000-8000-000000000032")
	uploadHistoryTestFile(t, router, rejectedJob.ID, "broken.flac", []byte("not audio"), http.StatusUnprocessableEntity)
	cancelResponse := testutil.ServeRequest(t, router, http.MethodDelete, "/api/v1/import-batches/"+batch.ID, nil, nil)
	if cancelResponse.Code != http.StatusNoContent {
		t.Fatalf("cancel rejected-only Import Batch status = %d, body = %s", cancelResponse.Code, cancelResponse.Body.String())
	}
	replayed := testutil.ServeRequest(t, router, http.MethodDelete, "/api/v1/import-batches/"+batch.ID, nil, nil)
	if replayed.Code != http.StatusNoContent {
		t.Fatalf("idempotent cancel rejected-only Import Batch status = %d, body = %s", replayed.Code, replayed.Body.String())
	}
}

func TestCanceledBatchClientFileIDCannotBeReused(t *testing.T) {
	store := managedimport.NewRepository(testutil.OpenMigratedDB(t))
	batch, err := store.CreateBatch(t.Context(), managedimport.BatchOptions{})
	if err != nil {
		t.Fatalf("create Import Batch: %v", err)
	}
	clientFileID := "00000000-0000-4000-8000-000000000040"
	job, err := store.CreateJob(t.Context(), batch.ID, clientFileID)
	if err != nil {
		t.Fatalf("create Import Job: %v", err)
	}
	if err := store.DeleteJob(t.Context(), job.ID); err != nil {
		t.Fatalf("cancel Import Job: %v", err)
	}
	if _, err := store.CreateJob(t.Context(), batch.ID, clientFileID); !errors.Is(err, managedimport.ErrInvalidState) {
		t.Fatalf("reuse canceled clientFileId error = %v", err)
	}
}

func TestPrunedFailedStandaloneHistoryIsNotRecreatedOnDeletion(t *testing.T) {
	store := managedimport.NewRepository(testutil.OpenMigratedDB(t))
	failedJob, err := store.CreateJob(t.Context(), "", "")
	if err != nil {
		t.Fatalf("create failed standalone Import Job: %v", err)
	}
	if markErr := store.MarkFailed(t.Context(), failedJob.ID, "failed.flac", "invalid_upload", "file", "upload failed"); markErr != nil {
		t.Fatalf("fail standalone Import Job: %v", markErr)
	}
	for range managedimport.IMPORT_HISTORY_LIMIT {
		batch, createErr := store.CreateBatch(t.Context(), managedimport.BatchOptions{})
		if createErr != nil {
			t.Fatalf("create newer Import Batch: %v", createErr)
		}
		if deleteErr := store.DeleteBatch(t.Context(), batch.ID); deleteErr != nil {
			t.Fatalf("cancel newer Import Batch: %v", deleteErr)
		}
	}
	if cleanupErr := store.DeleteJob(t.Context(), failedJob.ID); cleanupErr != nil {
		t.Fatalf("delete pruned failed Import Job: %v", cleanupErr)
	}
	history, err := store.ListHistory(t.Context())
	if err != nil {
		t.Fatalf("list Import History: %v", err)
	}
	if len(history.Items) != managedimport.IMPORT_HISTORY_LIMIT {
		t.Fatalf("Import History count after failed cleanup = %d", len(history.Items))
	}
	for _, item := range history.Items {
		if item.ImportID == failedJob.ID {
			t.Fatalf("pruned failed Import History %q was recreated", failedJob.ID)
		}
	}
}

func createHistoryTestBatch(t *testing.T, router http.Handler) managedimport.Batch {
	t.Helper()
	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches", nil, nil)
	if response.Code != http.StatusCreated {
		t.Fatalf("create Import Batch status = %d, body = %s", response.Code, response.Body.String())
	}
	var batch managedimport.Batch
	testutil.DecodeJSON(t, response, &batch)
	return batch
}

func createHistoryTestJob(t *testing.T, router http.Handler, batchID, fileID string) managedimport.Job {
	t.Helper()
	body := strings.NewReader(fmt.Sprintf(`{"batchId":%q,"clientFileId":%q}`, batchID, fileID))
	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", body, map[string]string{"Content-Type": "application/json"})
	if response.Code != http.StatusCreated {
		t.Fatalf("create Import Job status = %d, body = %s", response.Code, response.Body.String())
	}
	var job managedimport.Job
	testutil.DecodeJSON(t, response, &job)
	return job
}

func uploadHistoryTestFile(t *testing.T, router http.Handler, jobID, filename string, data []byte, wantStatus int) {
	t.Helper()
	response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(data), map[string]string{
		"Content-Type": "audio/flac", "X-Import-Filename": filename,
	})
	if response.Code != wantStatus {
		t.Fatalf("upload %q status = %d, want %d, body = %s", filename, response.Code, wantStatus, response.Body.String())
	}
}

func getImportHistory(t *testing.T, router http.Handler) managedimport.HistoryList {
	t.Helper()
	response := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/import-history", nil, nil)
	if response.Code != http.StatusOK {
		t.Fatalf("get Import History status = %d, body = %s", response.Code, response.Body.String())
	}
	var history managedimport.HistoryList
	testutil.DecodeJSON(t, response, &history)
	return history
}
