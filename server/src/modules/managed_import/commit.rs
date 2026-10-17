//! Writing a committed Managed Track and its commit journal
//! (repository_commit.go, queries/commit.sql).

use super::errors::{Error, Result, Sentinel};
use super::history;
use super::identity::{normalize_identity, track_identity_key};
use super::model::*;
use super::repository::{Repository, context, require_one};
use super::storage::PlacedFiles;
use super::types::{CommitIdentity, CommitJournal};
use crate::inspect::MediaInspection;
use sqlx::SqliteConnection;
use std::collections::HashMap;

/// Everything one commit writes.
#[derive(Clone, Copy)]
pub struct CommitData<'a> {
    pub job: &'a ImportJob,
    pub identity: &'a CommitIdentity,
    pub placement: &'a PlacedFiles,
    pub inspection: &'a MediaInspection,
    pub album_key: &'a str,
}

fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn positive(value: i64) -> Option<i64> {
    (value > 0).then_some(value)
}

impl Repository {
    /// The Album Artist and Album a file belongs to, new IDs when absent.
    pub async fn resolve_commit_identity(
        &self,
        album_artists: &[String],
        album_key: &str,
    ) -> Result<CommitIdentity> {
        let mut identity = CommitIdentity {
            track_id: uuid(),
            ..CommitIdentity::default()
        };
        let primary = normalize_identity(&album_artists[0]);
        let artist = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM artists WHERE name_normalized = ?"#,
            primary
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(context("resolve primary Album Artist identity"))?;
        identity.album_artist_id = artist.unwrap_or_else(uuid);
        let album = sqlx::query!(
            r#"SELECT albums.id AS "id!", album_artwork.file_path AS "file_path?",
                album_artwork.content_sha256 AS "content_sha256?"
            FROM albums
            LEFT JOIN album_artwork ON album_artwork.album_id = albums.id
            WHERE albums.identity_key = ?"#,
            album_key
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(context("resolve Managed Import Album identity"))?;
        match album {
            None => identity.album_id = uuid(),
            Some(album) => {
                identity.album_id = album.id;
                identity.existing_artwork_path = album.file_path.unwrap_or_default();
                identity.existing_artwork_sha256 = album.content_sha256.unwrap_or_default();
            }
        }
        Ok(identity)
    }

    /// Writes the Track hidden and journals the database commit, so
    /// recovery can finish or undo it after a crash.
    pub async fn commit_pending(&self, data: &CommitData<'_>, journal_id: &str) -> Result<()> {
        let operation = "pending Managed Import commit";
        let mut tx = self.begin(operation).await?;
        write_commit_data(&mut tx, data).await?;
        sqlx::query!(
            "UPDATE tracks SET is_pending_commit = 1 WHERE id = ?",
            data.identity.track_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("hide pending Managed Track"))?;
        let advanced = sqlx::query!(
            "UPDATE managed_import_commit_journal SET phase = ?, updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND phase = ?",
            COMMIT_PHASE_DATABASE_COMMITTED,
            journal_id,
            COMMIT_PHASE_VERIFIED
        )
        .execute(&mut *tx)
        .await
        .map_err(context("journal Managed Import database commit"))?
        .rows_affected();
        require_one(advanced)?;
        Self::commit(tx, operation).await
    }

    pub async fn finalize_commit(
        &self,
        job: &ImportJob,
        track_id: &str,
        journal_id: &str,
    ) -> Result<ImportResult> {
        let operation = "Managed Import finalization";
        let mut tx = self.begin(operation).await?;
        let published = sqlx::query!(
            "UPDATE tracks SET is_pending_commit = 0, updated_at = CURRENT_TIMESTAMP WHERE id = ? AND is_pending_commit = 1",
            track_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("publish committed Managed Track"))?
        .rows_affected();
        require_one(published)?;
        let result = mark_committed(&mut tx, job, track_id).await?;
        let completed = sqlx::query!(
            "UPDATE managed_import_commit_journal
            SET phase = 'completed', recovery_reason = COALESCE(recovery_reason, 'commit completed'), updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND phase = 'cleaned'",
            journal_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("complete Managed Import commit journal"))?
        .rows_affected();
        require_one(completed)?;
        history::archive_standalone(&mut tx, job.id(), HISTORY_RESULT_COMPLETED).await?;
        Self::commit(tx, operation).await?;
        Ok(result)
    }

    pub async fn rollback_pending_commit(
        &self,
        journal: &CommitJournal,
        reason: &str,
    ) -> Result<()> {
        let operation = "pending Managed Import rollback";
        let mut tx = self.begin(operation).await?;
        sqlx::query!(
            "DELETE FROM album_artwork WHERE source_track_id = ?",
            journal.track_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("delete pending Album Artwork"))?;
        sqlx::query!(
            "DELETE FROM tracks WHERE id = ? AND is_pending_commit = 1",
            journal.track_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("delete pending Managed Track"))?;
        let reset = reset_open_journal(&mut tx, &journal.id, COMMIT_PHASE_VERIFIED, reason)
            .await
            .map_err(|error| error.context("journal pending Managed Import rollback"))?;
        require_one(reset)?;
        Self::commit(tx, operation).await
    }

    pub async fn create_commit_journal(&self, journal: &CommitJournal) -> Result<()> {
        let created = i64::from(journal.is_artwork_created);
        sqlx::query!(
            "INSERT INTO managed_import_commit_journal (
                id, job_id, track_id, phase, staged_file_path, audio_file_path, artwork_file_path,
                audio_sha256, artwork_sha256, artwork_created
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            journal.id,
            journal.job_id,
            journal.track_id,
            journal.phase,
            journal.staged_file_path,
            journal.audio_file_path,
            journal.artwork_file_path,
            journal.audio_sha256,
            journal.artwork_sha256,
            created
        )
        .execute(&self.pool)
        .await
        .map_err(context("create Managed Import commit journal"))?;
        Ok(())
    }

    pub async fn mark_commit_placed(&self, journal_id: &str, artwork_created: bool) -> Result<()> {
        let created = i64::from(artwork_created);
        let placed = sqlx::query!(
            "UPDATE managed_import_commit_journal
            SET phase = 'placed', artwork_created = ?, updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND phase = 'prepared'",
            created,
            journal_id
        )
        .execute(&self.pool)
        .await
        .map_err(context("journal canonical Managed Import placement"))?
        .rows_affected();
        require_one(placed)
    }

    pub async fn mark_commit_artwork_created(&self, journal_id: &str) -> Result<()> {
        let marked = sqlx::query!(
            "UPDATE managed_import_commit_journal SET artwork_created = 1, updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND phase = 'prepared'",
            journal_id
        )
        .execute(&self.pool)
        .await
        .map_err(context("journal Canonical Album Artwork creation"))?
        .rows_affected();
        require_one(marked)
    }

    pub async fn update_commit_phase(&self, journal_id: &str, phase: &str) -> Result<()> {
        let updated = sqlx::query!(
            "UPDATE managed_import_commit_journal SET phase = ?, updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND phase NOT IN ('completed', 'rolled_back')",
            phase,
            journal_id
        )
        .execute(&self.pool)
        .await
        .map_err(|error| {
            Error::from(error).context(format!("update Managed Import commit journal to {phase:?}"))
        })?
        .rows_affected();
        require_one(updated)
    }

    pub async fn rollback_commit_journal(&self, journal_id: &str, reason: &str) -> Result<()> {
        let mut conn = self.conn().await?;
        let rolled_back =
            reset_open_journal(&mut conn, journal_id, COMMIT_PHASE_ROLLED_BACK, reason)
                .await
                .map_err(|error| error.context("roll back Managed Import commit journal"))?;
        require_one(rolled_back)
    }

    pub async fn record_commit_recovery_reason(
        &self,
        journal_id: &str,
        reason: &str,
    ) -> Result<()> {
        let recorded = sqlx::query!(
            "UPDATE managed_import_commit_journal SET recovery_reason = ?, updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND phase NOT IN ('completed', 'rolled_back')",
            reason,
            journal_id
        )
        .execute(&self.pool)
        .await
        .map_err(context("record Managed Import recovery reason"))?
        .rows_affected();
        require_one(recorded)
    }

    pub async fn list_incomplete_commit_journals(&self) -> Result<Vec<CommitJournal>> {
        let rows = sqlx::query!(
            r#"SELECT id AS "id!", job_id, track_id, phase, staged_file_path, audio_file_path, artwork_file_path,
                audio_sha256, artwork_sha256, artwork_created, recovery_reason
            FROM managed_import_commit_journal
            WHERE phase NOT IN ('completed', 'rolled_back')
            ORDER BY created_at, id"#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(context("list incomplete Managed Import commits"))?;
        Ok(rows
            .into_iter()
            .map(|row| CommitJournal {
                id: row.id,
                job_id: row.job_id,
                track_id: row.track_id,
                phase: row.phase,
                staged_file_path: row.staged_file_path,
                audio_file_path: row.audio_file_path,
                artwork_file_path: row.artwork_file_path,
                audio_sha256: row.audio_sha256,
                artwork_sha256: row.artwork_sha256,
                is_artwork_created: row.artwork_created != 0,
                recovery_reason: row.recovery_reason.unwrap_or_default(),
            })
            .collect())
    }

    async fn find_commit_journal(
        &self,
        job_id: Option<&str>,
        journal_id: Option<&str>,
    ) -> Result<Option<CommitJournal>> {
        let row = sqlx::query!(
            r#"SELECT id AS "id!", job_id, track_id, phase, staged_file_path, audio_file_path, artwork_file_path,
                audio_sha256, artwork_sha256, artwork_created, recovery_reason
            FROM managed_import_commit_journal
            WHERE (?1 IS NOT NULL AND id = ?1)
                OR (?2 IS NOT NULL AND job_id = ?2 AND phase NOT IN ('completed', 'rolled_back'))"#,
            journal_id,
            job_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(context("read Managed Import commit journal"))?;
        Ok(row.map(|row| CommitJournal {
            id: row.id,
            job_id: row.job_id,
            track_id: row.track_id,
            phase: row.phase,
            staged_file_path: row.staged_file_path,
            audio_file_path: row.audio_file_path,
            artwork_file_path: row.artwork_file_path,
            audio_sha256: row.audio_sha256,
            artwork_sha256: row.artwork_sha256,
            is_artwork_created: row.artwork_created != 0,
            recovery_reason: row.recovery_reason.unwrap_or_default(),
        }))
    }

    pub async fn get_commit_journal(&self, journal_id: &str) -> Result<CommitJournal> {
        self.find_commit_journal(None, Some(journal_id))
            .await?
            .ok_or_else(|| Error::sentinel(Sentinel::NotFound))
    }

    pub async fn find_incomplete_commit_journal(
        &self,
        job_id: &str,
    ) -> Result<Option<CommitJournal>> {
        self.find_commit_journal(Some(job_id), None).await
    }
}

async fn reset_open_journal(
    conn: &mut SqliteConnection,
    journal_id: &str,
    phase: &str,
    reason: &str,
) -> Result<u64> {
    Ok(sqlx::query!(
        "UPDATE managed_import_commit_journal SET phase = ?, recovery_reason = ?, updated_at = CURRENT_TIMESTAMP
        WHERE id = ? AND phase NOT IN ('completed', 'rolled_back')",
        phase,
        reason,
        journal_id
    )
    .execute(conn)
    .await
    .map_err(Error::from)?
    .rows_affected())
}

pub(super) async fn mark_committed(
    conn: &mut SqliteConnection,
    job: &ImportJob,
    track_id: &str,
) -> Result<ImportResult> {
    let revision = job.revision();
    let marked = sqlx::query!(
        "UPDATE managed_import_jobs
        SET status = 'committed', revision = revision + 1, track_id = ?, staged_file_path = NULL,
            outcome = CASE WHEN batch_id IS NULL THEN NULL ELSE 'imported' END, updated_at = CURRENT_TIMESTAMP
        WHERE id = ? AND status = 'awaiting_confirmation' AND revision = ?",
        track_id,
        job.job.id,
        revision
    )
    .execute(conn)
    .await
    .map_err(context("mark Managed Import committed"))?
    .rows_affected();
    require_one(marked)?;
    Ok(ImportResult {
        job_id: job.job.id.clone(),
        status: STATUS_COMMITTED.to_owned(),
        revision: revision + 1,
        track_id: track_id.to_owned(),
    })
}

async fn write_commit_data(conn: &mut SqliteConnection, data: &CommitData<'_>) -> Result<()> {
    let metadata = &data.inspection.metadata;
    let artist_ids = upsert_artists(
        conn,
        &metadata.album_artists,
        &metadata.artists,
        &data.identity.album_artist_id,
    )
    .await?;
    upsert_album_for(conn, data, &artist_ids).await?;
    insert_track(conn, data).await?;
    insert_relationships(conn, data, &artist_ids).await?;
    insert_artwork_for(conn, data).await
}

/// Artist IDs by normalized name; the primary Album Artist keeps the ID
/// the placement used.
pub(super) async fn upsert_artists(
    conn: &mut SqliteConnection,
    album_artists: &[String],
    artists: &[String],
    album_artist_id: &str,
) -> Result<HashMap<String, String>> {
    let mut ids = HashMap::new();
    let primary = normalize_identity(&album_artists[0]);
    for name in album_artists.iter().chain(artists) {
        let normalized = normalize_identity(name);
        if ids.contains_key(&normalized) {
            continue;
        }
        let preferred = if normalized == primary {
            album_artist_id
        } else {
            ""
        };
        let existing = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM artists WHERE name_normalized = ?"#,
            normalized
        )
        .fetch_optional(&mut *conn)
        .await
        .map_err(|error| Error::from(error).context(format!("lookup Artist {name:?}")))?;
        let id = match existing {
            Some(id) if !preferred.is_empty() && id != preferred => {
                return Err(Error::new(
                    "resolved Artist identity changed during Managed Import commit",
                ));
            }
            Some(id) => id,
            None => {
                let id = if preferred.is_empty() {
                    uuid()
                } else {
                    preferred.to_owned()
                };
                sqlx::query!(
                    "INSERT INTO artists (id, name, name_sort, name_normalized) VALUES (?, ?, ?, ?)",
                    id,
                    name,
                    normalized,
                    normalized
                )
                .execute(&mut *conn)
                .await
                .map_err(|error| Error::from(error).context(format!("create Artist {name:?}")))?;
                id
            }
        };
        ids.insert(normalized, id);
    }
    Ok(ids)
}

pub(super) async fn upsert_album_for(
    conn: &mut SqliteConnection,
    data: &CommitData<'_>,
    artist_ids: &HashMap<String, String>,
) -> Result<()> {
    let metadata = &data.inspection.metadata;
    let existing = sqlx::query_scalar!(
        r#"SELECT id AS "id!" FROM albums WHERE identity_key = ?"#,
        data.album_key
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(context("lookup Managed Import Album"))?;
    if let Some(existing) = existing {
        if existing != data.identity.album_id {
            return Err(Error::new(
                "resolved Album identity changed during Managed Import commit",
            ));
        }
        return Ok(());
    }
    let genres = crate::gojson::to_string(&metadata.genres);
    let artist_id = &artist_ids[&normalize_identity(&metadata.album_artists[0])];
    let title_sort = normalize_identity(&metadata.album);
    let year = positive(metadata.year);
    let release_date = year.map(|year| year.to_string());
    sqlx::query!(
        "INSERT INTO albums (id, artist_id, title, title_sort, year, release_date, genres, identity_key)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        data.identity.album_id,
        artist_id,
        metadata.album,
        title_sort,
        year,
        release_date,
        genres,
        data.album_key
    )
    .execute(&mut *conn)
    .await
    .map_err(context("create Managed Import Album"))?;
    Ok(())
}

/// The size and modification time of the placed audio file.
pub(super) fn file_facts(path: &str) -> Result<(i64, i64)> {
    let metadata =
        std::fs::metadata(path).map_err(|error| Error::new(format!("stat {path}: {error}")))?;
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs() as i64);
    Ok((metadata.len() as i64, mtime))
}

async fn insert_track(conn: &mut SqliteConnection, data: &CommitData<'_>) -> Result<()> {
    let metadata = &data.inspection.metadata;
    let audio = &data.inspection.audio;
    let (size, mtime) = file_facts(&data.placement.audio_path)
        .map_err(|error| error.context("stat canonical Managed Track"))?;
    let title_sort = normalize_identity(&metadata.title);
    let artist_name = metadata.artists.join(", ");
    let genre = metadata.genres.first().cloned().unwrap_or_default();
    let bit_depth = positive(audio.bit_depth);
    let track_total = positive(metadata.track_position.total);
    let disc_total = positive(metadata.disc_position.total);
    let bitrate_bps = audio.bitrate_kbps * BITS_PER_KILOBIT;
    let identity_key = track_identity_key(metadata);
    let replay_gain = &metadata.replay_gain;
    sqlx::query!(
        "INSERT INTO tracks (
            id, album_id, title, title_sort, artist_name, track_no, duration_ms, format,
            size_bytes, file_path, file_mtime, genre, sample_rate_hz, bit_depth, disc_no,
            track_total, disc_total, channel_count, bitrate_bps, codec, container,
            replaygain_track_gain_db, replaygain_track_peak, replaygain_album_gain_db,
            replaygain_album_peak, identity_key, lyrics
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        data.identity.track_id,
        data.identity.album_id,
        metadata.title,
        title_sort,
        artist_name,
        metadata.track_position.number,
        audio.duration_ms,
        audio.format,
        size,
        data.placement.audio_path,
        mtime,
        genre,
        audio.sample_rate_hz,
        bit_depth,
        metadata.disc_position.number,
        track_total,
        disc_total,
        audio.channel_count,
        bitrate_bps,
        audio.codec,
        audio.container,
        replay_gain.track_gain_db,
        replay_gain.track_peak,
        replay_gain.album_gain_db,
        replay_gain.album_peak,
        identity_key,
        metadata.lyrics
    )
    .execute(&mut *conn)
    .await
    .map_err(context("create Managed Track"))?;
    let source_id = uuid();
    sqlx::query!(
        "INSERT INTO track_sources (id, track_id, source_kind, file_path, content_sha256, source_format, size_bytes)
        VALUES (?, ?, 'managed', ?, ?, ?, ?)",
        source_id,
        data.identity.track_id,
        data.placement.audio_path,
        data.inspection.file_sha256,
        audio.format,
        size
    )
    .execute(&mut *conn)
    .await
    .map_err(context("create Managed Track source"))?;
    Ok(())
}

async fn insert_relationships(
    conn: &mut SqliteConnection,
    data: &CommitData<'_>,
    artist_ids: &HashMap<String, String>,
) -> Result<()> {
    let metadata = &data.inspection.metadata;
    insert_album_artist_credits(
        conn,
        &data.identity.album_id,
        &metadata.album_artists,
        artist_ids,
    )
    .await?;
    insert_track_artist_credits(conn, &data.identity.track_id, &metadata.artists, artist_ids)
        .await?;
    for (position, name) in metadata.genres.iter().enumerate() {
        let genre_id = upsert_genre(conn, name).await?;
        let position = position as i64;
        sqlx::query!(
            "INSERT INTO track_genres (track_id, genre_id, position) VALUES (?, ?, ?)",
            data.identity.track_id,
            genre_id,
            position
        )
        .execute(&mut *conn)
        .await
        .map_err(|error| Error::from(error).context(format!("create Track Genre {name:?}")))?;
    }
    Ok(())
}

pub(super) async fn insert_track_artist_credits(
    conn: &mut SqliteConnection,
    track_id: &str,
    names: &[String],
    artist_ids: &HashMap<String, String>,
) -> Result<()> {
    for (position, name) in names.iter().enumerate() {
        let artist_id = &artist_ids[&normalize_identity(name)];
        let position = position as i64;
        sqlx::query!(
            "INSERT INTO track_artists (track_id, artist_id, position) VALUES (?, ?, ?)",
            track_id,
            artist_id,
            position
        )
        .execute(&mut *conn)
        .await
        .map_err(|error| {
            Error::from(error).context(format!("create ordered Artist credit {name:?}"))
        })?;
    }
    Ok(())
}

pub(super) async fn insert_album_artist_credits(
    conn: &mut SqliteConnection,
    album_id: &str,
    names: &[String],
    artist_ids: &HashMap<String, String>,
) -> Result<()> {
    for (position, name) in names.iter().enumerate() {
        let artist_id = &artist_ids[&normalize_identity(name)];
        let position = position as i64;
        sqlx::query!(
            "INSERT OR IGNORE INTO album_artists (album_id, artist_id, position) VALUES (?, ?, ?)",
            album_id,
            artist_id,
            position
        )
        .execute(&mut *conn)
        .await
        .map_err(|error| {
            Error::from(error).context(format!("create ordered Artist credit {name:?}"))
        })?;
    }
    Ok(())
}

pub(super) async fn upsert_genre(conn: &mut SqliteConnection, name: &str) -> Result<String> {
    let normalized = normalize_identity(name);
    let existing = sqlx::query_scalar!(
        r#"SELECT id AS "id!" FROM genres WHERE name_normalized = ?"#,
        normalized
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|error| Error::from(error).context(format!("lookup Genre {name:?}")))?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = uuid();
    sqlx::query!(
        "INSERT INTO genres (id, name, name_normalized) VALUES (?, ?, ?)",
        id,
        name,
        normalized
    )
    .execute(&mut *conn)
    .await
    .map_err(|error| Error::from(error).context(format!("create Genre {name:?}")))?;
    Ok(id)
}

pub(super) async fn insert_artwork_for(
    conn: &mut SqliteConnection,
    data: &CommitData<'_>,
) -> Result<()> {
    let artwork = &data.inspection.album_artwork;
    if !data.identity.existing_artwork_path.is_empty() || artwork.sha256.is_empty() {
        return Ok(());
    }
    let id = uuid();
    let size = artwork.data.len() as i64;
    sqlx::query!(
        "INSERT INTO album_artwork (
            id, album_id, source_track_id, content_sha256, media_type, width, height, encoded_size_bytes, file_path
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        id,
        data.identity.album_id,
        data.identity.track_id,
        artwork.sha256,
        artwork.mime_type,
        artwork.width,
        artwork.height,
        size,
        data.placement.artwork_path
    )
    .execute(&mut *conn)
    .await
    .map_err(context("create Album Artwork"))?;
    Ok(())
}
