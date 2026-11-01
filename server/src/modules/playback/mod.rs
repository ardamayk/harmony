//! Playback: each user's queue with revisions, its invalidation events over
//! SSE, the Shared Playback State one Active Device reports (ADR 0040), and
//! Track file streaming.

mod events;
mod handlers;
mod playback_state;
mod repository;
mod serve_file;
mod service;
mod sources;

pub use events::{PlaybackStateBroker, QueueEventBroker};
pub use handlers::routes;
pub use playback_state::{Command, PlaybackState, SkipDirection, StoredState};
