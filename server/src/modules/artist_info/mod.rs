//! Artist photos from Deezer and biographies from Wikipedia, looked up online
//! on first request and stored (ADR 0027). Reads the Artist and their Albums
//! through the library's public service.

mod deezer;
mod fetch;
mod handlers;
mod repository;
mod service;
mod wikipedia;

pub use handlers::routes;
pub use service::{ArtistInfo, Sources};
