//! Managed Import Jobs and Batches (managedimport/repository.go,
//! repository_jobs.go, queries/jobs.sql). Multi-step transitions run in one
//! write transaction.

use super::errors::{Error, Issue, Result, Sentinel};
use super::history;
use super::model::*;
use crate::db;
use sqlx::{SqliteConnection, SqlitePool};
use std::collections::HashMap;

#[derive(Clone)]
pub struct Repository {
    pub(super) pool: SqlitePool,
}

/// `requireOne`: a guarded UPDATE or DELETE that matched no row.
pub(super) fn require_one(affected: u64) -> Result<()> {
    if affected == 1 {
        Ok(())
    } else {
        Err(Error::sentinel(Sentinel::InvalidState))
    }
}

pub(super) fn context(operation: &str) -> impl Fn(sqlx::Error) -> Error + '_ {
    move |error| Error::from(error).context(operation)
}

pub(super) fn encode_issues(issues: &[Issue]) -> String {
    crate::gojson::to_string(issues)
}

pub(super) fn decode_issues(stored: &str) -> Result<Vec<Issue>> {
    serde_json::from_str(stored)
        .map_err(|error| Error::new(format!("decode import validation issues: {error}")))
}

pub(super) fn now() -> String {
    crate::gojson::sqlite_time(std::time::SystemTime::now())
}

pub(super) async fn get_import_job(conn: &mut SqliteConnection, job_id: &str) -> Result<ImportJob> {
    let row = sqlx::query!(
        r#"SELECT id AS "id!", status, revision, validation_progress, batch_id, client_file_id,
            original_filename, staged_file_path, content_sha256, error_code, track_id, preview_json,
            error_field, error_reason, outcome, selected, replace_track_id, validation_issues,
            import_plan_json
        FROM managed_import_jobs WHERE id = ?"#,
        job_id
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|error| Error::from(error).context(format!("get Managed Import Job {job_id:?}")))?;
    let Some(row) = row else {
        return Err(Error::sentinel(Sentinel::NotFound));
    };
    let issues = decode_issues(&row.validation_issues)
        .map_err(|error| error.context(format!("get Managed Import Job {job_id:?}")))?;
    Ok(ImportJob {
        job: Job {
            id: row.id,
            status: row.status,
            revision: row.revision,
            validation_progress: row.validation_progress,
            error_code: row.error_code.unwrap_or_default(),
            track_id: row.track_id.unwrap_or_default(),
            replaces_track_id: row.replace_track_id.clone().unwrap_or_default(),
        },
        import_plan_json: row.import_plan_json.unwrap_or_default(),
        issues,
        batch_id: row.batch_id.unwrap_or_default(),
        client_file_id: row.client_file_id.unwrap_or_default(),
        original_filename: row.original_filename.unwrap_or_default(),
        staged_file_path: row.staged_file_path.unwrap_or_default(),
        content_sha256: row.content_sha256.unwrap_or_default(),
        preview_json: row.preview_json.unwrap_or_default(),
        error_field: row.error_field.unwrap_or_default(),
        error_reason: row.error_reason.unwrap_or_default(),
        outcome: row.outcome.unwrap_or_default(),
        selected: row.selected != 0,
        replace_track_id: row.replace_track_id.unwrap_or_default(),
    })
}

/// `batchFileFromJob`.
pub(super) fn batch_file_from_job(job: &ImportJob) -> Result<BatchFile> {
    let mut file = BatchFile {
        job_id: job.job.id.clone(),
        client_file_id: job.client_file_id.clone(),
        state: BATCH_FILE_UNRESOLVED.to_owned(),
        status: job.job.status.clone(),
        revision: job.job.revision,
        validation_progress: job.job.validation_progress,
        original_filename: job.original_filename.clone(),
        selected: job.selected,
        error_code: job.job.error_code.clone(),
        error_field: job.error_field.clone(),
        error_reason: job.error_reason.clone(),
        issues: job.issues.clone(),
        outcome: job.outcome.clone(),
        track_id: job.job.track_id.clone(),
        ..BatchFile::default()
    };
    if !job.preview_json.is_empty() {
        let preview: Preview = serde_json::from_str(&job.preview_json).map_err(|error| {
            Error::new(format!(
                "decode Import Preview for job {:?}: {error}",
                job.job.id
            ))
        })?;
        file.preview = Some(preview);
    }
    file.state = if job.outcome == OUTCOME_REJECTED {
        BATCH_FILE_REJECTED
    } else if !job.outcome.is_empty() {
        BATCH_FILE_COMPLETED
    } else if job.job.status == STATUS_AWAITING_CONFIRMATION {
        BATCH_FILE_ACCEPTED
    } else if job.job.status == STATUS_FAILED {
        BATCH_FILE_REJECTED
    } else {
        BATCH_FILE_UNRESOLVED
    }
    .to_owned();
    Ok(file)
}

impl Repository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub(super) async fn conn(&self) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>> {
        self.pool.acquire().await.map_err(Error::from)
    }

    pub(super) async fn begin(
        &self,
        operation: &str,
    ) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>> {
        db::begin_write(&self.pool)
            .await
            .map_err(|error| Error::from(error).context(format!("begin {operation}")))
    }

    pub(super) async fn commit(
        tx: sqlx::Transaction<'static, sqlx::Sqlite>,
        operation: &str,
    ) -> Result<()> {
        tx.commit()
            .await
            .map_err(|error| Error::from(error).context(format!("commit {operation}")))
    }

    pub async fn create_job(&self, batch_id: &str, client_file_id: &str) -> Result<Job> {
        let job = Job {
            id: uuid::Uuid::new_v4().to_string(),
            status: STATUS_UPLOADING.to_owned(),
            revision: 1,
            ..Job::default()
        };
        if batch_id.is_empty() {
            sqlx::query!(
                "INSERT INTO managed_import_jobs (id, status, revision, client_file_id)
                VALUES (?, 'uploading', 1, NULLIF(CAST(? AS TEXT), ''))",
                job.id,
                client_file_id
            )
            .execute(&self.pool)
            .await
            .map_err(context("create Managed Import Job"))?;
            return Ok(job);
        }
        let operation = "Managed Import Batch file creation";
        let mut tx = self.begin(operation).await?;
        let inserted = sqlx::query!(
            "INSERT INTO managed_import_jobs (id, status, revision, batch_id, batch_position, client_file_id)
            SELECT ?1, 'uploading', 1, batches.id, (
                SELECT COALESCE(MAX(position), 0) + 1 FROM (
                    SELECT jobs.batch_position AS position FROM managed_import_jobs jobs WHERE jobs.batch_id = ?2
                    UNION ALL
                    SELECT canceled.position FROM managed_import_canceled_files canceled WHERE canceled.batch_id = ?2
                )
            ), NULLIF(CAST(?3 AS TEXT), '')
            FROM managed_import_batches batches
            WHERE batches.id = ?2 AND batches.status = 'uploading' AND NOT EXISTS (
                SELECT 1 FROM managed_import_canceled_files canceled
                WHERE canceled.batch_id = ?2 AND canceled.file_id = NULLIF(CAST(?3 AS TEXT), '')
            )",
            job.id,
            batch_id,
            client_file_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("create Managed Import Job"))?
        .rows_affected();
        require_one(inserted)?;
        revise_batch(&mut tx, batch_id).await?;
        Self::commit(tx, operation).await?;
        Ok(job)
    }

    /// A batch owned by a Playback Client, or by the server (`server`).
    pub async fn create_batch(&self, server_owned: bool) -> Result<Batch> {
        let batch = Batch {
            id: uuid::Uuid::new_v4().to_string(),
            status: BATCH_STATUS_UPLOADING.to_owned(),
            revision: 1,
            ..Batch::default()
        };
        let owner = if server_owned { "server" } else { "client" };
        sqlx::query!(
            "INSERT INTO managed_import_batches (id, status, revision, owner) VALUES (?, 'uploading', 1, ?)",
            batch.id,
            owner
        )
        .execute(&self.pool)
        .await
        .map_err(context("create Managed Import Batch"))?;
        Ok(batch)
    }

    pub async fn get_job(&self, job_id: &str) -> Result<ImportJob> {
        get_import_job(&mut *self.conn().await?, job_id).await
    }

    pub async fn get_batch(&self, batch_id: &str) -> Result<Batch> {
        let row = sqlx::query!(
            r#"SELECT id AS "id!", status, revision FROM managed_import_batches WHERE id = ?"#,
            batch_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| {
            Error::from(error).context(format!("get Managed Import Batch {batch_id:?}"))
        })?
        .ok_or_else(|| Error::sentinel(Sentinel::NotFound))?;
        let jobs = self.list_batch_jobs(batch_id).await?;
        let files = jobs
            .iter()
            .map(batch_file_from_job)
            .collect::<Result<Vec<_>>>()?;
        Ok(Batch {
            albums: Vec::new(),
            id: row.id,
            status: row.status,
            revision: row.revision,
            files,
        })
    }

    pub async fn get_batch_status(&self, batch_id: &str) -> Result<String> {
        sqlx::query_scalar!(
            "SELECT status FROM managed_import_batches WHERE id = ?",
            batch_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| {
            Error::from(error).context(format!("get Managed Import Batch {batch_id:?} status"))
        })?
        .ok_or_else(|| Error::sentinel(Sentinel::NotFound))
    }

    /// The IDs are read in full before each file loads, so no read cursor
    /// stays open while a writer waits.
    pub async fn list_batch_jobs(&self, batch_id: &str) -> Result<Vec<ImportJob>> {
        let ids = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM managed_import_jobs WHERE batch_id = ? ORDER BY batch_position"#,
            batch_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(context("list Managed Import Batch files"))?;
        let mut jobs = Vec::with_capacity(ids.len());
        for id in ids {
            jobs.push(self.get_job(&id).await?);
        }
        Ok(jobs)
    }

    pub async fn heartbeat_batch(&self, batch_id: &str) -> Result<()> {
        let refreshed = sqlx::query!(
            "UPDATE managed_import_batches SET updated_at = CURRENT_TIMESTAMP WHERE id = ? AND status <> 'completed'",
            batch_id
        )
        .execute(&self.pool)
        .await
        .map_err(|error| {
            Error::from(error).context(format!("refresh Managed Import Batch {batch_id:?} liveness"))
        })?
        .rows_affected();
        if refreshed == 0 {
            return Err(Error::sentinel(Sentinel::NotFound));
        }
        Ok(())
    }

    pub async fn delete_batch(&self, batch_id: &str) -> Result<()> {
        let operation = "Managed Import Batch cancellation";
        let mut tx = self.begin(operation).await?;
        history::archive_batch(&mut tx, batch_id, HISTORY_RESULT_CANCELED).await?;
        sqlx::query!(
            "DELETE FROM managed_import_jobs WHERE batch_id = ?",
            batch_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("delete canceled Managed Import Batch files"))?;
        let deleted = sqlx::query!(
            "DELETE FROM managed_import_batches WHERE id = ? AND status != 'completed'",
            batch_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("delete canceled Managed Import Batch"))?
        .rows_affected();
        require_one(deleted)?;
        Self::commit(tx, operation).await
    }

    pub async fn delete_job(&self, job_id: &str) -> Result<()> {
        let operation = "Managed Import Job cancellation";
        let mut tx = self.begin(operation).await?;
        let job = sqlx::query!(
            "SELECT jobs.batch_id, jobs.status
            FROM managed_import_jobs jobs
            LEFT JOIN managed_import_batches batches ON batches.id = jobs.batch_id
            WHERE jobs.id = ? AND jobs.status != 'committed' AND (jobs.batch_id IS NULL OR batches.status != 'completed')",
            job_id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(context("resolve canceled Managed Import Job"))?
        .ok_or_else(|| Error::sentinel(Sentinel::InvalidState))?;
        let now = now();
        if job.batch_id.is_none() && job.status != STATUS_FAILED {
            sqlx::query!(
                "UPDATE managed_import_jobs SET updated_at = ? WHERE id = ?",
                now,
                job_id
            )
            .execute(&mut *tx)
            .await
            .map_err(context("timestamp canceled standalone Managed Import Job"))?;
            history::archive_standalone(&mut tx, job_id, HISTORY_RESULT_CANCELED).await?;
        }
        if let Some(batch_id) = &job.batch_id {
            sqlx::query!(
                "INSERT INTO managed_import_canceled_files (
                    batch_id, file_id, job_id, safe_filename, started_at, completed_at, content_sha256, position
                )
                SELECT batch_id, COALESCE(NULLIF(client_file_id, ''), id), id, original_filename,
                    created_at, ?1, content_sha256, batch_position
                FROM managed_import_jobs WHERE managed_import_jobs.id = ?2",
                now,
                job_id
            )
            .execute(&mut *tx)
            .await
            .map_err(context("retain canceled Managed Import Batch file"))?;
            revise_batch(&mut tx, batch_id)
                .await
                .map_err(|error| error.context("revise canceled Managed Import Batch file"))?;
        }
        sqlx::query!("DELETE FROM managed_import_jobs WHERE id = ?", job_id)
            .execute(&mut *tx)
            .await
            .map_err(context("delete canceled Managed Import Job"))?;
        Self::commit(tx, operation).await
    }

    /// Uncommitted client batches; server-owned batches live until confirmed
    /// or cancelled.
    pub async fn list_uncommitted_batch_ids(
        &self,
        updated_before: Option<&str>,
    ) -> Result<Vec<String>> {
        let ids = match updated_before {
            Some(before) => {
                sqlx::query_scalar!(
                    r#"SELECT id AS "id!" FROM managed_import_batches
                WHERE status != 'completed' AND owner = 'client' AND updated_at <= ?"#,
                    before
                )
                .fetch_all(&self.pool)
                .await
            }
            None => {
                sqlx::query_scalar!(
                    r#"SELECT id AS "id!" FROM managed_import_batches
                WHERE status != 'completed' AND owner = 'client'"#
                )
                .fetch_all(&self.pool)
                .await
            }
        };
        ids.map_err(Error::from)
    }

    /// The staged files of server-owned batches, which a restart keeps.
    pub async fn list_server_owned_staged_paths(&self) -> Result<Vec<String>> {
        sqlx::query_scalar!(
            r#"SELECT jobs.staged_file_path AS "path!" FROM managed_import_jobs jobs
            JOIN managed_import_batches batches ON batches.id = jobs.batch_id
            WHERE batches.owner = 'server' AND batches.status != 'completed'
                AND jobs.staged_file_path IS NOT NULL AND jobs.staged_file_path != ''"#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(context("list server-owned Managed Import staging"))
    }

    pub async fn list_uncommitted_standalone_job_ids(
        &self,
        updated_before: Option<&str>,
    ) -> Result<Vec<String>> {
        let ids = match updated_before {
            Some(before) => sqlx::query_scalar!(
                r#"SELECT id AS "id!" FROM managed_import_jobs
                WHERE batch_id IS NULL AND status != 'committed' AND updated_at <= ?"#,
                before
            )
            .fetch_all(&self.pool)
            .await,
            None => sqlx::query_scalar!(
                r#"SELECT id AS "id!" FROM managed_import_jobs WHERE batch_id IS NULL AND status != 'committed'"#
            )
            .fetch_all(&self.pool)
            .await,
        };
        ids.map_err(Error::from)
    }

    pub async fn is_batch_uncommitted_before(&self, batch_id: &str, before: &str) -> Result<bool> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS(
                SELECT 1 FROM managed_import_batches WHERE id = ? AND status != 'completed' AND updated_at <= ?
            ) AS "is_eligible!: bool""#,
            batch_id,
            before
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|error| Error::from(error).context(format!("check inactive Managed Import Batch {batch_id:?}")))
    }

    pub async fn is_standalone_job_uncommitted_before(
        &self,
        job_id: &str,
        before: &str,
    ) -> Result<bool> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS(
                SELECT 1 FROM managed_import_jobs
                WHERE id = ? AND batch_id IS NULL AND status != 'committed' AND updated_at <= ?
            ) AS "is_eligible!: bool""#,
            job_id,
            before
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|error| {
            Error::from(error).context(format!("check inactive Managed Import Job {job_id:?}"))
        })
    }

    pub async fn update_validation_progress(&self, job_id: &str, progress: i64) -> Result<()> {
        if !(0..=100).contains(&progress) {
            return Err(Error::new(format!(
                "managed import validation progress must be between 0 and 100: got {progress}"
            )));
        }
        let updated = sqlx::query!(
            "UPDATE managed_import_jobs
            SET validation_progress = ?1, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?2 AND status = 'uploading' AND validation_progress < ?1",
            progress,
            job_id
        )
        .execute(&self.pool)
        .await
        .map_err(context("update Managed Import validation progress"))?
        .rows_affected();
        if updated == 0 && self.get_job(job_id).await?.status() != STATUS_UPLOADING {
            return Err(Error::sentinel(Sentinel::InvalidState));
        }
        Ok(())
    }

    pub async fn reserve_batch_upload(
        &self,
        job_id: &str,
        upload_size: i64,
        batch_limit: i64,
    ) -> Result<()> {
        let reserved = sqlx::query!(
            "UPDATE managed_import_jobs
            SET upload_size_bytes = ?1, error_code = NULL, error_field = NULL, error_reason = NULL,
                updated_at = CURRENT_TIMESTAMP
            WHERE managed_import_jobs.id = ?2 AND managed_import_jobs.status = 'uploading'
                AND managed_import_jobs.batch_id IS NOT NULL AND ?1 + COALESCE((
                    SELECT SUM(sibling.upload_size_bytes) FROM managed_import_jobs AS sibling
                    WHERE sibling.batch_id = managed_import_jobs.batch_id AND sibling.id != managed_import_jobs.id
                ), 0) <= ?3",
            upload_size,
            job_id,
            batch_limit
        )
        .execute(&self.pool)
        .await
        .map_err(context("reserve Managed Import Batch upload bytes"))?
        .rows_affected();
        if reserved == 1 {
            return Ok(());
        }
        let job = self.get_job(job_id).await?;
        if job.status() != STATUS_UPLOADING || job.batch_id.is_empty() {
            return Err(Error::sentinel(Sentinel::InvalidState));
        }
        Err(Error::sentinel(Sentinel::BatchTooLarge))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn mark_preview(
        &self,
        job_id: &str,
        original_filename: &str,
        staged_file_path: &str,
        content_sha256: &str,
        preview_json: &str,
        upload_size: i64,
        batch_limit: i64,
    ) -> Result<ImportJob> {
        let operation = "Import Preview transition";
        let mut tx = self.begin(operation).await?;
        let marked = sqlx::query!(
            "UPDATE managed_import_jobs
            SET status = 'awaiting_confirmation', revision = revision + 1, original_filename = ?1,
                staged_file_path = ?2, content_sha256 = ?3, preview_json = ?4, upload_size_bytes = ?5,
                error_code = NULL, error_field = NULL, error_reason = NULL, validation_issues = '[]',
                outcome = NULL, selected = 1, validation_progress = 100, updated_at = CURRENT_TIMESTAMP
            WHERE managed_import_jobs.id = ?6 AND managed_import_jobs.status = 'uploading' AND (
                managed_import_jobs.batch_id IS NULL OR ?5 + COALESCE((
                    SELECT SUM(sibling.upload_size_bytes) FROM managed_import_jobs AS sibling
                    WHERE sibling.batch_id = managed_import_jobs.batch_id AND sibling.id != managed_import_jobs.id
                ), 0) <= ?7
            )",
            original_filename,
            staged_file_path,
            content_sha256,
            preview_json,
            upload_size,
            job_id,
            batch_limit
        )
        .execute(&mut *tx)
        .await
        .map_err(context("mark Import Preview ready"))?
        .rows_affected();
        if marked != 1 {
            let job = get_import_job(&mut tx, job_id).await?;
            if job.status() == STATUS_UPLOADING && !job.batch_id.is_empty() {
                return Err(Error::sentinel(Sentinel::BatchTooLarge));
            }
            return Err(Error::sentinel(Sentinel::InvalidState));
        }
        revise_job_batch(&mut tx, job_id)
            .await
            .map_err(|error| error.context("revise Managed Import Batch preview"))?;
        Self::commit(tx, operation).await?;
        self.get_job(job_id).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn mark_exact_duplicate(
        &self,
        job_id: &str,
        original_filename: &str,
        staged_path: &str,
        content_sha256: &str,
        preview_json: &str,
        track_id: &str,
        upload_size: i64,
    ) -> Result<()> {
        let operation = "Exact Duplicate transition";
        let mut tx = self.begin(operation).await?;
        let marked = sqlx::query!(
            "UPDATE managed_import_jobs
            SET status = 'failed', revision = revision + 1, original_filename = ?1,
                content_sha256 = ?2, track_id = ?3, preview_json = ?4,
                error_code = 'exact_duplicate', error_field = 'file', error_reason = 'file bytes match an existing Track',
                outcome = CASE WHEN batch_id IS NULL THEN NULL ELSE 'rejected' END, selected = 0,
                staged_file_path = ?5, upload_size_bytes = ?6,
                validation_progress = 100, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?7 AND status = 'uploading'",
            original_filename,
            content_sha256,
            track_id,
            preview_json,
            staged_path,
            upload_size,
            job_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("mark Exact Duplicate"))?
        .rows_affected();
        require_one(marked).map_err(|error| error.context("mark Exact Duplicate"))?;
        revise_uploading_job_batch(&mut tx, job_id)
            .await
            .map_err(|error| error.context("revise Exact Duplicate Batch file"))?;
        Self::commit(tx, operation).await
    }

    pub async fn clear_staged_file(&self, job_id: &str) -> Result<()> {
        let operation = "Managed Import staging cleanup";
        let mut tx = self.begin(operation).await?;
        let cleared = sqlx::query!(
            "UPDATE managed_import_jobs SET staged_file_path = NULL, upload_size_bytes = 0,
                status = CASE WHEN error_code = 'exact_duplicate' THEN 'failed' ELSE status END,
                outcome = CASE WHEN error_code = 'exact_duplicate' AND batch_id IS NOT NULL THEN 'rejected' ELSE outcome END,
                selected = CASE WHEN error_code = 'exact_duplicate' THEN 0 ELSE selected END,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND staged_file_path IS NOT NULL",
            job_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("clear Managed Import staging path"))?
        .rows_affected();
        require_one(cleared).map_err(|error| error.context("clear Managed Import staging path"))?;
        history::archive_standalone(&mut tx, job_id, HISTORY_RESULT_FAILED).await?;
        Self::commit(tx, operation).await
    }

    pub async fn mark_confirmation_exact_duplicate(&self, job_id: &str) -> Result<()> {
        let marked = sqlx::query!(
            "UPDATE managed_import_jobs
            SET status = 'failed', error_code = 'exact_duplicate', error_field = 'file',
                error_reason = 'file bytes match an existing Track', updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND status = 'awaiting_confirmation'",
            job_id
        )
        .execute(&self.pool)
        .await
        .map_err(context("mark confirmation Exact Duplicate"))?
        .rows_affected();
        require_one(marked).map_err(|error| error.context("mark confirmation Exact Duplicate"))
    }

    pub async fn mark_upload_interrupted(
        &self,
        job_id: &str,
        original_filename: &str,
    ) -> Result<()> {
        let operation = "interrupted Managed Import transition";
        let mut tx = self.begin(operation).await?;
        let marked = sqlx::query!(
            "UPDATE managed_import_jobs
            SET revision = revision + 1,
                original_filename = COALESCE(NULLIF(CAST(?1 AS TEXT), ''), original_filename),
                error_code = 'upload_interrupted', error_field = 'file',
                error_reason = 'upload stream was interrupted; retry this file',
                outcome = NULL, selected = 0, staged_file_path = NULL, content_sha256 = NULL,
                preview_json = NULL, upload_size_bytes = 0, validation_progress = 0, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?2 AND status = 'uploading' AND batch_id IS NOT NULL",
            original_filename,
            job_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("mark Managed Import upload interrupted"))?
        .rows_affected();
        require_one(marked)
            .map_err(|error| error.context("mark Managed Import upload interrupted"))?;
        revise_job_batch(&mut tx, job_id)
            .await
            .map_err(|error| error.context("revise interrupted Managed Import Batch file"))?;
        Self::commit(tx, operation).await
    }

    pub async fn mark_failed(
        &self,
        job_id: &str,
        original_filename: &str,
        error_code: &str,
        error_field: &str,
        error_reason: &str,
        issues: &[Issue],
    ) -> Result<()> {
        let encoded = encode_issues(issues);
        let operation = "failed Managed Import transition";
        let mut tx = self.begin(operation).await?;
        let marked = sqlx::query!(
            "UPDATE managed_import_jobs
            SET status = 'failed',
                original_filename = COALESCE(NULLIF(CAST(?1 AS TEXT), ''), original_filename),
                error_code = ?2, error_field = ?3, error_reason = ?4, validation_issues = ?5,
                outcome = CASE WHEN batch_id IS NULL THEN NULL ELSE 'rejected' END,
                selected = 0, staged_file_path = NULL, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?6 AND status IN ('uploading', 'awaiting_confirmation')",
            original_filename,
            error_code,
            error_field,
            error_reason,
            encoded,
            job_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("mark Managed Import failed"))?
        .rows_affected();
        require_one(marked).map_err(|error| error.context("mark Managed Import failed"))?;
        revise_uploading_job_batch(&mut tx, job_id)
            .await
            .map_err(|error| error.context("revise rejected Managed Import Batch file"))?;
        history::archive_standalone(&mut tx, job_id, HISTORY_RESULT_FAILED).await?;
        Self::commit(tx, operation).await
    }

    pub async fn start_batch_confirmation(
        &self,
        batch_id: &str,
        revision: i64,
        selected: &[String],
    ) -> Result<()> {
        let operation = "Managed Import Batch confirmation";
        let mut tx = self.begin(operation).await?;
        let unresolved = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM managed_import_jobs
            WHERE batch_id = ? AND status = 'uploading' AND COALESCE(error_code, '') <> 'upload_interrupted'",
            batch_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(context("count unresolved Managed Import Batch files"))?;
        if unresolved > 0 {
            return Err(Error::wrap(
                Sentinel::InvalidState,
                "Managed Import Batch has unresolved files",
            ));
        }
        let started = sqlx::query!(
            "UPDATE managed_import_batches SET status = 'confirming', revision = revision + 1, updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND status = 'uploading' AND revision = ?",
            batch_id,
            revision
        )
        .execute(&mut *tx)
        .await
        .map_err(context("start Managed Import Batch confirmation"))?
        .rows_affected();
        if started != 1 {
            let current = sqlx::query_scalar!(
                "SELECT revision FROM managed_import_batches WHERE id = ?",
                batch_id
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(context(
                "inspect Managed Import Batch confirmation conflict",
            ))?;
            return Err(match current {
                None => Error::sentinel(Sentinel::NotFound),
                Some(current) if current != revision => Error::sentinel(Sentinel::RevisionConflict),
                Some(_) => Error::sentinel(Sentinel::InvalidState),
            });
        }
        sqlx::query!(
            "UPDATE managed_import_jobs SET selected = 0 WHERE batch_id = ?",
            batch_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("clear Managed Import Batch selection"))?;
        for job_id in selected {
            let chosen = sqlx::query!(
                "UPDATE managed_import_jobs SET selected = 1 WHERE id = ? AND batch_id = ? AND status = 'awaiting_confirmation'",
                job_id,
                batch_id
            )
            .execute(&mut *tx)
            .await
            .map_err(context("select Managed Import Batch file"))?
            .rows_affected();
            if chosen != 1 {
                return Err(Error::wrap(
                    Sentinel::InvalidUpload,
                    format!("selected file {job_id:?} is not accepted"),
                ));
            }
        }
        Self::commit(tx, operation).await
    }

    pub async fn mark_batch_file_outcome(
        &self,
        job_id: &str,
        outcome: &str,
        error_code: &str,
        error_reason: &str,
    ) -> Result<()> {
        let status = if outcome == OUTCOME_IMPORTED || outcome == OUTCOME_REPLACED {
            STATUS_COMMITTED
        } else {
            STATUS_FAILED
        };
        let marked = sqlx::query!(
            "UPDATE managed_import_jobs
            SET status = ?1, outcome = ?2,
                error_code = NULLIF(CAST(?3 AS TEXT), ''),
                error_reason = NULLIF(CAST(?4 AS TEXT), ''),
                staged_file_path = NULL, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?5
                AND (status = 'awaiting_confirmation' OR (status = 'uploading' AND error_code = 'upload_interrupted'))
                AND outcome IS NULL",
            status,
            outcome,
            error_code,
            error_reason,
            job_id
        )
        .execute(&self.pool)
        .await
        .map_err(context("record Managed Import Batch file outcome"))?
        .rows_affected();
        require_one(marked)
            .map_err(|error| error.context("record Managed Import Batch file outcome"))
    }

    pub async fn complete_batch(&self, batch_id: &str) -> Result<()> {
        let operation = "Managed Import Batch completion";
        let mut tx = self.begin(operation).await?;
        let completed = sqlx::query!(
            "UPDATE managed_import_batches SET status = 'completed', revision = revision + 1, updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND status = 'confirming'",
            batch_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("complete Managed Import Batch"))?
        .rows_affected();
        require_one(completed)?;
        history::archive_batch(&mut tx, batch_id, HISTORY_RESULT_COMPLETED).await?;
        sqlx::query!(
            "UPDATE managed_import_jobs SET preview_json = NULL, import_plan_json = NULL WHERE batch_id = ?",
            batch_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("clear completed Import Preview payloads"))?;
        sqlx::query!(
            "DELETE FROM managed_import_artwork WHERE batch_id = ?",
            batch_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("clear completed import artwork"))?;
        Self::commit(tx, operation).await
    }

    pub async fn awaiting_preview_paths(&self, excluded_job_id: &str) -> Result<Vec<String>> {
        let paths = sqlx::query_scalar!(
            "SELECT staged_file_path FROM managed_import_jobs
            WHERE status = 'awaiting_confirmation' AND id != ? AND staged_file_path IS NOT NULL",
            excluded_job_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(context("list awaiting Import Preview files"))?;
        Ok(paths.into_iter().map(Option::unwrap_or_default).collect())
    }

    /// Stores each file's plan only while the Batch is still at the
    /// revision the plans were computed from.
    pub async fn save_import_plans(
        &self,
        batch: &Batch,
        plans: &HashMap<String, ImportPlan>,
    ) -> Result<()> {
        let operation = "import planning";
        let mut tx = self.begin(operation).await?;
        let revision = sqlx::query_scalar!(
            "SELECT revision FROM managed_import_batches WHERE id = ? AND status = 'uploading'",
            batch.id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(Error::from)?
        .ok_or_else(|| Error::new("sql: no rows in result set"))?;
        if revision != batch.revision {
            return Err(Error::sentinel(Sentinel::RevisionConflict));
        }
        for (job_id, plan) in plans {
            let encoded = crate::gojson::to_string(plan);
            let saved = sqlx::query!(
                "UPDATE managed_import_jobs
                SET import_plan_json = ?1, replace_track_id = NULLIF(CAST(?2 AS TEXT), '')
                WHERE id = ?3 AND batch_id = ?4 AND status = 'awaiting_confirmation'",
                encoded,
                plan.target_id,
                job_id,
                batch.id
            )
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?
            .rows_affected();
            require_one(saved)?;
        }
        Self::commit(tx, operation).await
    }

    pub async fn set_job_replace_target(&self, job_id: &str, track_id: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE managed_import_jobs SET replace_track_id = ? WHERE id = ?",
            track_id,
            job_id
        )
        .execute(&self.pool)
        .await
        .map_err(Error::from)?;
        Ok(())
    }

    pub async fn content_hash_exists(&self, content_sha256: &str) -> Result<bool> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM track_sources WHERE content_sha256 = ?) AS "hash_exists!: bool""#,
            content_sha256
        )
        .fetch_one(&self.pool)
        .await
        .map_err(context("check skipped import hash"))
    }
}

async fn revise_batch(conn: &mut SqliteConnection, batch_id: &str) -> Result<()> {
    sqlx::query!(
        "UPDATE managed_import_batches SET revision = revision + 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        batch_id
    )
    .execute(conn)
    .await
    .map_err(context("revise Managed Import Batch"))?;
    Ok(())
}

async fn revise_job_batch(conn: &mut SqliteConnection, job_id: &str) -> Result<()> {
    sqlx::query!(
        "UPDATE managed_import_batches SET revision = revision + 1, updated_at = CURRENT_TIMESTAMP
        WHERE id = (SELECT batch_id FROM managed_import_jobs WHERE managed_import_jobs.id = ?)",
        job_id
    )
    .execute(conn)
    .await
    .map_err(Error::from)?;
    Ok(())
}

async fn revise_uploading_job_batch(conn: &mut SqliteConnection, job_id: &str) -> Result<()> {
    sqlx::query!(
        "UPDATE managed_import_batches SET revision = revision + 1, updated_at = CURRENT_TIMESTAMP
        WHERE id = (SELECT batch_id FROM managed_import_jobs WHERE managed_import_jobs.id = ?)
            AND status = 'uploading'",
        job_id
    )
    .execute(conn)
    .await
    .map_err(Error::from)?;
    Ok(())
}
