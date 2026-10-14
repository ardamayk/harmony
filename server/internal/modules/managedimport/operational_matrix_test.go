package managedimport

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/config"
	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
	"github.com/go-chi/chi/v5"
)

// In-package matrix tests: these need the unexported capacity and phase-hook seams that the HTTP-only
// integration tests cannot reach.

func TestTrackReplacementRechecksCapacityBeforeCommitAndKeepsOldTrack(t *testing.T) {
	original := readStorageSafetyFixture(t)
	replacement := bytes.Replace(original, []byte("GENRE=Electronic"), []byte("GENRE=Ambient   "), 1)
	if len(replacement) != len(original) || bytes.Equal(replacement, original) {
		t.Fatalf("replacement fixture length = %d, want %d with different bytes", len(replacement), len(original))
	}
	const reserveBytes int64 = 1024
	availableBytes := int64(1 << 40)
	managedStoragePath := t.TempDir()
	module := newModule(testutil.OpenMigratedDB(t), config.Config{
		ManagedStoragePath:           managedStoragePath,
		ManagedStorageReserveBytes:   reserveBytes,
		ManagedImportFileLimitBytes:  int64(len(original) * 2),
		ManagedImportBatchLimitBytes: int64(len(original) * 4),
	}, library.NewMediaInspector(), func(string) (int64, error) { return availableBytes, nil })
	router := chi.NewRouter()
	module.RegisterRoutes(router)
	trackID := importStandaloneFLAC(t, router, original)
	job := createReplacementJob(t, router, trackID)
	preview := uploadFLACPreview(t, router, job.ID, replacement)
	if preview.Replacement == nil || preview.Status != STATUS_AWAITING_CONFIRMATION {
		t.Fatalf("replacement Import Preview = %+v", preview)
	}
	// Existing artwork is reused; only the replacement copy and reserve need free space.
	availableBytes = reserveBytes + int64(len(replacement)) - 1

	exhausted := confirmReplacement(router, job.ID, preview.Revision, preview.Replacement.ConfirmationToken)

	testutil.AssertErrorCode(t, exhausted, http.StatusInsufficientStorage, "insufficient_storage")
	assertSingleCanonicalAudio(t, managedStoragePath, original)
	storedJob, err := module.service.repo.GetJob(context.Background(), job.ID)
	if err != nil || storedJob.Status != STATUS_AWAITING_CONFIRMATION {
		t.Fatalf("job after exhausted commit = %+v, error = %v", storedJob, err)
	}
	if _, err := os.Stat(storedJob.StagedFilePath); err != nil {
		t.Fatalf("staged replacement should survive an exhausted commit for retry: %v", err)
	}

	availableBytes++
	committed := confirmReplacement(router, job.ID, preview.Revision, preview.Replacement.ConfirmationToken)
	if committed.Code != http.StatusOK {
		t.Fatalf("replacement confirm after capacity recovered = %d, body = %s", committed.Code, committed.Body.String())
	}
	assertSingleCanonicalAudio(t, managedStoragePath, replacement)
	assertDirectoryEmpty(t, filepath.Join(managedStoragePath, ".staging"))
}

func TestModuleStartRecoversPendingCommitJournalInOneRestartPass(t *testing.T) {
	for _, phase := range []commitPhase{
		COMMIT_PHASE_PREPARED,
		COMMIT_PHASE_PLACED,
		COMMIT_PHASE_VERIFIED,
		COMMIT_PHASE_DATABASE_COMMITTED,
		COMMIT_PHASE_CLEANED,
	} {
		t.Run(string(phase), func(t *testing.T) {
			databasePath := filepath.Join(t.TempDir(), "restart.db")
			database := openCommitRecoveryDatabase(t, databasePath)
			storageRoot := t.TempDir()
			storage := newStorage(storageRoot, StorageLimits{FileBytes: 1024, BatchBytes: 1024}, unlimitedStorageCapacity)
			service, job, inspection := prepareCommitRecoveryJob(t, database, storage)
			service.commitPhaseHook = func(reached commitPhase) error {
				if reached == phase {
					return errInjectedCommitCrash
				}
				return nil
			}
			if _, err := service.Confirm(context.Background(), job.ID, job.Revision); !errors.Is(err, errInjectedCommitCrash) {
				t.Fatalf("interrupt commit at %q: %v", phase, err)
			}
			if err := database.Close(); err != nil {
				t.Fatalf("close database before restart: %v", err)
			}

			database = openCommitRecoveryDatabase(t, databasePath)
			module := newModule(database, config.Config{ManagedStoragePath: storageRoot}, recoveryInspector{inspection: inspection}, unlimitedStorageCapacity)
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			if err := module.Start(ctx); err != nil {
				t.Fatalf("restart Managed Import module: %v", err)
			}
			if err := module.Start(ctx); err != nil {
				t.Fatalf("repeat restart Managed Import module: %v", err)
			}

			libraryStore := library.NewRepository(database)
			tracks, err := libraryStore.ListTracks(context.Background(), 100, 0, "", "")
			if err != nil {
				t.Fatalf("list Tracks after restart: %v", err)
			}
			var pendingJournals int
			if err := database.QueryRow(`SELECT COUNT(*) FROM managed_import_commit_journal WHERE phase NOT IN (?, ?)`, COMMIT_PHASE_COMPLETED, COMMIT_PHASE_ROLLED_BACK).Scan(&pendingJournals); err != nil {
				t.Fatalf("count pending journals: %v", err)
			}
			if pendingJournals != 0 {
				t.Fatalf("pending commit journals after restart = %d", pendingJournals)
			}
			storedJob, getErr := module.service.repo.GetJob(context.Background(), job.ID)
			if phase == COMMIT_PHASE_DATABASE_COMMITTED || phase == COMMIT_PHASE_CLEANED {
				if getErr != nil || storedJob.Status != STATUS_COMMITTED || tracks.Total != 1 {
					t.Fatalf("committed recovery: job = %+v (%v), tracks = %d", storedJob, getErr, tracks.Total)
				}
				track, err := libraryStore.GetTrack(context.Background(), storedJob.TrackID)
				if err != nil {
					t.Fatalf("get recovered Track: %v", err)
				}
				if contents, readErr := os.ReadFile(track.FilePath); readErr != nil || string(contents) != "managed audio" {
					t.Fatalf("recovered canonical bytes = %q, error = %v", contents, readErr)
				}
				assertDirectoryEmpty(t, filepath.Join(storageRoot, ".staging"))
				return
			}
			// Before the database commit the journal rolls back, and restart cleanup then discards the
			// uncommitted standalone job together with its staging so no pending state survives.
			if !errors.Is(getErr, ErrNotFound) || tracks.Total != 0 {
				t.Fatalf("rolled-back recovery: job = %+v (%v), tracks = %d", storedJob, getErr, tracks.Total)
			}
			assertDirectoryEmpty(t, filepath.Join(storageRoot, ".staging"))
			assertNoCanonicalFiles(t, storageRoot)
		})
	}
}

func importStandaloneFLAC(t *testing.T, router http.Handler, fixture []byte) string {
	t.Helper()
	jobID := testutil.CreateResourceID(t, router, "/api/v1/imports")
	preview := uploadFLACPreview(t, router, jobID, fixture)
	confirm := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports/"+jobID+"/confirm", strings.NewReader(fmt.Sprintf(`{"revision":%d}`, preview.Revision)), map[string]string{"Content-Type": "application/json"})
	if confirm.Code != http.StatusOK {
		t.Fatalf("confirm status = %d, body = %s", confirm.Code, confirm.Body.String())
	}
	var result Result
	testutil.DecodeJSON(t, confirm, &result)
	return result.TrackID
}

func uploadFLACPreview(t *testing.T, router http.Handler, jobID string, fixture []byte) Preview {
	t.Helper()
	response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(fixture), map[string]string{
		"Content-Type": "audio/flac", "X-Import-Filename": "matrix.flac",
	})
	if response.Code != http.StatusOK {
		t.Fatalf("upload status = %d, body = %s", response.Code, response.Body.String())
	}
	var preview Preview
	testutil.DecodeJSON(t, response, &preview)
	return preview
}

func createReplacementJob(t *testing.T, router http.Handler, trackID string) Job {
	t.Helper()
	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/library/tracks/"+trackID+"/replacement", nil, nil)
	if response.Code != http.StatusCreated {
		t.Fatalf("create Track Replacement status = %d, body = %s", response.Code, response.Body.String())
	}
	var job Job
	testutil.DecodeJSON(t, response, &job)
	return job
}

func confirmReplacement(router http.Handler, jobID string, revision int, token string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(http.MethodPost, "/api/v1/imports/"+jobID+"/replacement", strings.NewReader(fmt.Sprintf(`{"revision":%d,"confirmationToken":%q}`, revision, token)))
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set(TRACK_REPLACEMENT_CONFIRMATION_HEADER, "1")
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)
	return response
}

func assertSingleCanonicalAudio(t *testing.T, root string, expected []byte) {
	t.Helper()
	var found int
	err := filepath.WalkDir(filepath.Join(root, "library"), func(path string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if entry.IsDir() || filepath.Ext(path) != ".flac" {
			return nil
		}
		found++
		contents, readErr := os.ReadFile(path)
		if readErr != nil {
			return readErr
		}
		if !bytes.Equal(contents, expected) {
			t.Fatalf("canonical audio at %q differs from the expected bytes", path)
		}
		return nil
	})
	if err != nil {
		t.Fatalf("walk canonical library: %v", err)
	}
	if found != 1 {
		t.Fatalf("canonical audio count = %d, want 1", found)
	}
}
