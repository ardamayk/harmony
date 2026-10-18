//! The module's only database access: lyrics looked up online, with the
//! outcome of the last lookup.

use sqlx::SqlitePool;

#[derive(Debug, thiserror::Error)]
#[error("{0}: {1}")]
pub struct RepositoryError(String, sqlx::Error);

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

/// A stored row: the lyrics (empty for an instrumental or none found) and
/// how many seconds ago the lookup ran.
#[derive(Clone, Debug, PartialEq)]
pub struct Stored {
    pub outcome: Outcome,
    pub lyrics: String,
    pub age_seconds: i64,
}

#[derive(Clone)]
pub struct Repository {
    pool: SqlitePool,
}

impl Repository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get(&self, track_id: &str) -> Result<Option<Stored>, RepositoryError> {
        let row = sqlx::query!(
            r#"SELECT outcome, lyrics,
                CAST((julianday('now') - julianday(checked_at)) * 86400 AS INTEGER) AS "age_seconds!: i64"
            FROM track_online_lyrics WHERE track_id = ?1"#,
            track_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| RepositoryError(format!("get Track {track_id} online lyrics"), error))?;
        Ok(row.map(|row| Stored {
            outcome: Outcome::parse(&row.outcome),
            lyrics: row.lyrics.unwrap_or_default(),
            age_seconds: row.age_seconds,
        }))
    }

    pub async fn put(
        &self,
        track_id: &str,
        outcome: Outcome,
        lyrics: &str,
    ) -> Result<(), RepositoryError> {
        let outcome = outcome.as_str();
        sqlx::query!(
            "INSERT INTO track_online_lyrics (track_id, outcome, lyrics, checked_at)
            VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
            ON CONFLICT (track_id) DO UPDATE SET outcome = excluded.outcome,
                lyrics = excluded.lyrics, checked_at = excluded.checked_at",
            track_id,
            outcome,
            lyrics
        )
        .execute(&self.pool)
        .await
        .map_err(|error| RepositoryError(format!("store Track {track_id} online lyrics"), error))?;
        Ok(())
    }
}
