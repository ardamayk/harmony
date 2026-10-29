//! External Source (ADR 0032): search one site through the operator's own
//! yt-dlp and play its External Tracks in full through this server. Nothing
//! is stored, so there is no repository; the feature is off until
//! `EXTERNAL_SOURCE_SEARCH` is configured.

mod handlers;
mod service;
mod timing;
mod token;
mod yt_dlp;

pub use handlers::routes;
pub use service::{ExternalError, ExternalSource};
