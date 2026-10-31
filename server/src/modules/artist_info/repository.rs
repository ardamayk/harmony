//! The module's only database access: stored Artist photos, biographies,
//! top Track titles and play counts, and Album genres and covers, with the outcome of
//! their last lookup.

use sqlx::SqlitePool;
use std::collections::HashMap;

#[derive(Debug, thiserror::Error)]
#[error("{0}: {1}")]
pub struct RepositoryError(String, sqlx::Error);

trait Context<T> {
    fn context(self, what: impl FnOnce() -> String) -> Result<T, RepositoryError>;
}

impl<T> Context<T> for Result<T, sqlx::Error> {
    fn context(self, what: impl FnOnce() -> String) -> Result<T, RepositoryError> {
        self.map_err(|error| RepositoryError(what(), error))
    }
}

/// How the last lookup ended; stored as text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Found,
    Missing,
    Failed,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Outcome::Found => "found",
            Outcome::Missing => "missing",
            Outcome::Failed => "failed",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "found" => Outcome::Found,
            "missing" => Outcome::Missing,
            _ => Outcome::Failed,
        }
    }
}

/// A stored row: what was found (if anything), and how many seconds ago.
#[derive(Clone, Debug, PartialEq)]
pub struct Stored<T> {
    pub outcome: Outcome,
    pub value: Option<T>,
    pub age_seconds: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Photo {
    pub media_type: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Biography {
    pub text: String,
    pub source_url: String,
}

#[derive(Clone)]
pub struct Repository {
    pool: SqlitePool,
}

impl Repository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn photo(&self, artist_id: &str) -> Result<Option<Stored<Photo>>, RepositoryError> {
        let row = sqlx::query!(
            r#"SELECT outcome, media_type, data,
                CAST((julianday('now') - julianday(checked_at)) * 86400 AS INTEGER) AS "age_seconds!: i64"
            FROM artist_photos WHERE artist_id = ?1"#,
            artist_id
        )
        .fetch_optional(&self.pool)
        .await
        .context(|| format!("get Artist photo {artist_id}"))?;
        Ok(row.map(|row| Stored {
            outcome: Outcome::parse(&row.outcome),
            value: row
                .media_type
                .zip(row.data)
                .map(|(media_type, data)| Photo { media_type, data }),
            age_seconds: row.age_seconds,
        }))
    }

    pub async fn put_photo(
        &self,
        artist_id: &str,
        outcome: Outcome,
        photo: Option<&Photo>,
    ) -> Result<(), RepositoryError> {
        let outcome = outcome.as_str();
        let media_type = photo.map(|photo| photo.media_type.as_str());
        let data = photo.map(|photo| photo.data.as_slice());
        sqlx::query!(
            "INSERT INTO artist_photos (artist_id, outcome, media_type, data, checked_at)
            VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)
            ON CONFLICT (artist_id) DO UPDATE SET outcome = excluded.outcome,
                media_type = excluded.media_type, data = excluded.data, checked_at = excluded.checked_at",
            artist_id,
            outcome,
            media_type,
            data
        )
        .execute(&self.pool)
        .await
        .context(|| format!("store Artist photo {artist_id}"))?;
        Ok(())
    }

    pub async fn biography(
        &self,
        artist_id: &str,
    ) -> Result<Option<Stored<Biography>>, RepositoryError> {
        let row = sqlx::query!(
            r#"SELECT outcome, text, source_url,
                CAST((julianday('now') - julianday(checked_at)) * 86400 AS INTEGER) AS "age_seconds!: i64"
            FROM artist_biographies WHERE artist_id = ?1"#,
            artist_id
        )
        .fetch_optional(&self.pool)
        .await
        .context(|| format!("get Artist biography {artist_id}"))?;
        Ok(row.map(|row| Stored {
            outcome: Outcome::parse(&row.outcome),
            value: row
                .text
                .zip(row.source_url)
                .map(|(text, source_url)| Biography { text, source_url }),
            age_seconds: row.age_seconds,
        }))
    }

    pub async fn put_biography(
        &self,
        artist_id: &str,
        outcome: Outcome,
        biography: Option<&Biography>,
    ) -> Result<(), RepositoryError> {
        let outcome = outcome.as_str();
        let text = biography.map(|biography| biography.text.as_str());
        let source_url = biography.map(|biography| biography.source_url.as_str());
        sqlx::query!(
            "INSERT INTO artist_biographies (artist_id, outcome, text, source_url, checked_at)
            VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)
            ON CONFLICT (artist_id) DO UPDATE SET outcome = excluded.outcome,
                text = excluded.text, source_url = excluded.source_url, checked_at = excluded.checked_at",
            artist_id,
            outcome,
            text,
            source_url
        )
        .execute(&self.pool)
        .await
        .context(|| format!("store Artist biography {artist_id}"))?;
        Ok(())
    }

    pub async fn top_titles(
        &self,
        artist_id: &str,
    ) -> Result<Option<Stored<Vec<String>>>, RepositoryError> {
        let row = sqlx::query!(
            r#"SELECT outcome, titles,
                CAST((julianday('now') - julianday(checked_at)) * 86400 AS INTEGER) AS "age_seconds!: i64"
            FROM artist_top_tracks WHERE artist_id = ?1"#,
            artist_id
        )
        .fetch_optional(&self.pool)
        .await
        .context(|| format!("get Artist top Tracks {artist_id}"))?;
        Ok(row.map(|row| Stored {
            outcome: Outcome::parse(&row.outcome),
            // A list this module wrote; anything unreadable counts as none.
            value: row
                .titles
                .and_then(|titles| serde_json::from_str(&titles).ok()),
            age_seconds: row.age_seconds,
        }))
    }

    pub async fn put_top_titles(
        &self,
        artist_id: &str,
        outcome: Outcome,
        titles: Option<&Vec<String>>,
    ) -> Result<(), RepositoryError> {
        let outcome = outcome.as_str();
        let titles = titles.map(|titles| serde_json::to_string(titles).expect("titles encode"));
        sqlx::query!(
            "INSERT INTO artist_top_tracks (artist_id, outcome, titles, checked_at)
            VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
            ON CONFLICT (artist_id) DO UPDATE SET outcome = excluded.outcome,
                titles = excluded.titles, checked_at = excluded.checked_at",
            artist_id,
            outcome,
            titles
        )
        .execute(&self.pool)
        .await
        .context(|| format!("store Artist top Tracks {artist_id}"))?;
        Ok(())
    }

    pub async fn play_counts(
        &self,
        artist_id: &str,
    ) -> Result<Option<Stored<HashMap<String, i64>>>, RepositoryError> {
        let row = sqlx::query!(
            r#"SELECT outcome, counts,
                CAST((julianday('now') - julianday(checked_at)) * 86400 AS INTEGER) AS "age_seconds!: i64"
            FROM artist_play_counts WHERE artist_id = ?1"#,
            artist_id
        )
        .fetch_optional(&self.pool)
        .await
        .context(|| format!("get Artist play counts {artist_id}"))?;
        Ok(row.map(|row| Stored {
            outcome: Outcome::parse(&row.outcome),
            // A map this module wrote; anything unreadable counts as none.
            value: row
                .counts
                .and_then(|counts| serde_json::from_str(&counts).ok()),
            age_seconds: row.age_seconds,
        }))
    }

    pub async fn put_play_counts(
        &self,
        artist_id: &str,
        outcome: Outcome,
        counts: Option<&HashMap<String, i64>>,
    ) -> Result<(), RepositoryError> {
        let outcome = outcome.as_str();
        let counts = counts.map(|counts| serde_json::to_string(counts).expect("counts encode"));
        sqlx::query!(
            "INSERT INTO artist_play_counts (artist_id, outcome, counts, checked_at)
            VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
            ON CONFLICT (artist_id) DO UPDATE SET outcome = excluded.outcome,
                counts = excluded.counts, checked_at = excluded.checked_at",
            artist_id,
            outcome,
            counts
        )
        .execute(&self.pool)
        .await
        .context(|| format!("store Artist play counts {artist_id}"))?;
        Ok(())
    }

    /// Each Album's last explicit Track lookup.
    pub async fn album_explicit_checks(
        &self,
    ) -> Result<HashMap<String, Stored<()>>, RepositoryError> {
        let rows = sqlx::query!(
            r#"SELECT album_id AS "album_id!", outcome,
                CAST((julianday('now') - julianday(checked_at)) * 86400 AS INTEGER) AS "age_seconds!: i64"
            FROM album_explicit_checks"#
        )
        .fetch_all(&self.pool)
        .await
        .context(|| "list Album explicit checks".to_owned())?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let stored = Stored {
                    outcome: Outcome::parse(&row.outcome),
                    value: None,
                    age_seconds: row.age_seconds,
                };
                (row.album_id, stored)
            })
            .collect())
    }

    pub async fn put_album_explicit_check(
        &self,
        album_id: &str,
        outcome: Outcome,
    ) -> Result<(), RepositoryError> {
        let outcome = outcome.as_str();
        sqlx::query!(
            "INSERT INTO album_explicit_checks (album_id, outcome, checked_at)
            VALUES (?1, ?2, CURRENT_TIMESTAMP)
            ON CONFLICT (album_id) DO UPDATE SET outcome = excluded.outcome,
                checked_at = excluded.checked_at",
            album_id,
            outcome
        )
        .execute(&self.pool)
        .await
        .context(|| format!("store Album explicit check {album_id}"))?;
        Ok(())
    }

    pub async fn album_genres(
        &self,
        album_id: &str,
    ) -> Result<Option<Stored<Vec<String>>>, RepositoryError> {
        let row = sqlx::query!(
            r#"SELECT outcome, genres,
                CAST((julianday('now') - julianday(checked_at)) * 86400 AS INTEGER) AS "age_seconds!: i64"
            FROM album_online_genres WHERE album_id = ?1"#,
            album_id
        )
        .fetch_optional(&self.pool)
        .await
        .context(|| format!("get Album online genres {album_id}"))?;
        Ok(row.map(|row| Stored {
            outcome: Outcome::parse(&row.outcome),
            // A list this module wrote; anything unreadable counts as none.
            value: row
                .genres
                .and_then(|genres| serde_json::from_str(&genres).ok()),
            age_seconds: row.age_seconds,
        }))
    }

    /// The stored sharper cover and the hash of the cover it stands in for.
    pub async fn album_cover(
        &self,
        album_id: &str,
    ) -> Result<Option<(String, Stored<Photo>)>, RepositoryError> {
        let row = sqlx::query!(
            r#"SELECT source_sha256, outcome, media_type, data,
                CAST((julianday('now') - julianday(checked_at)) * 86400 AS INTEGER) AS "age_seconds!: i64"
            FROM album_online_covers WHERE album_id = ?1"#,
            album_id
        )
        .fetch_optional(&self.pool)
        .await
        .context(|| format!("get Album online cover {album_id}"))?;
        Ok(row.map(|row| {
            let value = match (row.media_type, row.data) {
                (Some(media_type), Some(data)) => Some(Photo { media_type, data }),
                _ => None,
            };
            (
                row.source_sha256,
                Stored {
                    outcome: Outcome::parse(&row.outcome),
                    value,
                    age_seconds: row.age_seconds,
                },
            )
        }))
    }

    pub async fn put_album_cover(
        &self,
        album_id: &str,
        source_sha256: &str,
        outcome: Outcome,
        cover: Option<&Photo>,
    ) -> Result<(), RepositoryError> {
        let outcome = outcome.as_str();
        let media_type = cover.map(|cover| cover.media_type.as_str());
        let data = cover.map(|cover| cover.data.as_slice());
        sqlx::query!(
            "INSERT INTO album_online_covers (album_id, source_sha256, outcome, media_type, data, checked_at)
            VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)
            ON CONFLICT (album_id) DO UPDATE SET source_sha256 = excluded.source_sha256,
                outcome = excluded.outcome, media_type = excluded.media_type,
                data = excluded.data, checked_at = excluded.checked_at",
            album_id,
            source_sha256,
            outcome,
            media_type,
            data
        )
        .execute(&self.pool)
        .await
        .context(|| format!("store Album online cover {album_id}"))?;
        Ok(())
    }

    pub async fn put_album_genres(
        &self,
        album_id: &str,
        outcome: Outcome,
        genres: Option<&Vec<String>>,
    ) -> Result<(), RepositoryError> {
        let outcome = outcome.as_str();
        let genres = genres.map(|genres| serde_json::to_string(genres).expect("genres encode"));
        sqlx::query!(
            "INSERT INTO album_online_genres (album_id, outcome, genres, checked_at)
            VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
            ON CONFLICT (album_id) DO UPDATE SET outcome = excluded.outcome,
                genres = excluded.genres, checked_at = excluded.checked_at",
            album_id,
            outcome,
            genres
        )
        .execute(&self.pool)
        .await
        .context(|| format!("store Album online genres {album_id}"))?;
        Ok(())
    }
}
