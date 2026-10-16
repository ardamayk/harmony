//! Queue invalidation events (playback/queue-events.go): each subscriber
//! holds only the latest invalidation, so a slow client catches up with one
//! event instead of a backlog.

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
}
