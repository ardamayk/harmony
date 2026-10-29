//! Soulseek Acquisitions in the database: the module's only SQL. An
//! acquisition is written whole (its row and its files) in one IMMEDIATE
//! transaction, so a reader never sees half a step of the worker.

use super::acquisitions::{
    Acquisition, AcquisitionFile, AcquisitionSource, AcquisitionState, FileKind, FileState,
};
use super::versions::{Quality, RemoteFile, Scope};
use crate::db;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, thiserror::Error)]
#[error("{operation}: {source}")]
pub struct RepositoryError {
    operation: &'static str,
    source: Box<dyn std::error::Error + Send + Sync>,
}

type Result<T> = std::result::Result<T, RepositoryError>;

fn context<E: std::error::Error + Send + Sync + 'static>(
    operation: &'static str,
) -> impl FnOnce(E) -> RepositoryError {
    move |error| RepositoryError {
        operation,
        source: Box::new(error),
    }
}

#[derive(Clone)]
pub struct Repository {
    pool: SqlitePool,
}

impl Repository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, user_id: &str, acquisition: &Acquisition) -> Result<()> {
        let mut tx = db::begin_write(&self.pool)
            .await
            .map_err(context("begin Soulseek Acquisition creation"))?;
        let scope = acquisition.scope.name();
        let quality = quality_json(&acquisition.quality);
        let sources = sources_json(&acquisition.sources);
        let source_index = acquisition.source_index as i64;
        let state = acquisition.state.name();
        sqlx::query!(
            "INSERT INTO soulseek_acquisitions (
                id, user_id, query, scope, version_key, quality_json, sources_json,
                source_index, state, batch_id, failure_reason, progress_at_ms
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            acquisition.id,
            user_id,
            acquisition.query,
            scope,
            acquisition.version_key,
            quality,
            sources,
            source_index,
            state,
            acquisition.batch_id,
            acquisition.failure_reason,
            acquisition.progress_at_ms,
        )
        .execute(&mut *tx)
        .await
        .map_err(context("create Soulseek Acquisition"))?;
        insert_files(&mut tx, acquisition).await?;
        tx.commit()
            .await
            .map_err(context("commit Soulseek Acquisition creation"))
    }

    /// Writes the acquisition's state and files as they are now.
    pub async fn save(&self, acquisition: &Acquisition) -> Result<()> {
        let mut tx = db::begin_write(&self.pool)
            .await
            .map_err(context("begin Soulseek Acquisition update"))?;
        let source_index = acquisition.source_index as i64;
        let state = acquisition.state.name();
        sqlx::query!(
            "UPDATE soulseek_acquisitions
            SET source_index = ?, state = ?, batch_id = ?, failure_reason = ?, progress_at_ms = ?,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?",
            source_index,
            state,
            acquisition.batch_id,
            acquisition.failure_reason,
            acquisition.progress_at_ms,
            acquisition.id,
        )
        .execute(&mut *tx)
        .await
        .map_err(context("update Soulseek Acquisition"))?;
        sqlx::query!(
            "DELETE FROM soulseek_acquisition_files WHERE acquisition_id = ?",
            acquisition.id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("replace Soulseek Acquisition files"))?;
        insert_files(&mut tx, acquisition).await?;
        tx.commit()
            .await
            .map_err(context("commit Soulseek Acquisition update"))
    }

    /// Removes the acquisition and its files.
    pub async fn delete(&self, user_id: &str, id: &str) -> Result<()> {
        sqlx::query!(
            "DELETE FROM soulseek_acquisitions WHERE id = ? AND user_id = ?",
            id,
            user_id
        )
        .execute(&self.pool)
        .await
        .map_err(context("delete Soulseek Acquisition"))?;
        Ok(())
    }

    /// The user's active acquisition of this query, scope and version.
    pub async fn find_active(
        &self,
        user_id: &str,
        query: &str,
        scope: Scope,
        version_key: &str,
    ) -> Result<Option<Acquisition>> {
        let scope = scope.name();
        let id = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM soulseek_acquisitions
            WHERE user_id = ? AND query = ? AND scope = ? AND version_key = ?
                AND state IN ('downloading', 'importing', 'review')
            ORDER BY created_at DESC, rowid DESC LIMIT 1"#,
            user_id,
            query,
            scope,
            version_key,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(context("find active Soulseek Acquisition"))?;
        match id {
            Some(id) => self.get(user_id, &id).await,
            None => Ok(None),
        }
    }

    pub async fn get(&self, user_id: &str, id: &str) -> Result<Option<Acquisition>> {
        let row = sqlx::query!(
            r#"SELECT id AS "id!", query, scope, version_key, quality_json, sources_json,
                source_index, state, batch_id, failure_reason, progress_at_ms,
                created_at AS "created_at: String", updated_at AS "updated_at: String"
            FROM soulseek_acquisitions WHERE id = ? AND user_id = ?"#,
            id,
            user_id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(context("get Soulseek Acquisition"))?;
        let Some(row) = row else {
            return Ok(None);
        };
        let files = self.files(&row.id).await?;
        Ok(Some(Acquisition {
            scope: Scope::parse(&row.scope).unwrap_or(Scope::Album),
            quality: parse_quality(&row.quality_json)?,
            sources: parse_sources(&row.sources_json)?,
            source_index: usize::try_from(row.source_index).unwrap_or_default(),
            state: AcquisitionState::parse(&row.state).unwrap_or(AcquisitionState::Failed),
            id: row.id,
            query: row.query,
            version_key: row.version_key,
            batch_id: row.batch_id,
            failure_reason: row.failure_reason,
            progress_at_ms: row.progress_at_ms,
            created_at: row.created_at,
            updated_at: row.updated_at,
            files,
        }))
    }

    /// The user's acquisitions, newest first.
    pub async fn list(&self, user_id: &str) -> Result<Vec<Acquisition>> {
        let ids = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM soulseek_acquisitions WHERE user_id = ?
            ORDER BY created_at DESC, rowid DESC"#,
            user_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(context("list Soulseek Acquisitions"))?;
        let mut acquisitions = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(acquisition) = self.get(user_id, &id).await? {
                acquisitions.push(acquisition);
            }
        }
        Ok(acquisitions)
    }

    /// Every user's acquisitions the worker still drives, as (user, id).
    pub async fn list_active(&self) -> Result<Vec<(String, String)>> {
        let rows = sqlx::query!(
            r#"SELECT user_id, id AS "id!" FROM soulseek_acquisitions
            WHERE state IN ('downloading', 'importing', 'review') ORDER BY created_at, rowid"#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(context("list active Soulseek Acquisitions"))?;
        Ok(rows.into_iter().map(|row| (row.user_id, row.id)).collect())
    }

    async fn files(&self, acquisition_id: &str) -> Result<Vec<AcquisitionFile>> {
        let rows = sqlx::query!(
            r#"SELECT position, kind, name, source_index, remote_filename, size, transfer_id,
                state, bytes_transferred, job_id, outcome
            FROM soulseek_acquisition_files WHERE acquisition_id = ? ORDER BY position"#,
            acquisition_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(context("list Soulseek Acquisition files"))?;
        Ok(rows
            .into_iter()
            .map(|row| AcquisitionFile {
                kind: if row.kind == "artwork" {
                    FileKind::Artwork
                } else {
                    FileKind::Audio
                },
                name: row.name,
                source_index: usize::try_from(row.source_index).unwrap_or_default(),
                remote_filename: row.remote_filename,
                size: u64::try_from(row.size).unwrap_or_default(),
                transfer_id: row.transfer_id,
                state: FileState::parse(&row.state).unwrap_or(FileState::Failed),
                bytes_transferred: u64::try_from(row.bytes_transferred).unwrap_or_default(),
                job_id: row.job_id,
                outcome: row.outcome,
            })
            .collect())
    }
}

async fn insert_files(
    tx: &mut sqlx::Transaction<'static, sqlx::Sqlite>,
    acquisition: &Acquisition,
) -> Result<()> {
    for (position, file) in acquisition.files.iter().enumerate() {
        let position = position as i64;
        let kind = match file.kind {
            FileKind::Audio => "audio",
            FileKind::Artwork => "artwork",
        };
        let source_index = file.source_index as i64;
        let size = i64::try_from(file.size).unwrap_or(i64::MAX);
        let state = file.state.name();
        let bytes = i64::try_from(file.bytes_transferred).unwrap_or(i64::MAX);
        sqlx::query!(
            "INSERT INTO soulseek_acquisition_files (
                acquisition_id, position, kind, name, source_index, remote_filename, size,
                transfer_id, state, bytes_transferred, job_id, outcome
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            acquisition.id,
            position,
            kind,
            file.name,
            source_index,
            file.remote_filename,
            size,
            file.transfer_id,
            state,
            bytes,
            file.job_id,
            file.outcome,
        )
        .execute(&mut **tx)
        .await
        .map_err(context("write Soulseek Acquisition file"))?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredQuality {
    format: String,
    lossless: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bit_depth: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sample_rate_hz: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bitrate_kbps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    variable_bitrate: Option<bool>,
}

#[derive(Serialize, Deserialize)]
struct StoredSource {
    id: String,
    username: String,
    folder: String,
    files: Vec<StoredFile>,
    #[serde(default)]
    artwork: Vec<StoredFile>,
}

#[derive(Serialize, Deserialize)]
struct StoredFile {
    filename: String,
    size: u64,
}

fn quality_json(quality: &Quality) -> String {
    let stored = StoredQuality {
        format: quality.format.clone(),
        lossless: quality.lossless,
        bit_depth: quality.bit_depth,
        sample_rate_hz: quality.sample_rate_hz,
        bitrate_kbps: quality.bitrate_kbps,
        variable_bitrate: quality.variable_bitrate,
    };
    crate::gojson::to_string(&stored)
}

fn parse_quality(json: &str) -> Result<Quality> {
    let stored: StoredQuality =
        serde_json::from_str(json).map_err(context("decode Soulseek Acquisition quality"))?;
    Ok(Quality {
        format: stored.format,
        lossless: stored.lossless,
        bit_depth: stored.bit_depth,
        sample_rate_hz: stored.sample_rate_hz,
        bitrate_kbps: stored.bitrate_kbps,
        variable_bitrate: stored.variable_bitrate,
    })
}

fn stored_files(files: &[RemoteFile]) -> Vec<StoredFile> {
    files
        .iter()
        .map(|file| StoredFile {
            filename: file.filename.clone(),
            size: file.size,
        })
        .collect()
}

fn remote_files(files: Vec<StoredFile>) -> Vec<RemoteFile> {
    files
        .into_iter()
        .map(|file| RemoteFile {
            filename: file.filename,
            size: file.size,
        })
        .collect()
}

fn sources_json(sources: &[AcquisitionSource]) -> String {
    let stored: Vec<StoredSource> = sources
        .iter()
        .map(|source| StoredSource {
            id: source.id.clone(),
            username: source.username.clone(),
            folder: source.folder.clone(),
            files: stored_files(&source.files),
            artwork: stored_files(&source.artwork),
        })
        .collect();
    crate::gojson::to_string(&stored)
}

fn parse_sources(json: &str) -> Result<Vec<AcquisitionSource>> {
    let stored: Vec<StoredSource> =
        serde_json::from_str(json).map_err(context("decode Soulseek Acquisition sources"))?;
    Ok(stored
        .into_iter()
        .map(|source| AcquisitionSource {
            id: source.id,
            username: source.username,
            folder: source.folder,
            files: remote_files(source.files),
            artwork: remote_files(source.artwork),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_sources_use_the_go_json_encoding_and_round_trip() {
        let sources = vec![AcquisitionSource {
            id: "abc".to_owned(),
            username: "a&b".to_owned(),
            folder: r"Music\<Live> & More".to_owned(),
            files: vec![RemoteFile {
                filename: r"Music\<Live> & More\01.flac".to_owned(),
                size: 1,
            }],
            artwork: Vec::new(),
        }];
        let json = sources_json(&sources);
        assert!(
            json.contains("u003cLive") && !json.contains(['<', '>', '&']),
            "{json}"
        );
        assert_eq!(parse_sources(&json).unwrap(), sources);
    }
}
