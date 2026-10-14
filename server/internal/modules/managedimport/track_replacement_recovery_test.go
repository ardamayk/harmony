package managedimport

import (
	"bytes"
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/config"
	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/ardam/navidrome-replacement/server/internal/testutil"
	"github.com/go-chi/chi/v5"
)

var errInjectedReplacementCrash = errors.New("injected Track Replacement crash")

type replacementFixture struct {
	database  *sql.DB
	service   *Service
	router    http.Handler
	trackID   string
	original  []byte
	upgrade   []byte
	job       importJob
	preview   Preview
	storage   string
	audioPath string
}

func TestTrackReplacementRecoversFromCrashAtEveryPhase(t *testing.T) {
	phases := []replacementPhase{
		REPLACEMENT_PHASE_PREPARED,
		REPLACEMENT_PHASE_PLACED,
		REPLACEMENT_PHASE_VERIFIED,
		REPLACEMENT_PHASE_SWAPPED,
	}
	for _, phase := range phases {
		t.Run(string(phase), func(t *testing.T) {
			fixture := prepareReplacementFixture(t)
			fixture.service.replacementPhaseHook = crashAtReplacementPhase(phase)

			_, err := fixture.service.ConfirmReplacement(context.Background(), fixture.job.ID, replacementConfirmation(fixture.preview))
			if !errors.Is(err, errInjectedReplacementCrash) {
				t.Fatalf("confirm at phase %q error = %v", phase, err)
			}
			if recoverErr := fixture.service.RecoverPendingTrackReplacements(context.Background()); recoverErr != nil {
				t.Fatalf("recover after crash at phase %q: %v", phase, recoverErr)
			}

			assertReplacementRolledBack(t, fixture)
			fixture.service.replacementPhaseHook = nil
			result, err := fixture.service.ConfirmReplacement(context.Background(), fixture.job.ID, replacementConfirmation(fixture.preview))
			if err != nil {
				t.Fatalf("retry after recovery at phase %q: %v", phase, err)
			}
			if result.TrackID != fixture.trackID {
				t.Fatalf("retried replacement result = %+v", result)
			}
			assertReplacementCompleted(t, fixture)
		})
	}
}

func TestTrackReplacementCompletesAfterCrashFollowingDatabaseCommit(t *testing.T) {
	fixture := prepareReplacementFixture(t)
	fixture.service.replacementPhaseHook = crashAtReplacementPhase(REPLACEMENT_PHASE_DATABASE_COMMITTED)

	_, err := fixture.service.ConfirmReplacement(context.Background(), fixture.job.ID, replacementConfirmation(fixture.preview))
	if !errors.Is(err, errInjectedReplacementCrash) {
		t.Fatalf("confirm error = %v", err)
	}
	retiredPath := filepath.Join(filepath.Dir(fixture.audioPath), ".retired-"+fixture.trackID+".flac")
	if _, err := os.Stat(retiredPath); err != nil {
		t.Fatalf("previous file should still be retained until completion: %v", err)
	}
	fixture.service.replacementPhaseHook = nil
	if err := fixture.service.RecoverPendingTrackReplacements(context.Background()); err != nil {
		t.Fatalf("recover after database commit: %v", err)
	}

	assertReplacementCompleted(t, fixture)
	if _, err := os.Stat(retiredPath); !os.IsNotExist(err) {
		t.Fatalf("retired previous file should be deleted after recovery: %v", err)
	}
}

func TestTrackReplacementRollsBackFailedDatabaseCommitInProcess(t *testing.T) {
	fixture := prepareReplacementFixture(t)
	fixture.service.replacementPhaseHook = func(reached replacementPhase) error {
		if reached != REPLACEMENT_PHASE_SWAPPED {
			return nil
		}
		_, err := fixture.database.Exec(`UPDATE tracks SET revision = revision + 1 WHERE id = ?`, fixture.trackID)
		return err
	}

	_, err := fixture.service.ConfirmReplacement(context.Background(), fixture.job.ID, replacementConfirmation(fixture.preview))
	if !errors.Is(err, ErrReplacementConflict) {
		t.Fatalf("confirm error = %v, want database commit conflict", err)
	}

	if _, err := fixture.database.Exec(`UPDATE tracks SET revision = revision - 1 WHERE id = ?`, fixture.trackID); err != nil {
		t.Fatal(err)
	}
	assertReplacementRolledBack(t, fixture)
	var phase string
	if err := fixture.database.QueryRow(`SELECT phase FROM managed_track_replacements WHERE job_id = ?`, fixture.job.ID).Scan(&phase); err != nil || phase != string(REPLACEMENT_PHASE_ROLLED_BACK) {
		t.Fatalf("journal phase = %q (error = %v)", phase, err)
	}
	fixture.service.replacementPhaseHook = nil
	if _, err := fixture.service.ConfirmReplacement(context.Background(), fixture.job.ID, replacementConfirmation(fixture.preview)); err != nil {
		t.Fatalf("retry after rolled back database commit: %v", err)
	}
	assertReplacementCompleted(t, fixture)
}

func crashAtReplacementPhase(phase replacementPhase) func(replacementPhase) error {
	return func(reached replacementPhase) error {
		if reached == phase {
			return errInjectedReplacementCrash
		}
		return nil
	}
}

func replacementConfirmation(preview Preview) TrackReplacementConfirmation {
	return TrackReplacementConfirmation{Revision: preview.Revision, ConfirmationToken: preview.Replacement.ConfirmationToken}
}

func prepareReplacementFixture(t *testing.T) replacementFixture {
	t.Helper()
	database := testutil.OpenMigratedDB(t)
	storagePath := t.TempDir()
	module := newModule(database, config.Config{ManagedStoragePath: storagePath}, library.NewMediaInspector(), unlimitedStorageCapacity)
	router := chi.NewRouter()
	module.RegisterRoutes(router)
	original := readReplacementFLACFixture(t)
	trackID := importReplacementFixture(t, router, original)
	upgrade := bytes.Replace(original, []byte("GENRE=Electronic"), []byte("GENRE=Ambient   "), 1)
	job := createReplacementJobThroughRouter(t, router, trackID)
	preview := uploadReplacementThroughRouter(t, router, job.ID, upgrade)
	if preview.Replacement == nil {
		t.Fatalf("replacement preview missing: %+v", preview)
	}
	persistedJob, err := module.service.repo.GetJob(context.Background(), job.ID)
	if err != nil {
		t.Fatalf("load replacement job: %v", err)
	}
	var audioPath string
	if err := database.QueryRow(`SELECT file_path FROM track_sources WHERE track_id = ?`, trackID).Scan(&audioPath); err != nil {
		t.Fatalf("read managed file path: %v", err)
	}
	return replacementFixture{
		database: database, service: module.service, router: router, trackID: trackID, original: original,
		upgrade: upgrade, job: persistedJob, preview: preview, storage: storagePath, audioPath: audioPath,
	}
}

func assertReplacementRolledBack(t *testing.T, fixture replacementFixture) {
	t.Helper()
	stored, err := os.ReadFile(fixture.audioPath)
	if err != nil || !bytes.Equal(stored, fixture.original) {
		t.Fatalf("previous managed file is not intact after rollback (error = %v)", err)
	}
	var filePath, contentSHA256 string
	var revision int
	if scanErr := fixture.database.QueryRow(`SELECT file_path, content_sha256, revision FROM track_sources WHERE track_id = ?`, fixture.trackID).Scan(&filePath, &contentSHA256, &revision); scanErr != nil {
		t.Fatalf("read Track source after rollback: %v", scanErr)
	}
	if filePath != fixture.audioPath || contentSHA256 != sha256Hex(fixture.original) || revision != 1 {
		t.Fatalf("Track source changed after rollback: %s %s %d", filePath, contentSHA256, revision)
	}
	staged, err := os.ReadFile(fixture.job.StagedFilePath)
	if err != nil || !bytes.Equal(staged, fixture.upgrade) {
		t.Fatalf("staged replacement should be restored for retry (error = %v)", err)
	}
	job, err := fixture.service.repo.GetJob(context.Background(), fixture.job.ID)
	if err != nil || job.Status != STATUS_AWAITING_CONFIRMATION {
		t.Fatalf("job after rollback = %+v (error = %v)", job, err)
	}
	var incomplete int
	if err := fixture.database.QueryRow(`SELECT COUNT(*) FROM managed_track_replacements WHERE phase NOT IN ('completed', 'rolled_back')`).Scan(&incomplete); err != nil || incomplete != 0 {
		t.Fatalf("incomplete replacement journals = %d (error = %v)", incomplete, err)
	}
	assertNoHiddenReplacementFiles(t, fixture.storage)
}

func assertReplacementCompleted(t *testing.T, fixture replacementFixture) {
	t.Helper()
	var filePath, contentSHA256 string
	if err := fixture.database.QueryRow(`SELECT file_path, content_sha256 FROM track_sources WHERE track_id = ?`, fixture.trackID).Scan(&filePath, &contentSHA256); err != nil {
		t.Fatalf("read Track source after replacement: %v", err)
	}
	stored, err := os.ReadFile(filePath)
	if err != nil || !bytes.Equal(stored, fixture.upgrade) || contentSHA256 != sha256Hex(fixture.upgrade) {
		t.Fatalf("replacement is not authoritative after completion (error = %v)", err)
	}
	job, err := fixture.service.repo.GetJob(context.Background(), fixture.job.ID)
	if err != nil || job.Status != STATUS_COMMITTED || job.Outcome != OUTCOME_REPLACED || job.TrackID != fixture.trackID {
		t.Fatalf("job after replacement = %+v (error = %v)", job, err)
	}
	if _, err := os.Stat(fixture.job.StagedFilePath); !os.IsNotExist(err) {
		t.Fatalf("staged replacement should be consumed: %v", err)
	}
	assertNoHiddenReplacementFiles(t, fixture.storage)
}

func assertNoHiddenReplacementFiles(t *testing.T, storagePath string) {
	t.Helper()
	err := filepath.WalkDir(filepath.Join(storagePath, "library"), func(path string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if !entry.IsDir() && strings.HasPrefix(entry.Name(), ".") {
			t.Fatalf("hidden replacement artifact remains: %s", path)
		}
		return nil
	})
	if err != nil {
		t.Fatalf("walk Managed Storage: %v", err)
	}
}

func readReplacementFLACFixture(t *testing.T) []byte {
	t.Helper()
	fixture, err := os.ReadFile(filepath.Join("..", "library", "testdata", "strict-import.flac"))
	if err != nil {
		t.Fatalf("read strict FLAC fixture: %v", err)
	}
	return fixture
}

func importReplacementFixture(t *testing.T, router http.Handler, fixture []byte) string {
	t.Helper()
	jobResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports", nil, nil)
	var job Job
	testutil.DecodeJSON(t, jobResponse, &job)
	preview := uploadReplacementThroughRouter(t, router, job.ID, fixture)
	body := strings.NewReader(fmt.Sprintf(`{"revision":%d}`, preview.Revision))
	confirmResponse := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/imports/"+job.ID+"/confirm", body, map[string]string{"Content-Type": "application/json"})
	if confirmResponse.Code != http.StatusOK {
		t.Fatalf("confirm status = %d, body = %s", confirmResponse.Code, confirmResponse.Body.String())
	}
	var result Result
	testutil.DecodeJSON(t, confirmResponse, &result)
	return result.TrackID
}

func createReplacementJobThroughRouter(t *testing.T, router http.Handler, trackID string) Job {
	t.Helper()
	response := testutil.ServeRequest(t, router, http.MethodPost, "/api/v1/library/tracks/"+trackID+"/replacement", nil, nil)
	if response.Code != http.StatusCreated {
		t.Fatalf("create replacement job status = %d, body = %s", response.Code, response.Body.String())
	}
	var job Job
	testutil.DecodeJSON(t, response, &job)
	return job
}

func uploadReplacementThroughRouter(t *testing.T, router http.Handler, jobID string, fixture []byte) Preview {
	t.Helper()
	response := testutil.ServeRequest(t, router, http.MethodPut, "/api/v1/imports/"+jobID+"/file", bytes.NewReader(fixture), map[string]string{
		"Content-Type": "audio/flac", "X-Import-Filename": "upload.flac",
	})
	if response.Code != http.StatusOK {
		t.Fatalf("upload status = %d, body = %s", response.Code, response.Body.String())
	}
	var preview Preview
	if err := json.NewDecoder(response.Body).Decode(&preview); err != nil {
		t.Fatalf("decode preview: %v", err)
	}
	return preview
}

func sha256Hex(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}
