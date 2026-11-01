//! Queue invalidation events (playback/queue-events.go): each subscriber
//! holds only the latest invalidation, so a slow client catches up with one
//! event instead of a backlog. The Shared Playback State broker beside it
//! holds the latest snapshot the same way (ADR 0040).

use super::playback_state::PlaybackState;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

#[derive(Clone, Debug, PartialEq)]
pub struct Invalidation {
    pub revision: String,
    pub sequence: String,
}

/// One subscriber: its ID and the slot holding its latest invalidation.
type Subscriber = (u64, watch::Sender<Option<Invalidation>>);

#[derive(Default)]
pub struct QueueEventBroker {
    subscribers: Mutex<HashMap<String, Vec<Subscriber>>>,
    next_id: Mutex<u64>,
}

/// Unsubscribes when dropped.
pub struct Subscription {
    broker: Arc<QueueEventBroker>,
    user_id: String,
    id: u64,
    pub events: watch::Receiver<Option<Invalidation>>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut subscribers = self
            .broker
            .subscribers
            .lock()
            .expect("queue subscribers lock");
        if let Some(list) = subscribers.get_mut(&self.user_id) {
            list.retain(|(id, _)| *id != self.id);
            if list.is_empty() {
                subscribers.remove(&self.user_id);
            }
        }
    }
}

impl QueueEventBroker {
    pub fn subscribe(self: &Arc<Self>, user_id: &str) -> Subscription {
        let (sender, events) = watch::channel(None);
        let id = {
            let mut next = self.next_id.lock().expect("queue subscriber ids lock");
            *next += 1;
            *next
        };
        self.subscribers
            .lock()
            .expect("queue subscribers lock")
            .entry(user_id.to_owned())
            .or_default()
            .push((id, sender));
        Subscription {
            broker: self.clone(),
            user_id: user_id.to_owned(),
            id,
            events,
        }
    }

    pub fn publish(&self, user_id: &str, revision: &str, sequence: &str) {
        let subscribers = self.subscribers.lock().expect("queue subscribers lock");
        for (_, sender) in subscribers.get(user_id).into_iter().flatten() {
            sender.send_replace(Some(Invalidation {
                revision: revision.to_owned(),
                sequence: sequence.to_owned(),
            }));
        }
    }
}

/// One playback-state subscriber: its ID and the slot holding the snapshot
/// it has not been sent yet.
type StateSubscriber = (u64, watch::Sender<Option<Arc<PlaybackState>>>);

#[derive(Default)]
pub struct PlaybackStateBroker {
    subscribers: Mutex<HashMap<String, Vec<StateSubscriber>>>,
    next_id: Mutex<u64>,
}

/// Unsubscribes when dropped.
pub struct PlaybackStateSubscription {
    broker: Arc<PlaybackStateBroker>,
    user_id: String,
    id: u64,
    pub events: watch::Receiver<Option<Arc<PlaybackState>>>,
}

impl Drop for PlaybackStateSubscription {
    fn drop(&mut self) {
        let mut subscribers = self
            .broker
            .subscribers
            .lock()
            .expect("playback state subscribers lock");
        if let Some(list) = subscribers.get_mut(&self.user_id) {
            list.retain(|(id, _)| *id != self.id);
            if list.is_empty() {
                subscribers.remove(&self.user_id);
            }
        }
    }
}

impl PlaybackStateBroker {
    pub fn subscribe(self: &Arc<Self>, user_id: &str) -> PlaybackStateSubscription {
        let (sender, events) = watch::channel(None);
        let id = {
            let mut next = self
                .next_id
                .lock()
                .expect("playback state subscriber ids lock");
            *next += 1;
            *next
        };
        self.subscribers
            .lock()
            .expect("playback state subscribers lock")
            .entry(user_id.to_owned())
            .or_default()
            .push((id, sender));
        PlaybackStateSubscription {
            broker: self.clone(),
            user_id: user_id.to_owned(),
            id,
            events,
        }
    }

    pub fn publish(&self, user_id: &str, state: &PlaybackState) {
        let subscribers = self
            .subscribers
            .lock()
            .expect("playback state subscribers lock");
        for (_, sender) in subscribers.get(user_id).into_iter().flatten() {
            // Publishing follows the commit, so two commands can arrive reversed.
            sender.send_if_modified(|slot| {
                let is_newer = slot
                    .as_ref()
                    .is_none_or(|held| held.revision < state.revision);
                if is_newer {
                    *slot = Some(Arc::new(state.clone()));
                }
                is_newer
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscribers_see_only_the_latest_invalidation_for_their_user() {
        let broker = Arc::new(QueueEventBroker::default());
        let mut mine = broker.subscribe("me");
        let other = broker.subscribe("other");
        broker.publish("me", "1", "1");
        broker.publish("me", "2", "2");

        mine.events.changed().await.expect("changed");
        assert_eq!(
            mine.events.borrow_and_update().clone(),
            Some(Invalidation {
                revision: "2".into(),
                sequence: "2".into()
            })
        );
        assert!(!other.events.has_changed().expect("open"));
        drop(other);
        assert!(
            !broker
                .subscribers
                .lock()
                .expect("lock")
                .contains_key("other")
        );
    }

    #[tokio::test]
    async fn a_snapshot_published_late_does_not_replace_a_newer_one() {
        let broker = Arc::new(PlaybackStateBroker::default());
        let mine = broker.subscribe("me");
        let mut state = PlaybackState {
            revision: 2,
            queue_item_id: None,
            track_id: None,
            playing: false,
            position_ms: 0,
            position_at_ms: 0,
            active_device_id: None,
            active_device_name: None,
            remote_command_revision: 0,
            skip_request: None,
            server_time_ms: 1_000,
        };
        broker.publish("me", &state);
        state.revision = 1;
        broker.publish("me", &state);

        let held = mine.events.borrow().clone().expect("snapshot");
        assert_eq!(held.revision, 2);
    }

    #[tokio::test]
    async fn playback_state_subscribers_see_only_the_latest_snapshot_for_their_user() {
        let broker = Arc::new(PlaybackStateBroker::default());
        let mut mine = broker.subscribe("me");
        let other = broker.subscribe("other");
        let mut state = PlaybackState {
            revision: 1,
            queue_item_id: None,
            track_id: None,
            playing: false,
            position_ms: 0,
            position_at_ms: 0,
            active_device_id: Some("d".into()),
            active_device_name: None,
            remote_command_revision: 0,
            skip_request: None,
            server_time_ms: 1_000,
        };
        broker.publish("me", &state);
        state.revision = 2;
        state.playing = true;
        broker.publish("me", &state);

        mine.events.changed().await.expect("changed");
        let snapshot = mine.events.borrow_and_update().clone().expect("snapshot");
        assert_eq!((snapshot.revision, snapshot.playing), (2, true));
        assert!(!other.events.has_changed().expect("open"));
        drop(other);
        assert!(
            !broker
                .subscribers
                .lock()
                .expect("lock")
                .contains_key("other")
        );
    }
}
