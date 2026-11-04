//! Username and password sign-in (ADR 0042), on only with `AUTH_USERNAME`
//! and `AUTH_PASSWORD` or `AUTH_PASSWORD_HASH`: a session cookie for browsers, Basic for the
//! Desktop Client and scripts, both under the same attempt limits. While it
//! is on, the gate replaces the loopback Host trust of `app::loopback_host`.

mod handlers;
mod limits;
mod repository;
mod service;

pub use handlers::{SESSION_COOKIE, SignedIn, gate, routes};
pub use service::{Auth, User, hash_password};
