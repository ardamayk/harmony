//! Soulseek Acquisition's domain rules: whether the operator configured
//! slskd, and what slskd's state means for the listener.

use super::repository::Repository;
use super::slskd::{Application, Share, Slskd, SlskdError};
use super::versions::{self, Scope, Version};
use crate::config::SoulseekConfig;
use crate::dependencies::{self, Dependency};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// The longest Soulseek query, as for External Source searches.
const MAX_QUERY_CHARS: usize = 200;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SoulseekError {
    #[error("Soulseek Acquisition is not configured on this server")]
    Disabled,
    #[error("{0}")]
    BadRequest(String),
    /// slskd did not answer; the message says why.
    #[error("{0}")]
    SlskdUnreachable(String),
    #[error("slskd is not logged in to Soulseek; check its Soulseek account")]
    NotLoggedIn,
    #[error("slskd no longer knows this search; start a new one")]
    SearchNotFound,
    #[error("the search has no such Acquisition Version or source")]
    VersionNotFound,
    #[error("there is no such Soulseek Acquisition")]
    AcquisitionNotFound,
    /// The acquisition cannot do that now; the message says why.
    #[error("{0}")]
    Conflict(String),
    /// A database or Managed Import failure, logged and answered as 500.
    #[error("{0}")]
    Internal(String),
}

impl From<SlskdError> for SoulseekError {
    fn from(error: SlskdError) -> Self {
        match error {
            SlskdError::Unreachable(_) | SlskdError::Refused(_) => {
                Self::SlskdUnreachable(error.to_string())
            }
            SlskdError::NotLoggedIn => Self::NotLoggedIn,
            SlskdError::NotFound => Self::SearchNotFound,
        }
    }
}

/// A search as the listener sees it: its Acquisition Versions so far.
#[derive(Clone, Debug, PartialEq)]
pub struct SoulseekSearch {
    pub id: Uuid,
    pub query: String,
    pub scope: Scope,
    /// False while slskd is still collecting responses.
    pub complete: bool,
    pub versions: Vec<Version>,
}

/// One version of a search, ready to acquire: its sources in the order to
/// try them, each with its remote files and cover images.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedVersion {
    pub search_id: Uuid,
    pub query: String,
    pub scope: Scope,
    pub version: Version,
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
    repo: Repository,
    /// Serializes the worker's steps with starting and cancelling, so each
    /// sees an acquisition between steps.
    acquisitions: tokio::sync::Mutex<()>,
    /// Wakes the worker early, when an acquisition starts.
    pub(super) wake: tokio::sync::Notify,
}

impl Soulseek {
    pub fn new(
        config: Option<&SoulseekConfig>,
        managed_storage_path: &str,
        pool: sqlx::SqlitePool,
    ) -> Self {
        Self {
            config: config.cloned(),
            slskd: config.map(Slskd::new),
            managed_storage_path: PathBuf::from(managed_storage_path),
            repo: Repository::new(pool),
            acquisitions: tokio::sync::Mutex::new(()),
            wake: tokio::sync::Notify::new(),
        }
    }

    pub(super) fn repo(&self) -> &Repository {
        &self.repo
    }

    pub(super) async fn acquisition_lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.acquisitions.lock().await
    }

    pub(super) fn slskd_client(&self) -> Option<&Slskd> {
        self.slskd.as_ref()
    }

    /// slskd's completed downloads as this server sees them.
    pub(super) fn downloads_path(&self) -> Option<&Path> {
        self.config
            .as_ref()
            .map(|config| config.downloads_path.as_path())
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

    /// Starts an slskd search for `query`. The search is not stored: its id
    /// is slskd's, and it carries the scope (see [`search_id`]).
    pub async fn start_search(&self, query: &str, scope: Scope) -> Result<Uuid, SoulseekError> {
        let slskd = self.slskd()?;
        let query: String = query
            .chars()
            .filter(|character| !character.is_control())
            .collect();
        let query = query.trim();
        if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
            return Err(SoulseekError::BadRequest(format!(
                "query must be 1 to {MAX_QUERY_CHARS} characters"
            )));
        }
        let id = search_id(scope);
        slskd.start_search(id, query).await?;
        Ok(id)
    }

    /// A search's state and its Acquisition Versions so far.
    pub async fn search(&self, search_id: &str) -> Result<SoulseekSearch, SoulseekError> {
        let slskd = self.slskd()?;
        let (id, scope) = parse_search_id(search_id).ok_or(SoulseekError::SearchNotFound)?;
        let search = slskd.search(id).await?;
        Ok(SoulseekSearch {
            id,
            query: search.search_text,
            scope,
            complete: search.is_complete,
            versions: versions::group(scope, &search.responses),
        })
    }

    /// Reads the search again and finds one version, for starting an
    /// acquisition. With `source_id`, that source comes first and the rest
    /// keep their order.
    pub async fn resolve_version(
        &self,
        search_id: &str,
        version_key: &str,
        source_id: Option<&str>,
    ) -> Result<ResolvedVersion, SoulseekError> {
        let search = self.search(search_id).await?;
        let mut version = search
            .versions
            .into_iter()
            .find(|version| version.key == version_key)
            .ok_or(SoulseekError::VersionNotFound)?;
        if let Some(source_id) = source_id {
            let index = version
                .sources
                .iter()
                .position(|source| source.id == source_id)
                .ok_or(SoulseekError::VersionNotFound)?;
            let chosen = version.sources.remove(index);
            version.sources.insert(0, chosen);
        }
        Ok(ResolvedVersion {
            search_id: search.id,
            query: search.query,
            scope: search.scope,
            version,
        })
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

/// A new search id. Searches are not stored, so the id carries the scope:
/// a UUID of version 8 (custom) whose last bit is 0 for Album and 1 for
/// Track scope; the other bits are random. An id of another version is not
/// a search this server started.
fn search_id(scope: Scope) -> Uuid {
    let mut bytes = *Uuid::new_v4().as_bytes();
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[15] = (bytes[15] & !1) | u8::from(scope == Scope::Track);
    Uuid::from_bytes(bytes)
}

fn parse_search_id(search_id: &str) -> Option<(Uuid, Scope)> {
    let id = Uuid::parse_str(search_id).ok()?;
    if id.get_version_num() != 8 {
        return None;
    }
    let scope = if id.as_bytes()[15] & 1 == 1 {
        Scope::Track
    } else {
        Scope::Album
    };
    Some((id, scope))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_ids_carry_their_scope() {
        for scope in [Scope::Album, Scope::Track] {
            let id = search_id(scope);
            assert_eq!(id.get_version_num(), 8);
            assert_eq!(id.get_variant(), uuid::Variant::RFC4122);
            assert_eq!(parse_search_id(&id.to_string()), Some((id, scope)));
        }
        assert_ne!(search_id(Scope::Album), search_id(Scope::Album));
        assert_eq!(parse_search_id(&Uuid::new_v4().to_string()), None);
        assert_eq!(parse_search_id("s1"), None);
    }
}
