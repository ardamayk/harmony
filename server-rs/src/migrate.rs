//! Applies the goose migrations in server/migrations exactly as goose does,
//! sharing its `goose_db_version` table, so the Go and Rust servers can open
//! the same database during the move (ADR 0025).
//!
//! Only the `-- +goose Up` section runs. SQLite parses trigger bodies itself,
//! so `StatementBegin`/`StatementEnd` need no special handling; a file marked
//! `-- +goose NO TRANSACTION` runs outside a transaction.

use sqlx::{Connection, SqliteConnection};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("read migrations: {0}")]
    Io(#[from] std::io::Error),
    #[error("migration {0}: {1}")]
    Invalid(String, &'static str),
    #[error("migration {0}: {1}")]
    Sql(String, sqlx::Error),
    #[error("migration bookkeeping: {0}")]
    Bookkeeping(#[from] sqlx::Error),
}

#[derive(Debug, PartialEq)]
pub struct Migration {
    pub version: i64,
    pub name: String,
    pub up: String,
    pub in_transaction: bool,
}

/// Parses one goose SQL migration; `name` is its file name.
pub fn parse(name: &str, source: &str) -> Result<Migration, MigrateError> {
    let version = name
        .split('_')
        .next()
        .and_then(|prefix| prefix.parse::<i64>().ok())
        .ok_or_else(|| MigrateError::Invalid(name.to_owned(), "file name has no version"))?;
    let mut up = String::new();
    let mut section = None;
    let mut in_transaction = true;
    for line in source.lines() {
        let annotation = line
            .trim()
            .strip_prefix("-- +goose ")
            .map(str::trim)
            .map(str::to_ascii_uppercase);
        match annotation.as_deref() {
            Some("UP") => section = Some(true),
            Some("DOWN") => section = Some(false),
            Some("NO TRANSACTION") => in_transaction = false,
            // StatementBegin/End and any other annotation: SQLite needs none.
            Some(_) => {}
            _ if section == Some(true) => {
                up.push_str(line);
                up.push('\n');
            }
            _ => {}
        }
    }
    if section.is_none() {
        return Err(MigrateError::Invalid(
            name.to_owned(),
            "no -- +goose Up section",
        ));
    }
    Ok(Migration {
        version,
        name: name.to_owned(),
        up,
        in_transaction,
    })
}

/// Reads every `*.sql` migration in `directory`, ordered by version.
pub fn load(directory: &Path) -> Result<Vec<Migration>, MigrateError> {
    let mut migrations = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "sql") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        migrations.push(parse(&name, &std::fs::read_to_string(&path)?)?);
    }
    migrations.sort_by_key(|migration| migration.version);
    for pair in migrations.windows(2) {
        if pair[0].version == pair[1].version {
            return Err(MigrateError::Invalid(
                pair[1].name.clone(),
                "duplicate version",
            ));
        }
    }
    Ok(migrations)
}

/// Brings the database up to the newest migration and returns the versions
/// it applied.
pub async fn up(
    connection: &mut SqliteConnection,
    migrations: &[Migration],
) -> Result<Vec<i64>, MigrateError> {
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS goose_db_version (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            version_id INTEGER NOT NULL,
            is_applied INTEGER NOT NULL,
            tstamp TIMESTAMP DEFAULT (datetime('now'))
        )",
    )
    .execute(&mut *connection)
    .await?;
    let initialized: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM goose_db_version WHERE version_id = 0 LIMIT 1")
            .fetch_optional(&mut *connection)
            .await?;
    if initialized.is_none() {
        sqlx::query("INSERT INTO goose_db_version (version_id, is_applied) VALUES (0, 1)")
            .execute(&mut *connection)
            .await?;
    }
    let applied = applied_versions(connection).await?;
    let mut newly_applied = Vec::new();
    for migration in migrations {
        if applied.contains(&migration.version) {
            continue;
        }
        if migration.in_transaction {
            let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
            apply(&mut transaction, migration).await?;
            transaction.commit().await?;
        } else {
            apply(connection, migration).await?;
        }
        newly_applied.push(migration.version);
    }
    Ok(newly_applied)
}

async fn apply(
    connection: &mut SqliteConnection,
    migration: &Migration,
) -> Result<(), MigrateError> {
    // The SQL comes from the repository's own migration files.
    sqlx::raw_sql(sqlx::AssertSqlSafe(migration.up.clone()))
        .execute(&mut *connection)
        .await
        .map_err(|error| MigrateError::Sql(migration.name.clone(), error))?;
    sqlx::query("INSERT INTO goose_db_version (version_id, is_applied) VALUES (?, 1)")
        .bind(migration.version)
        .execute(&mut *connection)
        .await?;
    Ok(())
}

/// Versions whose latest goose row marks them applied (goose records a
/// rollback as a later row with `is_applied = 0`).
async fn applied_versions(connection: &mut SqliteConnection) -> Result<BTreeSet<i64>, sqlx::Error> {
    let rows: Vec<(i64, bool)> = sqlx::query_as(
        "SELECT version_id, is_applied FROM goose_db_version
         WHERE id IN (SELECT MAX(id) FROM goose_db_version GROUP BY version_id)",
    )
    .fetch_all(&mut *connection)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(version, applied)| (applied && version > 0).then_some(version))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_the_up_section() {
        let migration = parse(
            "007_queue.sql",
            "-- +goose Up\nCREATE TABLE a (id TEXT);\n-- +goose StatementBegin\nCREATE TRIGGER t AFTER INSERT ON a BEGIN SELECT 1; END;\n-- +goose StatementEnd\n\n-- +goose Down\nDROP TABLE a;\n",
        )
        .expect("parse");
        assert_eq!(migration.version, 7);
        assert!(migration.in_transaction);
        assert!(migration.up.contains("CREATE TABLE a"));
        assert!(migration.up.contains("CREATE TRIGGER t"));
        assert!(!migration.up.contains("DROP TABLE"));
        assert!(!migration.up.contains("+goose"));
    }

    #[test]
    fn honors_no_transaction() {
        let migration = parse(
            "012_x.sql",
            "-- +goose NO TRANSACTION\n-- +goose Up\nVACUUM;\n",
        )
        .expect("parse");
        assert!(!migration.in_transaction);
    }

    #[test]
    fn rejects_files_without_a_version_or_up_section() {
        assert!(parse("init.sql", "-- +goose Up\n").is_err());
        assert!(parse("001_x.sql", "CREATE TABLE a (id TEXT);").is_err());
    }

    #[tokio::test]
    async fn applies_pending_migrations_once_and_records_them_like_goose() {
        let mut connection = SqliteConnection::connect("sqlite::memory:")
            .await
            .expect("connect");
        let migrations = vec![
            parse("001_a.sql", "-- +goose Up\nCREATE TABLE a (id TEXT);\n").expect("a"),
            parse("002_b.sql", "-- +goose Up\nCREATE TABLE b (id TEXT);\n").expect("b"),
        ];

        assert_eq!(
            up(&mut connection, &migrations).await.expect("first"),
            [1, 2]
        );
        assert!(
            up(&mut connection, &migrations)
                .await
                .expect("second")
                .is_empty()
        );

        let rows: Vec<(i64, i64)> =
            sqlx::query_as("SELECT version_id, is_applied FROM goose_db_version ORDER BY id")
                .fetch_all(&mut connection)
                .await
                .expect("rows");
        assert_eq!(rows, [(0, 1), (1, 1), (2, 1)]);
    }

    #[tokio::test]
    async fn a_failing_migration_leaves_nothing_behind() {
        let mut connection = SqliteConnection::connect("sqlite::memory:")
            .await
            .expect("connect");
        let migrations = vec![
            parse(
                "001_a.sql",
                "-- +goose Up\nCREATE TABLE a (id TEXT);\nNOT SQL;\n",
            )
            .expect("a"),
        ];

        assert!(up(&mut connection, &migrations).await.is_err());
        let table: Option<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_master WHERE name = 'a'")
                .fetch_optional(&mut connection)
                .await
                .expect("query");
        assert_eq!(table, None);
    }

    #[tokio::test]
    async fn every_repository_migration_applies_to_an_empty_database() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../server/migrations");
        let migrations = load(&directory).expect("load");
        let mut connection = SqliteConnection::connect("sqlite::memory:")
            .await
            .expect("connect");
        crate::searchdata::register_on(&mut connection)
            .await
            .expect("register search functions");

        let applied = up(&mut connection, &migrations).await.expect("migrate");
        assert_eq!(applied.len(), migrations.len());
    }
}
