//! Each user's queue (playback/repository.go). Every mutation runs in one
//! IMMEDIATE transaction that first advances the revision, so a stale
//! expected revision fails with a conflict before anything changes.

use crate::db;
use sqlx::{Sqlite, SqlitePool, Transaction};

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
    pool: SqlitePool,
}

type Tx = Transaction<'static, Sqlite>;

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
    pub fn new(pool: SqlitePool) -> Self {
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
}
