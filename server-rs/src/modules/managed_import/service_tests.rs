//! Managed Import flows end to end against a migrated database and real
//! Managed Storage (managed_import_integration_test.go,
//! commit_recovery_test.go, import_history_integration_test.go).

use super::errors::{Error, Sentinel};
use super::model::*;
use super::repository::Repository;
use super::service::{Hook, Service};
use super::storage::{Limits, Storage};
use bytes::Bytes;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const FLAC: &[u8] = include_bytes!("../../../testdata/strict-import.flac");

struct Harness {
    _dir: tempfile::TempDir,
    pool: SqlitePool,
    root: PathBuf,
    service: Service,
}

async fn harness_with(phase: Option<Hook>, result: Option<Hook>) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../server/migrations");
    let pool =
        crate::db::open_and_migrate(dir.path().join("app.db").to_str().unwrap(), &migrations)
            .await
            .unwrap();
    let root = dir.path().join("managed");
    let storage = Storage::with_capacity(
        root.to_str().unwrap(),
        Limits {
            reserve_bytes: 0,
            file_bytes: 64 << 20,
            batch_bytes: 256 << 20,
        },
        Arc::new(|_: &Path| Ok(i64::MAX / 2)),
    );
    let service =
        Service::new(Repository::new(pool.clone()), Arc::new(storage)).with_hooks(phase, result);
    Harness {
        _dir: dir,
        pool,
        root,
        service,
    }
}

async fn harness() -> Harness {
    harness_with(None, None).await
}

fn body(
    data: &[u8],
) -> impl futures_util::Stream<Item = super::errors::Result<Bytes>> + Unpin + Send {
    let chunks: Vec<super::errors::Result<Bytes>> = data
        .chunks(4096)
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect();
    futures_util::stream::iter(chunks)
}

impl Harness {
    async fn upload(
        &self,
        job_id: &str,
        name: &str,
        data: &[u8],
    ) -> super::errors::Result<Preview> {
        self.service
            .upload(
                &CancellationToken::new(),
                job_id,
                name,
                body(data),
                data.len() as i64,
            )
            .await
    }

    async fn count(&self, sql: &'static str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(&self.pool).await.unwrap()
    }

    fn staged_files(&self) -> usize {
        std::fs::read_dir(self.root.join(".staging")).map_or(0, |entries| entries.count())
    }

    fn library_files(&self) -> Vec<PathBuf> {
        fn walk(path: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
                if entry.file_type().unwrap().is_dir() {
                    walk(&entry.path(), out);
                } else {
                    out.push(entry.path());
                }
            }
        }
        let mut files = Vec::new();
        walk(&self.root.join("library"), &mut files);
        files.sort();
        files
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn imports_a_standalone_file_and_records_history() {
    let h = harness().await;
    let job = h.service.create_job("", "").await.unwrap();
    assert_eq!((job.status.as_str(), job.revision), (STATUS_UPLOADING, 1));
    let preview = h.upload(&job.id, "strict-import.flac", FLAC).await.unwrap();
    assert_eq!(preview.status, STATUS_AWAITING_CONFIRMATION);
    assert_eq!(preview.revision, 2);
    assert_eq!(preview.file.format, "flac");
    assert_eq!(preview.file.size_bytes, FLAC.len() as i64);
    assert_eq!(preview.duplicate_classification, DUPLICATE_NONE);
    assert_eq!(
        h.service
            .get_job(&job.id)
            .await
            .unwrap()
            .validation_progress,
        100
    );
    let result = h
        .service
        .confirm_with_decision(&CancellationToken::new(), &job.id, 2, "")
        .await
        .unwrap();
    assert_eq!(
        (result.status.as_str(), result.revision),
        (STATUS_COMMITTED, 3)
    );
    assert_eq!(h.count("SELECT COUNT(*) FROM visible_tracks").await, 1);
    assert_eq!(
        h.count("SELECT COUNT(*) FROM managed_import_commit_journal WHERE phase = 'completed'")
            .await,
        1
    );
    assert_eq!(h.staged_files(), 0);
    let files = h.library_files();
    assert_eq!(files.len(), 2, "{files:?}");
    assert!(
        files
            .iter()
            .any(|file| file.extension().is_some_and(|ext| ext == "flac"))
    );
    // Confirming again with the previous revision returns the same result.
    let again = h
        .service
        .confirm_with_decision(&CancellationToken::new(), &job.id, 2, "")
        .await
        .unwrap();
    assert_eq!(again.track_id, result.track_id);
    let history = h.service.list_history().await.unwrap();
    assert_eq!(history.items.len(), 1);
    let item = &history.items[0];
    assert_eq!(
        (item.result_code.as_str(), item.counts.imported),
        (HISTORY_RESULT_COMPLETED, 1)
    );
    assert_eq!(item.files[0].created_track_id, result.track_id);
    assert!(item.started_at.ends_with('Z'), "{}", item.started_at);
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_exact_duplicates_and_stale_revisions() {
    let h = harness().await;
    let first = h.service.create_job("", "").await.unwrap();
    h.upload(&first.id, "a.flac", FLAC).await.unwrap();
    let stale = h
        .service
        .confirm_with_decision(&CancellationToken::new(), &first.id, 1, "")
        .await
        .unwrap_err();
    assert!(stale.is(Sentinel::RevisionConflict));
    h.service
        .confirm_with_decision(&CancellationToken::new(), &first.id, 2, "")
        .await
        .unwrap();
    let second = h.service.create_job("", "").await.unwrap();
    let preview = h.upload(&second.id, "b.flac", FLAC).await.unwrap();
    assert_eq!(preview.status, STATUS_FAILED);
    assert_eq!(preview.duplicate_classification, DUPLICATE_EXACT);
    let job = h.service.get_job(&second.id).await.unwrap();
    assert_eq!(job.error_code, ERROR_CODE_EXACT_DUPLICATE);
    let error = h
        .service
        .confirm_with_decision(&CancellationToken::new(), &second.id, 2, "")
        .await
        .unwrap_err();
    assert!(error.is(Sentinel::ExactDuplicate));
    assert_eq!(h.staged_files(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn records_strict_profile_failures_with_their_issues() {
    let h = harness().await;
    let job = h.service.create_job("", "").await.unwrap();
    let error = h
        .upload(&job.id, "noise.flac", b"not audio at all")
        .await
        .unwrap_err();
    let validation = error.as_validation().unwrap();
    assert_eq!(
        (validation.code.as_str(), validation.field.as_str()),
        ("unsupported_format", "container")
    );
    let issues: String =
        sqlx::query_scalar("SELECT validation_issues FROM managed_import_jobs WHERE id = ?")
            .bind(&job.id)
            .fetch_one(&h.pool)
            .await
            .unwrap();
    assert_eq!(
        issues,
        r#"[{"code":"unsupported_format","field":"container","reason":"file container or audio codec is not supported"}]"#
    );
    let invalid_name = h.service.create_job("", "").await.unwrap();
    let error = h
        .upload(&invalid_name.id, "../x.flac", FLAC)
        .await
        .unwrap_err();
    assert!(error.is(Sentinel::InvalidUpload));
    assert_eq!(
        error.message(),
        "managed import upload is invalid: filename must not contain path segments"
    );
    assert_eq!(h.staged_files(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn confirms_a_batch_with_album_previews() {
    let h = harness().await;
    let batch = h.service.create_batch().await.unwrap();
    let client_file_id = uuid::Uuid::new_v4().to_string();
    let invalid = h
        .service
        .create_job(&batch.id, "not-a-uuid")
        .await
        .unwrap_err();
    assert!(invalid.is(Sentinel::InvalidUpload));
    let job = h
        .service
        .create_job(&batch.id, &client_file_id)
        .await
        .unwrap();
    h.upload(&job.id, "strict-import.flac", FLAC).await.unwrap();
    let current = h.service.get_batch(&batch.id).await.unwrap();
    assert_eq!(current.files.len(), 1);
    assert_eq!(current.files[0].state, BATCH_FILE_ACCEPTED);
    assert_eq!(current.files[0].phase, "ready");
    assert_eq!(current.albums.len(), 1);
    assert!(current.albums[0].existing_albums.is_empty());
    let confirmation = BatchConfirmation {
        revision: current.revision,
        selected_file_ids: Some(vec![job.id.clone()]),
        ..BatchConfirmation::default()
    };
    let completed = h
        .service
        .confirm_batch(&CancellationToken::new(), &batch.id, &confirmation)
        .await
        .unwrap();
    assert_eq!(completed.status, BATCH_STATUS_COMPLETED);
    assert_eq!(completed.files[0].outcome, OUTCOME_IMPORTED);
    assert_eq!(completed.revision, current.revision + 2);
    // A retry of the same confirmation sees the completed Batch.
    let retried = h
        .service
        .confirm_batch(&CancellationToken::new(), &batch.id, &confirmation)
        .await
        .unwrap();
    assert_eq!(retried.status, BATCH_STATUS_COMPLETED);
    let history = h.service.list_history().await.unwrap();
    assert_eq!(history.items[0].import_id, batch.id);
    assert_eq!(history.items[0].files[0].file_id, client_file_id);
}

#[tokio::test(flavor = "multi_thread")]
async fn canceling_a_batch_removes_staging_and_archives_it() {
    let h = harness().await;
    let batch = h.service.create_batch().await.unwrap();
    let job = h
        .service
        .create_job(&batch.id, &uuid::Uuid::new_v4().to_string())
        .await
        .unwrap();
    h.upload(&job.id, "a.flac", FLAC).await.unwrap();
    assert_eq!(h.staged_files(), 1);
    h.service.cancel_batch(&batch.id).await.unwrap();
    assert_eq!(h.staged_files(), 0);
    assert!(
        h.service
            .get_batch(&batch.id)
            .await
            .unwrap_err()
            .is(Sentinel::NotFound)
    );
    let history = h.service.list_history().await.unwrap();
    assert_eq!(history.items[0].result_code, HISTORY_RESULT_CANCELED);
    assert_eq!(history.items[0].counts.canceled, 1);
    h.service.cancel_batch(&batch.id).await.unwrap();
}

fn stop_at(phase: &'static str) -> Hook {
    Arc::new(move |current: &str| {
        if current == phase {
            Err(Error::new(format!("simulated crash after {phase}")))
        } else {
            Ok(())
        }
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn recovers_a_commit_from_every_durable_phase() {
    let phases = [
        (COMMIT_PHASE_PREPARED, false),
        (COMMIT_PHASE_PLACED, false),
        (COMMIT_PHASE_VERIFIED, false),
        (COMMIT_PHASE_DATABASE_COMMITTED, true),
        (COMMIT_PHASE_CLEANED, true),
    ];
    for (phase, completes) in phases {
        let crashed = harness_with(Some(stop_at(phase)), None).await;
        let job = crashed.service.create_job("", "").await.unwrap();
        crashed.upload(&job.id, "a.flac", FLAC).await.unwrap();
        let error = crashed
            .service
            .confirm_with_decision(&CancellationToken::new(), &job.id, 2, "")
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("simulated crash"),
            "{phase}: {error}"
        );
        assert_eq!(
            crashed.count("SELECT COUNT(*) FROM visible_tracks").await,
            0,
            "{phase}"
        );
        // A restarted server finishes or undoes the commit.
        let restarted = Service::new(
            Repository::new(crashed.pool.clone()),
            crashed.service.storage.clone(),
        );
        restarted.cleanup_restart().await.unwrap();
        let visible = crashed.count("SELECT COUNT(*) FROM visible_tracks").await;
        let journal: String = sqlx::query_scalar("SELECT phase FROM managed_import_commit_journal")
            .fetch_one(&crashed.pool)
            .await
            .unwrap();
        if completes {
            assert_eq!((visible, journal.as_str()), (1, "completed"), "{phase}");
            assert_eq!(crashed.library_files().len(), 2, "{phase}");
        } else {
            assert_eq!((visible, journal.as_str()), (0, "rolled_back"), "{phase}");
            assert!(
                crashed.library_files().is_empty(),
                "{phase}: {:?}",
                crashed.library_files()
            );
            // Restart cleanup cancels the uncommitted job and its staging.
            assert_eq!(crashed.staged_files(), 0, "{phase}");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn rolls_back_when_the_database_commit_fails() {
    let failing: Hook = Arc::new(|_: &str| Err(Error::new("database commit result lost")));
    let h = harness_with(None, Some(failing)).await;
    let job = h.service.create_job("", "").await.unwrap();
    h.upload(&job.id, "a.flac", FLAC).await.unwrap();
    let error = h
        .service
        .confirm_with_decision(&CancellationToken::new(), &job.id, 2, "")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("result lost"));
    // The database did commit; the next confirmation finishes it.
    let restarted = Service::new(Repository::new(h.pool.clone()), h.service.storage.clone());
    let result = restarted
        .confirm_with_decision(&CancellationToken::new(), &job.id, 2, "")
        .await
        .unwrap();
    assert_eq!(result.status, STATUS_COMMITTED);
    assert_eq!(h.count("SELECT COUNT(*) FROM visible_tracks").await, 1);
}
