//! Soulseek Acquisition's domain rules: whether the operator configured
//! slskd, and what slskd's state means for the listener.

use super::slskd::{Application, Share, Slskd};
use crate::config::SoulseekConfig;
use crate::dependencies::{self, Dependency};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SoulseekError {
    #[error("Soulseek Acquisition is not configured on this server")]
    Disabled,
    #[error("Soulseek Acquisition is not implemented yet")]
    NotImplemented,
}

/// slskd as the settings show it. `slskd` is `None` when slskd did not
/// answer with the configured API key.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub slskd: Option<Application>,
    pub sharing: Sharing,
}

/// Whether slskd shares Managed Storage (Soulseek Sharing).
#[derive(Clone, Debug, PartialEq)]
pub enum Sharing {
    /// `file_count` is known only when a share is Managed Storage itself
    /// rather than a directory above it.
    Shared {
        file_count: Option<u64>,
    },
    NotShared,
    /// slskd did not answer.
    Unknown,
}

pub struct Soulseek {
    config: Option<SoulseekConfig>,
    slskd: Option<Slskd>,
    managed_storage_path: PathBuf,
}

impl Soulseek {
    pub fn new(config: Option<&SoulseekConfig>, managed_storage_path: &str) -> Self {
        Self {
            config: config.cloned(),
            slskd: config.map(Slskd::new),
            managed_storage_path: PathBuf::from(managed_storage_path),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.is_some()
    }

    pub fn ensure_enabled(&self) -> Result<&SoulseekConfig, SoulseekError> {
        self.config.as_ref().ok_or(SoulseekError::Disabled)
    }

    fn slskd(&self) -> Result<&Slskd, SoulseekError> {
        self.slskd.as_ref().ok_or(SoulseekError::Disabled)
    }

    /// slskd's reachability, login and Soulseek Sharing. It never fails once
    /// enabled: an unreachable slskd is part of the answer.
    pub async fn status(&self) -> Result<Status, SoulseekError> {
        let slskd = self.slskd()?;
        let (application, shares) = tokio::join!(slskd.application(), slskd.shares());
        let Ok(application) = application else {
            return Ok(Status {
                slskd: None,
                sharing: Sharing::Unknown,
            });
        };
        let sharing = match shares {
            Ok(shares) => sharing(&shares, &self.managed_storage_paths()),
            Err(_) => Sharing::Unknown,
        };
        Ok(Status {
            slskd: Some(application),
            sharing,
        })
    }

    /// Managed Storage's path as slskd is assumed to see it: the configured
    /// absolute path and its resolved form. slskd must see Managed Storage
    /// at the same path as the Music Server (for example the same bind
    /// mount in both containers); there is no setting for a second path.
    fn managed_storage_paths(&self) -> Vec<PathBuf> {
        let configured = &self.managed_storage_path;
        let mut paths = Vec::with_capacity(2);
        if configured.is_absolute() {
            paths.push(configured.clone());
        }
        if let Ok(resolved) = std::fs::canonicalize(configured)
            && !paths.contains(&resolved)
        {
            paths.push(resolved);
        }
        paths
    }
}

/// Managed Storage is shared when a share is Managed Storage or a directory
/// above it, and no exclusion covers it. A share inside Managed Storage
/// shares only part of it, which does not count.
fn sharing(shares: &[Share], managed_storage: &[PathBuf]) -> Sharing {
    let covers = |share: &Share| {
        managed_storage
            .iter()
            .any(|path| path.starts_with(&share.local_path))
    };
    if shares.iter().any(|share| share.excluded && covers(share)) {
        return Sharing::NotShared;
    }
    let covering: Vec<&Share> = shares
        .iter()
        .filter(|share| !share.excluded && covers(share))
        .collect();
    if covering.is_empty() {
        return Sharing::NotShared;
    }
    let exact: Vec<&Share> = covering
        .into_iter()
        .filter(|share| {
            managed_storage
                .iter()
                .any(|path| path == Path::new(&share.local_path))
        })
        .collect();
    let file_count = if exact.is_empty() {
        None
    } else {
        exact.iter().map(|share| share.files).sum()
    };
    Sharing::Shared { file_count }
}

/// slskd as an optional Server Dependency: available when it answers with
/// the configured API key, with its version.
pub async fn probe_dependency(config: &SoulseekConfig) -> Dependency {
    let application = Slskd::new(config).application().await.ok();
    Dependency {
        name: dependencies::SLSKD.to_owned(),
        required: false,
        available: application.is_some(),
        version: application
            .and_then(|application| application.version)
            .unwrap_or_default(),
    }
}
