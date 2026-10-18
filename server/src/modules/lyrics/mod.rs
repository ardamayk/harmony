//! Track lyrics: the file's own, else LRCLIB's, looked up online on first
//! request and stored (ADR 0029). Reads Tracks through the library's public
//! service.

mod handlers;
mod lrclib;
mod repository;
mod service;

pub use handlers::routes;
pub use service::Lyrics;
