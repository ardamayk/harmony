//! Import History: terminal imports kept without their staged audio
//! (repository_history.go, queries/history.sql).

use super::errors::{Error, Result};
use super::model::*;
use super::repository::{Repository, context, decode_issues, encode_issues, now};
use crate::gojson;
use sqlx::SqliteConnection;

/// A Job or canceled file as it stood when its import ended.
struct Source {
    file: HistoryFile,
    status: String,
    outcome: String,
    error_code: String,
    track_id: String,
}

#[allow(clippy::too_many_arguments)]
fn source(
    file_id: String,
    job_id: String,
    safe_filename: Option<String>,
    started_at: &str,
    completed_at: &str,
    content_sha256: Option<String>,
    status: String,
    outcome: Option<String>,
    error_code: Option<String>,
    track_id: Option<String>,
    issues: &str,
) -> Result<Source> {
    let issues = decode_issues(issues)
        .map_err(|error| error.context("read terminal Managed Import file"))?;
    Ok(Source {
        file: HistoryFile {
            issues,
            file_id,
            job_id,
            safe_filename: safe_filename.unwrap_or_default(),
            started_at: gojson::sqlite_time_from_stored(started_at),
            completed_at: gojson::sqlite_time_from_stored(completed_at),
            content_sha256: content_sha256.unwrap_or_default(),
            ..HistoryFile::default()
        },
        status,
        outcome: outcome.unwrap_or_default(),
        error_code: error_code.unwrap_or_default(),
        track_id: track_id.unwrap_or_default(),
    })
}

fn is_canceled(source: &Source, terminal: &str) -> bool {
    if source.error_code == IMPORT_CANCELED_RESULT_CODE {
        return true;
    }
    terminal == HISTORY_RESULT_CANCELED
        && source.outcome.is_empty()
        && source.status != STATUS_COMMITTED
        && source.status != STATUS_FAILED
}

fn apply_result(source: &mut Source, terminal: &str, terminal_at: &str) {
    if is_canceled(source, terminal) {
        source.file.result_code = HISTORY_RESULT_CANCELED.to_owned();
        if source.error_code != IMPORT_CANCELED_RESULT_CODE {
            source.file.completed_at = terminal_at.to_owned();
        }
        return;
    }
    let file = &mut source.file;
    file.result_code = source.outcome.clone();
    if source.status == STATUS_COMMITTED && file.result_code.is_empty() {
        file.result_code = OUTCOME_IMPORTED.to_owned();
    } else if file.result_code.is_empty() {
        file.result_code = source.status.clone();
    }
    if !source.error_code.is_empty()
        && source.outcome != OUTCOME_IMPORTED
        && source.outcome != OUTCOME_REPLACED
    {
        file.result_code = source.error_code.clone();
    }
    if source.outcome == OUTCOME_REPLACED {
        file.replaced_track_id = source.track_id.clone();
    } else if source.status == STATUS_COMMITTED {
        file.created_track_id = source.track_id.clone();
    }
}

fn increment(counts: &mut HistoryCounts, source: &Source, terminal: &str) {
    counts.total += 1;
    if is_canceled(source, terminal) {
        counts.canceled += 1;
        return;
    }
    match source.outcome.as_str() {
        OUTCOME_IMPORTED => counts.imported += 1,
        "" if source.status == STATUS_COMMITTED => counts.imported += 1,
        OUTCOME_REJECTED => counts.rejected += 1,
        OUTCOME_REPLACED => counts.replaced += 1,
        OUTCOME_NOT_ATTEMPTED => counts.not_attempted += 1,
        _ => counts.failed += 1,
    }
}

fn result_code(counts: &HistoryCounts, terminal: &str) -> String {
    if terminal == HISTORY_RESULT_CANCELED {
        return terminal.to_owned();
    }
    let succeeded = counts.imported + counts.replaced;
    if succeeded == counts.total {
        HISTORY_RESULT_COMPLETED
    } else if succeeded > 0 {
        HISTORY_RESULT_PARTIALLY_COMPLETED
    } else {
        HISTORY_RESULT_FAILED
    }
    .to_owned()
}

pub(super) async fn archive_batch(
    conn: &mut SqliteConnection,
    batch_id: &str,
    terminal: &str,
) -> Result<()> {
    let completed_at = now();
    let started_at = sqlx::query_scalar!(
        r#"SELECT created_at AS "created_at!: String" FROM managed_import_batches WHERE id = ?"#,
        batch_id
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(|error| {
        Error::from(error).context(format!("read terminal Managed Import Batch {batch_id:?}"))
    })?;
    let rows = sqlx::query!(
        r#"SELECT COALESCE(NULLIF(client_file_id, ''), id) AS "file_id!: String", id AS "job_id!: String",
            original_filename AS "safe_filename?: String", created_at AS "started_at!: String",
            updated_at AS "completed_at!: String", content_sha256 AS "content_sha256?: String",
            status AS "status!: String", outcome AS "outcome?: String", error_code AS "error_code?: String",
            track_id AS "track_id?: String", validation_issues AS "validation_issues!: String",
            batch_position AS "position!: i64"
        FROM managed_import_jobs jobs WHERE jobs.batch_id = ?1
        UNION ALL
        SELECT file_id, job_id, safe_filename, started_at, completed_at, content_sha256,
            'failed', 'not_attempted', 'canceled', NULL, '[]', position
        FROM managed_import_canceled_files canceled WHERE canceled.batch_id = ?1
        ORDER BY 12"#,
        batch_id
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(context("read terminal Managed Import Batch files"))?;
    let mut item = HistoryItem {
        import_id: batch_id.to_owned(),
        started_at: gojson::sqlite_time_from_stored(&started_at),
        completed_at: completed_at.clone(),
        ..HistoryItem::default()
    };
    for row in rows {
        let mut source = source(
            row.file_id,
            row.job_id,
            row.safe_filename,
            &row.started_at,
            &row.completed_at,
            row.content_sha256,
            row.status,
            row.outcome,
            row.error_code,
            row.track_id,
            &row.validation_issues,
        )?;
        apply_result(&mut source, terminal, &completed_at);
        increment(&mut item.counts, &source, terminal);
        item.files.push(source.file);
    }
    item.result_code = result_code(&item.counts, terminal);
    insert(conn, &item).await
}

pub(super) async fn archive_standalone(
    conn: &mut SqliteConnection,
    job_id: &str,
    terminal: &str,
) -> Result<()> {
    let row = sqlx::query!(
        r#"SELECT COALESCE(NULLIF(client_file_id, ''), id) AS "file_id!: String", id AS "job_id!: String",
            original_filename AS safe_filename, created_at AS "started_at!: String",
            updated_at AS "completed_at!: String", content_sha256, status, outcome, error_code, track_id,
            validation_issues
        FROM managed_import_jobs WHERE id = ? AND batch_id IS NULL"#,
        job_id
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(context("read terminal Managed Import file"))?;
    let Some(row) = row else {
        return Ok(());
    };
    let mut source = source(
        row.file_id,
        row.job_id,
        row.safe_filename,
        &row.started_at,
        &row.completed_at,
        row.content_sha256,
        row.status,
        row.outcome,
        row.error_code,
        row.track_id,
        &row.validation_issues,
    )?;
    let completed_at = source.file.completed_at.clone();
    apply_result(&mut source, terminal, &completed_at);
    let mut counts = HistoryCounts::default();
    increment(&mut counts, &source, terminal);
    let item = HistoryItem {
        import_id: job_id.to_owned(),
        started_at: source.file.started_at.clone(),
        completed_at: source.file.completed_at.clone(),
        result_code: result_code(&counts, terminal),
        counts,
        files: vec![source.file],
    };
    insert(conn, &item).await?;
    // A canceled standalone job is deleted by the caller in the same
    // transaction; only terminal rows lose their payloads here.
    sqlx::query!(
        "UPDATE managed_import_jobs SET preview_json = NULL, staged_file_path = NULL
        WHERE id = ? AND batch_id IS NULL AND status IN ('committed', 'failed')",
        job_id
    )
    .execute(&mut *conn)
    .await
    .map_err(context("clear terminal standalone Import payloads"))?;
    Ok(())
}

/// Keeps the first archive of an import and prunes history to its limit.
async fn insert(conn: &mut SqliteConnection, item: &HistoryItem) -> Result<()> {
    let inserted = sqlx::query!(
        "INSERT OR IGNORE INTO managed_import_history (
            import_id, started_at, completed_at, result_code, total_count, imported_count,
            rejected_count, failed_count, replaced_count, not_attempted_count, canceled_count
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        item.import_id,
        item.started_at,
        item.completed_at,
        item.result_code,
        item.counts.total,
        item.counts.imported,
        item.counts.rejected,
        item.counts.failed,
        item.counts.replaced,
        item.counts.not_attempted,
        item.counts.canceled
    )
    .execute(&mut *conn)
    .await
    .map_err(|error| {
        Error::from(error).context(format!("store Import History {:?}", item.import_id))
    })?
    .rows_affected();
    if inserted == 0 {
        return Ok(());
    }
    for (position, file) in item.files.iter().enumerate() {
        let issues = encode_issues(&file.issues);
        let position = position as i64;
        sqlx::query!(
            "INSERT INTO managed_import_history_files (
                import_id, file_id, job_id, safe_filename, started_at, completed_at, content_sha256,
                result_code, created_track_id, replaced_track_id, position, validation_issues
            ) VALUES (
                ?1, ?2, ?3, NULLIF(CAST(?4 AS TEXT), ''), ?5, ?6, NULLIF(CAST(?7 AS TEXT), ''),
                ?8, NULLIF(CAST(?9 AS TEXT), ''), NULLIF(CAST(?10 AS TEXT), ''), ?11, ?12
            )",
            item.import_id,
            file.file_id,
            file.job_id,
            file.safe_filename,
            file.started_at,
            file.completed_at,
            file.content_sha256,
            file.result_code,
            file.created_track_id,
            file.replaced_track_id,
            position,
            issues
        )
        .execute(&mut *conn)
        .await
        .map_err(|error| {
            Error::from(error).context(format!("store Import History file {:?}", file.file_id))
        })?;
    }
    sqlx::query!(
        "DELETE FROM managed_import_history WHERE import_id IN (
            SELECT import_id FROM managed_import_history ORDER BY completed_at DESC, rowid DESC LIMIT -1 OFFSET ?
        )",
        IMPORT_HISTORY_LIMIT
    )
    .execute(&mut *conn)
    .await
    .map_err(context("prune Import History"))?;
    Ok(())
}

impl Repository {
    pub async fn list_history(&self) -> Result<HistoryList> {
        let mut conn = self.conn().await?;
        let rows = sqlx::query!(
            r#"SELECT import_id AS "import_id!", started_at AS "started_at!: String",
                completed_at AS "completed_at!: String", result_code, total_count, imported_count,
                rejected_count, failed_count, replaced_count, not_attempted_count, canceled_count
            FROM managed_import_history
            ORDER BY completed_at DESC, rowid DESC
            LIMIT ?"#,
            IMPORT_HISTORY_LIMIT
        )
        .fetch_all(&mut *conn)
        .await
        .map_err(context("list Import History"))?;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let files = sqlx::query!(
                r#"SELECT file_id, job_id, safe_filename, started_at AS "started_at!: String",
                    completed_at AS "completed_at!: String", content_sha256, result_code,
                    created_track_id, replaced_track_id, validation_issues
                FROM managed_import_history_files WHERE import_id = ? ORDER BY position"#,
                row.import_id
            )
            .fetch_all(&mut *conn)
            .await
            .map_err(|error| {
                Error::from(error)
                    .context(format!("list Import History files for {:?}", row.import_id))
            })?;
            let files = files
                .into_iter()
                .map(|file| {
                    Ok(HistoryFile {
                        issues: decode_issues(&file.validation_issues)
                            .map_err(|error| error.context("read Import History file"))?,
                        file_id: file.file_id,
                        job_id: file.job_id,
                        safe_filename: file.safe_filename.unwrap_or_default(),
                        started_at: gojson::time(&file.started_at),
                        completed_at: gojson::time(&file.completed_at),
                        content_sha256: file.content_sha256.unwrap_or_default(),
                        result_code: file.result_code,
                        created_track_id: file.created_track_id.unwrap_or_default(),
                        replaced_track_id: file.replaced_track_id.unwrap_or_default(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            items.push(HistoryItem {
                import_id: row.import_id,
                started_at: gojson::time(&row.started_at),
                completed_at: gojson::time(&row.completed_at),
                result_code: row.result_code,
                counts: HistoryCounts {
                    total: row.total_count,
                    imported: row.imported_count,
                    rejected: row.rejected_count,
                    failed: row.failed_count,
                    replaced: row.replaced_count,
                    not_attempted: row.not_attempted_count,
                    canceled: row.canceled_count,
                },
                files,
            });
        }
        Ok(HistoryList { items })
    }
}
