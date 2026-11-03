//! Users and their sessions: the auth module's only database access.

use crate::db::{self, Db};

#[derive(Clone)]
pub struct Repository {
    pool: Db,
}

pub struct StoredUser {
    pub id: String,
    pub username: String,
    pub password_hash: String,
}

pub struct StoredSession {
    pub user_id: String,
    pub expires_at: i64,
    pub last_seen_at: i64,
}

impl Repository {
    pub fn new(pool: Db) -> Self {
        Self { pool }
    }

    /// Writes the configured user and drops expired sessions; a changed
    /// password hash also ends every session that user had.
    pub async fn sync_user(
        &self,
        id: &str,
        username: &str,
        password_hash: &str,
        now: i64,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = db::begin_write(&self.pool).await?;
        let stored = sqlx::query_scalar!("SELECT password_hash FROM users WHERE id = ?", id)
            .fetch_optional(&mut *transaction)
            .await?;
        sqlx::query!(
            "INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET
                 username = excluded.username,
                 password_hash = excluded.password_hash,
                 updated_at = CURRENT_TIMESTAMP
             WHERE username != excluded.username OR password_hash != excluded.password_hash",
            id,
            username,
            password_hash,
        )
        .execute(&mut *transaction)
        .await?;
        if stored.is_some_and(|stored| stored != password_hash) {
            sqlx::query!("DELETE FROM sessions WHERE user_id = ?", id)
                .execute(&mut *transaction)
                .await?;
        }
        sqlx::query!("DELETE FROM sessions WHERE expires_at <= ?", now)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await
    }

    pub async fn user_by_username(
        &self,
        username: &str,
    ) -> Result<Option<StoredUser>, sqlx::Error> {
        sqlx::query_as!(
            StoredUser,
            "SELECT id AS \"id!\", username, password_hash FROM users WHERE username = ?",
            username
        )
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn user(&self, id: &str) -> Result<Option<StoredUser>, sqlx::Error> {
        sqlx::query_as!(
            StoredUser,
            "SELECT id AS \"id!\", username, password_hash FROM users WHERE id = ?",
            id
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Adds a session and drops expired ones, which sign-in is a good moment for.
    pub async fn insert_session(
        &self,
        token_hash: &str,
        user_id: &str,
        now: i64,
        expires_at: i64,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = db::begin_write(&self.pool).await?;
        sqlx::query!("DELETE FROM sessions WHERE expires_at <= ?", now)
            .execute(&mut *transaction)
            .await?;
        sqlx::query!(
            "INSERT INTO sessions (token_hash, user_id, created_at, expires_at, last_seen_at)
             VALUES (?, ?, ?, ?, ?)",
            token_hash,
            user_id,
            now,
            expires_at,
            now,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await
    }

    pub async fn session(&self, token_hash: &str) -> Result<Option<StoredSession>, sqlx::Error> {
        sqlx::query_as!(
            StoredSession,
            "SELECT user_id, expires_at, last_seen_at FROM sessions WHERE token_hash = ?",
            token_hash
        )
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn renew_session(
        &self,
        token_hash: &str,
        now: i64,
        expires_at: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE sessions SET expires_at = ?, last_seen_at = ? WHERE token_hash = ?",
            expires_at,
            now,
            token_hash,
        )
        .execute(&self.pool)
        .await
        .map(drop)
    }

    pub async fn delete_session(&self, token_hash: &str) -> Result<(), sqlx::Error> {
        sqlx::query!("DELETE FROM sessions WHERE token_hash = ?", token_hash)
            .execute(&self.pool)
            .await
            .map(drop)
    }
}
