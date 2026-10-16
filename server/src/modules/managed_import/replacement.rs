//! Track Replacement (track_replacement.go, repository_replacement.go,
//! queries/replacement.sql): a reviewed file replaces a Managed Track in
//! place, keeping its ID, Playlists and Queue entries. The previous file is
//! deleted only after the replacement was placed, hashed, swapped in, read
//! back and committed.

use super::commit::{
    CommitData, file_facts, insert_album_artist_credits, insert_track_artist_credits,
    upsert_artists, upsert_genre,
};
use super::deletion::{
    PlaylistReference, QueueInvalidation, QueueReference, advance_queues, delete_album_if_empty,
    delete_unreferenced_artists, list_playlists, list_queues, move_or_delete_artwork_reference,
    token_of, tokens_equal, track_and_album_artist_ids,
};
use super::errors::{Error, Result, Sentinel, ValidationError};
use super::history;
use super::identity::{album_identity_key, normalize_identity, track_identity_key};
use super::model::*;
use super::preview::{TargetRow, album_artist_names, read_target_row, track_artist_names};
use super::repository::{Repository, context, require_one};
use super::service::Service;
use super::storage::{PlacedFiles, ReplacementPlacement, Storage};
use super::types::*;
use crate::inspect::{MediaInspection, NormalizedMetadata};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use sqlx::SqliteConnection;
use tokio_util::sync::CancellationToken;

pub const REPLACEMENT_PHASE_PREPARED: &str = "prepared";
pub const REPLACEMENT_PHASE_PLACED: &str = "placed";
pub const REPLACEMENT_PHASE_VERIFIED: &str = "verified";
pub const REPLACEMENT_PHASE_SWAPPED: &str = "swapped";
pub const REPLACEMENT_PHASE_DATABASE_COMMITTED: &str = "database_committed";

/// A Managed Track as a replacement sees it (`replacementTarget`).
#[derive(Clone, Debug, Default)]
pub struct Target {
    pub row: TargetRow,
    pub relative_path: String,
    pub artists: Vec<String>,
    pub album_artists: Vec<String>,
    pub genres: Vec<String>,
    pub playlists: Vec<PlaylistReference>,
    pub queues: Vec<QueueReference>,
}

impl Target {
    fn storage_target(&self) -> ReplacementTarget {
        ReplacementTarget {
            track_id: self.row.track_id.clone(),
            album_id: self.row.album_id.clone(),
            relative_path: self.relative_path.clone(),
            artwork_path: self.row.artwork_path.clone(),
            is_sole_track: self.row.is_sole_track,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldDiff {
    pub field: String,
    pub current: String,
    pub replacement: String,
    pub is_changed: bool,
}

fn field_diff(field: &str, current: impl ToString, replacement: impl ToString) -> FieldDiff {
    let (current, replacement) = (current.to_string(), replacement.to_string());
    FieldDiff {
        field: field.to_owned(),
        is_changed: current != replacement,
        current,
        replacement,
    }
}

fn optional_number(value: i64) -> String {
    if value <= 0 {
        String::new()
    } else {
        value.to_string()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryChange {
    pub current_album_id: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub replacement_album_id: String,
    pub moves_album: bool,
    pub creates_album: bool,
    pub removes_empty_album: bool,
    pub removes_empty_artists: Vec<String>,
    pub creates_artists: Vec<String>,
    pub creates_genres: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkChange {
    pub current_media_type: String,
    #[serde(rename = "currentSha256")]
    pub current_sha256: String,
    pub replacement_media_type: String,
    #[serde(rename = "replacementSha256")]
    pub replacement_sha256: String,
    pub is_changed: bool,
    pub replaces_album_artwork: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDeletion {
    pub path: String,
    pub size_bytes: i64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackReplacementPreview {
    pub track_id: String,
    pub track_title: String,
    pub source_format: FieldDiff,
    pub technical_properties: Vec<FieldDiff>,
    pub metadata: Vec<FieldDiff>,
    pub library: LibraryChange,
    pub artwork: ArtworkChange,
    pub canonical_path: FieldDiff,
    pub old_file: FileDeletion,
    pub playlist_references: Vec<PlaylistReference>,
    pub queue_references: Vec<QueueReference>,
    pub confirmation_token: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct TrackReplacementConfirmation {
    pub revision: i64,
    #[serde(deserialize_with = "crate::gojson::nullable")]
    pub confirmation_token: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackReplacementResult {
    pub job_id: String,
    pub status: String,
    pub revision: i64,
    pub track_id: String,
    pub deleted_files: i64,
}

/// Everything a Track Replacement needs from Import Preview to commit.
pub struct ReplacementState {
    pub preview: TrackReplacementPreview,
    target: Target,
    identity: CommitIdentity,
    placement: ReplacementPlacement,
    inspection: MediaInspection,
    album_key: String,
    staged_size: i64,
}

/// The token payload in the Go struct's field order.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct TokenPayload<'a> {
    #[serde(rename = "TrackID")]
    track_id: &'a str,
    title: &'a str,
    #[serde(rename = "AlbumID")]
    album_id: &'a str,
    file_path: &'a str,
    #[serde(rename = "ContentSHA256")]
    content_sha256: &'a str,
    artwork_path: &'a str,
    #[serde(rename = "ArtworkSHA256")]
    artwork_sha256: &'a str,
    size_bytes: i64,
    track_revision: i64,
    source_revision: i64,
    playlists: &'a [PlaylistReference],
    queues: &'a [QueueReference],
    #[serde(rename = "ReplacementSHA256")]
    replacement_sha256: &'a str,
    replacement_album_key: &'a str,
    artwork_mode: &'a str,
    is_sole_track: bool,
    library: &'a LibraryChange,
    previous_artwork_path: &'a str,
    retired_artwork_path: &'a str,
}

fn replacement_token(state: &ReplacementState) -> String {
    let target = &state.target.row;
    let paths = state.placement.relatives();
    token_of(&TokenPayload {
        track_id: &target.track_id,
        title: &target.title,
        album_id: &target.album_id,
        file_path: &target.file_path,
        content_sha256: &target.content_sha256,
        artwork_path: &target.artwork_path,
        artwork_sha256: &target.artwork_sha256,
        size_bytes: target.size_bytes,
        track_revision: target.track_revision,
        source_revision: target.source_revision,
        playlists: &state.target.playlists,
        queues: &state.target.queues,
        replacement_sha256: &state.inspection.file_sha256,
        replacement_album_key: &state.album_key,
        artwork_mode: &state.placement.artwork_mode,
        is_sole_track: target.is_sole_track,
        library: &state.preview.library,
        previous_artwork_path: paths.previous_artwork,
        retired_artwork_path: paths.retired_artwork,
    })
}

fn build_preview(state: &ReplacementState, library: LibraryChange) -> TrackReplacementPreview {
    let target = &state.target.row;
    let metadata = &state.inspection.metadata;
    let audio = &state.inspection.audio;
    let artwork = &state.inspection.album_artwork;
    let joined = |values: &[String]| values.join(", ");
    let paths = state.placement.relatives();
    TrackReplacementPreview {
        track_id: target.track_id.clone(),
        track_title: target.title.clone(),
        source_format: field_diff("format", &target.format, &audio.format),
        technical_properties: vec![
            field_diff("container", &target.container, &audio.container),
            field_diff("codec", &target.codec, &audio.codec),
            field_diff("durationMs", target.duration_ms, audio.duration_ms),
            field_diff("sampleRateHz", target.sample_rate_hz, audio.sample_rate_hz),
            field_diff("channelCount", target.channel_count, audio.channel_count),
            field_diff(
                "bitDepth",
                optional_number(target.bit_depth),
                optional_number(audio.bit_depth),
            ),
            field_diff("bitrateKbps", target.bitrate_kbps, audio.bitrate_kbps),
            field_diff("sizeBytes", target.size_bytes, state.staged_size),
        ],
        metadata: vec![
            field_diff("title", &target.title, &metadata.title),
            field_diff(
                "artists",
                joined(&state.target.artists),
                joined(&metadata.artists),
            ),
            field_diff(
                "albumArtists",
                joined(&state.target.album_artists),
                joined(&metadata.album_artists),
            ),
            field_diff("album", &target.album, &metadata.album),
            field_diff(
                "year",
                optional_number(target.year),
                optional_number(metadata.year),
            ),
            field_diff(
                "genres",
                joined(&state.target.genres),
                joined(&metadata.genres),
            ),
            field_diff("discNo", target.disc_no, metadata.disc_position.number),
            field_diff("trackNo", target.track_no, metadata.track_position.number),
            field_diff(
                "discTotal",
                optional_number(target.disc_total),
                optional_number(metadata.disc_position.total),
            ),
            field_diff(
                "trackTotal",
                optional_number(target.track_total),
                optional_number(metadata.track_position.total),
            ),
        ],
        library,
        artwork: ArtworkChange {
            current_media_type: target.artwork_type.clone(),
            current_sha256: target.artwork_sha256.clone(),
            replacement_media_type: artwork.mime_type.clone(),
            replacement_sha256: artwork.sha256.clone(),
            is_changed: target.artwork_sha256 != artwork.sha256,
            replaces_album_artwork: state.placement.artwork_mode
                == REPLACEMENT_ARTWORK_MODE_REPLACE,
        },
        canonical_path: field_diff("canonicalPath", &state.target.relative_path, paths.audio),
        old_file: FileDeletion {
            path: state.target.relative_path.clone(),
            size_bytes: target.size_bytes,
        },
        playlist_references: state.target.playlists.clone(),
        queue_references: state.target.queues.clone(),
        confirmation_token: String::new(),
    }
}

pub fn attach(preview: &mut Preview, state: &ReplacementState) {
    let encoded = crate::gojson::to_string(&state.preview);
    preview.replacement = RawValue::from_string(encoded).ok();
}

fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

async fn missing_names(
    conn: &mut SqliteConnection,
    names: &[String],
    artists: bool,
) -> Result<Vec<String>> {
    let mut missing = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for name in names {
        let normalized = normalize_identity(name);
        if !seen.insert(normalized.clone()) {
            continue;
        }
        let found = if artists {
            sqlx::query_scalar!(
                r#"SELECT EXISTS (SELECT 1 FROM artists WHERE name_normalized = ?) AS "found!: bool""#,
                normalized
            )
            .fetch_one(&mut *conn)
            .await
        } else {
            sqlx::query_scalar!(
                r#"SELECT EXISTS (SELECT 1 FROM genres WHERE name_normalized = ?) AS "found!: bool""#,
                normalized
            )
            .fetch_one(&mut *conn)
            .await
        }
        .map_err(|error| Error::from(error).context(format!("inspect replacement name {name:?}")))?;
        if !found {
            missing.push(name.clone());
        }
    }
    Ok(missing)
}

impl Repository {
    /// The Managed Track to replace; its file must still match.
    pub async fn load_replacement_target(
        &self,
        storage: &Storage,
        track_id: &str,
    ) -> Result<Target> {
        let mut conn = self.conn().await?;
        let row = read_target_row(&mut conn, track_id).await?;
        let (relative_path, size) =
            storage.resolve_managed_file(&row.file_path, &row.content_sha256)?;
        if row.size_bytes != size {
            return Err(Error::wrap(
                Sentinel::ReplacementConflict,
                "managed file size changed",
            ));
        }
        let names = |error: Error| error.context("list Track Replacement names");
        let artists = track_artist_names(&mut conn, track_id)
            .await
            .map_err(names)?;
        let album_artists = album_artist_names(&mut conn, &row.album_id)
            .await
            .map_err(names)?;
        let genres = sqlx::query_scalar!(
            "SELECT genres.name FROM track_genres JOIN genres ON genres.id = track_genres.genre_id
            WHERE track_genres.track_id = ? ORDER BY track_genres.position",
            track_id
        )
        .fetch_all(&mut *conn)
        .await
        .map_err(context("list Track Replacement names"))?;
        let playlists = list_playlists(&mut conn, track_id).await?;
        let queues = list_queues(&mut conn, track_id).await?;
        Ok(Target {
            row,
            relative_path,
            artists,
            album_artists,
            genres,
            playlists,
            queues,
        })
    }

    pub async fn create_replacement_job(&self, track_id: &str) -> Result<Job> {
        let job = Job {
            id: uuid(),
            status: STATUS_UPLOADING.to_owned(),
            revision: 1,
            replaces_track_id: track_id.to_owned(),
            ..Job::default()
        };
        sqlx::query!(
            "INSERT INTO managed_import_jobs (id, status, revision, replace_track_id) VALUES (?, 'uploading', 1, ?)",
            job.id,
            track_id
        )
        .execute(&self.pool)
        .await
        .map_err(context("create Track Replacement job"))?;
        Ok(job)
    }

    /// Artists that lose every reference once the Track adopts the
    /// replacement credits.
    async fn orphaned_artist_names(
        &self,
        target: &Target,
        metadata: &NormalizedMetadata,
        removes_album: bool,
    ) -> Result<Vec<String>> {
        let retained: std::collections::HashSet<String> = metadata
            .artists
            .iter()
            .chain(&metadata.album_artists)
            .map(|name| normalize_identity(name))
            .collect();
        let includes_album = i64::from(removes_album);
        let rows = sqlx::query!(
            r#"SELECT DISTINCT artists.id AS "id!", artists.name, artists.name_normalized FROM artists
            WHERE artists.id IN (
                SELECT track_artists.artist_id FROM track_artists WHERE track_artists.track_id = ?1
                UNION SELECT album_artists.artist_id FROM album_artists
                WHERE album_artists.album_id = ?2 AND CAST(?3 AS INTEGER)
            )
            ORDER BY artists.name"#,
            target.row.track_id,
            target.row.album_id,
            includes_album
        )
        .fetch_all(&self.pool)
        .await
        .map_err(context("list replacement Artist references"))?;
        let mut names = Vec::new();
        for row in rows {
            if retained.contains(row.name_normalized.as_deref().unwrap_or_default()) {
                continue;
            }
            let referenced = sqlx::query_scalar!(
                r#"SELECT (EXISTS (
                        SELECT 1 FROM track_artists
                        WHERE track_artists.artist_id = ?1 AND track_artists.track_id != ?2
                    ) OR EXISTS (
                        SELECT 1 FROM album_artists
                        WHERE album_artists.artist_id = ?1
                            AND NOT (CAST(?3 AS INTEGER) AND album_artists.album_id = ?4)
                    )) AS "is_referenced: bool""#,
                row.id,
                target.row.track_id,
                includes_album,
                target.row.album_id
            )
            .fetch_one(&self.pool)
            .await
            .map_err(context("inspect replacement Artist reference"))?;
            if !referenced {
                names.push(row.name);
            }
        }
        Ok(names)
    }

    async fn album_exists(&self, album_key: &str) -> Result<bool> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM albums WHERE identity_key = ?) AS "exists!: bool""#,
            album_key
        )
        .fetch_one(&self.pool)
        .await
        .map_err(context("inspect replacement Album"))
    }

    async fn create_replacement_journal(&self, journal: &ReplacementJournal) -> Result<()> {
        let created = i64::from(journal.is_artwork_created);
        sqlx::query!(
            "INSERT INTO managed_track_replacements (
                id, job_id, track_id, phase, staged_file_path, pending_audio_path, audio_file_path,
                previous_audio_path, retired_audio_path, audio_sha256, previous_audio_sha256, artwork_mode,
                pending_artwork_path, artwork_file_path, previous_artwork_path, retired_artwork_path,
                artwork_sha256, previous_artwork_sha256, artwork_created, previous_album_id
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            journal.id,
            journal.job_id,
            journal.track_id,
            journal.phase,
            journal.staged_file_path,
            journal.pending_audio_path,
            journal.audio_file_path,
            journal.previous_audio_path,
            journal.retired_audio_path,
            journal.audio_sha256,
            journal.previous_audio_sha256,
            journal.artwork_mode,
            journal.pending_artwork_path,
            journal.artwork_file_path,
            journal.previous_artwork_path,
            journal.retired_artwork_path,
            journal.artwork_sha256,
            journal.previous_artwork_sha256,
            created,
            journal.previous_album_id
        )
        .execute(&self.pool)
        .await
        .map_err(context("create Track Replacement journal"))?;
        Ok(())
    }

    async fn update_replacement_phase(&self, journal_id: &str, from: &str, to: &str) -> Result<()> {
        let advanced = sqlx::query!(
            "UPDATE managed_track_replacements SET phase = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ? AND phase = ?",
            to,
            journal_id,
            from
        )
        .execute(&self.pool)
        .await
        .map_err(|error| Error::from(error).context(format!("journal Track Replacement phase {to:?}")))?
        .rows_affected();
        require_one(advanced)
    }

    async fn mark_replacement_artwork_created(&self, journal_id: &str) -> Result<()> {
        let marked = sqlx::query!(
            "UPDATE managed_track_replacements SET artwork_created = 1, updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND phase = 'prepared'",
            journal_id
        )
        .execute(&self.pool)
        .await
        .map_err(context("journal Track Replacement artwork creation"))?
        .rows_affected();
        require_one(marked)
    }

    async fn rollback_replacement_journal(&self, journal_id: &str, reason: &str) -> Result<()> {
        let rolled_back = sqlx::query!(
            "UPDATE managed_track_replacements SET phase = 'rolled_back', recovery_reason = ?, updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND phase NOT IN ('database_committed', 'completed', 'rolled_back')",
            reason,
            journal_id
        )
        .execute(&self.pool)
        .await
        .map_err(context("roll back Track Replacement journal"))?
        .rows_affected();
        require_one(rolled_back)
    }

    async fn record_replacement_recovery_reason(
        &self,
        journal_id: &str,
        reason: &str,
    ) -> Result<()> {
        let recorded = sqlx::query!(
            "UPDATE managed_track_replacements SET recovery_reason = ?, updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND phase NOT IN ('completed', 'rolled_back')",
            reason,
            journal_id
        )
        .execute(&self.pool)
        .await
        .map_err(context("record Track Replacement recovery reason"))?
        .rows_affected();
        require_one(recorded)
    }

    async fn replacement_journals(
        &self,
        filter: JournalFilter<'_>,
    ) -> Result<Vec<ReplacementJournal>> {
        let (journal_id, job_id, incomplete_job, latest_job, all_incomplete) = match filter {
            JournalFilter::Id(id) => (Some(id), None, None, None, false),
            JournalFilter::IncompleteForJob(job) => (None, None, Some(job), None, false),
            JournalFilter::LatestForJob(job) => (None, None, None, Some(job), false),
            JournalFilter::AllIncomplete => (None, None::<&str>, None, None, true),
        };
        let _ = job_id;
        let rows = sqlx::query!(
            r#"SELECT id AS "id!", job_id, track_id, phase, staged_file_path, pending_audio_path, audio_file_path,
                previous_audio_path, retired_audio_path, audio_sha256, previous_audio_sha256, artwork_mode,
                pending_artwork_path, artwork_file_path, previous_artwork_path, retired_artwork_path,
                artwork_sha256, previous_artwork_sha256, artwork_created, previous_album_id, recovery_reason
            FROM managed_track_replacements
            WHERE (?1 IS NOT NULL AND id = ?1)
                OR (?2 IS NOT NULL AND job_id = ?2 AND phase NOT IN ('completed', 'rolled_back'))
                OR (?3 IS NOT NULL AND id = (
                    SELECT id FROM managed_track_replacements WHERE job_id = ?3 ORDER BY created_at DESC, id DESC LIMIT 1
                ))
                OR (?4 AND phase NOT IN ('completed', 'rolled_back'))
            ORDER BY created_at, id"#,
            journal_id,
            incomplete_job,
            latest_job,
            all_incomplete
        )
        .fetch_all(&self.pool)
        .await
        .map_err(context("read Track Replacement journal"))?;
        Ok(rows
            .into_iter()
            .map(|row| ReplacementJournal {
                id: row.id,
                job_id: row.job_id,
                track_id: row.track_id,
                phase: row.phase,
                staged_file_path: row.staged_file_path,
                pending_audio_path: row.pending_audio_path,
                audio_file_path: row.audio_file_path,
                previous_audio_path: row.previous_audio_path,
                retired_audio_path: row.retired_audio_path,
                audio_sha256: row.audio_sha256,
                previous_audio_sha256: row.previous_audio_sha256,
                artwork_mode: row.artwork_mode,
                pending_artwork_path: row.pending_artwork_path,
                artwork_file_path: row.artwork_file_path,
                previous_artwork_path: row.previous_artwork_path,
                retired_artwork_path: row.retired_artwork_path,
                artwork_sha256: row.artwork_sha256,
                previous_artwork_sha256: row.previous_artwork_sha256,
                is_artwork_created: row.artwork_created != 0,
                previous_album_id: row.previous_album_id,
                recovery_reason: row.recovery_reason.unwrap_or_default(),
            })
            .collect())
    }

    pub async fn find_incomplete_replacement_journal(
        &self,
        job_id: &str,
    ) -> Result<Option<ReplacementJournal>> {
        Ok(self
            .replacement_journals(JournalFilter::IncompleteForJob(job_id))
            .await?
            .into_iter()
            .next())
    }

    async fn get_replacement_journal(&self, journal_id: &str) -> Result<ReplacementJournal> {
        self.replacement_journals(JournalFilter::Id(journal_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| Error::sentinel(Sentinel::NotFound))
    }

    /// Writes the replacement's metadata onto the existing Track ID and
    /// journals the database commit.
    async fn commit_replacement_data(
        &self,
        data: &CommitData<'_>,
        target: &Target,
        placement: &ReplacementPlacement,
        journal_id: &str,
    ) -> Result<Vec<QueueInvalidation>> {
        let operation = "Track Replacement commit";
        let mut tx = self.begin(operation).await?;
        let invalidations = write_replacement_data(&mut tx, data, target, placement).await?;
        let advanced = sqlx::query!(
            "UPDATE managed_track_replacements SET phase = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ? AND phase = ?",
            REPLACEMENT_PHASE_DATABASE_COMMITTED,
            journal_id,
            REPLACEMENT_PHASE_SWAPPED
        )
        .execute(&mut *tx)
        .await
        .map_err(context("journal Track Replacement database commit"))?
        .rows_affected();
        require_one(advanced)?;
        Self::commit(tx, operation).await?;
        Ok(invalidations)
    }

    /// Records the replaced outcome once the previous file is gone.
    async fn finalize_replacement(
        &self,
        job: &ImportJob,
        journal_id: &str,
    ) -> Result<TrackReplacementResult> {
        let operation = "Track Replacement finalization";
        let mut tx = self.begin(operation).await?;
        let revision = job.revision();
        let marked = sqlx::query!(
            "UPDATE managed_import_jobs
            SET status = 'committed', revision = revision + 1, track_id = replace_track_id, outcome = 'replaced',
                staged_file_path = NULL, updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND status = 'awaiting_confirmation' AND revision = ?",
            job.job.id,
            revision
        )
        .execute(&mut *tx)
        .await
        .map_err(context("mark Track Replacement committed"))?
        .rows_affected();
        require_one(marked)?;
        let completed = sqlx::query!(
            "UPDATE managed_track_replacements
            SET phase = 'completed', recovery_reason = COALESCE(recovery_reason, 'replacement completed'), updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND phase = 'database_committed'",
            journal_id
        )
        .execute(&mut *tx)
        .await
        .map_err(context("complete Track Replacement journal"))?
        .rows_affected();
        require_one(completed)?;
        history::archive_standalone(&mut tx, job.id(), HISTORY_RESULT_COMPLETED).await?;
        Self::commit(tx, operation).await?;
        Ok(TrackReplacementResult {
            job_id: job.job.id.clone(),
            status: STATUS_COMMITTED.to_owned(),
            revision: revision + 1,
            track_id: job.replace_track_id.clone(),
            deleted_files: 0,
        })
    }
}

enum JournalFilter<'a> {
    Id(&'a str),
    IncompleteForJob(&'a str),
    LatestForJob(&'a str),
    AllIncomplete,
}

async fn write_replacement_data(
    conn: &mut SqliteConnection,
    data: &CommitData<'_>,
    target: &Target,
    placement: &ReplacementPlacement,
) -> Result<Vec<QueueInvalidation>> {
    let metadata = &data.inspection.metadata;
    let artist_ids = upsert_artists(
        conn,
        &metadata.album_artists,
        &metadata.artists,
        &data.identity.album_artist_id,
    )
    .await?;
    super::commit::upsert_album_for(conn, data, &artist_ids).await?;
    let previous_artist_ids =
        track_and_album_artist_ids(conn, &target.row.track_id, &target.row.album_id).await?;
    let moves_album = data.identity.album_id != target.row.album_id;
    if moves_album {
        move_or_delete_artwork_reference(conn, &target.row.track_id, &target.row.album_id).await?;
    }
    update_replaced_track(conn, data, target).await?;
    sqlx::query!(
        "DELETE FROM track_artists WHERE track_id = ?",
        target.row.track_id
    )
    .execute(&mut *conn)
    .await
    .map_err(context("clear replaced Track relationships"))?;
    sqlx::query!(
        "DELETE FROM track_genres WHERE track_id = ?",
        target.row.track_id
    )
    .execute(&mut *conn)
    .await
    .map_err(context("clear replaced Track relationships"))?;
    insert_album_artist_credits(
        conn,
        &data.identity.album_id,
        &metadata.album_artists,
        &artist_ids,
    )
    .await?;
    insert_track_artist_credits(
        conn,
        &data.identity.track_id,
        &metadata.artists,
        &artist_ids,
    )
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
    write_replacement_artwork(conn, data, target, placement).await?;
    if moves_album {
        delete_album_if_empty(conn, &target.row.album_id)
            .await
            .map_err(|error| error.context("delete emptied Album after Track Replacement"))?;
    }
    delete_unreferenced_artists(conn, &previous_artist_ids)
        .await
        .map_err(|error| error.context("delete unreferenced Artist after Track Replacement"))?;
    advance_queues(conn, &target.queues)
        .await
        .map_err(|error| error.context("advance Queue revision after Track Replacement"))
}

async fn update_replaced_track(
    conn: &mut SqliteConnection,
    data: &CommitData<'_>,
    target: &Target,
) -> Result<()> {
    let metadata = &data.inspection.metadata;
    let audio = &data.inspection.audio;
    let (size, mtime) = file_facts(&data.placement.audio_path)
        .map_err(|error| error.context("stat replacement Managed Track"))?;
    let title_sort = normalize_identity(&metadata.title);
    let artist_name = metadata.artists.join(", ");
    let genre = metadata.genres.first().cloned().unwrap_or_default();
    let positive = |value: i64| (value > 0).then_some(value);
    let bit_depth = positive(audio.bit_depth);
    let track_total = positive(metadata.track_position.total);
    let disc_total = positive(metadata.disc_position.total);
    let bitrate_bps = audio.bitrate_kbps * BITS_PER_KILOBIT;
    let identity_key = track_identity_key(metadata);
    let replay_gain = &metadata.replay_gain;
    let updated = sqlx::query!(
        "UPDATE tracks SET album_id = ?, title = ?, title_sort = ?, artist_name = ?, track_no = ?, duration_ms = ?,
            format = ?, size_bytes = ?, file_path = ?, file_mtime = ?, genre = ?, sample_rate_hz = ?, bit_depth = ?,
            disc_no = ?, track_total = ?, disc_total = ?, channel_count = ?, bitrate_bps = ?, codec = ?, container = ?,
            replaygain_track_gain_db = ?, replaygain_track_peak = ?, replaygain_album_gain_db = ?,
            replaygain_album_peak = ?, identity_key = ?, revision = revision + 1, updated_at = CURRENT_TIMESTAMP
        WHERE id = ? AND revision = ? AND missing_at IS NULL",
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
        target.row.track_id,
        target.row.track_revision
    )
    .execute(&mut *conn)
    .await
    .map_err(context("update replaced Managed Track"))?
    .rows_affected();
    if updated != 1 {
        return Err(Error::wrap(
            Sentinel::ReplacementConflict,
            "Track changed during replacement",
        ));
    }
    let updated = sqlx::query!(
        "UPDATE track_sources SET file_path = ?, content_sha256 = ?, source_format = ?, size_bytes = ?,
            revision = revision + 1, updated_at = CURRENT_TIMESTAMP
        WHERE track_id = ? AND revision = ? AND source_kind = 'managed'",
        data.placement.audio_path,
        data.inspection.file_sha256,
        audio.format,
        size,
        target.row.track_id,
        target.row.source_revision
    )
    .execute(&mut *conn)
    .await
    .map_err(context("update replaced Managed Track source"))?
    .rows_affected();
    if updated != 1 {
        return Err(Error::wrap(
            Sentinel::ReplacementConflict,
            "Track source changed during replacement",
        ));
    }
    Ok(())
}

async fn write_replacement_artwork(
    conn: &mut SqliteConnection,
    data: &CommitData<'_>,
    target: &Target,
    placement: &ReplacementPlacement,
) -> Result<()> {
    let artwork = &data.inspection.album_artwork;
    match placement.artwork_mode.as_str() {
        REPLACEMENT_ARTWORK_MODE_CREATE => {
            let mut identity = data.identity.clone();
            identity.existing_artwork_path.clear();
            let shell = CommitData {
                identity: &identity,
                ..*data
            };
            super::commit::insert_artwork_for(conn, &shell).await
        }
        REPLACEMENT_ARTWORK_MODE_REPLACE => {
            let size = artwork.data.len() as i64;
            let replaced = sqlx::query!(
                "UPDATE album_artwork SET source_track_id = ?, content_sha256 = ?, media_type = ?, width = ?, height = ?,
                    encoded_size_bytes = ?, file_path = ?, revision = revision + 1, updated_at = CURRENT_TIMESTAMP
                WHERE album_id = ?",
                target.row.track_id,
                artwork.sha256,
                artwork.mime_type,
                artwork.width,
                artwork.height,
                size,
                data.placement.artwork_path,
                data.identity.album_id
            )
            .execute(&mut *conn)
            .await
            .map_err(context("replace Album Artwork"))?
            .rows_affected();
            require_one(replaced)
        }
        _ => Ok(()),
    }
}

impl Service {
    pub async fn create_replacement_job(&self, track_id: &str) -> Result<Job> {
        self.repo
            .load_replacement_target(&self.storage, track_id)
            .await?;
        self.repo.create_replacement_job(track_id).await
    }

    pub(super) async fn build_replacement_state(
        &self,
        cancel: &CancellationToken,
        job: &ImportJob,
        inspection: &MediaInspection,
        staged_path: &str,
    ) -> Result<ReplacementState> {
        let target = self
            .repo
            .load_replacement_target(&self.storage, &job.replace_track_id)
            .await?;
        let inspection = self.apply_import_plan(job, inspection.clone()).await?;
        let metadata = &inspection.metadata;
        let album_key = album_identity_key(metadata);
        let mut identity = self
            .repo
            .resolve_commit_identity(&metadata.album_artists, &album_key)
            .await?;
        identity.track_id = target.row.track_id.clone();
        let occupants = self
            .repo
            .position_candidates(metadata, &target.row.track_id)
            .await?;
        if !occupants.is_empty() {
            let reason = "another Track already occupies this Album position";
            return Err(Error::validation(
                ValidationError::new(ERROR_CODE_ALBUM_POSITION_CONFLICT, "TRACKNUMBER", reason),
                reason,
            ));
        }
        self.validate_album_positions_excluding(cancel, job.id(), metadata, &target.row.track_id)
            .await?;
        let placement = self.storage.plan_replacement_placement(
            staged_path,
            &inspection,
            &identity,
            &target.storage_target(),
        )?;
        let staged_size = self.storage.staged_file_size(staged_path)?;
        let library = self
            .library_change(&target, metadata, &identity, &album_key)
            .await?;
        let mut state = ReplacementState {
            preview: TrackReplacementPreview::default(),
            target,
            identity,
            placement,
            inspection,
            album_key,
            staged_size,
        };
        state.preview = build_preview(&state, library);
        state.preview.confirmation_token = replacement_token(&state);
        Ok(state)
    }

    async fn library_change(
        &self,
        target: &Target,
        metadata: &NormalizedMetadata,
        identity: &CommitIdentity,
        album_key: &str,
    ) -> Result<LibraryChange> {
        let mut change = LibraryChange {
            current_album_id: target.row.album_id.clone(),
            replacement_album_id: identity.album_id.clone(),
            moves_album: identity.album_id != target.row.album_id,
            ..LibraryChange::default()
        };
        change.creates_album = !self.repo.album_exists(album_key).await?;
        if change.creates_album {
            change.replacement_album_id.clear();
        }
        change.removes_empty_album = change.moves_album && target.row.is_sole_track;
        change.removes_empty_artists = self
            .repo
            .orphaned_artist_names(target, metadata, change.removes_empty_album)
            .await?;
        let mut conn = self.repo.conn().await?;
        let names: Vec<String> = metadata
            .album_artists
            .iter()
            .chain(&metadata.artists)
            .cloned()
            .collect();
        change.creates_artists = missing_names(&mut conn, &names, true).await?;
        change.creates_genres = missing_names(&mut conn, &metadata.genres, false).await?;
        Ok(change)
    }

    /// Commits a reviewed Track Replacement.
    pub async fn confirm_replacement(
        &self,
        cancel: &CancellationToken,
        job_id: &str,
        confirmation: &TrackReplacementConfirmation,
    ) -> Result<TrackReplacementResult> {
        let _batch = self.batch_lock().await;
        let _deletion = self.deletion_lock.lock().await;
        let _commit = self.commit_lock().await;
        let job = self.recovered_replacement_job(job_id).await?;
        if job.status() == STATUS_COMMITTED {
            if confirmation.revision != job.revision() - 1 {
                return Err(Error::sentinel(Sentinel::RevisionConflict));
            }
            let mut result = TrackReplacementResult {
                job_id: job.job.id.clone(),
                status: job.job.status.clone(),
                revision: job.revision(),
                track_id: job.job.track_id.clone(),
                deleted_files: 1,
            };
            let latest = self
                .repo
                .replacement_journals(JournalFilter::LatestForJob(job.id()))
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| Error::sentinel(Sentinel::NotFound))?;
            if !latest.previous_artwork_path.is_empty() {
                result.deleted_files += 1;
            }
            return Ok(result);
        }
        if job.status() == STATUS_FAILED && job.error_code() == ERROR_CODE_EXACT_DUPLICATE {
            if confirmation.revision != job.revision() {
                return Err(Error::sentinel(Sentinel::RevisionConflict));
            }
            if !job.staged_file_path.is_empty() {
                return Err(self.finish_exact_duplicate_cleanup(&job).await);
            }
            return Err(Error::sentinel(Sentinel::ExactDuplicate));
        }
        if job.status() != STATUS_AWAITING_CONFIRMATION {
            return Err(Error::sentinel(Sentinel::InvalidState));
        }
        if confirmation.revision != job.revision() {
            return Err(Error::sentinel(Sentinel::RevisionConflict));
        }
        let state = self.prepare_replacement_confirmation(cancel, &job).await?;
        if !tokens_equal(
            &state.preview.confirmation_token,
            &confirmation.confirmation_token,
        ) {
            return Err(Error::sentinel(Sentinel::ReplacementConflict));
        }
        self.commit_replacement(&job, state).await
    }

    async fn recovered_replacement_job(&self, job_id: &str) -> Result<ImportJob> {
        let job = self.repo.get_job(job_id).await?;
        if job.replace_track_id.is_empty() {
            return Err(Error::sentinel(Sentinel::NotReplacementJob));
        }
        let Some(journal) = self
            .repo
            .find_incomplete_replacement_journal(job.id())
            .await?
        else {
            return Ok(job);
        };
        self.recover_replacement(&journal).await?;
        self.repo.get_job(job_id).await
    }

    async fn prepare_replacement_confirmation(
        &self,
        cancel: &CancellationToken,
        job: &ImportJob,
    ) -> Result<ReplacementState> {
        let inspection = self.inspect_staged_job(cancel, job).await?;
        if inspection.file_sha256 != job.content_sha256 {
            let reason = "staged file changed after Import Preview";
            return Err(Error::validation(
                ValidationError::new("staged_file_changed", "file", reason),
                reason,
            ));
        }
        let (classification, _) = self
            .repo
            .classify_duplicate(&inspection.file_sha256)
            .await?;
        if classification == DUPLICATE_EXACT {
            return Err(self.reject_exact_duplicate(job).await);
        }
        let state = self
            .build_replacement_state(cancel, job, &inspection, &job.staged_file_path)
            .await?;
        self.preflight_commit(state.staged_size, &inspection)?;
        Ok(state)
    }

    pub(super) async fn commit_replacement(
        &self,
        job: &ImportJob,
        state: ReplacementState,
    ) -> Result<TrackReplacementResult> {
        let journal = self.prepare_replacement_journal(job, &state).await?;
        let placement = self.place_and_verify_replacement(&journal, &state).await?;
        self.swap_and_stream_replacement(&journal, &placement, &state)
            .await?;
        let shell_placement = PlacedFiles::paths(&placement.audio_path, &placement.artwork_path);
        let data = CommitData {
            job,
            identity: &state.identity,
            placement: &shell_placement,
            inspection: &state.inspection,
            album_key: &state.album_key,
        };
        let mut committed = self
            .repo
            .commit_replacement_data(&data, &state.target, &placement, &journal.id)
            .await;
        if committed.is_ok()
            && let Err(error) = self.after_result()
        {
            committed = Err(error);
        }
        let invalidations = match committed {
            Ok(invalidations) => invalidations,
            Err(error) => {
                let persisted = match self.repo.get_replacement_journal(&journal.id).await {
                    Ok(persisted) => persisted,
                    Err(read) => return Err(Error::join([Some(error), Some(read)]).unwrap()),
                };
                if persisted.phase == REPLACEMENT_PHASE_SWAPPED {
                    let rolled_back = self
                        .rollback_replacement_files(&journal, &placement, "database commit failed")
                        .await
                        .err();
                    return Err(Error::join([Some(error), rolled_back]).unwrap());
                }
                return Err(error);
            }
        };
        self.after_replacement_phase(REPLACEMENT_PHASE_DATABASE_COMMITTED)?;
        self.publish_queue_invalidations(&invalidations);
        self.complete_replacement(job, &journal, &placement).await
    }

    async fn prepare_replacement_journal(
        &self,
        job: &ImportJob,
        state: &ReplacementState,
    ) -> Result<ReplacementJournal> {
        let placement = &state.placement;
        let paths = self.storage.replacement_journal_paths(placement);
        let mut journal = ReplacementJournal {
            id: uuid(),
            job_id: job.id().to_owned(),
            track_id: state.target.row.track_id.clone(),
            phase: REPLACEMENT_PHASE_PREPARED.to_owned(),
            staged_file_path: job.staged_file_path.clone(),
            pending_audio_path: paths.pending_audio,
            audio_file_path: placement.audio_path.clone(),
            previous_audio_path: paths.previous_audio,
            retired_audio_path: paths.retired_audio,
            audio_sha256: state.inspection.file_sha256.clone(),
            previous_audio_sha256: state.target.row.content_sha256.clone(),
            artwork_mode: placement.artwork_mode.clone(),
            pending_artwork_path: paths.pending_artwork,
            artwork_file_path: placement.artwork_path.clone(),
            previous_artwork_path: paths.previous_artwork,
            retired_artwork_path: paths.retired_artwork,
            artwork_sha256: state.inspection.album_artwork.sha256.clone(),
            previous_album_id: state.target.row.album_id.clone(),
            ..ReplacementJournal::default()
        };
        if !journal.previous_artwork_path.is_empty() {
            journal.previous_artwork_sha256 = state.target.row.artwork_sha256.clone();
        }
        self.repo.create_replacement_journal(&journal).await?;
        self.after_replacement_phase(REPLACEMENT_PHASE_PREPARED)?;
        Ok(journal)
    }

    async fn place_and_verify_replacement(
        &self,
        journal: &ReplacementJournal,
        state: &ReplacementState,
    ) -> Result<ReplacementPlacement> {
        let handle = tokio::runtime::Handle::current();
        let repo = self.repo.clone();
        let journal_id = journal.id.clone();
        let mark = move || -> Result<()> {
            tokio::task::block_in_place(|| {
                handle.block_on(repo.mark_replacement_artwork_created(&journal_id))
            })
        };
        let placed = self.storage.place_replacement(
            state.placement.clone(),
            &state.inspection,
            &state.identity,
            Some(&mark),
        );
        let placement = match placed {
            Ok(placement) => placement,
            Err(error) => {
                let journaled = self
                    .repo
                    .rollback_replacement_journal(&journal.id, "pending placement failed")
                    .await
                    .err();
                return Err(Error::join([Some(error), journaled]).unwrap());
            }
        };
        self.advance_replacement_phase(
            journal,
            &placement,
            REPLACEMENT_PHASE_PREPARED,
            REPLACEMENT_PHASE_PLACED,
        )
        .await?;
        if let Err(error) = self.storage.verify_replacement(
            &placement,
            &journal.audio_sha256,
            &journal.artwork_sha256,
        ) {
            let rolled_back = self
                .rollback_replacement_files(journal, &placement, "pending verification failed")
                .await
                .err();
            return Err(Error::join([Some(error), rolled_back]).unwrap());
        }
        self.advance_replacement_phase(
            journal,
            &placement,
            REPLACEMENT_PHASE_PLACED,
            REPLACEMENT_PHASE_VERIFIED,
        )
        .await?;
        Ok(placement)
    }

    async fn swap_and_stream_replacement(
        &self,
        journal: &ReplacementJournal,
        placement: &ReplacementPlacement,
        state: &ReplacementState,
    ) -> Result<()> {
        if let Err(error) = self.storage.swap_replacement(placement) {
            let rolled_back = self
                .rollback_replacement_files(journal, placement, "canonical swap failed")
                .await
                .err();
            return Err(Error::join([Some(error), rolled_back]).unwrap());
        }
        self.advance_replacement_phase(
            journal,
            placement,
            REPLACEMENT_PHASE_VERIFIED,
            REPLACEMENT_PHASE_SWAPPED,
        )
        .await?;
        if let Err(error) = self
            .storage
            .stream_swapped_replacement(placement, &state.inspection.file_sha256)
        {
            let rolled_back = self
                .rollback_replacement_files(
                    journal,
                    placement,
                    "canonical stream verification failed",
                )
                .await
                .err();
            return Err(Error::join([Some(error), rolled_back]).unwrap());
        }
        Ok(())
    }

    /// Journals a durable phase, then runs the phase hook; a hook failure
    /// simulates a crash right after that phase.
    async fn advance_replacement_phase(
        &self,
        journal: &ReplacementJournal,
        placement: &ReplacementPlacement,
        from: &str,
        to: &str,
    ) -> Result<()> {
        if let Err(error) = self
            .repo
            .update_replacement_phase(&journal.id, from, to)
            .await
        {
            let rolled_back = self
                .rollback_replacement_files(journal, placement, "journal update failed")
                .await
                .err();
            return Err(Error::join([Some(error), rolled_back]).unwrap());
        }
        self.after_replacement_phase(to)
    }

    async fn rollback_replacement_files(
        &self,
        journal: &ReplacementJournal,
        placement: &ReplacementPlacement,
        reason: &str,
    ) -> Result<()> {
        if let Err(error) = self.storage.rollback_replacement(placement) {
            let recorded = self
                .repo
                .record_replacement_recovery_reason(
                    &journal.id,
                    &format!("{reason}; filesystem rollback failed"),
                )
                .await
                .err();
            return Err(Error::join([Some(error), recorded]).unwrap());
        }
        self.repo
            .rollback_replacement_journal(&journal.id, reason)
            .await
    }

    /// Runs after the database made the replacement authoritative: deletes
    /// the retired file and finishes the job.
    async fn complete_replacement(
        &self,
        job: &ImportJob,
        journal: &ReplacementJournal,
        placement: &ReplacementPlacement,
    ) -> Result<TrackReplacementResult> {
        let never = || false;
        let (deleted, removed) = self
            .storage
            .complete_replacement_files(&never, journal, placement);
        if let Err(error) = removed {
            let recorded = self
                .repo
                .record_replacement_recovery_reason(
                    &journal.id,
                    &format!("previous file cleanup failed: {error}"),
                )
                .await
                .err();
            return Err(Error::join([Some(error), recorded]).unwrap());
        }
        let mut result = self.repo.finalize_replacement(job, &journal.id).await?;
        result.deleted_files = deleted;
        Ok(result)
    }

    /// Finishes or undoes every Track Replacement a restart interrupted.
    pub async fn recover_pending_track_replacements(&self) -> Result<()> {
        let _commit = self.commit_lock().await;
        let journals = self
            .repo
            .replacement_journals(JournalFilter::AllIncomplete)
            .await?;
        let mut failures = Vec::new();
        for journal in journals {
            failures.push(
                self.recover_replacement(&journal).await.err().map(|error| {
                    error.context(format!("recover Track Replacement {:?}", journal.id))
                }),
            );
        }
        match Error::join(failures) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub(super) async fn recover_replacement(&self, journal: &ReplacementJournal) -> Result<()> {
        let placement = match self.storage.replacement_placement_from_journal(journal) {
            Ok(placement) => placement,
            Err(error) => {
                let recorded = self
                    .repo
                    .record_replacement_recovery_reason(
                        &journal.id,
                        "unsafe journaled storage path",
                    )
                    .await
                    .err();
                return Err(Error::join([Some(error), recorded]).unwrap());
            }
        };
        match journal.phase.as_str() {
            REPLACEMENT_PHASE_PREPARED
            | REPLACEMENT_PHASE_PLACED
            | REPLACEMENT_PHASE_VERIFIED
            | REPLACEMENT_PHASE_SWAPPED => {
                let reason = format!(
                    "restart rolled back replacement from {} phase",
                    journal.phase
                );
                self.rollback_replacement_files(journal, &placement, &reason)
                    .await
            }
            REPLACEMENT_PHASE_DATABASE_COMMITTED => {
                let job = self.repo.get_job(&journal.job_id).await?;
                self.complete_replacement(&job, journal, &placement)
                    .await
                    .map(|_| ())
            }
            phase => Err(Error::new(format!(
                "unsupported Track Replacement phase {phase:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_changes_encode_like_go() {
        let change = LibraryChange {
            current_album_id: "a".into(),
            ..LibraryChange::default()
        };
        assert_eq!(
            crate::gojson::to_string(&change),
            r#"{"currentAlbumId":"a","movesAlbum":false,"createsAlbum":false,"removesEmptyAlbum":false,"removesEmptyArtists":[],"createsArtists":[],"createsGenres":[]}"#
        );
        assert_eq!(optional_number(0), "");
        assert!(field_diff("x", 1, 2).is_changed);
    }
}
