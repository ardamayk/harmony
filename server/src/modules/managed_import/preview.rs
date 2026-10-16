//! Reads that classify an upload against the library and build Import and
//! Album Previews (repository_preview.go, queries/preview.sql).

use super::errors::{Error, Result, Sentinel};
use super::identity::{album_identity_key, normalize_identity, sha256_hex};
use super::model::*;
use super::repository::{Repository, context, require_one};
use crate::inspect::{AlbumArtwork, NormalizedMetadata};
use sqlx::SqliteConnection;

/// The current state of a Managed Track (`replacementTarget`). Track
/// Replacement reads the remaining fields.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct TargetRow {
    pub track_id: String,
    pub title: String,
    pub album_id: String,
    pub track_revision: i64,
    pub track_no: i64,
    pub track_total: i64,
    pub disc_no: i64,
    pub disc_total: i64,
    pub duration_ms: i64,
    pub sample_rate_hz: i64,
    pub bit_depth: i64,
    pub channel_count: i64,
    pub bitrate_kbps: i64,
    pub codec: String,
    pub container: String,
    pub album: String,
    pub year: i64,
    pub album_key: String,
    pub file_path: String,
    pub content_sha256: String,
    pub size_bytes: i64,
    pub format: String,
    pub source_revision: i64,
    pub artwork_path: String,
    pub artwork_sha256: String,
    pub artwork_type: String,
    pub is_sole_track: bool,
}

/// `readReplacementTargetRow`: a visible Managed Track or why not.
pub async fn read_target_row(conn: &mut SqliteConnection, track_id: &str) -> Result<TargetRow> {
    let row = sqlx::query!(
        r#"SELECT tracks.title, tracks.album_id, tracks.revision, tracks.track_no, tracks.track_total, tracks.disc_no,
            tracks.disc_total, tracks.duration_ms, tracks.sample_rate_hz, tracks.bit_depth, tracks.channel_count,
            tracks.bitrate_bps, tracks.codec, tracks.container, albums.title AS album_title, albums.year,
            albums.identity_key AS album_key, track_sources.source_kind, track_sources.file_path,
            track_sources.content_sha256, track_sources.size_bytes, track_sources.source_format,
            track_sources.revision AS source_revision, album_artwork.file_path AS "artwork_path?",
            album_artwork.content_sha256 AS "artwork_sha256?", album_artwork.media_type AS "artwork_type?",
            NOT EXISTS (
                SELECT 1 FROM tracks siblings
                WHERE siblings.album_id = tracks.album_id AND siblings.id != tracks.id AND siblings.missing_at IS NULL
            ) AS "is_sole_track!: bool"
        FROM tracks
        JOIN albums ON albums.id = tracks.album_id
        JOIN track_sources ON track_sources.track_id = tracks.id
        LEFT JOIN album_artwork ON album_artwork.album_id = tracks.album_id
        WHERE tracks.id = ? AND tracks.missing_at IS NULL AND tracks.is_pending_commit = 0"#,
        track_id
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(context("load Track Replacement target"))?
    .ok_or_else(|| Error::sentinel(Sentinel::TrackNotFound))?;
    if row.source_kind != "managed" {
        return Err(Error::sentinel(Sentinel::NotManagedTrack));
    }
    let disc_no = row.disc_no.unwrap_or(0);
    Ok(TargetRow {
        track_id: track_id.to_owned(),
        title: row.title,
        album_id: row.album_id,
        track_revision: row.revision,
        track_no: row.track_no.unwrap_or(0),
        track_total: row.track_total.unwrap_or(0),
        disc_no: if disc_no == 0 { 1 } else { disc_no },
        disc_total: row.disc_total.unwrap_or(0),
        duration_ms: row.duration_ms,
        sample_rate_hz: row.sample_rate_hz.unwrap_or(0),
        bit_depth: row.bit_depth.unwrap_or(0),
        channel_count: row.channel_count.unwrap_or(0),
        bitrate_kbps: row.bitrate_bps.unwrap_or(0) / BITS_PER_KILOBIT,
        codec: row.codec.unwrap_or_default(),
        container: row.container.unwrap_or_default(),
        album: row.album_title,
        year: row.year.unwrap_or(0),
        album_key: row.album_key.unwrap_or_default(),
        file_path: row.file_path,
        content_sha256: row.content_sha256.unwrap_or_default(),
        size_bytes: row.size_bytes,
        format: row.source_format,
        source_revision: row.source_revision,
        artwork_path: row.artwork_path.unwrap_or_default(),
        artwork_sha256: row.artwork_sha256.unwrap_or_default(),
        artwork_type: row.artwork_type.unwrap_or_default(),
        is_sole_track: row.is_sole_track,
    })
}

/// sqlc lists come back nil when empty, which Go encodes as null.
fn nil_if_empty(values: Vec<String>) -> Option<Vec<String>> {
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

/// `albumPreviewKey`.
pub fn album_preview_key(file: &PreviewFile) -> String {
    sha256_hex(&preview_album_key(file))
}

/// `previewAlbumKey`.
pub fn preview_album_key(file: &PreviewFile) -> String {
    super::identity::album_key_of(
        file.album_artists.as_deref().unwrap_or_default(),
        &file.album,
    )
}

impl Repository {
    pub async fn existing_album_artwork(&self, album_key: &str) -> Result<Option<AlbumArtwork>> {
        let row = sqlx::query!(
            "SELECT album_artwork.content_sha256, album_artwork.media_type, album_artwork.width, album_artwork.height
            FROM album_artwork JOIN albums ON albums.id = album_artwork.album_id
            WHERE albums.identity_key = ?",
            album_key
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(context("read existing Album artwork"))?;
        Ok(row.map(|row| AlbumArtwork {
            sha256: row.content_sha256,
            mime_type: row.media_type,
            width: row.width,
            height: row.height,
            ..AlbumArtwork::default()
        }))
    }

    pub async fn album_requires_disc_number(
        &self,
        metadata: &NormalizedMetadata,
        excluded_track_id: &str,
    ) -> Result<bool> {
        let key = album_identity_key(metadata);
        sqlx::query_scalar!(
            r#"SELECT EXISTS (
                SELECT 1 FROM albums JOIN tracks ON tracks.album_id = albums.id
                WHERE albums.identity_key = ? AND tracks.missing_at IS NULL AND tracks.id != ?
                    AND (tracks.disc_no > 1 OR tracks.disc_total > 1)
            ) AS "requires!: bool""#,
            key,
            excluded_track_id
        )
        .fetch_one(&self.pool)
        .await
        .map_err(context("inspect existing Album disc positions"))
    }

    /// The tag whose total disagrees with the Album's existing Tracks.
    pub async fn album_position_total_conflict(
        &self,
        metadata: &NormalizedMetadata,
        excluded_track_id: &str,
    ) -> Result<&'static str> {
        let key = album_identity_key(metadata);
        let row = sqlx::query!(
            r#"WITH params AS (
                SELECT CAST(?1 AS INTEGER) AS wanted_disc_total,
                    CAST(?2 AS INTEGER) AS wanted_disc_number,
                    CAST(?3 AS INTEGER) AS wanted_track_total
            ),
            matching_tracks AS (
                SELECT tracks.disc_no, tracks.disc_total, tracks.track_no, tracks.track_total
                FROM albums JOIN tracks ON tracks.album_id = albums.id
                WHERE albums.identity_key = ?4 AND tracks.missing_at IS NULL AND tracks.id != ?5
            )
            SELECT
                EXISTS (
                    SELECT 1 FROM matching_tracks m, params p
                    WHERE p.wanted_disc_total > 0
                        AND ((m.disc_total IS NOT NULL AND m.disc_total != p.wanted_disc_total) OR m.disc_no > p.wanted_disc_total)
                ) AS "has_disc_conflict!: bool",
                EXISTS (
                    SELECT 1 FROM matching_tracks m, params p
                    WHERE m.disc_no = p.wanted_disc_number AND p.wanted_track_total > 0
                        AND ((m.track_total IS NOT NULL AND m.track_total != p.wanted_track_total) OR m.track_no > p.wanted_track_total)
                ) AS "has_track_conflict!: bool""#,
            metadata.disc_position.total,
            metadata.disc_position.number,
            metadata.track_position.total,
            key,
            excluded_track_id
        )
        .fetch_one(&self.pool)
        .await
        .map_err(context("inspect existing Album position totals"))?;
        Ok(if row.has_disc_conflict {
            "TOTALDISCS"
        } else if row.has_track_conflict {
            "TOTALTRACKS"
        } else {
            ""
        })
    }

    /// `ClassifyDuplicate`: exact when a Track has these bytes.
    pub async fn classify_duplicate(
        &self,
        file_sha256: &str,
    ) -> Result<(&'static str, Vec<DuplicateCandidate>)> {
        let track_id = sqlx::query_scalar!(
            r#"SELECT tracks.id AS "id!" FROM track_sources JOIN tracks ON tracks.id = track_sources.track_id
            WHERE track_sources.content_sha256 = ?"#,
            file_sha256
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(context("inspect exact Managed Import duplicate"))?;
        let Some(track_id) = track_id else {
            return Ok((DUPLICATE_NONE, Vec::new()));
        };
        let candidate = self.read_duplicate_candidate(&track_id).await?;
        Ok((DUPLICATE_EXACT, vec![candidate]))
    }

    /// Tracks already at the file's disc and track position in its Album.
    pub async fn position_candidates(
        &self,
        metadata: &NormalizedMetadata,
        excluded_track_id: &str,
    ) -> Result<Vec<DuplicateCandidate>> {
        let key = album_identity_key(metadata);
        let ids = sqlx::query_scalar!(
            r#"SELECT tracks.id AS "id!" FROM tracks JOIN albums ON albums.id = tracks.album_id
            WHERE tracks.missing_at IS NULL AND tracks.id != ? AND albums.identity_key = ?
                AND COALESCE(tracks.disc_no, 1) = ? AND tracks.track_no = ?
            ORDER BY tracks.id"#,
            excluded_track_id,
            key,
            metadata.disc_position.number,
            metadata.track_position.number
        )
        .fetch_all(&self.pool)
        .await
        .map_err(context(
            "inspect possible Managed Import position duplicates",
        ))?;
        self.read_duplicate_candidates(&ids).await
    }

    async fn read_duplicate_candidates(&self, ids: &[String]) -> Result<Vec<DuplicateCandidate>> {
        let mut candidates = Vec::with_capacity(ids.len());
        for id in ids {
            candidates.push(self.read_duplicate_candidate(id).await?);
        }
        Ok(candidates)
    }

    async fn read_duplicate_candidate(&self, track_id: &str) -> Result<DuplicateCandidate> {
        let mut conn = self.conn().await?;
        let row = sqlx::query!(
            r#"SELECT tracks.id AS "id!", tracks.title, albums.title AS album_title,
                COALESCE(tracks.disc_no, 1) AS "disc_no!: i64", tracks.track_no, tracks.format,
                tracks.duration_ms, tracks.revision
            FROM tracks JOIN albums ON albums.id = tracks.album_id
            WHERE tracks.id = ?"#,
            track_id
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(context("read Managed Import duplicate candidate"))?;
        let artists = track_artist_names(&mut conn, track_id)
            .await
            .map_err(|error| error.context("read duplicate candidate Artists"))?;
        let mut candidate = DuplicateCandidate {
            revision: row.revision,
            title_key: normalize_identity(&row.title),
            current_file: None,
            track_id: row.id,
            title: row.title,
            artists: nil_if_empty(artists),
            album: row.album_title,
            disc_no: row.disc_no,
            track_no: row.track_no.unwrap_or(0),
            format: row.format,
            duration_ms: row.duration_ms,
        };
        let target = match read_target_row(&mut conn, track_id).await {
            Err(error) if error.is(Sentinel::TrackNotFound) => return Ok(candidate),
            other => other?,
        };
        let genres = sqlx::query_scalar!(
            "SELECT genres.name FROM track_genres JOIN genres ON genres.id = track_genres.genre_id
            WHERE track_genres.track_id = ? ORDER BY track_genres.position",
            track_id
        )
        .fetch_all(&mut *conn)
        .await
        .map_err(context("list Track Replacement names"))?;
        let album_artists = album_artist_names(&mut conn, &target.album_id)
            .await
            .map_err(|error| error.context("list Track Replacement names"))?;
        candidate.current_file = Some(Box::new(PreviewFile {
            original_filename: format!("{}.{}", target.title, target.format),
            title: target.title,
            title_key: candidate.title_key.clone(),
            artists: candidate.artists.clone(),
            album_artists: nil_if_empty(album_artists),
            album: target.album,
            genres: nil_if_empty(genres),
            year: target.year,
            track_no: target.track_no,
            track_total: target.track_total,
            disc_no: target.disc_no,
            disc_total: target.disc_total,
            format: target.format,
            codec: target.codec,
            container: target.container,
            duration_ms: target.duration_ms,
            size_bytes: target.size_bytes,
            sample_rate_hz: target.sample_rate_hz,
            channel_count: target.channel_count,
            bit_depth: target.bit_depth,
            bitrate_kbps: target.bitrate_kbps,
            artwork_media_type: target.artwork_type,
            ..PreviewFile::default()
        }));
        Ok(candidate)
    }

    pub async fn album_preview(&self, batch_id: &str, file: &PreviewFile) -> Result<AlbumPreview> {
        let key = album_preview_key(file);
        let existing_albums = self.match_albums(&preview_album_key(file)).await?;
        let artworks = self.list_artwork_options(batch_id, &key).await?;
        Ok(AlbumPreview {
            key,
            title: file.album.clone(),
            album_artists: file.album_artists.clone(),
            existing_albums,
            artworks,
        })
    }

    /// The Album with this key and every separate edition created from it.
    async fn match_albums(&self, key: &str) -> Result<Vec<AlbumMatch>> {
        let prefix = format!("{key}\u{1f}managed-import-edition:");
        let rows = sqlx::query!(
            r#"WITH params AS (SELECT CAST(?1 AS TEXT) AS album_key, CAST(?2 AS TEXT) AS prefix)
            SELECT albums.id AS "id!", albums.identity_key, COALESCE(albums.year, 0) AS "year!: i64",
                EXISTS(SELECT 1 FROM album_artwork WHERE album_artwork.album_id = albums.id) AS "has_artwork!: bool"
            FROM albums, params p
            WHERE albums.identity_key = p.album_key OR substr(albums.identity_key, 1, length(p.prefix)) = p.prefix
            ORDER BY albums.identity_key, albums.id"#,
            key,
            prefix
        )
        .fetch_all(&self.pool)
        .await
        .map_err(context("match import Albums"))?;
        let mut matches = Vec::with_capacity(rows.len());
        for row in rows {
            let ids = sqlx::query_scalar!(
                r#"SELECT id AS "id!" FROM visible_tracks WHERE album_id = ? ORDER BY disc_no, track_no, id"#,
                row.id
            )
            .fetch_all(&self.pool)
            .await
            .map_err(context("list import Album tracks"))?;
            matches.push(AlbumMatch {
                tracks: self.read_duplicate_candidates(&ids).await?,
                id: row.id,
                year: row.year,
                has_artwork: row.has_artwork,
                identity_key: row.identity_key.unwrap_or_default(),
            });
        }
        Ok(matches)
    }

    pub async fn save_artwork(
        &self,
        batch_id: &str,
        id: &str,
        job_id: &str,
        album_key: &str,
        artwork: &AlbumArtwork,
    ) -> Result<()> {
        let saved = sqlx::query!(
            "INSERT INTO managed_import_artwork (batch_id, id, job_id, album_key, media_type, width, height, content_sha256, data)
            SELECT batches.id, ?1, NULLIF(CAST(?2 AS TEXT), ''), ?3, ?4, ?5, ?6, ?7, ?8
            FROM managed_import_batches batches WHERE batches.id = ?9 AND batches.status = 'uploading'
            ON CONFLICT(batch_id, id) DO UPDATE SET media_type = excluded.media_type, width = excluded.width,
                height = excluded.height, content_sha256 = excluded.content_sha256, data = excluded.data",
            id,
            job_id,
            album_key,
            artwork.mime_type,
            artwork.width,
            artwork.height,
            artwork.sha256,
            artwork.data,
            batch_id
        )
        .execute(&self.pool)
        .await
        .map_err(context("stage import Album artwork"))?
        .rows_affected();
        require_one(saved)
    }

    /// Each distinct staged image once, uploaded covers first.
    async fn list_artwork_options(
        &self,
        batch_id: &str,
        album_key: &str,
    ) -> Result<Vec<ArtworkOption>> {
        let rows = sqlx::query!(
            r#"SELECT id AS "id!", COALESCE(job_id, '') AS "job_id!: String", media_type, content_sha256
            FROM managed_import_artwork WHERE batch_id = ? AND album_key = ?
            ORDER BY (job_id IS NULL) DESC, id"#,
            batch_id,
            album_key
        )
        .fetch_all(&self.pool)
        .await
        .map_err(context("list import artwork"))?;
        let mut seen = std::collections::HashSet::new();
        Ok(rows
            .into_iter()
            .filter(|row| seen.insert(row.content_sha256.clone()))
            .map(|row| ArtworkOption {
                id: row.id,
                job_id: row.job_id,
                media_type: row.media_type,
                content_sha256: row.content_sha256,
            })
            .collect())
    }

    pub async fn load_artwork(&self, batch_id: &str, id: &str) -> Result<AlbumArtwork> {
        let row = sqlx::query!(
            "SELECT media_type, width, height, content_sha256, data FROM managed_import_artwork WHERE batch_id = ? AND id = ?",
            batch_id,
            id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(context("load staged import artwork"))?
        .ok_or_else(|| Error::sentinel(Sentinel::NotFound))?;
        Ok(AlbumArtwork {
            mime_type: row.media_type,
            width: row.width,
            height: row.height,
            sha256: row.content_sha256,
            data: row.data,
            warning: String::new(),
        })
    }
}

pub async fn track_artist_names(
    conn: &mut SqliteConnection,
    track_id: &str,
) -> Result<Vec<String>> {
    sqlx::query_scalar!(
        "SELECT artists.name FROM track_artists JOIN artists ON artists.id = track_artists.artist_id
        WHERE track_artists.track_id = ? ORDER BY track_artists.position",
        track_id
    )
    .fetch_all(conn)
    .await
    .map_err(Error::from)
}

pub async fn album_artist_names(
    conn: &mut SqliteConnection,
    album_id: &str,
) -> Result<Vec<String>> {
    sqlx::query_scalar!(
        "SELECT artists.name FROM album_artists JOIN artists ON artists.id = album_artists.artist_id
        WHERE album_artists.album_id = ? ORDER BY album_artists.position",
        album_id
    )
    .fetch_all(conn)
    .await
    .map_err(Error::from)
}
