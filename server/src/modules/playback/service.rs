//! Queue rules: the item limit, Track existence, item sources, and
//! resolving stored Track IDs into library Tracks.

use super::repository::{MAX_QUEUE_ITEMS, QueueError, Repository};
use super::sources::{self, QueueItemSource};
use crate::app::AppState;
use crate::modules::library::{LibraryError, LibraryService, Track};
use serde::Serialize;
use serde_json::value::RawValue;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QueueItem {
    id: String,
    track_id: String,
    position: i64,
    track: Track,
    source: QueueItemSourceOut,
}

/// A source as Go marshals it in responses (basedOn kept for suggestions).
struct QueueItemSourceOut(QueueItemSource);

impl Serialize for QueueItemSourceOut {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let encoded = sources::encode(&self.0).map_err(serde::ser::Error::custom)?;
        RawValue::from_string(encoded)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

#[derive(Serialize)]
pub struct Queue {
    items: Vec<QueueItem>,
    pub revision: String,
    #[serde(skip)]
    pub event_sequence: String,
}

pub struct Service {
    repo: Repository,
    tracks: LibraryService,
}

impl Service {
    pub fn new(state: &AppState) -> Self {
        Self {
            repo: Repository::new(state.db.clone()),
            tracks: LibraryService::new(state.db.clone()),
        }
    }

    /// The queue, skipping items whose Track no longer exists.
    pub async fn get_queue(&self, user_id: &str) -> Result<Queue, QueueError> {
        let snapshot = self.repo.read(user_id).await?;
        let mut items = Vec::with_capacity(snapshot.items.len());
        for row in snapshot.items {
            let source = sources::parse_json(&row.source).map_err(|error| {
                QueueError::Other(format!("decode queue item {:?} source: {error}", row.id))
            })?;
            let track = match self.tracks.get_track(&row.track_id).await {
                Ok(track) => track,
                Err(LibraryError::NotFound) => continue,
                Err(error) => {
                    return Err(QueueError::Other(format!(
                        "resolve queue track {:?}: {error}",
                        row.track_id
                    )));
                }
            };
            items.push(QueueItem {
                id: row.id,
                track_id: row.track_id,
                position: row.position,
                track,
                source: QueueItemSourceOut(source),
            });
        }
        Ok(Queue {
            items,
            revision: snapshot.revision,
            event_sequence: snapshot.event_sequence,
        })
    }

    pub async fn replace(
        &self,
        user_id: &str,
        revision: &str,
        track_ids: &[String],
        source: &QueueItemSource,
    ) -> Result<(), QueueError> {
        if track_ids.len() as i64 > MAX_QUEUE_ITEMS {
            return Err(QueueError::LimitExceeded);
        }
        let encoded = sources::encode(source).map_err(QueueError::InvalidSource)?;
        for track_id in track_ids {
            self.require_track(track_id).await?;
        }
        self.repo
            .replace(user_id, revision, track_ids, &encoded)
            .await
    }

    pub async fn append(
        &self,
        user_id: &str,
        revision: &str,
        track_id: &str,
        source: &QueueItemSource,
    ) -> Result<(), QueueError> {
        let encoded = sources::encode(source).map_err(QueueError::InvalidSource)?;
        self.require_track(track_id).await?;
        self.repo
            .append(user_id, revision, track_id, &encoded)
            .await
    }

    pub async fn remove(
        &self,
        user_id: &str,
        revision: &str,
        item_id: &str,
    ) -> Result<(), QueueError> {
        self.repo.remove(user_id, revision, item_id).await
    }

    pub async fn clear(
        &self,
        user_id: &str,
        revision: &str,
        after_item_id: &str,
    ) -> Result<(), QueueError> {
        self.repo.clear(user_id, revision, after_item_id).await
    }

    /// Reorders only when `item_ids` is a permutation of the current items.
    pub async fn reorder(
        &self,
        user_id: &str,
        revision: &str,
        item_ids: &[String],
    ) -> Result<(), QueueError> {
        self.repo
            .reorder(user_id, revision, item_ids, |current| {
                same_items(current, item_ids)
            })
            .await
    }

    pub async fn track_file_path(&self, track_id: &str) -> Result<String, LibraryError> {
        self.tracks.get_track_file_path(track_id).await
    }

    async fn require_track(&self, track_id: &str) -> Result<(), QueueError> {
        self.tracks
            .get_track(track_id)
            .await
            .map(|_| ())
            .map_err(|_| QueueError::TrackNotFound)
    }
}

fn same_items(current: &[String], requested: &[String]) -> bool {
    if current.len() != requested.len() {
        return false;
    }
    let mut seen: std::collections::HashSet<&String> = current.iter().collect();
    requested.iter().all(|item_id| seen.remove(item_id)) && seen.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorders_must_be_permutations() {
        let ids = |values: &[&str]| -> Vec<String> {
            values.iter().map(|value| (*value).to_owned()).collect()
        };
        assert!(same_items(&ids(&["a", "b", "c"]), &ids(&["c", "a", "b"])));
        assert!(!same_items(&ids(&["a", "b"]), &ids(&["a", "a"])));
        assert!(!same_items(&ids(&["a", "b"]), &ids(&["a"])));
        assert!(!same_items(&ids(&["a", "b"]), &ids(&["a", "x"])));
    }
}
