//! Library reads (library/repository.go). Every Album and Track carries its
//! ordered Artist credits, Genres, release identifiers and artwork. Lists of
//! IDs bind as one JSON array read through `json_each`, so the queries stay
//! compile-time checked.

use super::genres::decode_genres;
use super::types::*;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::collections::HashMap;

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("not found")]
    NotFound,
    #[error("{0}: {1}")]
    Database(String, sqlx::Error),
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
}

trait Context<T> {
    fn context(self, what: impl FnOnce() -> String) -> Result<T, LibraryError>;
}

impl<T> Context<T> for Result<T, sqlx::Error> {
    fn context(self, what: impl FnOnce() -> String) -> Result<T, LibraryError> {
        self.map_err(|error| LibraryError::Database(what(), error))
    }
}

/// An Album's original cover and the SHA-256 of its bytes, which identifies
/// it for scaled-down variants.
#[derive(Clone, Debug, PartialEq)]
pub struct AlbumCover {
    pub media_type: String,
    pub data: Vec<u8>,
    pub source_sha256: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WaveformSource {
    pub file_path: String,
    pub duration_ms: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WaveformRecord {
    pub peaks: Vec<u8>,
    pub source_size_bytes: i64,
    pub source_modified_at: i64,
}

#[derive(Clone)]
pub struct Repository {
    pool: SqlitePool,
}

struct TrackRow {
    id: String,
    title: String,
    artist_name: String,
    album_id: String,
    album_title: String,
    track_no: Option<i64>,
    disc_no: i64,
    track_total: Option<i64>,
    disc_total: Option<i64>,
    duration_ms: i64,
    format: String,
    size_bytes: i64,
    genre: String,
    sample_rate_hz: i64,
    bit_depth: i64,
    channel_count: i64,
    bitrate_bps: i64,
    codec: Option<String>,
    container: Option<String>,
    sample_format: Option<String>,
    file_path: String,
    replaygain_track_gain_db: Option<f64>,
    replaygain_track_peak: Option<f64>,
    replaygain_album_gain_db: Option<f64>,
    replaygain_album_peak: Option<f64>,
    title_sort: String,
    file_mtime: i64,
    content_sha256: Option<String>,
    identity_key: Option<String>,
    revision: i64,
    created_at: String,
    updated_at: String,
}

/// `SELECT <track columns> FROM library_track_reads t <tail>` into
/// [`TrackRow`]. The columns spell out their nullability: sqlx cannot infer
/// it through the view's COALESCEs.
macro_rules! select_tracks {
    ($tail:literal $(, $argument:expr)* $(,)?) => {
        sqlx::query_as!(
            TrackRow,
            "SELECT " + r#"t.id AS "id!", t.title AS "title!", t.artist_name AS "artist_name!",
           t.album_id AS "album_id!", t.album_title AS "album_title!", t.track_no,
           t.disc_no AS "disc_no!: i64", t.track_total, t.disc_total,
           t.duration_ms AS "duration_ms!: i64", t.format AS "format!",
           t.size_bytes AS "size_bytes!: i64", t.genre AS "genre!",
           t.sample_rate_hz AS "sample_rate_hz!: i64", t.bit_depth AS "bit_depth!: i64",
           t.channel_count AS "channel_count!: i64", t.bitrate_bps AS "bitrate_bps!: i64",
           t.codec, t.container, t.sample_format, t.file_path AS "file_path!",
           t.replaygain_track_gain_db, t.replaygain_track_peak,
           t.replaygain_album_gain_db, t.replaygain_album_peak,
           t.title_sort AS "title_sort!", t.file_mtime AS "file_mtime!: i64",
           t.content_sha256, t.identity_key, t.revision AS "revision!: i64",
           t.created_at AS "created_at!: String", t.updated_at AS "updated_at!: String""# + " FROM library_track_reads t " + $tail
            $(, $argument)*
        )
    };
}

fn ids_json(ids: &[String]) -> String {
    serde_json::to_string(ids).expect("ids encode")
}

impl Repository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list_artists(
        &self,
        limit: i64,
        offset: i64,
        q: &str,
    ) -> Result<Page<Artist>, LibraryError> {
        let total = sqlx::query_scalar!(
            r#"WITH active_artist_albums AS (
                SELECT album_artists.artist_id, album_artists.album_id FROM album_artists
                WHERE EXISTS (SELECT 1 FROM visible_tracks WHERE visible_tracks.album_id = album_artists.album_id)
            )
            SELECT COUNT(DISTINCT a.id) AS "total!: i64" FROM artists a
            INNER JOIN active_artist_albums credits ON credits.artist_id = a.id
            WHERE CAST(?1 AS TEXT) = '' OR a.name LIKE '%' || CAST(?1 AS TEXT) || '%'"#,
            q
        )
        .fetch_one(&self.pool)
        .await
        .context(|| format!("count Artists for query {q:?}"))?;
        let rows = sqlx::query!(
            r#"WITH params AS (SELECT CAST(?1 AS TEXT) AS q),
            active_artist_albums AS (
                SELECT album_artists.artist_id, album_artists.album_id FROM album_artists
                WHERE EXISTS (SELECT 1 FROM visible_tracks WHERE visible_tracks.album_id = album_artists.album_id)
            )
            SELECT a.id AS "id!", a.name AS "name!", COUNT(credits.album_id) AS "album_count!: i64"
            FROM params p, artists a
            INNER JOIN active_artist_albums credits ON credits.artist_id = a.id
            WHERE p.q = '' OR a.name LIKE '%' || p.q || '%'
            GROUP BY a.id, a.name
            ORDER BY (p.q != '' AND a.name = p.q COLLATE NOCASE) DESC, a.name_sort, a.id
            LIMIT ?2 OFFSET ?3"#,
            q,
            limit,
            offset
        )
        .fetch_all(&self.pool)
        .await
        .context(|| format!("list Artists for query {q:?}"))?;
        Ok(Page {
            items: rows
                .into_iter()
                .map(|row| Artist {
                    id: row.id,
                    name: row.name,
                    album_count: row.album_count,
                })
                .collect(),
            total,
        })
    }

    pub async fn list_albums(
        &self,
        limit: i64,
        offset: i64,
        artist_id: &str,
        q: &str,
    ) -> Result<Page<Album>, LibraryError> {
        let what = || format!("Albums for Artist {artist_id:?} and query {q:?}");
        let total = sqlx::query_scalar!(
            r#"SELECT COUNT(DISTINCT al.id) AS "total!: i64" FROM albums al
            INNER JOIN visible_tracks t ON t.album_id = al.id
            WHERE (CAST(?1 AS TEXT) = '' OR EXISTS (
                    SELECT 1 FROM album_artists credit WHERE credit.album_id = al.id AND credit.artist_id = CAST(?1 AS TEXT)))
              AND (CAST(?2 AS TEXT) = '' OR al.title LIKE '%' || CAST(?2 AS TEXT) || '%' OR EXISTS (
                    SELECT 1 FROM album_artists credit
                    INNER JOIN artists credited ON credited.id = credit.artist_id
                    WHERE credit.album_id = al.id AND credited.name LIKE '%' || CAST(?2 AS TEXT) || '%'))"#,
            artist_id,
            q
        )
        .fetch_one(&self.pool)
        .await
        .context(|| format!("count {}", what()))?;
        let rows = sqlx::query!(
            r#"WITH params AS (SELECT CAST(?1 AS TEXT) AS artist_id, CAST(?2 AS TEXT) AS q)
            SELECT al.id AS "id!", al.title AS "title!",
                COALESCE(primary_artist.id, legacy_artist.id) AS "artist_id!: String",
                COALESCE(primary_artist.name, legacy_artist.name) AS "artist_name!: String",
                al.year AS "year?: i64", al.release_date AS "release_date?: String",
                al.genres AS "genres!", COUNT(t.id) AS "track_count!: i64"
            FROM params p, albums al
            INNER JOIN artists legacy_artist ON legacy_artist.id = al.artist_id
            LEFT JOIN album_artists primary_credit ON primary_credit.album_id = al.id AND primary_credit.position = 0
            LEFT JOIN artists primary_artist ON primary_artist.id = primary_credit.artist_id
            INNER JOIN visible_tracks t ON t.album_id = al.id
            WHERE (p.artist_id = '' OR EXISTS (
                    SELECT 1 FROM album_artists credit WHERE credit.album_id = al.id AND credit.artist_id = p.artist_id))
              AND (p.q = '' OR al.title LIKE '%' || p.q || '%' OR EXISTS (
                    SELECT 1 FROM album_artists credit
                    INNER JOIN artists credited ON credited.id = credit.artist_id
                    WHERE credit.album_id = al.id AND credited.name LIKE '%' || p.q || '%'))
            GROUP BY al.id, al.title, primary_artist.id, primary_artist.name,
                legacy_artist.id, legacy_artist.name, al.year, al.release_date, al.genres
            ORDER BY
                CASE WHEN p.artist_id != '' THEN al.year IS NULL END,
                CASE WHEN p.artist_id != '' THEN al.year END DESC,
                al.title_sort
            LIMIT ?3 OFFSET ?4"#,
            artist_id,
            q,
            limit,
            offset
        )
        .fetch_all(&self.pool)
        .await
        .context(|| format!("list {}", what()))?;
        let mut items: Vec<Album> = rows
            .into_iter()
            .map(|row| {
                let mut album = to_album(
                    row.id,
                    row.title,
                    row.artist_id,
                    row.artist_name,
                    row.year,
                    row.release_date,
                    &row.genres,
                );
                album.track_count = row.track_count;
                album
            })
            .collect();
        self.enrich_albums(&mut items)
            .await
            .map_err(|error| wrap(error, || format!("enrich Album list for {}", what())))?;
        Ok(Page { items, total })
    }

    pub async fn get_album(&self, album_id: &str) -> Result<AlbumDetail, LibraryError> {
        let row = sqlx::query!(
            r#"SELECT al.id AS "id!", al.title AS "title!",
                COALESCE(primary_artist.id, legacy_artist.id) AS "artist_id!: String",
                COALESCE(primary_artist.name, legacy_artist.name) AS "artist_name!: String",
                al.year AS "year?: i64", al.release_date AS "release_date?: String", al.genres AS "genres!"
            FROM albums al
            INNER JOIN artists legacy_artist ON legacy_artist.id = al.artist_id
            LEFT JOIN album_artists primary_credit ON primary_credit.album_id = al.id AND primary_credit.position = 0
            LEFT JOIN artists primary_artist ON primary_artist.id = primary_credit.artist_id
            WHERE al.id = ?"#,
            album_id
        )
        .fetch_optional(&self.pool)
        .await
        .context(|| format!("get Album {album_id:?}"))?
        .ok_or(LibraryError::NotFound)?;
        let track_rows = select_tracks!(
            "WHERE t.album_id = ?
            ORDER BY t.disc_no, COALESCE(t.track_no, 9999), t.title_sort, t.id",
            album_id
        )
        .fetch_all(&self.pool)
        .await
        .context(|| format!("list Album {album_id:?} Tracks"))?;
        let mut albums = vec![to_album(
            row.id,
            row.title,
            row.artist_id,
            row.artist_name,
            row.year,
            row.release_date,
            &row.genres,
        )];
        self.enrich_albums(&mut albums)
            .await
            .map_err(|error| wrap(error, || format!("enrich Album {album_id:?}")))?;
        let mut tracks: Vec<Track> = track_rows.into_iter().map(to_track).collect();
        self.enrich_tracks(&mut tracks)
            .await
            .map_err(|error| wrap(error, || format!("enrich Album {album_id:?} Tracks")))?;
        let mut album = albums.remove(0);
        album.track_count = tracks.len() as i64;
        Ok(AlbumDetail { album, tracks })
    }

    pub async fn list_tracks(
        &self,
        limit: i64,
        offset: i64,
        artist_id: &str,
        q: &str,
    ) -> Result<Page<Track>, LibraryError> {
        let total = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "total!: i64" FROM library_track_reads t
            WHERE (CAST(?1 AS TEXT) = '' OR EXISTS (
                    SELECT 1 FROM track_artists credit WHERE credit.track_id = t.id AND credit.artist_id = CAST(?1 AS TEXT)
                ) OR EXISTS (
                    SELECT 1 FROM album_artists credit WHERE credit.album_id = t.album_id AND credit.artist_id = CAST(?1 AS TEXT)))
              AND (CAST(?2 AS TEXT) = '' OR t.title LIKE '%' || CAST(?2 AS TEXT) || '%' OR t.album_title LIKE '%' || CAST(?2 AS TEXT) || '%' OR EXISTS (
                    SELECT 1 FROM track_artists credit
                    INNER JOIN artists credited ON credited.id = credit.artist_id
                    WHERE credit.track_id = t.id AND credited.name LIKE '%' || CAST(?2 AS TEXT) || '%'
                ) OR EXISTS (
                    SELECT 1 FROM track_genres relation
                    INNER JOIN genres genre ON genre.id = relation.genre_id
                    WHERE relation.track_id = t.id AND genre.name LIKE '%' || CAST(?2 AS TEXT) || '%'))"#,
            artist_id,
            q
        )
        .fetch_one(&self.pool)
        .await
        .context(|| format!("count Tracks for query {q:?}"))?;
        let rows = select_tracks!(
            "WHERE (CAST(?1 AS TEXT) = '' OR EXISTS (
                    SELECT 1 FROM track_artists credit WHERE credit.track_id = t.id AND credit.artist_id = CAST(?1 AS TEXT)
                ) OR EXISTS (
                    SELECT 1 FROM album_artists credit WHERE credit.album_id = t.album_id AND credit.artist_id = CAST(?1 AS TEXT)))
              AND (CAST(?2 AS TEXT) = '' OR t.title LIKE '%' || CAST(?2 AS TEXT) || '%' OR t.album_title LIKE '%' || CAST(?2 AS TEXT) || '%' OR EXISTS (
                    SELECT 1 FROM track_artists credit
                    INNER JOIN artists credited ON credited.id = credit.artist_id
                    WHERE credit.track_id = t.id AND credited.name LIKE '%' || CAST(?2 AS TEXT) || '%'
                ) OR EXISTS (
                    SELECT 1 FROM track_genres relation
                    INNER JOIN genres genre ON genre.id = relation.genre_id
                    WHERE relation.track_id = t.id AND genre.name LIKE '%' || CAST(?2 AS TEXT) || '%'))
            ORDER BY t.title_sort
            LIMIT ?3 OFFSET ?4",
            artist_id,
            q,
            limit,
            offset
        )
        .fetch_all(&self.pool)
        .await
        .context(|| format!("list Tracks for query {q:?}"))?;
        let mut items: Vec<Track> = rows.into_iter().map(to_track).collect();
        self.enrich_tracks(&mut items)
            .await
            .map_err(|error| wrap(error, || format!("enrich Track list for query {q:?}")))?;
        Ok(Page { items, total })
    }

    pub async fn get_track(&self, track_id: &str) -> Result<Track, LibraryError> {
        let row = select_tracks!("WHERE t.id = ?", track_id)
            .fetch_optional(&self.pool)
            .await
            .context(|| format!("get Track {track_id:?}"))?
            .ok_or(LibraryError::NotFound)?;
        let mut tracks = vec![to_track(row)];
        self.enrich_tracks(&mut tracks)
            .await
            .map_err(|error| wrap(error, || format!("enrich Track {track_id:?}")))?;
        Ok(tracks.remove(0))
    }

    /// The stored lyrics; empty when the file carried none.
    pub async fn get_track_lyrics(&self, track_id: &str) -> Result<String, LibraryError> {
        sqlx::query_scalar!(
            r#"SELECT lyrics AS "lyrics!" FROM tracks WHERE id = ?"#,
            track_id
        )
        .fetch_optional(&self.pool)
        .await
        .context(|| format!("get Track lyrics {track_id:?}"))?
        .ok_or(LibraryError::NotFound)
    }

    pub async fn get_track_waveform_source(
        &self,
        track_id: &str,
    ) -> Result<WaveformSource, LibraryError> {
        let row = sqlx::query!(
            r#"SELECT file_path AS "file_path!", duration_ms AS "duration_ms!: i64"
            FROM library_track_reads WHERE id = ?"#,
            track_id
        )
        .fetch_optional(&self.pool)
        .await
        .context(|| format!("get Track source {track_id:?}"))?
        .ok_or(LibraryError::NotFound)?;
        Ok(WaveformSource {
            file_path: row.file_path,
            duration_ms: row.duration_ms,
        })
    }

    pub async fn get_track_file_path(&self, track_id: &str) -> Result<String, LibraryError> {
        Ok(self.get_track_waveform_source(track_id).await?.file_path)
    }

    /// Managed Album Artwork first, then a cover embedded in the album row.
    pub async fn get_album_cover_source(&self, album_id: &str) -> Result<AlbumCover, LibraryError> {
        let artwork = sqlx::query!(
            r#"SELECT media_type AS "media_type!", file_path AS "file_path!", content_sha256 AS "content_sha256!"
            FROM visible_album_artwork WHERE album_id = ?"#,
            album_id
        )
        .fetch_optional(&self.pool)
        .await
        .context(|| format!("get Album Artwork {album_id:?}"))?;
        if let Some(artwork) = artwork {
            let data = tokio::fs::read(&artwork.file_path).await.map_err(|error| {
                LibraryError::Io(format!("read Album Artwork {album_id:?}"), error)
            })?;
            return Ok(AlbumCover {
                media_type: artwork.media_type,
                data,
                source_sha256: artwork.content_sha256,
            });
        }
        let cover = sqlx::query!(
            "SELECT cover_mime, cover_data FROM albums WHERE id = ?",
            album_id
        )
        .fetch_optional(&self.pool)
        .await
        .context(|| format!("get embedded cover {album_id:?}"))?
        .ok_or(LibraryError::NotFound)?;
        let data = cover.cover_data.unwrap_or_default();
        if data.is_empty() {
            return Err(LibraryError::NotFound);
        }
        let media_type = cover
            .cover_mime
            .filter(|mime| !mime.is_empty())
            .unwrap_or_else(|| "image/jpeg".to_owned());
        let source_sha256 = hex::encode(Sha256::digest(&data));
        Ok(AlbumCover {
            media_type,
            data,
            source_sha256,
        })
    }

    pub async fn get_album_cover_variant(
        &self,
        album_id: &str,
        size: i64,
    ) -> Result<Option<AlbumCover>, LibraryError> {
        let row = sqlx::query!(
            "SELECT source_sha256, media_type, data FROM album_cover_variants WHERE album_id = ? AND size = ?",
            album_id,
            size
        )
        .fetch_optional(&self.pool)
        .await
        .context(|| format!("get cover variant {album_id:?} at {size}"))?;
        Ok(row.map(|row| AlbumCover {
            media_type: row.media_type,
            data: row.data,
            source_sha256: row.source_sha256,
        }))
    }

    pub async fn put_album_cover_variant(
        &self,
        album_id: &str,
        size: i64,
        variant: &AlbumCover,
    ) -> Result<(), LibraryError> {
        sqlx::query!(
            "INSERT INTO album_cover_variants (album_id, size, source_sha256, media_type, data, generated_at)
            VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
            ON CONFLICT(album_id, size) DO UPDATE SET
                source_sha256 = excluded.source_sha256,
                media_type = excluded.media_type,
                data = excluded.data,
                generated_at = CURRENT_TIMESTAMP",
            album_id,
            size,
            variant.source_sha256,
            variant.media_type,
            variant.data
        )
        .execute(&self.pool)
        .await
        .context(|| format!("put cover variant {album_id:?} at {size}"))?;
        Ok(())
    }

    pub async fn get_track_waveform(
        &self,
        track_id: &str,
    ) -> Result<Option<WaveformRecord>, LibraryError> {
        let row = sqlx::query!(
            "SELECT peaks, source_size_bytes, source_modified_at FROM track_waveforms WHERE track_id = ?",
            track_id
        )
        .fetch_optional(&self.pool)
        .await
        .context(|| format!("get waveform {track_id:?}"))?;
        Ok(row.map(|row| WaveformRecord {
            peaks: row.peaks,
            source_size_bytes: row.source_size_bytes,
            source_modified_at: row.source_modified_at,
        }))
    }

    pub async fn put_track_waveform(
        &self,
        track_id: &str,
        record: &WaveformRecord,
    ) -> Result<(), LibraryError> {
        let peak_count = record.peaks.len() as i64;
        sqlx::query!(
            "INSERT INTO track_waveforms (track_id, peak_count, peaks, source_size_bytes, source_modified_at, generated_at)
            VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
            ON CONFLICT(track_id) DO UPDATE SET
                peak_count = excluded.peak_count,
                peaks = excluded.peaks,
                source_size_bytes = excluded.source_size_bytes,
                source_modified_at = excluded.source_modified_at,
                generated_at = CURRENT_TIMESTAMP",
            track_id,
            peak_count,
            record.peaks,
            record.source_size_bytes,
            record.source_modified_at
        )
        .execute(&self.pool)
        .await
        .context(|| format!("put waveform {track_id:?}"))?;
        Ok(())
    }

    async fn enrich_albums(&self, albums: &mut [Album]) -> Result<(), LibraryError> {
        if albums.is_empty() {
            return Ok(());
        }
        let ids: Vec<String> = albums.iter().map(|album| album.id.clone()).collect();
        let ids_json = ids_json(&ids);
        let mut credits = self.album_artist_credits(&ids_json, &ids).await?;
        let mut genres: HashMap<String, Vec<Genre>> = HashMap::new();
        for row in sqlx::query!(
            r#"SELECT tracks.album_id AS "album_id!", genres.id AS "id!", genres.name AS "name!"
            FROM visible_tracks tracks
            INNER JOIN track_genres relations ON relations.track_id = tracks.id
            INNER JOIN genres ON genres.id = relations.genre_id
            WHERE tracks.album_id IN (SELECT value FROM json_each(?))
            GROUP BY tracks.album_id, genres.id, genres.name
            ORDER BY tracks.album_id, MIN(relations.position), genres.name"#,
            ids_json
        )
        .fetch_all(&self.pool)
        .await
        .context(|| format!("read Album Genres for IDs {ids:?}"))?
        {
            genres.entry(row.album_id).or_default().push(Genre {
                id: row.id,
                name: row.name,
            });
        }
        let mut identifiers: HashMap<String, Vec<ReleaseIdentifier>> = HashMap::new();
        for row in sqlx::query!(
            "SELECT album_id, scheme, value FROM album_release_identifiers
            WHERE album_id IN (SELECT value FROM json_each(?))
            ORDER BY album_id, scheme",
            ids_json
        )
        .fetch_all(&self.pool)
        .await
        .context(|| format!("read Album release identifiers for IDs {ids:?}"))?
        {
            identifiers
                .entry(row.album_id)
                .or_default()
                .push(ReleaseIdentifier {
                    scheme: row.scheme,
                    value: row.value,
                });
        }
        let mut artwork: HashMap<String, AlbumArtworkMetadata> = HashMap::new();
        for row in sqlx::query!(
            r#"SELECT album_id AS "album_id!", source_track_id AS "source_track_id!",
                content_sha256 AS "content_sha256!", media_type AS "media_type!",
                width AS "width!: i64", height AS "height!: i64",
                encoded_size_bytes AS "encoded_size_bytes!: i64"
            FROM visible_album_artwork
            WHERE album_id IN (SELECT value FROM json_each(?))"#,
            ids_json
        )
        .fetch_all(&self.pool)
        .await
        .context(|| format!("read Album Artwork for IDs {ids:?}"))?
        {
            artwork.insert(
                row.album_id,
                AlbumArtworkMetadata {
                    source_track_id: row.source_track_id,
                    content_sha256: row.content_sha256,
                    media_type: row.media_type,
                    width: row.width,
                    height: row.height,
                    size_bytes: row.encoded_size_bytes,
                },
            );
        }
        for album in albums.iter_mut() {
            album.album_artists = credits.remove(&album.id).unwrap_or_default();
            if album.album_artists.is_empty() {
                album.album_artists = vec![ArtistCredit {
                    id: album.artist_id.clone(),
                    name: album.artist_name.clone(),
                }];
            }
            album.artist_id = album.album_artists[0].id.clone();
            album.artist_name = album.album_artists[0].name.clone();
            album.genre_items = genres.remove(&album.id).unwrap_or_default();
            album.release_identifiers = identifiers.remove(&album.id).unwrap_or_default();
            album.artwork = artwork.remove(&album.id);
        }
        Ok(())
    }

    async fn enrich_tracks(&self, tracks: &mut [Track]) -> Result<(), LibraryError> {
        if tracks.is_empty() {
            return Ok(());
        }
        let track_ids: Vec<String> = tracks.iter().map(|track| track.id.clone()).collect();
        let mut album_ids: Vec<String> = Vec::new();
        for track in tracks.iter() {
            if !album_ids.contains(&track.album_id) {
                album_ids.push(track.album_id.clone());
            }
        }
        let track_ids_json = ids_json(&track_ids);
        let mut artists: HashMap<String, Vec<ArtistCredit>> = HashMap::new();
        for row in sqlx::query!(
            r#"SELECT credits.track_id AS "track_id!", artists.id AS "id!", artists.name AS "name!"
            FROM track_artists credits
            INNER JOIN artists ON artists.id = credits.artist_id
            WHERE credits.track_id IN (SELECT value FROM json_each(?))
            ORDER BY credits.track_id, credits.position"#,
            track_ids_json
        )
        .fetch_all(&self.pool)
        .await
        .context(|| format!("read Track Artists for IDs {track_ids:?}"))?
        {
            artists.entry(row.track_id).or_default().push(ArtistCredit {
                id: row.id,
                name: row.name,
            });
        }
        let album_artists = self
            .album_artist_credits(&ids_json(&album_ids), &album_ids)
            .await?;
        let mut genres: HashMap<String, Vec<Genre>> = HashMap::new();
        for row in sqlx::query!(
            r#"SELECT relations.track_id AS "track_id!", genres.id AS "id!", genres.name AS "name!"
            FROM track_genres relations
            INNER JOIN genres ON genres.id = relations.genre_id
            WHERE relations.track_id IN (SELECT value FROM json_each(?))
            ORDER BY relations.track_id, relations.position"#,
            track_ids_json
        )
        .fetch_all(&self.pool)
        .await
        .context(|| format!("read Track Genres for IDs {track_ids:?}"))?
        {
            genres.entry(row.track_id).or_default().push(Genre {
                id: row.id,
                name: row.name,
            });
        }
        for track in tracks.iter_mut() {
            track.artists = artists.remove(&track.id).unwrap_or_default();
            if track.artists.is_empty() {
                track.artists = vec![ArtistCredit {
                    id: String::new(),
                    name: track.artist_name.clone(),
                }];
            }
            track.album_artists = album_artists
                .get(&track.album_id)
                .cloned()
                .unwrap_or_default();
            track.genres = genres.remove(&track.id).unwrap_or_default();
        }
        Ok(())
    }

    async fn album_artist_credits(
        &self,
        ids_json: &str,
        ids: &[String],
    ) -> Result<HashMap<String, Vec<ArtistCredit>>, LibraryError> {
        let mut credits: HashMap<String, Vec<ArtistCredit>> = HashMap::new();
        for row in sqlx::query!(
            r#"SELECT credits.album_id AS "album_id!", artists.id AS "id!", artists.name AS "name!"
            FROM album_artists credits
            INNER JOIN artists ON artists.id = credits.artist_id
            WHERE credits.album_id IN (SELECT value FROM json_each(?))
            ORDER BY credits.album_id, credits.position"#,
            ids_json
        )
        .fetch_all(&self.pool)
        .await
        .context(|| format!("read Album Artists for IDs {ids:?}"))?
        {
            credits.entry(row.album_id).or_default().push(ArtistCredit {
                id: row.id,
                name: row.name,
            });
        }
        Ok(credits)
    }
}

fn wrap(error: LibraryError, what: impl FnOnce() -> String) -> LibraryError {
    match error {
        LibraryError::NotFound => LibraryError::NotFound,
        LibraryError::Database(inner, cause) => {
            LibraryError::Database(format!("{}: {inner}", what()), cause)
        }
        LibraryError::Io(inner, cause) => LibraryError::Io(format!("{}: {inner}", what()), cause),
    }
}

fn to_album(
    id: String,
    title: String,
    artist_id: String,
    artist_name: String,
    year: Option<i64>,
    release_date: Option<String>,
    genres: &str,
) -> Album {
    Album {
        id,
        title,
        artist_id,
        artist_name,
        album_artists: Vec::new(),
        year,
        release_date,
        release_identifiers: Vec::new(),
        track_count: 0,
        genres: decode_genres(genres),
        genre_items: Vec::new(),
        artwork: None,
    }
}

fn to_track(row: TrackRow) -> Track {
    let mut track = Track {
        id: row.id,
        title: row.title,
        artist_name: row.artist_name,
        artists: Vec::new(),
        album_artists: Vec::new(),
        album_id: row.album_id,
        album_title: row.album_title,
        disc_no: row.disc_no,
        track_no: row.track_no,
        track_total: row.track_total,
        disc_total: row.disc_total,
        duration_ms: row.duration_ms,
        format: row.format,
        codec: row.codec.unwrap_or_default(),
        container: row.container.unwrap_or_default(),
        sample_format: row.sample_format.unwrap_or_default(),
        size_bytes: row.size_bytes,
        genre: row.genre,
        genres: Vec::new(),
        sample_rate_hz: row.sample_rate_hz,
        bit_depth: row.bit_depth,
        channel_count: row.channel_count,
        bitrate_bps: row.bitrate_bps,
        bitrate_kbps: 0,
        replay_gain: ReplayGainMetadata {
            track_gain_db: row.replaygain_track_gain_db,
            track_peak: row.replaygain_track_peak,
            album_gain_db: row.replaygain_album_gain_db,
            album_peak: row.replaygain_album_peak,
        },
        title_sort: row.title_sort,
        file_path: row.file_path,
        file_mtime: row.file_mtime,
        content_sha256: row.content_sha256.unwrap_or_default(),
        identity_key: row.identity_key.unwrap_or_default(),
        revision: row.revision,
        created_at: crate::gojson::time(&row.created_at),
        updated_at: crate::gojson::time(&row.updated_at),
    };
    if track.bitrate_bps > 0 {
        track.bitrate_kbps = track.bitrate_bps / 1000;
    } else if track.bit_depth <= 0 && track.sample_rate_hz <= 0 {
        // Estimated from size and length when nothing better is known.
        if track.size_bytes > 0 && track.duration_ms > 0 {
            track.bitrate_kbps = (track.size_bytes * 8 / track.duration_ms).max(0);
        }
    }
    track
}
