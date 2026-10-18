//! The module's only database access: stored Artist photos and biographies
//! with the outcome of their last lookup.

use sqlx::SqlitePool;

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
}
