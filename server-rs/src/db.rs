//! The SQLite database, opened the way the Go server opens it
//! (server/internal/db): WAL so readers never block a writer's commit,
//! foreign keys on, a 5s busy timeout, and write transactions that begin
//! IMMEDIATE (see [`begin_write`]).

use crate::migrate::{self, MigrateError};
use crate::searchdata;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("create database directory: {0}")]
    Directory(std::io::Error),
    #[error("open database: {0}")]
    Open(sqlx::Error),
    #[error("run migrations: {0}")]
    Migrate(#[from] MigrateError),
    #[error("prepare Library Search data: {0}")]
    SearchData(String),
}

pub async fn open_and_migrate(
    database_path: &str,
    migrations_directory: &Path,
) -> Result<SqlitePool, DbError> {
    if let Some(parent) = Path::new(database_path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(DbError::Directory)?;
    }
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{database_path}"))
        .map_err(DbError::Open)?
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
        .journal_mode(SqliteJournalMode::Wal);
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .after_connect(|connection, _| Box::pin(searchdata::register_on(connection)))
        .connect_with(options)
        .await
        .map_err(DbError::Open)?;
    let migrations = migrate::load(migrations_directory)?;
    let mut connection = pool.acquire().await.map_err(DbError::Open)?;
    migrate::up(&mut connection, &migrations).await?;
    drop(connection);
    searchdata::rebuild(&pool, false)
        .await
        .map_err(DbError::SearchData)?;
    Ok(pool)
}

/// Starts a write transaction with BEGIN IMMEDIATE, so the writer takes the
/// lock up front and waits through busy_timeout instead of failing with
/// SQLITE_BUSY when a read turns into a write mid-transaction.
pub async fn begin_write(pool: &SqlitePool) -> Result<Transaction<'static, Sqlite>, sqlx::Error> {
    pool.begin_with("BEGIN IMMEDIATE").await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn opens_in_wal_mode_with_foreign_keys_and_migrates() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("nested/app.db");
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../server/migrations");

        let pool = open_and_migrate(path.to_str().expect("utf-8 path"), &migrations)
            .await
            .expect("open");

        let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .expect("journal mode");
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .expect("foreign keys");
        assert_eq!(journal, "wal");
        assert_eq!(foreign_keys, 1);

        let mut transaction = begin_write(&pool).await.expect("begin");
        sqlx::query("UPDATE user_preferences SET theme = theme")
            .execute(&mut *transaction)
            .await
            .expect("write");
        transaction.commit().await.expect("commit");
    }
}
