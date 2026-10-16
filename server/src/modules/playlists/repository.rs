//! Playlist rows (playlists/repository.go, queries.sql).

use super::DEFAULT_FAVORITES_NAME;
use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub track_count: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum PlaylistError {
    #[error("playlist not found")]
    NotFound,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

#[derive(Clone)]
pub struct Repository {
    pool: SqlitePool,
}

impl Repository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn ensure_default(&self, user_id: &str) -> Result<Playlist, PlaylistError> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query!(
            "INSERT OR IGNORE INTO playlists (id, user_id, name, is_default) VALUES (?, ?, ?, 1)",
            id,
            user_id,
            DEFAULT_FAVORITES_NAME
        )
        .execute(&self.pool)
        .await?;
        let row = sqlx::query!(
            r#"SELECT p.id AS "id!", p.name AS "name!", p.is_default AS "is_default!: bool",
                COUNT(pt.track_id) AS "track_count!: i64"
            FROM playlists p
            LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
            WHERE p.user_id = ? AND p.name = ?
            GROUP BY p.id, p.name, p.is_default"#,
            user_id,
            DEFAULT_FAVORITES_NAME
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(Playlist {
            id: row.id,
            name: row.name,
            is_default: row.is_default,
            track_count: row.track_count,
        })
    }

    pub async fn list(&self, user_id: &str) -> Result<Vec<Playlist>, PlaylistError> {
        let rows = sqlx::query!(
            r#"SELECT p.id AS "id!", p.name AS "name!", p.is_default AS "is_default!: bool",
                COUNT(pt.track_id) AS "track_count!: i64"
            FROM playlists p
            LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
            WHERE p.user_id = ?
            GROUP BY p.id, p.name, p.is_default
            ORDER BY p.is_default DESC, p.name COLLATE NOCASE"#,
            user_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| Playlist {
                id: row.id,
                name: row.name,
                is_default: row.is_default,
                track_count: row.track_count,
            })
            .collect())
    }

    /// The new playlist's ID; a database error is returned as is.
    pub async fn create(&self, user_id: &str, name: &str) -> Result<String, sqlx::Error> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query!(
            "INSERT INTO playlists (id, user_id, name, is_default) VALUES (?, ?, ?, 0)",
            id,
            user_id,
            name
        )
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn get(&self, user_id: &str, playlist_id: &str) -> Result<Playlist, PlaylistError> {
        let row = sqlx::query!(
            r#"SELECT p.id AS "id!", p.name AS "name!", p.is_default AS "is_default!: bool",
                COUNT(pt.track_id) AS "track_count!: i64"
            FROM playlists p
            LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
            WHERE p.user_id = ? AND p.id = ?
            GROUP BY p.id, p.name, p.is_default"#,
            user_id,
            playlist_id
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(PlaylistError::NotFound)?;
        Ok(Playlist {
            id: row.id,
            name: row.name,
            is_default: row.is_default,
            track_count: row.track_count,
        })
    }

    pub async fn track_ids(&self, playlist_id: &str) -> Result<Vec<String>, PlaylistError> {
        Ok(sqlx::query_scalar!(
            "SELECT track_id FROM playlist_tracks WHERE playlist_id = ? ORDER BY position",
            playlist_id
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Appends at the end; a Track already present is left where it is.
    pub async fn append_track(
        &self,
        playlist_id: &str,
        track_id: &str,
    ) -> Result<(), PlaylistError> {
        sqlx::query!(
            "INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, position)
            SELECT ?1, ?2, COALESCE(MAX(position) + 1, 0)
            FROM playlist_tracks WHERE playlist_id = ?1",
            playlist_id,
            track_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn remove_track(
        &self,
        playlist_id: &str,
        track_id: &str,
    ) -> Result<(), PlaylistError> {
        sqlx::query!(
            "DELETE FROM playlist_tracks WHERE playlist_id = ? AND track_id = ?",
            playlist_id,
            track_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn count_tracks(&self, playlist_id: &str) -> Result<i64, PlaylistError> {
        Ok(sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!: i64" FROM playlist_tracks WHERE playlist_id = ?"#,
            playlist_id
        )
        .fetch_one(&self.pool)
        .await?)
    }

    /// Never deletes the default Favorites playlist.
    pub async fn delete_user_playlist(
        &self,
        user_id: &str,
        playlist_id: &str,
    ) -> Result<(), PlaylistError> {
        sqlx::query!(
            "DELETE FROM playlists WHERE id = ? AND user_id = ? AND is_default = 0",
            playlist_id,
            user_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
