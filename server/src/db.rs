//! The SQLite database, opened the way the Go server opens it
//! (server/internal/db): WAL so readers never block a writer's commit,
//! foreign keys on, a 5s busy timeout, and write transactions that begin
//! IMMEDIATE (see [`begin_write`]).
//!
//! [`Db`] is the pool with its work timed as spans of the current request
//! (ADR 0033). sqlx's own query log cannot do this: SQLite runs on a worker
//! thread, which does not know the request.

use crate::migrate::{self, MigrateError};
use crate::observability::{self, Step};
use crate::searchdata;
use futures_util::future::BoxFuture;
use futures_util::stream::{BoxStream, StreamExt};
use sqlx::pool::PoolConnection;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteQueryResult, SqliteRow,
    SqliteStatement, SqliteTypeInfo,
};
use sqlx::{
    Describe, Either, Execute, Executor, SqlStr, Sqlite, SqliteConnection, SqlitePool, Transaction,
};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

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

/// Turns off SQLite's memory statistics, process-wide and before the first
/// connection. They take one global mutex on every allocation, which
/// serialized concurrent requests across the pool (eight connections served
/// no more than one).
fn disable_memory_statistics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // SAFETY: sqlite3_config is called once, before SQLite initializes
        // (no connection exists yet), with the documented int argument.
        let code = unsafe {
            libsqlite3_sys::sqlite3_config(
                libsqlite3_sys::SQLITE_CONFIG_MEMSTATUS,
                0 as std::ffi::c_int,
            )
        };
        if code != libsqlite3_sys::SQLITE_OK {
            tracing::warn!(code, "SQLite memory statistics stay on");
        }
    });
}

pub async fn open_and_migrate(
    database_path: &str,
    migrations_directory: &Path,
) -> Result<Db, DbError> {
    disable_memory_statistics();
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
        .map(Db::from)
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
pub async fn begin_write(pool: &Db) -> Result<Tx, sqlx::Error> {
    hold("db.transaction.write", pool.0.begin_with("BEGIN IMMEDIATE")).await
}

/// The connection pool. A query run on it is a span of the current request,
/// and so is a connection taken from it for several statements.
#[derive(Clone, Debug)]
pub struct Db(SqlitePool);

impl From<SqlitePool> for Db {
    fn from(pool: SqlitePool) -> Self {
        Self(pool)
    }
}

impl Deref for Db {
    type Target = SqlitePool;

    fn deref(&self) -> &SqlitePool {
        &self.0
    }
}

impl Db {
    /// A read transaction: one snapshot for several statements.
    pub async fn begin(&self) -> Result<Tx, sqlx::Error> {
        hold("db.transaction.read", self.0.begin()).await
    }

    pub async fn acquire(&self) -> Result<Held<PoolConnection<Sqlite>>, sqlx::Error> {
        hold("db.connection", self.0.acquire()).await
    }

    fn timed<'q, E: Execute<'q, Sqlite>>(&self, query: E) -> (Named<E>, QueryTimer) {
        let sql = Arc::new(OnceLock::new());
        let timer = QueryTimer {
            started: Instant::now(),
            idle_connections: self.0.num_idle(),
            sql: sql.clone(),
        };
        (Named { query, sql }, timer)
    }
}

impl<'p> Executor<'p> for &'_ Db {
    type Database = Sqlite;

    fn fetch_many<'e, 'q: 'e, E>(
        self,
        query: E,
    ) -> BoxStream<'e, Result<Either<SqliteQueryResult, SqliteRow>, sqlx::Error>>
    where
        'p: 'e,
        E: 'q + Execute<'q, Sqlite>,
    {
        let (query, timer) = self.timed(query);
        (&self.0)
            .fetch_many(query)
            .map(move |row| {
                // The stream keeps the timer, which reports when it drops.
                let _ = &timer;
                row
            })
            .boxed()
    }

    fn fetch_optional<'e, 'q: 'e, E>(
        self,
        query: E,
    ) -> BoxFuture<'e, Result<Option<SqliteRow>, sqlx::Error>>
    where
        'p: 'e,
        E: 'q + Execute<'q, Sqlite>,
    {
        let (query, timer) = self.timed(query);
        let row = (&self.0).fetch_optional(query);
        Box::pin(async move {
            let row = row.await;
            drop(timer);
            row
        })
    }

    fn prepare_with<'e>(
        self,
        sql: SqlStr,
        parameters: &'e [SqliteTypeInfo],
    ) -> BoxFuture<'e, Result<SqliteStatement, sqlx::Error>>
    where
        'p: 'e,
    {
        (&self.0).prepare_with(sql, parameters)
    }

    fn describe<'e>(self, sql: SqlStr) -> BoxFuture<'e, Result<Describe<Sqlite>, sqlx::Error>>
    where
        'p: 'e,
    {
        (&self.0).describe(sql)
    }
}

/// A query that keeps its text for [`QueryTimer`], since running it uses it up.
struct Named<E> {
    query: E,
    sql: Arc<OnceLock<SqlStr>>,
}

impl<'q, E: Execute<'q, Sqlite>> Execute<'q, Sqlite> for Named<E> {
    fn sql(self) -> SqlStr {
        let sql = self.query.sql();
        let _ = self.sql.set(sql.clone());
        sql
    }

    fn statement(&self) -> Option<&SqliteStatement> {
        self.query.statement()
    }

    fn take_arguments(
        &mut self,
    ) -> Result<Option<sqlx::sqlite::SqliteArguments>, sqlx::error::BoxDynError> {
        self.query.take_arguments()
    }

    fn persistent(&self) -> bool {
        self.query.persistent()
    }
}

/// Reports a query when dropped: done, or given up with its request.
struct QueryTimer {
    started: Instant,
    /// Zero means the query may have waited for a connection.
    idle_connections: usize,
    sql: Arc<OnceLock<SqlStr>>,
}

impl Drop for QueryTimer {
    fn drop(&mut self) {
        if let Some(sql) = self.sql.get() {
            observability::record_query(self.started, sql.as_str(), self.idle_connections);
        }
    }
}

/// A connection kept across statements. They run on it directly, so its span
/// covers them together: the wait for it, then all of them, then the commit.
pub struct Held<T> {
    /// Taken by the commit.
    connection: Option<T>,
    op: &'static str,
    started: Instant,
    wait: Duration,
    commit: Option<Duration>,
}

pub type Tx = Held<Transaction<'static, Sqlite>>;

async fn hold<T>(
    op: &'static str,
    connecting: impl Future<Output = Result<T, sqlx::Error>>,
) -> Result<Held<T>, sqlx::Error> {
    let started = Instant::now();
    let connection = connecting.await?;
    Ok(Held {
        connection: Some(connection),
        op,
        started,
        wait: started.elapsed(),
        commit: None,
    })
}

impl Tx {
    pub async fn commit(mut self) -> Result<(), sqlx::Error> {
        let started = Instant::now();
        let transaction = self.connection.take().expect("held until the commit");
        let result = transaction.commit().await;
        self.commit = Some(started.elapsed());
        result
    }
}

impl<T: Deref<Target = SqliteConnection>> Deref for Held<T> {
    type Target = SqliteConnection;

    fn deref(&self) -> &SqliteConnection {
        self.connection.as_deref().expect("held until the commit")
    }
}

impl<T: DerefMut<Target = SqliteConnection>> DerefMut for Held<T> {
    fn deref_mut(&mut self) -> &mut SqliteConnection {
        self.connection
            .as_deref_mut()
            .expect("held until the commit")
    }
}

impl<T> Drop for Held<T> {
    fn drop(&mut self) {
        let total = self.started.elapsed();
        let mut steps = vec![Step {
            op: "db.wait",
            offset: Duration::ZERO,
            duration: self.wait,
        }];
        steps.extend(self.commit.map(|commit| Step {
            op: "db.commit",
            offset: total.saturating_sub(commit),
            duration: commit,
        }));
        observability::record_span(self.started, self.op, total, &[], &steps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn opens_in_wal_mode_with_foreign_keys_and_migrates() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("nested/app.db");
        let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");

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

    fn spans(work: impl std::future::Future<Output = ()>) -> Vec<sentry::protocol::Span> {
        let options = sentry::ClientOptions::new().traces_sample_rate(1.0);
        let envelopes = sentry::test::with_captured_envelopes_options(
            || {
                let transaction = sentry::start_transaction(sentry::TransactionContext::new(
                    "GET /api/v1/library/tracks",
                    "http.server",
                ));
                sentry::configure_scope(|scope| {
                    scope.set_span(Some(transaction.clone().into()));
                });
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("runtime")
                    .block_on(work);
                transaction.finish();
            },
            options,
        );
        envelopes
            .iter()
            .flat_map(|envelope| envelope.items())
            .find_map(|item| match item {
                sentry::protocol::EnvelopeItem::Transaction(transaction) => {
                    Some(transaction.spans.clone())
                }
                _ => None,
            })
            .expect("transaction")
    }

    async fn memory_db() -> Db {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("open");
        sqlx::query("CREATE TABLE tracks (title TEXT)")
            .execute(&pool)
            .await
            .expect("schema");
        pool.into()
    }

    #[test]
    fn a_query_on_the_pool_is_a_span_with_its_text_and_no_values() {
        let spans = spans(async {
            let pool = memory_db().await;
            sqlx::query("INSERT INTO tracks (title) VALUES (?)")
                .bind("secret-title")
                .execute(&pool)
                .await
                .expect("write");
            let title: Option<String> =
                sqlx::query_scalar("SELECT title FROM tracks WHERE title = ?")
                    .bind("secret-title")
                    .fetch_optional(&pool)
                    .await
                    .expect("read");
            assert_eq!(title.as_deref(), Some("secret-title"));
        });
        let queries: Vec<_> = spans
            .iter()
            .filter(|span| span.op.as_deref() == Some("db.sql.query"))
            .map(|span| span.description.clone().unwrap_or_default())
            .collect();
        assert_eq!(
            queries,
            [
                "INSERT INTO tracks (title) VALUES (?)",
                "SELECT title FROM tracks WHERE title = ?"
            ]
        );
        assert!(!format!("{spans:?}").contains("secret-title"));
    }

    #[test]
    fn a_transaction_is_one_span_with_its_wait_and_its_commit() {
        let spans = spans(async {
            let pool = memory_db().await;
            let mut transaction = begin_write(&pool).await.expect("begin");
            sqlx::query("INSERT INTO tracks (title) VALUES ('a')")
                .execute(&mut *transaction)
                .await
                .expect("write");
            transaction.commit().await.expect("commit");
            // A read transaction ends without a commit.
            let mut snapshot = pool.begin().await.expect("begin read");
            sqlx::query("SELECT title FROM tracks")
                .fetch_all(&mut *snapshot)
                .await
                .expect("read");
        });
        let ops: Vec<_> = spans
            .iter()
            .filter_map(|span| span.op.as_deref())
            .filter(|op| *op != "db.sql.query")
            .collect();
        for op in [
            "db.transaction.write",
            "db.transaction.read",
            "db.wait",
            "db.commit",
        ] {
            assert!(ops.contains(&op), "{op} in {ops:?}");
        }
        assert_eq!(ops.iter().filter(|op| **op == "db.commit").count(), 1);
        let write = spans
            .iter()
            .find(|span| span.op.as_deref() == Some("db.transaction.write"))
            .expect("write transaction");
        let commit = spans
            .iter()
            .find(|span| span.op.as_deref() == Some("db.commit"))
            .expect("commit");
        assert_eq!(commit.parent_span_id, Some(write.span_id));
    }

    #[test]
    fn nothing_is_recorded_outside_a_request() {
        let envelopes = sentry::test::with_captured_envelopes(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime")
                .block_on(async {
                    let pool = memory_db().await;
                    let transaction = begin_write(&pool).await.expect("begin");
                    transaction.commit().await.expect("commit");
                });
        });
        assert!(envelopes.is_empty());
    }
}
