package managedimport_test

import (
	"bufio"
	"bytes"
	"context"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/ardam/navidrome-replacement/server/internal/config"
	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/ardam/navidrome-replacement/server/internal/modules/managedimport"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
	"github.com/go-chi/chi/v5"
)

type waitingImportInspector struct {
	started chan struct{}
	release chan struct{}
}

func (inspector waitingImportInspector) Inspect(ctx context.Context, path string, report library.InspectionProgressReporter) (library.MediaInspection, error) {
	close(inspector.started)
	select {
	case <-inspector.release:
		return library.NewMediaInspector().Inspect(ctx, path, report)
	case <-ctx.Done():
		return library.MediaInspection{}, ctx.Err()
	}
}

func TestImportReportsValidationSeparatelyFromTransfer(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	inspector := waitingImportInspector{started: make(chan struct{}), release: make(chan struct{})}
	module := managedimport.NewModule(database, config.Config{ManagedStoragePath: t.TempDir()}, inspector)
	router := chi.NewRouter()
	module.RegisterRoutes(router)
	batch := createHistoryTestBatch(t, router)
	job := createHistoryTestJob(t, router, batch.ID, "00000000-0000-4000-8000-000000000001")
	fixture := readStrictFLACFixture(t)
	finished := make(chan struct{})
	go func() {
		defer close(finished)
		request := httptest.NewRequest(http.MethodPut, "/api/v1/imports/"+job.ID+"/file", bytes.NewReader(fixture))
		request.Header.Set("X-Import-Filename", "ready.flac")
		router.ServeHTTP(httptest.NewRecorder(), request)
	}()
	defer func() { close(inspector.release); <-finished }()
	select {
	case <-inspector.started:
	case <-time.After(5 * time.Second):
		t.Fatal("inspection did not start")
	}
	response := testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/import-batches/"+batch.ID, nil, nil)
	var progress struct {
		Files []struct {
			Phase            string
			TransferredBytes int64
		}
	}
	testutil.DecodeJSON(t, response, &progress)
	if len(progress.Files) != 1 || progress.Files[0].Phase != "validating" || progress.Files[0].TransferredBytes != int64(len(fixture)) {
		t.Fatalf("validation progress = %+v", progress)
	}
}

func TestImportLivenessPreservesPreviewRevisionAndCannotReviveCanceledBatch(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	module := managedimport.NewModule(database, config.Config{ManagedStoragePath: t.TempDir()}, library.NewMediaInspector())
	router := chi.NewRouter()
	module.RegisterRoutes(router)
	batch := createHistoryTestBatch(t, router)
	path := "/api/v1/import-batches/" + batch.ID

	response := testutil.ServeRequest(t, router, http.MethodPost, path+"/heartbeat", nil, nil)
	if response.Code != http.StatusNoContent {
		t.Fatalf("heartbeat = %d: %s", response.Code, response.Body.String())
	}
	current := getImportBatch(t, router, batch.ID)
	if current.Revision != batch.Revision {
		t.Fatalf("heartbeat changed preview revision: %d to %d", batch.Revision, current.Revision)
	}
	response = testutil.ServeRequest(t, router, http.MethodDelete, path, nil, nil)
	if response.Code != http.StatusNoContent {
		t.Fatalf("cancel = %d: %s", response.Code, response.Body.String())
	}
	response = testutil.ServeRequest(t, router, http.MethodPost, path+"/heartbeat", nil, nil)
	if response.Code != http.StatusNotFound && response.Code != http.StatusConflict {
		t.Fatalf("heartbeat revived canceled batch: %d", response.Code)
	}
}

func TestImportHeartbeatKeepsReadyPreviewUntilClientLeaseExpires(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	storage := managedimport.NewStorage(t.TempDir(), managedimport.StorageLimits{FileBytes: 1024 * 1024, BatchBytes: 2 * 1024 * 1024})
	service := managedimport.NewService(managedimport.NewRepository(database), storage, library.NewMediaInspector())
	handlers := managedimport.NewHandlers(service)
	router := chi.NewRouter()
	router.Post("/api/v1/import-batches", handlers.CreateBatch)
	router.Get("/api/v1/import-batches/{batchId}", handlers.GetBatch)
	router.Post("/api/v1/import-batches/{batchId}/heartbeat", handlers.HeartbeatBatch)
	router.Post("/api/v1/imports", handlers.CreateJob)
	router.Put("/api/v1/imports/{importId}/file", handlers.UploadFile)
	batch := createHistoryTestBatch(t, router)
	job := createHistoryTestJob(t, router, batch.ID, "00000000-0000-4000-8000-000000000001")
	uploadHistoryTestFile(t, router, job.ID, "ready.flac", readStrictFLACFixture(t), http.StatusOK)
	// Seed an hour-old preview; assertions observe only the HTTP API.
	if _, err := database.Exec(`UPDATE managed_import_batches SET updated_at = ? WHERE id = ?`, time.Now().Add(-time.Hour), batch.ID); err != nil {
		t.Fatal(err)
	}
	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/import-batches/"+batch.ID+"/heartbeat", nil, nil)
	if response.Code != http.StatusNoContent {
		t.Fatalf("heartbeat = %d", response.Code)
	}
	if err := service.CleanupInactive(context.Background(), time.Now()); err != nil {
		t.Fatal(err)
	}
	current := getImportBatch(t, router, batch.ID)
	if current.Files[0].State != managedimport.BATCH_FILE_ACCEPTED {
		t.Fatalf("ready preview lost: %+v", current.Files)
	}
	if err := service.CleanupInactive(context.Background(), time.Now().Add(16*time.Minute)); err != nil {
		t.Fatal(err)
	}
	response = testutil.ServeRequest(t, router, http.MethodGet, "/api/v1/import-batches/"+batch.ID, nil, nil)
	if response.Code != http.StatusNotFound {
		t.Fatalf("expired preview still available: %d", response.Code)
	}
}

func TestImportConfirmsReadyFilesAfterInterruptedTransfer(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	module := managedimport.NewModule(database, config.Config{ManagedStoragePath: t.TempDir()}, library.NewMediaInspector())
	router := chi.NewRouter()
	module.RegisterRoutes(router)
	batch := createHistoryTestBatch(t, router)
	ready := createHistoryTestJob(t, router, batch.ID, "00000000-0000-4000-8000-000000000001")
	interrupted := createHistoryTestJob(t, router, batch.ID, "00000000-0000-4000-8000-000000000002")
	uploadHistoryTestFile(t, router, ready.ID, "ready.flac", readStrictFLACFixture(t), http.StatusOK)
	request := httptest.NewRequest(http.MethodPut, "/api/v1/imports/"+interrupted.ID+"/file", strings.NewReader("partial"))
	request.ContentLength = 100
	request.Header.Set("X-Import-Filename", "interrupted.flac")
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)
	if response.Code != http.StatusRequestTimeout {
		t.Fatalf("interrupted upload = %d", response.Code)
	}
	batch = getImportBatch(t, router, batch.ID)
	response = confirmImportBatch(t, router, batch, []string{ready.ID})
	if response.Code != http.StatusOK {
		t.Fatalf("confirm ready files = %d: %s", response.Code, response.Body.String())
	}
	testutil.DecodeJSON(t, response, &batch)
	if batch.Status != managedimport.BATCH_STATUS_COMPLETED {
		t.Fatalf("batch did not finish: %s", batch.Status)
	}
	for _, file := range batch.Files {
		if file.JobID == interrupted.ID && (file.Outcome != managedimport.OUTCOME_NOT_ATTEMPTED || file.ErrorCode != "upload_interrupted") {
			t.Fatalf("interrupted file outcome lost: %+v", file)
		}
	}
}

func TestImportTransferOutlivesServerTotalReadDeadlineWhileBytesArrive(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	module := managedimport.NewModule(database, config.Config{ManagedStoragePath: t.TempDir()}, library.NewMediaInspector())
	router := chi.NewRouter()
	module.RegisterRoutes(router)
	batch := createHistoryTestBatch(t, router)
	job := createHistoryTestJob(t, router, batch.ID, "00000000-0000-4000-8000-000000000001")
	server := httptest.NewUnstartedServer(router)
	server.Config.ReadTimeout = 20 * time.Millisecond
	server.Start()
	defer server.Close()
	reader, writer := io.Pipe()
	defer closeImportTestResource(t, reader)
	go func() {
		defer closeImportTestResource(t, writer)
		for index := 0; index < 10; index++ {
			if _, err := writer.Write([]byte("not audio")); err != nil {
				return
			}
			time.Sleep(10 * time.Millisecond)
		}
	}()
	request, err := http.NewRequest(http.MethodPut, server.URL+"/api/v1/imports/"+job.ID+"/file", reader)
	if err != nil {
		t.Fatal(err)
	}
	request.ContentLength = 90
	request.Header.Set("X-Import-Filename", "invalid.flac")
	response, err := server.Client().Do(request)
	if err != nil {
		t.Fatal(err)
	}
	defer closeImportTestResource(t, response.Body)
	if response.StatusCode != http.StatusUnprocessableEntity {
		t.Fatalf("expected complete transfer followed by validation rejection, got %d", response.StatusCode)
	}
}

func TestImportCancellationInterruptsBlockedNetworkRead(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	module := managedimport.NewModule(database, config.Config{ManagedStoragePath: t.TempDir()}, library.NewMediaInspector())
	router := chi.NewRouter()
	module.RegisterRoutes(router)
	batch := createHistoryTestBatch(t, router)
	job := createHistoryTestJob(t, router, batch.ID, "00000000-0000-4000-8000-000000000001")
	server := httptest.NewServer(router)
	defer server.Close()
	reader, writer := io.Pipe()
	defer closeImportTestResource(t, writer)
	defer closeImportTestResource(t, reader)
	request, err := http.NewRequest(http.MethodPut, server.URL+"/api/v1/imports/"+job.ID+"/file", reader)
	if err != nil {
		t.Fatal(err)
	}
	request.ContentLength = 100
	request.Header.Set("X-Import-Filename", "blocked.flac")
	finished := make(chan struct{})
	go func() {
		defer close(finished)
		response, uploadErr := server.Client().Do(request)
		if uploadErr == nil {
			closeImportTestResource(t, response.Body)
		}
	}()
	if _, writeErr := writer.Write([]byte("partial")); writeErr != nil {
		t.Fatal(writeErr)
	}
	deadline := time.Now().Add(time.Second)
	for getImportBatch(t, router, batch.ID).Files[0].TransferredBytes == 0 {
		if time.Now().After(deadline) {
			t.Fatal("upload did not start")
		}
		time.Sleep(time.Millisecond)
	}
	client := &http.Client{Timeout: 2 * time.Second}
	cancelRequest, err := http.NewRequest(http.MethodDelete, server.URL+"/api/v1/import-batches/"+batch.ID, nil)
	if err != nil {
		t.Fatal(err)
	}
	response, err := client.Do(cancelRequest)
	if err != nil {
		t.Fatalf("cancel blocked behind upload read: %v", err)
	}
	defer closeImportTestResource(t, response.Body)
	if response.StatusCode != http.StatusNoContent {
		t.Fatalf("cancel = %d", response.StatusCode)
	}
	closeImportTestResource(t, writer)
	select {
	case <-finished:
	case <-time.After(time.Second):
		t.Fatal("upload did not stop")
	}
}

func TestImportStalledTransferTimesOutAfterThirtySeconds(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	module := managedimport.NewModule(database, config.Config{ManagedStoragePath: t.TempDir()}, library.NewMediaInspector())
	router := chi.NewRouter()
	module.RegisterRoutes(router)
	batch := createHistoryTestBatch(t, router)
	job := createHistoryTestJob(t, router, batch.ID, "00000000-0000-4000-8000-000000000001")
	server := httptest.NewServer(router)
	defer server.Close()
	connection, err := net.Dial("tcp", server.Listener.Addr().String())
	if err != nil {
		t.Fatal(err)
	}
	defer closeImportTestResource(t, connection)
	if deadlineErr := connection.SetDeadline(time.Now().Add(35 * time.Second)); deadlineErr != nil {
		t.Fatal(deadlineErr)
	}
	started := time.Now()
	if _, writeErr := io.WriteString(connection, "PUT /api/v1/imports/"+job.ID+"/file HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100\r\nX-Import-Filename: stalled.flac\r\n\r\npartial"); writeErr != nil {
		t.Fatal(writeErr)
	}
	response, err := http.ReadResponse(bufio.NewReader(connection), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer closeImportTestResource(t, response.Body)
	if response.StatusCode != http.StatusRequestTimeout || time.Since(started) < 29*time.Second {
		t.Fatalf("stalled upload = %d after %s", response.StatusCode, time.Since(started))
	}
	current := getImportBatch(t, router, batch.ID)
	if current.Files[0].ErrorCode != "upload_interrupted" {
		t.Fatalf("stalled upload is not retryable: %+v", current.Files[0])
	}
}

func closeImportTestResource(t *testing.T, resource io.Closer) {
	t.Helper()
	if err := resource.Close(); err != nil {
		t.Errorf("close import test resource: %v", err)
	}
}
