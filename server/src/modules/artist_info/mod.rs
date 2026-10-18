//! Artist photos and popular Tracks from Deezer and biographies from
//! Wikipedia, looked up online on first request and stored (ADR 0027). Reads
//! the Artist, their Albums and Tracks through the library's public service.

mod deezer;
mod fetch;
mod handlers;
mod popular;
mod repository;
mod service;
mod wikipedia;

pub use handlers::routes;
pub use service::{ArtistInfo, Sources};
