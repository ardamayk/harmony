package managedimport

import (
	"context"
	"testing"

	"github.com/ardam/navidrome-replacement/server/internal/testutil"
)

func TestMarkPreviewRollsBackJobWhenBatchRevisionFails(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	store := NewRepository(database)
	batch, err := store.CreateBatch(context.Background(), BatchOptions{})
	if err != nil {
		t.Fatalf("create batch: %v", err)
	}
	job, err := store.CreateJob(context.Background(), batch.ID, "")
	if err != nil {
		t.Fatalf("create job: %v", err)
	}
	if _, err = database.Exec(`
		CREATE TRIGGER fail_managed_import_batch_revision
		BEFORE UPDATE OF revision ON managed_import_batches
		BEGIN
			SELECT RAISE(ABORT, 'forced batch revision failure');
		END`); err != nil {
		t.Fatalf("create failure trigger: %v", err)
	}

	_, err = store.MarkPreview(context.Background(), job.ID, "track.flac", "/tmp/staged", "sha256", `{}`, 10, 100)
	if err == nil {
		t.Fatal("MarkPreview succeeded despite batch revision failure")
	}
	storedJob, err := store.GetJob(context.Background(), job.ID)
	if err != nil {
		t.Fatalf("get job: %v", err)
	}
	if storedJob.Status != STATUS_UPLOADING || storedJob.Revision != 1 {
		t.Fatalf("job after rolled-back preview = %+v", storedJob)
	}
}

func TestMarkFailedRollsBackJobWhenBatchRevisionFails(t *testing.T) {
	database := testutil.OpenMigratedDB(t)
	store := NewRepository(database)
	batch, err := store.CreateBatch(context.Background(), BatchOptions{})
	if err != nil {
		t.Fatalf("create batch: %v", err)
	}
	job, err := store.CreateJob(context.Background(), batch.ID, "")
	if err != nil {
		t.Fatalf("create job: %v", err)
	}
	if _, err = database.Exec(`
		CREATE TRIGGER fail_managed_import_batch_revision
		BEFORE UPDATE OF revision ON managed_import_batches
		BEGIN
			SELECT RAISE(ABORT, 'forced batch revision failure');
		END`); err != nil {
		t.Fatalf("create failure trigger: %v", err)
	}

	err = store.MarkFailed(context.Background(), job.ID, "track.flac", "invalid_metadata", "title", "TITLE is required")
	if err == nil {
		t.Fatal("MarkFailed succeeded despite batch revision failure")
	}
	storedJob, err := store.GetJob(context.Background(), job.ID)
	if err != nil {
		t.Fatalf("get job: %v", err)
	}
	if storedJob.Status != STATUS_UPLOADING || storedJob.ErrorCode != "" {
		t.Fatalf("job after rolled-back failure = %+v", storedJob)
	}
}
