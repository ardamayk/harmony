//! Library Search over the prepared texts of crate::searchdata, behind
//! `GET /api/v1/library/search`.

mod handlers;
mod index;
mod rank;
mod repository;
mod service;

pub use handlers::routes;
pub use service::SearchService;
