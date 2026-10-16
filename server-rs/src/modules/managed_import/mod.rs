//! Managed Import (server/internal/modules/managedimport): uploads, the
//! Strict Import Profile, Import Previews, commits into Managed Storage,
//! Track Replacement and Permanent Track Deletion.

mod commit;
pub mod errors;
mod handlers;
mod history;
mod identity;
pub mod model;
mod plan;
mod preview;
mod replacement;
mod repository;
pub mod service;
pub mod storage;
pub mod types;

pub use handlers::routes;

use crate::config::Config;
use std::sync::Arc;

/// Everything Managed Import keeps across requests.
pub struct ManagedImport {
    pub service: Arc<service::Service>,
}

impl ManagedImport {
    pub fn new(pool: sqlx::SqlitePool, config: &Config) -> Self {
        let storage = storage::Storage::new(
            &config.managed_storage_path,
            storage::Limits {
                reserve_bytes: config.managed_storage_reserve_bytes,
                file_bytes: config.managed_import_file_limit_bytes,
                batch_bytes: config.managed_import_batch_limit_bytes,
            },
        );
        let service = service::Service::new(repository::Repository::new(pool), Arc::new(storage));
        Self {
            service: Arc::new(service),
        }
    }
}

#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod storage_tests;
