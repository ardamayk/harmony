//! Soulseek Acquisition (ADR 0036): search the Soulseek network through the
//! operator's own slskd and hand a chosen Acquisition Version to Managed
//! Import. The feature is off until `SLSKD_URL`, `SLSKD_API_KEY` and
//! `SLSKD_DOWNLOADS_PATH` are configured.

mod handlers;
mod service;
mod slskd;
mod versions;

pub use handlers::routes;
pub use service::{
    ResolvedVersion, Sharing, Soulseek, SoulseekError, SoulseekSearch, Status, probe_dependency,
};
pub use slskd::Application;
pub use versions::{Quality, RemoteFile, Scope, Source, Version};
