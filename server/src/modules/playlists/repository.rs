//! Playlist rows (playlists/repository.go, queries.sql).

use super::service::DEFAULT_FAVORITES_NAME;
use crate::db::Db;
use serde::Serialize;

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
    pool: Db,
}

impl Repository {
    pub fn new(pool: Db) -> Self {
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

    /// Removes the Track and, in the same write transaction, deletes a user
    /// playlist it left empty, so a concurrent append either lands first (and
    /// keeps the playlist) or runs after the delete. The default Favorites
    /// playlist is never deleted. Returns whether the playlist was deleted.
    pub async fn remove_track(
        &self,
        user_id: &str,
        playlist_id: &str,
        track_id: &str,
    ) -> Result<bool, PlaylistError> {
        let mut transaction = crate::db::begin_write(&self.pool).await?;
        sqlx::query!(
            "DELETE FROM playlist_tracks WHERE playlist_id = ? AND track_id = ?",
            playlist_id,
            track_id
        )
        .execute(&mut *transaction)
        .await?;
        let deleted = sqlx::query!(
            "DELETE FROM playlists WHERE id = ?1 AND user_id = ?2 AND is_default = 0
            AND NOT EXISTS (SELECT 1 FROM playlist_tracks WHERE playlist_id = ?1)",
            playlist_id,
            user_id
        )
        .execute(&mut *transaction)
        .await?
        .rows_affected()
            > 0;
        transaction.commit().await?;
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    async fn repository() -> (tempfile::TempDir, Repository) {
        let dir = tempfile::tempdir().unwrap();
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let pool =
            crate::db::open_and_migrate(dir.path().join("app.db").to_str().unwrap(), &migrations)
                .await
                .unwrap();
        for statement in [
            "INSERT INTO artists (id, name, name_sort) VALUES ('ar', 'A', 'a')",
            "INSERT INTO albums (id, artist_id, title, title_sort) VALUES ('al', 'ar', 'B', 'b')",
            "INSERT INTO tracks (id, album_id, title, title_sort, artist_name, format, file_path)
             VALUES ('t1', 'al', 'One', 'one', 'A', 'flac', '/1.flac'),
                    ('t2', 'al', 'Two', 'two', 'A', 'flac', '/2.flac')",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        (dir, Repository::new(pool))
    }

    #[tokio::test]
    async fn removing_the_last_track_deletes_only_an_empty_user_playlist() {
        let (_dir, repo) = repository().await;
        let empty = repo.create("u", "Empty").await.unwrap();
        repo.append_track(&empty, "t1").await.unwrap();
        assert!(repo.remove_track("u", &empty, "t1").await.unwrap());
        assert!(matches!(
            repo.get("u", &empty).await,
            Err(PlaylistError::NotFound)
        ));

        let kept = repo.create("u", "Kept").await.unwrap();
        repo.append_track(&kept, "t1").await.unwrap();
        repo.append_track(&kept, "t2").await.unwrap();
        assert!(!repo.remove_track("u", &kept, "t1").await.unwrap());
        assert_eq!(repo.track_ids(&kept).await.unwrap(), ["t2"]);

        let favorites = repo.ensure_default("u").await.unwrap();
        repo.append_track(&favorites.id, "t1").await.unwrap();
        assert!(!repo.remove_track("u", &favorites.id, "t1").await.unwrap());
        assert!(repo.get("u", &favorites.id).await.is_ok());
    }
}
