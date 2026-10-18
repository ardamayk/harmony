//! Library reads: Artists, Albums, Tracks, covers, lyrics and waveforms.
//! Ingestion, deletion and replacement belong to Managed Import.

mod covers;
pub mod genres;
mod handlers;
mod repository;
mod service;
mod types;
mod waveform;

pub use genres::split_genres;
pub use handlers::routes;
pub use repository::LibraryError;
pub use service::Service as LibraryService;
pub use types::{Artist, Track};
pub use waveform::WaveformService;
