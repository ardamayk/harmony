//! Listen Together: one Room on the Owner's Music Server, held in memory, so
//! a restart ends it (ADR 0037). Guests reach it through the Guest Listener,
//! whose allowlist lives in `gate`. Reads Tracks through the library's public
//! service.

mod gate;
mod handlers;
mod room_state;
mod service;

pub use gate::guest_gate;
pub use handlers::{guest_routes, owner_routes};
pub use room_state::{Command, RoomState, SkipDirection};
pub use service::{Client, GuestSession, ListenTogether, RoomError, Timings};
