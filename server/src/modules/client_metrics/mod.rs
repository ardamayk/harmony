//! How the Web and Desktop Clients feel, reported by the clients and passed
//! to the owner's Sentry as metrics (ADR 0033). Clients never talk to Sentry
//! themselves, and with `SENTRY_DSN` unset this does nothing. Only the Owner's
//! listener takes them.

mod handlers;

pub use handlers::routes;
