//! Each user's queue (playback/repository.go). Every mutation runs in one
//! IMMEDIATE transaction that first advances the revision, so a stale
//! expected revision fails with a conflict before anything changes.

use super::playback_state::{ReportError, SkipDirection, StoredState};
use crate::db::{self, Db, Tx};

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("not found")]
    NotFound,
    #[error("queue revision conflict")]
    RevisionConflict,
    #[error("queue order does not match current items")]
    InvalidOrder,
    #[error("queue exceeds maximum of {MAX_QUEUE_ITEMS} items")]
    LimitExceeded,
    #[error("{0}")]
    InvalidSource(String),
    #[error("track not found")]
    TrackNotFound,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("{0}")]
    Other(String),
}

pub const MAX_QUEUE_ITEMS: i64 = 1000;

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("the report came from a device that is no longer the Active Device")]
    DeviceNotActive,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl From<ReportError> for StateError {
    fn from(error: ReportError) -> Self {
        match error {
            ReportError::NotActive => Self::DeviceNotActive,
        }
    }
}

pub struct ItemRow {
    pub id: String,
    pub track_id: String,
    pub position: i64,
    pub source: String,
}

pub struct Snapshot {
    pub items: Vec<ItemRow>,
    pub revision: String,
    pub event_sequence: String,
}

#[derive(Clone)]
pub struct Repository {
    pool: Db,
}

async fn item_ids(transaction: &mut Tx, user_id: &str) -> Result<Vec<String>, QueueError> {
    Ok(sqlx::query_scalar!(
        r#"SELECT id AS "id!" FROM playback_queue WHERE user_id = ? ORDER BY position"#,
        user_id
    )
    .fetch_all(&mut **transaction)
    .await?)
}

async fn insert_item(
    transaction: &mut Tx,
    user_id: &str,
    position: i64,
    track_id: &str,
    source: &str,
) -> Result<(), QueueError> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query!(
        "INSERT INTO playback_queue (id, user_id, position, track_id, source) VALUES (?, ?, ?, ?, ?)",
        id,
        user_id,
        position,
        track_id,
        source
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn delete_item(
    transaction: &mut Tx,
    user_id: &str,
    item_id: &str,
) -> Result<u64, QueueError> {
    Ok(sqlx::query!(
        "DELETE FROM playback_queue WHERE id = ? AND user_id = ?",
        item_id,
        user_id
    )
    .execute(&mut **transaction)
    .await?
    .rows_affected())
}

async fn clear(transaction: &mut Tx, user_id: &str) -> Result<(), QueueError> {
    sqlx::query!("DELETE FROM playback_queue WHERE user_id = ?", user_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn set_positions(
    transaction: &mut Tx,
    user_id: &str,
    item_ids: &[String],
) -> Result<(), QueueError> {
    for (position, item_id) in item_ids.iter().enumerate() {
        let position = position as i64;
        sqlx::query!(
            "UPDATE playback_queue SET position = ? WHERE id = ? AND user_id = ?",
            position,
            item_id,
            user_id
        )
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

impl Repository {
    pub fn new(pool: Db) -> Self {
        Self { pool }
    }

    pub async fn read(&self, user_id: &str) -> Result<Snapshot, QueueError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| QueueError::Other(format!("begin queue read: {error}")))?;
        let state = sqlx::query!(
            r#"SELECT
                CAST(COALESCE((SELECT revision FROM playback_queue_state s WHERE s.user_id = ?1), 0) AS TEXT) AS "revision!: String",
                CAST(COALESCE((SELECT event_sequence FROM playback_queue_state s WHERE s.user_id = ?1), 0) AS TEXT) AS "event_sequence!: String""#,
            user_id
        )
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| QueueError::Other(format!("get queue revision: {error}")))?;
        let items = sqlx::query_as!(
            ItemRow,
            r#"SELECT id AS "id!", track_id, position, source FROM playback_queue
            WHERE user_id = ? ORDER BY position"#,
            user_id
        )
        .fetch_all(&mut *transaction)
        .await
        .map_err(|error| QueueError::Other(format!("get queue: {error}")))?;
        transaction.commit().await?;
        Ok(Snapshot {
            items,
            revision: state.revision,
            event_sequence: state.event_sequence,
        })
    }

    async fn begin_mutation(
        &self,
        user_id: &str,
        expected_revision: &str,
    ) -> Result<Tx, QueueError> {
        let mut transaction = db::begin_write(&self.pool).await?;
        sqlx::query!(
            "INSERT OR IGNORE INTO playback_queue_state (user_id, revision) VALUES (?, 0)",
            user_id
        )
        .execute(&mut *transaction)
        .await?;
        let advanced = sqlx::query!(
            "UPDATE playback_queue_state
            SET revision = revision + 1, event_sequence = event_sequence + 1
            WHERE user_id = ? AND CAST(revision AS TEXT) = CAST(? AS TEXT)",
            user_id,
            expected_revision
        )
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if advanced == 0 {
            return Err(QueueError::RevisionConflict);
        }
        Ok(transaction)
    }

    pub async fn replace(
        &self,
        user_id: &str,
        expected_revision: &str,
        track_ids: &[String],
        source: &str,
    ) -> Result<(), QueueError> {
        let mut transaction = self.begin_mutation(user_id, expected_revision).await?;
        clear(&mut transaction, user_id).await?;
        for (position, track_id) in track_ids.iter().enumerate() {
            insert_item(&mut transaction, user_id, position as i64, track_id, source).await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn append(
        &self,
        user_id: &str,
        expected_revision: &str,
        track_id: &str,
        source: &str,
    ) -> Result<(), QueueError> {
        let mut transaction = self.begin_mutation(user_id, expected_revision).await?;
        let slot = sqlx::query!(
            r#"SELECT COUNT(*) AS "item_count!: i64", CAST(COALESCE(MAX(position) + 1, 0) AS INTEGER) AS "next_position!: i64"
            FROM playback_queue WHERE user_id = ?"#,
            user_id
        )
        .fetch_one(&mut *transaction)
        .await?;
        if slot.item_count >= MAX_QUEUE_ITEMS {
            return Err(QueueError::LimitExceeded);
        }
        insert_item(
            &mut transaction,
            user_id,
            slot.next_position,
            track_id,
            source,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Deletes the item and closes the gap in positions.
    pub async fn remove(
        &self,
        user_id: &str,
        expected_revision: &str,
        item_id: &str,
    ) -> Result<(), QueueError> {
        let mut transaction = self.begin_mutation(user_id, expected_revision).await?;
        if delete_item(&mut transaction, user_id, item_id).await? == 0 {
            return Err(QueueError::NotFound);
        }
        let remaining = item_ids(&mut transaction, user_id).await?;
        set_positions(&mut transaction, user_id, &remaining).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Deletes every item after `after_item_id` (all of them when empty);
    /// the rest keep their IDs and positions.
    pub async fn clear(
        &self,
        user_id: &str,
        expected_revision: &str,
        after_item_id: &str,
    ) -> Result<(), QueueError> {
        let mut transaction = self.begin_mutation(user_id, expected_revision).await?;
        if after_item_id.is_empty() {
            clear(&mut transaction, user_id).await?;
        } else {
            let current = item_ids(&mut transaction, user_id).await?;
            let index = current
                .iter()
                .position(|item_id| item_id == after_item_id)
                .ok_or(QueueError::NotFound)?;
            for item_id in &current[index + 1..] {
                delete_item(&mut transaction, user_id, item_id).await?;
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Applies `item_ids` as the order once `accept` approves the current IDs.
    pub async fn reorder(
        &self,
        user_id: &str,
        expected_revision: &str,
        item_ids_in_order: &[String],
        accept: impl FnOnce(&[String]) -> bool,
    ) -> Result<(), QueueError> {
        let mut transaction = self.begin_mutation(user_id, expected_revision).await?;
        let current = item_ids(&mut transaction, user_id).await?;
        if !accept(&current) {
            return Err(QueueError::InvalidOrder);
        }
        // Below zero first, so renumbering cannot hit UNIQUE(user_id, position).
        sqlx::query!(
            "UPDATE playback_queue SET position = -position - 1 WHERE user_id = ?",
            user_id
        )
        .execute(&mut *transaction)
        .await?;
        set_positions(&mut transaction, user_id, item_ids_in_order).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// The Shared Playback State row, or the empty state when the user has
    /// none yet.
    pub async fn read_state(&self, user_id: &str) -> Result<StoredState, StateError> {
        Ok(state_row(&self.pool, user_id)
            .await?
            .map(StoredState::from)
            .unwrap_or_default())
    }

    /// Reads the row, applies `apply` and writes it back, all inside one
    /// IMMEDIATE transaction, so two commands for the same user cannot
    /// interleave their read and write. An error from `apply` writes nothing.
    pub async fn update_state<R>(
        &self,
        user_id: &str,
        apply: impl FnOnce(StoredState) -> Result<(StoredState, R), StateError>,
    ) -> Result<R, StateError> {
        let mut transaction = db::begin_write(&self.pool).await?;
        let stored = state_row(&mut *transaction, user_id)
            .await?
            .map(StoredState::from)
            .unwrap_or_default();
        let (stored, result) = apply(stored)?;
        write_state(&mut transaction, user_id, &stored).await?;
        transaction.commit().await?;
        Ok(result)
    }
}

/// The row as the database holds it, before the skip direction is parsed.
struct StateRow {
    queue_item_id: Option<String>,
    track_id: Option<String>,
    playing: bool,
    position_ms: i64,
    position_at_ms: i64,
    last_report_at_ms: i64,
    active_device_id: Option<String>,
    active_device_name: Option<String>,
    revision: i64,
    remote_command_revision: i64,
    skip_revision: Option<i64>,
    skip_direction: Option<String>,
}

impl From<StateRow> for StoredState {
    fn from(row: StateRow) -> Self {
        Self {
            queue_item_id: row.queue_item_id,
            track_id: row.track_id,
            playing: row.playing,
            position_ms: row.position_ms,
            position_at_ms: row.position_at_ms,
            last_report_at_ms: row.last_report_at_ms,
            active_device_id: row.active_device_id,
            active_device_name: row.active_device_name,
            revision: row.revision,
            remote_command_revision: row.remote_command_revision,
            skip_revision: row.skip_revision,
            // The CHECK constraint keeps the column to the two directions;
            // anything else reads as no skip at all.
            skip_direction: row.skip_direction.as_deref().and_then(SkipDirection::parse),
        }
    }
}

async fn state_row(
    executor: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
    user_id: &str,
) -> Result<Option<StateRow>, sqlx::Error> {
    sqlx::query_as!(
        StateRow,
        r#"SELECT
            queue_item_id AS "queue_item_id?",
            track_id AS "track_id?",
            playing AS "playing!: bool",
            position_ms AS "position_ms!: i64",
            position_at_ms AS "position_at_ms!: i64",
            last_report_at_ms AS "last_report_at_ms!: i64",
            active_device_id AS "active_device_id?",
            active_device_name AS "active_device_name?",
            revision AS "revision!: i64",
            remote_command_revision AS "remote_command_revision!: i64",
            skip_revision AS "skip_revision?",
            skip_direction AS "skip_direction?"
        FROM playback_state WHERE user_id = ?"#,
        user_id
    )
    .fetch_optional(executor)
    .await
}

async fn write_state(
    transaction: &mut Tx,
    user_id: &str,
    stored: &StoredState,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO playback_state (
            user_id, queue_item_id, track_id, playing, position_ms, position_at_ms,
            last_report_at_ms, active_device_id, active_device_name, revision,
            remote_command_revision, skip_revision, skip_direction
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT (user_id) DO UPDATE SET
            queue_item_id = excluded.queue_item_id,
            track_id = excluded.track_id,
            playing = excluded.playing,
            position_ms = excluded.position_ms,
            position_at_ms = excluded.position_at_ms,
            last_report_at_ms = excluded.last_report_at_ms,
            active_device_id = excluded.active_device_id,
            active_device_name = excluded.active_device_name,
            revision = excluded.revision,
            remote_command_revision = excluded.remote_command_revision,
            skip_revision = excluded.skip_revision,
            skip_direction = excluded.skip_direction"#,
        user_id,
        stored.queue_item_id,
        stored.track_id,
        stored.playing,
        stored.position_ms,
        stored.position_at_ms,
        stored.last_report_at_ms,
        stored.active_device_id,
        stored.active_device_name,
        stored.revision,
        stored.remote_command_revision,
        stored.skip_revision,
        stored.skip_direction.map(|direction| direction.as_str()),
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::playback_state::Playback;
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
             VALUES ('t1', 'al', 'One', 'one', 'A', 'flac', '/1.flac')",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        (dir, Repository::new(pool))
    }

    #[tokio::test]
    async fn a_missing_row_reads_as_the_empty_state() {
        let (_dir, repo) = repository().await;
        assert_eq!(repo.read_state("u").await.unwrap(), StoredState::default());
    }

    #[tokio::test]
    async fn update_state_applies_and_persists_inside_one_transaction() {
        let (_dir, repo) = repository().await;
        let revision = repo
            .update_state("u", |mut stored| {
                stored.claim(
                    "d1",
                    "Desktop",
                    Playback {
                        queue_item_id: None,
                        track_id: Some("t1".into()),
                        position_ms: 5,
                        playing: true,
                    },
                    1_000,
                );
                let revision = stored.revision;
                Ok((stored, revision))
            })
            .await
            .unwrap();
        assert_eq!(revision, 1);
        let stored = repo.read_state("u").await.unwrap();
        assert_eq!(stored.active_device_id.as_deref(), Some("d1"));
        assert_eq!((stored.position_ms, stored.playing), (5, true));
        // A refused apply leaves the stored row untouched.
        let refused = repo
            .update_state("u", |_stored| {
                Err::<(StoredState, ()), _>(StateError::DeviceNotActive)
            })
            .await;
        assert!(matches!(refused, Err(StateError::DeviceNotActive)));
        assert_eq!(repo.read_state("u").await.unwrap().revision, 1);
    }

    #[tokio::test]
    async fn deleting_the_track_nulls_the_state_track_id() {
        let (_dir, repo) = repository().await;
        repo.update_state("u", |mut stored| {
            stored.claim(
                "d1",
                "Desktop",
                Playback {
                    queue_item_id: Some("q1".into()),
                    track_id: Some("t1".into()),
                    position_ms: 0,
                    playing: false,
                },
                1_000,
            );
            Ok((stored, ()))
        })
        .await
        .unwrap();
        sqlx::query("DELETE FROM tracks WHERE id = 't1'")
            .execute(&repo.pool)
            .await
            .unwrap();
        let stored = repo.read_state("u").await.unwrap();
        assert_eq!(stored.track_id, None);
        // The Queue item has no foreign key and outlives its item.
        assert_eq!(stored.queue_item_id.as_deref(), Some("q1"));
    }
}
