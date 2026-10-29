//! Managed Storage (managedimport/storage.go, track_replacement_storage.go,
//! root_nofollow_unix.go, capacity_unix.go).
//!
//! Every operation runs beneath one directory handle opened without
//! following symbolic links on any path component, like Go's `os.Root`;
//! paths that must not be links are checked component by component.

use super::errors::{Error, Result, Sentinel, ValidationError};
use super::types::*;
use crate::inspect::MediaInspection;
use bytes::Bytes;
use cap_fs_ext::DirExt;
use cap_std::fs::{
    Dir, DirBuilder, DirBuilderExt, MetadataExt, OpenOptions, OpenOptionsExt, Permissions,
    PermissionsExt,
};
use futures_util::{Stream, StreamExt};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::io::{ErrorKind, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock};

pub const MAX_SLUG_BYTES: usize = 80;
pub const CANONICAL_LIBRARY_ROOT: &str = "library";
const STAGING: &str = ".staging";
/// How much of an upload may land between two capacity probes; each probe
/// asks for the bytes that land before the next one.
const CAPACITY_CHECK_INTERVAL_BYTES: i64 = 4 << 20;

static SLUG_SEPARATORS: LazyLock<Regex> = LazyLock::new(|| Regex::new("[^a-z0-9]+").unwrap());

pub type Capacity = Arc<dyn Fn(&Path) -> std::io::Result<i64> + Send + Sync>;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub reserve_bytes: i64,
    pub file_bytes: i64,
    pub batch_bytes: i64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Requirement {
    pub selected_bytes: i64,
    /// Artwork plus replacement copies that coexist during a commit.
    pub temporary_bytes: i64,
}

#[derive(Clone, Debug)]
pub struct StagedUpload {
    pub path: String,
    pub size: i64,
    pub sha256: String,
}

#[derive(Clone, Debug, Default)]
pub struct PlacedFiles {
    pub audio_path: String,
    pub artwork_path: String,
    audio_relative: String,
    artwork_relative: String,
    staged_relative: String,
    pub artwork_created: bool,
}

/// Every path a Track Replacement touches; the replacement audio lands on
/// a pending path first so the previous file stays until verification.
#[derive(Clone, Debug, Default)]
pub struct ReplacementPlacement {
    pub audio_path: String,
    pub artwork_path: String,
    pub artwork_mode: String,
    staged_relative: String,
    pending_audio_relative: String,
    audio_relative: String,
    previous_audio_relative: String,
    retired_audio_relative: String,
    pending_artwork_relative: String,
    artwork_relative: String,
    previous_artwork_relative: String,
    retired_artwork_relative: String,
    pub artwork_created: bool,
}

/// The relative paths a replacement preview and its token show.
pub struct ReplacementRelatives<'a> {
    pub audio: &'a str,
    pub previous_artwork: &'a str,
    pub retired_artwork: &'a str,
}

impl PlacedFiles {
    /// Paths only, for writing a replacement's rows.
    pub fn paths(audio_path: &str, artwork_path: &str) -> Self {
        Self {
            audio_path: audio_path.to_owned(),
            artwork_path: artwork_path.to_owned(),
            ..Self::default()
        }
    }
}

impl ReplacementPlacement {
    pub fn relatives(&self) -> ReplacementRelatives<'_> {
        ReplacementRelatives {
            audio: &self.audio_relative,
            previous_artwork: &self.previous_artwork_relative,
            retired_artwork: &self.retired_artwork_relative,
        }
    }
}

/// The absolute journal paths of a replacement placement.
pub struct ReplacementPaths {
    pub pending_audio: String,
    pub previous_audio: String,
    pub retired_audio: String,
    pub pending_artwork: String,
    pub previous_artwork: String,
    pub retired_artwork: String,
}

pub struct Storage {
    root: PathBuf,
    initialization_error: Option<Error>,
    reserve_bytes: i64,
    file_limit: i64,
    batch_limit: i64,
    capacity: Capacity,
}

/// Lexically cleaned absolute path (`filepath.Abs`).
fn absolute(path: &str) -> PathBuf {
    let joined = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut clean = PathBuf::from("/");
    for component in joined.components() {
        match component {
            Component::ParentDir => {
                clean.pop();
            }
            Component::Normal(part) => clean.push(part),
            _ => {}
        }
    }
    clean
}

fn join(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("/")
}

/// `filepath.Join(dir, name)` where `dir` may be `.`.
fn join_dir(dir: &str, name: &str) -> String {
    if dir == "." {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

fn parent(path: &str) -> String {
    match path.rfind('/') {
        Some(index) => path[..index].to_owned(),
        None => ".".to_owned(),
    }
}

fn base(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn path_extension(path: &str) -> &str {
    let name = base(path);
    name.rfind('.').map_or("", |index| &name[index..])
}

fn io(context: impl std::fmt::Display, error: std::io::Error) -> Error {
    Error::new(format!("{context}: {error}"))
}

fn not_found(error: &std::io::Error) -> bool {
    error.kind() == ErrorKind::NotFound
}

/// `statfs` free blocks available to unprivileged users.
pub fn available_storage_bytes(path: &Path) -> std::io::Result<i64> {
    let statistics = rustix::fs::statfs(path).map_err(std::io::Error::from)?;
    let block_size = statistics.f_bsize as u64;
    let blocks = statistics.f_bavail;
    if block_size != 0 && blocks > i64::MAX as u64 / block_size {
        return Ok(i64::MAX);
    }
    Ok((blocks * block_size) as i64)
}

impl Storage {
    pub fn new(root: &str, limits: Limits) -> Self {
        Self::with_capacity(root, limits, Arc::new(available_storage_bytes))
    }

    pub fn with_capacity(root: &str, limits: Limits, capacity: Capacity) -> Self {
        let absolute_root = absolute(root);
        let initialization_error = if root.trim().is_empty() {
            Some(Error::new("managed storage root is not configured"))
        } else if absolute_root == Path::new("/") {
            Some(Error::wrap(
                Sentinel::UnsafeStoragePath,
                "filesystem root cannot be Managed Storage",
            ))
        } else if limits.reserve_bytes < 0 || limits.file_bytes <= 0 || limits.batch_bytes <= 0 {
            Some(Error::new("managed storage limits are invalid"))
        } else if limits.file_bytes == i64::MAX || limits.batch_bytes == i64::MAX {
            Some(Error::new(
                "managed import limits exceed supported byte counts",
            ))
        } else {
            None
        };
        Self {
            root: absolute_root,
            initialization_error,
            reserve_bytes: limits.reserve_bytes,
            file_limit: limits.file_bytes,
            batch_limit: limits.batch_bytes,
            capacity,
        }
    }

    pub fn batch_limit(&self) -> i64 {
        self.batch_limit
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Opens the root by walking every component from `/` without following
    /// links, creating missing components, then checks the opened directory
    /// is still the one at the path.
    fn open_root(&self) -> Result<Dir> {
        if let Some(error) = &self.initialization_error {
            return Err(error.clone());
        }
        let mut directory = Dir::open_ambient_dir("/", cap_std::ambient_authority())
            .map_err(|error| io("open Managed Storage root: open filesystem root", error))?;
        for component in self.root.components() {
            let Component::Normal(name) = component else {
                continue;
            };
            let name = Path::new(name);
            let opened = match directory.open_dir_nofollow(name) {
                Err(error) if not_found(&error) => {
                    let mut builder = DirBuilder::new();
                    builder.mode(0o700);
                    match directory.create_dir_with(name, &builder) {
                        Err(error) if error.kind() != ErrorKind::AlreadyExists => {
                            return Err(io(
                                format!(
                                    "open Managed Storage root: create Managed Storage root component {name:?}"
                                ),
                                error,
                            ));
                        }
                        _ => {}
                    }
                    directory.open_dir_nofollow(name)
                }
                other => other,
            };
            directory = match opened {
                Ok(next) => next,
                Err(error)
                    if error.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error())
                        || error.raw_os_error()
                            == Some(rustix::io::Errno::NOTDIR.raw_os_error()) =>
                {
                    return Err(Error::wrap(
                        Sentinel::UnsafeStoragePath,
                        format!(
                            "Managed Storage root component {name:?} changed or is a symbolic link"
                        ),
                    )
                    .context("open Managed Storage root"));
                }
                Err(error) => {
                    return Err(io(
                        format!(
                            "open Managed Storage root: open Managed Storage root component {name:?}"
                        ),
                        error,
                    ));
                }
            };
        }
        let opened = directory
            .dir_metadata()
            .map_err(|error| io("inspect opened Managed Storage root", error))?;
        let at_path = std::fs::symlink_metadata(&self.root)
            .map_err(|error| io("inspect Managed Storage root identity", error))?;
        use std::os::unix::fs::MetadataExt as _;
        if opened.dev() != at_path.dev() || opened.ino() != at_path.ino() {
            return Err(Error::wrap(
                Sentinel::UnsafeStoragePath,
                "Managed Storage root changed while opening",
            ));
        }
        Ok(directory)
    }

    /// `relativePath`: the path beneath the root, never the root itself.
    pub fn relative_path(&self, path: &str) -> Result<String> {
        let absolute_path = absolute(path);
        match absolute_path.strip_prefix(&self.root) {
            Ok(relative) if !relative.as_os_str().is_empty() => {
                Ok(relative.to_string_lossy().into_owned())
            }
            _ => Err(Error::wrap(
                Sentinel::UnsafeStoragePath,
                format!("path {path:?} escapes Managed Storage"),
            )),
        }
    }

    fn optional_relative_path(&self, path: &str) -> Result<String> {
        if path.is_empty() {
            Ok(String::new())
        } else {
            self.relative_path(path)
        }
    }

    pub fn absolute_path(&self, relative: &str) -> String {
        self.root.join(relative).to_string_lossy().into_owned()
    }

    fn optional_absolute_path(&self, relative: &str) -> String {
        if relative.is_empty() {
            String::new()
        } else {
            self.absolute_path(relative)
        }
    }

    fn validate_upload_length(&self, content_length: i64) -> Result<()> {
        if content_length > self.file_limit {
            return Err(Error::sentinel(Sentinel::UploadTooLarge));
        }
        if content_length > self.batch_limit {
            return Err(Error::sentinel(Sentinel::BatchTooLarge));
        }
        Ok(())
    }

    pub fn upload_reservation_size(&self, content_length: i64) -> Result<i64> {
        self.validate_upload_length(content_length)?;
        Ok(content_length.max(0))
    }

    fn stream_limit(&self) -> (i64, Sentinel) {
        if self.batch_limit < self.file_limit {
            (self.batch_limit, Sentinel::BatchTooLarge)
        } else {
            (self.file_limit, Sentinel::UploadTooLarge)
        }
    }

    /// Room for the selected and temporary bytes on top of the reserve.
    pub fn preflight(&self, requirement: Requirement) -> Result<()> {
        if requirement.selected_bytes < 0 || requirement.temporary_bytes < 0 {
            return Err(Error::new(
                "managed storage preflight byte counts must not be negative",
            ));
        }
        drop(self.open_root()?);
        let required = [
            self.reserve_bytes,
            requirement.selected_bytes,
            requirement.temporary_bytes,
        ]
        .into_iter()
        .try_fold(0i64, i64::checked_add)
        .ok_or_else(|| Error::new("managed storage preflight byte count overflow"))?;
        let available = (self.capacity)(&self.root)
            .map_err(|error| io("inspect Managed Storage capacity", error))?;
        if available < required {
            return Err(Error::wrap(
                Sentinel::InsufficientStorage,
                format!("{available} bytes available, {required} bytes required"),
            ));
        }
        Ok(())
    }

    /// Streams an upload into `.staging`, hashing it and checking capacity
    /// as it lands; a short or failed upload leaves nothing behind.
    /// A body error carrying `UploadInterrupted` or `Cancelled` marks the
    /// upload interrupted; any other body error is reported as it is.
    pub async fn stage_upload<S>(&self, mut body: S, content_length: i64) -> Result<StagedUpload>
    where
        S: Stream<Item = Result<Bytes>> + Unpin,
    {
        self.validate_upload_length(content_length)?;
        if content_length > 0 {
            self.preflight(Requirement {
                selected_bytes: content_length,
                temporary_bytes: 0,
            })?;
        }
        let root = self.open_root()?;
        ensure_directory(&root, STAGING, 0o700)?;
        let relative = format!("{STAGING}/.import-{}.upload", uuid::Uuid::new_v4());
        let mut options = OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        let file = root
            .open_with(&relative, &options)
            .map_err(|error| io("create Managed Import staging file", error))?;
        let mut file = tokio::fs::File::from_std(file.into_std());
        let (limit, limit_sentinel) = self.stream_limit();
        let mut hasher = Sha256::new();
        let mut written = 0i64;
        let mut since_check = 0i64;
        let mut checked = false;
        let mut failure: Option<Error> = None;
        let mut interrupted = false;
        let mut over_limit = false;
        while let Some(chunk) = body.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    interrupted =
                        error.is(Sentinel::UploadInterrupted) || error.is(Sentinel::Cancelled);
                    failure = Some(error);
                    break;
                }
            };
            if written + chunk.len() as i64 > limit {
                over_limit = true;
                break;
            }
            if !checked || since_check + chunk.len() as i64 >= CAPACITY_CHECK_INTERVAL_BYTES {
                let remaining = if content_length >= 0 {
                    content_length - written
                } else {
                    -1
                };
                let mut probe = CAPACITY_CHECK_INTERVAL_BYTES;
                if remaining >= 0 && remaining < probe {
                    probe = remaining;
                }
                let probe = probe.max(chunk.len() as i64);
                if let Err(error) = self.preflight(Requirement {
                    selected_bytes: probe,
                    temporary_bytes: 0,
                }) {
                    failure = Some(error);
                    break;
                }
                checked = true;
                since_check = 0;
            }
            use tokio::io::AsyncWriteExt;
            if let Err(error) = file.write_all(&chunk).await {
                failure = Some(io("write Managed Import staging file", error));
                break;
            }
            hasher.update(&chunk);
            written += chunk.len() as i64;
            since_check += chunk.len() as i64;
        }
        use tokio::io::AsyncWriteExt;
        let close = file
            .flush()
            .await
            .err()
            .map(|error| io("close Managed Import staging file", error));
        drop(file);
        let short = content_length >= 0 && written < content_length;
        if over_limit || failure.is_some() || close.is_some() || short {
            let removed = remove_rooted_file(&root, &relative, "Managed Import staging file").err();
            if over_limit {
                return Err(Error::join([Some(Error::sentinel(limit_sentinel)), removed]).unwrap());
            }
            if short || interrupted {
                let interrupted = Error::join([
                    Some(Error::sentinel(Sentinel::UploadInterrupted)),
                    Some(Error::new("unexpected EOF")),
                    failure,
                    close,
                    removed,
                ]);
                return Err(interrupted.unwrap());
            }
            return Err(Error::join([failure, close, removed]).unwrap());
        }
        Ok(StagedUpload {
            path: self.absolute_path(&relative),
            size: written,
            sha256: hex::encode(hasher.finalize()),
        })
    }

    pub fn staged_file_size(&self, path: &str) -> Result<i64> {
        let relative = self.relative_path(path)?;
        let root = self.open_root()?;
        reject_symlinks(&root, &relative)?;
        let metadata = root
            .metadata(&relative)
            .map_err(|error| io("stat staged Managed Track", error))?;
        if !metadata.is_file() {
            return Err(Error::wrap(
                Sentinel::UnsafeStoragePath,
                "staged Managed Track is not a regular file",
            ));
        }
        Ok(metadata.len() as i64)
    }

    pub fn remove_staged(&self, path: &str) -> Result<()> {
        let relative = self.relative_path(path)?;
        let root = self.open_root()?;
        remove_rooted_file(&root, &relative, "Managed Import staging file")
    }

    /// Removes every upload left in `.staging`.
    pub fn remove_all_staged(&self) -> Result<()> {
        self.remove_all_staged_except(&std::collections::HashSet::new())
    }

    /// Removes every upload left in `.staging` except the staged paths in
    /// `keep` (as jobs store them).
    pub fn remove_all_staged_except(&self, keep: &std::collections::HashSet<String>) -> Result<()> {
        let root = self.open_root()?;
        let entries = match root.read_dir(STAGING) {
            Err(error) if not_found(&error) => return Ok(()),
            Err(error) => return Err(io("open Managed Import staging directory", error)),
            Ok(entries) => entries,
        };
        let mut failures = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| io("list Managed Import staging files", error))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(".import-") || !name.ends_with(".upload") {
                continue;
            }
            let relative = format!("{STAGING}/{name}");
            if keep.contains(&self.absolute_path(&relative)) {
                continue;
            }
            failures
                .push(remove_rooted_file(&root, &relative, "Managed Import staging file").err());
        }
        match Error::join(failures) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub fn place(
        &self,
        staged_path: &str,
        inspection: &MediaInspection,
        identity: &CommitIdentity,
        artwork_created: Option<&dyn Fn() -> Result<()>>,
    ) -> Result<PlacedFiles> {
        let placement = self.plan_placement(staged_path, inspection, identity)?;
        self.place_planned(
            placement,
            inspection,
            identity,
            0o750,
            "place Managed Track at Canonical Library Path",
            artwork_created,
        )
    }

    fn place_planned(
        &self,
        mut placement: PlacedFiles,
        inspection: &MediaInspection,
        identity: &CommitIdentity,
        directory_mode: u32,
        audio_operation: &str,
        artwork_created: Option<&dyn Fn() -> Result<()>>,
    ) -> Result<PlacedFiles> {
        let root = self.open_root()?;
        ensure_directory(&root, &parent(&placement.audio_relative), directory_mode)?;
        let should_create = self.prepare_artwork(&root, &mut placement, inspection, identity)?;
        root.rename(&placement.staged_relative, &root, &placement.audio_relative)
            .map_err(|error| io(audio_operation, error))?;
        if should_create {
            let artwork = &inspection.album_artwork;
            match write_rooted_artwork(
                &root,
                &placement.artwork_relative,
                &artwork.data,
                &artwork.sha256,
                artwork_created,
            ) {
                Ok(created) => placement.artwork_created = created,
                Err(error) => {
                    let restored = restore_rooted_file(
                        &root,
                        &placement.audio_relative,
                        &placement.staged_relative,
                    )
                    .err();
                    return Err(Error::join([Some(error), restored]).unwrap());
                }
            }
        }
        Ok(placement)
    }

    pub fn verify_placement(
        &self,
        placement: &PlacedFiles,
        audio_sha256: &str,
        artwork_sha256: &str,
    ) -> Result<()> {
        let root = self.open_root()?;
        verify_rooted_file_hash(&root, &placement.audio_relative, audio_sha256, &|| false)
            .map_err(|error| error.context("verify canonical Managed Track"))?;
        if !placement.artwork_relative.is_empty() {
            verify_rooted_file_hash(&root, &placement.artwork_relative, artwork_sha256, &|| {
                false
            })
            .map_err(|error| error.context("verify canonical Album Artwork"))?;
        }
        Ok(())
    }

    pub fn cleanup_placement(&self, placement: &PlacedFiles) -> Result<()> {
        self.remove_staged(&self.absolute_path(&placement.staged_relative))
    }

    pub fn placement_from_journal(&self, journal: &CommitJournal) -> Result<PlacedFiles> {
        Ok(PlacedFiles {
            audio_path: journal.audio_file_path.clone(),
            artwork_path: journal.artwork_file_path.clone(),
            audio_relative: self.relative_path(&journal.audio_file_path)?,
            artwork_relative: self.optional_relative_path(&journal.artwork_file_path)?,
            staged_relative: self.relative_path(&journal.staged_file_path)?,
            artwork_created: journal.is_artwork_created,
        })
    }

    fn album_directory(&self, inspection: &MediaInspection, identity: &CommitIdentity) -> String {
        let metadata = &inspection.metadata;
        join(&[
            CANONICAL_LIBRARY_ROOT,
            &format!(
                "{}-{}",
                slug(&metadata.album_artists[0]),
                identity.album_artist_id
            ),
            &format!("{}-{}", slug(&metadata.album), identity.album_id),
        ])
    }

    pub fn plan_placement(
        &self,
        staged_path: &str,
        inspection: &MediaInspection,
        identity: &CommitIdentity,
    ) -> Result<PlacedFiles> {
        let mut album_relative = self.album_directory(inspection, identity);
        let mut artwork_relative = String::new();
        if !inspection.album_artwork.sha256.is_empty() {
            artwork_relative = format!(
                "{album_relative}/cover{}",
                artwork_extension(&inspection.album_artwork.mime_type)
            );
        }
        if !identity.existing_artwork_path.is_empty() {
            (artwork_relative, album_relative) = self.existing_canonical_album(identity)?;
        }
        self.plan_placement_in_album(
            staged_path,
            inspection,
            &identity.track_id,
            &album_relative,
            &artwork_relative,
        )
    }

    fn plan_placement_in_album(
        &self,
        staged_path: &str,
        inspection: &MediaInspection,
        track_id: &str,
        album_relative: &str,
        artwork_relative: &str,
    ) -> Result<PlacedFiles> {
        let staged_relative = self.relative_path(staged_path)?;
        let extension = source_extension(&inspection.audio.format)?;
        let metadata = &inspection.metadata;
        let audio_filename = format!(
            "{:02}-{:02}-{}-{}{}",
            metadata.disc_position.number,
            metadata.track_position.number,
            slug(&metadata.title),
            track_id,
            extension
        );
        let audio_relative = format!("{album_relative}/{audio_filename}");
        Ok(PlacedFiles {
            audio_path: self.absolute_path(&audio_relative),
            artwork_path: self.optional_absolute_path(artwork_relative),
            audio_relative,
            artwork_relative: artwork_relative.to_owned(),
            staged_relative,
            artwork_created: false,
        })
    }

    /// The artwork and album directory of an existing canonical Album.
    fn existing_canonical_album(&self, identity: &CommitIdentity) -> Result<(String, String)> {
        let artwork_relative = self.relative_path(&identity.existing_artwork_path)?;
        let album_relative = parent(&artwork_relative);
        let parts: Vec<&str> = album_relative.split('/').collect();
        let is_canonical = parts.len() == 3
            && parts[0] == CANONICAL_LIBRARY_ROOT
            && parts[1].ends_with(&format!("-{}", identity.album_artist_id))
            && parts[2].ends_with(&format!("-{}", identity.album_id))
            && base(&artwork_relative).starts_with("cover.");
        if !is_canonical {
            return Err(Error::wrap(
                Sentinel::UnsafeStoragePath,
                "existing Album Artwork path is not canonical",
            ));
        }
        Ok((artwork_relative, album_relative))
    }

    fn prepare_artwork(
        &self,
        root: &Dir,
        placement: &mut PlacedFiles,
        inspection: &MediaInspection,
        identity: &CommitIdentity,
    ) -> Result<bool> {
        if !identity.existing_artwork_path.is_empty() {
            placement.artwork_path = identity.existing_artwork_path.clone();
            verify_rooted_file_hash(
                root,
                &placement.artwork_relative,
                &identity.existing_artwork_sha256,
                &|| false,
            )
            .map_err(|error| error.context("verify existing Album Artwork"))?;
            return Ok(false);
        }
        if placement.artwork_relative.is_empty() {
            return Ok(false);
        }
        match root.metadata(&placement.artwork_relative) {
            Ok(_) => {
                verify_matching_artwork(
                    root,
                    &placement.artwork_relative,
                    &inspection.album_artwork.sha256,
                )?;
                Ok(false)
            }
            Err(error) if not_found(&error) => Ok(true),
            Err(error) => Err(io("inspect Canonical Album Artwork path", error)),
        }
    }

    pub fn rollback(&self, placement: &PlacedFiles) -> Result<()> {
        let root = self.open_root()?;
        let artwork = if placement.artwork_created {
            remove_rooted_file(
                &root,
                &placement.artwork_relative,
                "Canonical Album Artwork",
            )
            .err()
        } else {
            None
        };
        let restored =
            restore_rooted_file(&root, &placement.audio_relative, &placement.staged_relative).err();
        let directories = remove_empty_directories_under(
            &root,
            &parent(&placement.audio_relative),
            CANONICAL_LIBRARY_ROOT,
        )
        .err();
        result_of(Error::join([artwork, restored, directories]))
    }

    pub fn cleanup_prepared_placement(&self, placement: &PlacedFiles) -> Result<()> {
        let root = self.open_root()?;
        let artwork = if placement.artwork_created {
            remove_rooted_file(
                &root,
                &placement.artwork_relative,
                "uncommitted Canonical Album Artwork",
            )
            .err()
        } else {
            None
        };
        let directories = remove_empty_directories_under(
            &root,
            &parent(&placement.audio_relative),
            CANONICAL_LIBRARY_ROOT,
        )
        .err();
        result_of(Error::join([artwork, directories]))
    }

    /// The managed file must still be a regular file with the expected hash.
    pub fn resolve_managed_file(&self, path: &str, expected_hash: &str) -> Result<(String, i64)> {
        let relative = self.relative_path(path)?;
        let root = self.open_root()?;
        reject_symlinks(&root, &relative).map_err(|error| {
            Error::wrap(
                Sentinel::UnsafeStoragePath,
                format!("cannot resolve managed Track source: {error}"),
            )
        })?;
        let metadata = match root.metadata(&relative) {
            Ok(metadata) if metadata.is_file() => metadata,
            _ => {
                return Err(Error::wrap(
                    Sentinel::UnsafeStoragePath,
                    "managed Track source is not a regular file",
                ));
            }
        };
        verify_rooted_file_hash(&root, &relative, expected_hash, &|| false).map_err(|error| {
            Error::wrap(
                Sentinel::UnsafeStoragePath,
                format!("managed Track source identity changed: {error}"),
            )
        })?;
        Ok((relative, metadata.len() as i64))
    }

    /// Removes a managed file whose bytes still match; `Ok(true)` also when
    /// it is already gone. Nothing is removed once cancelled.
    pub fn remove_managed_file(
        &self,
        cancelled: &dyn Fn() -> bool,
        path: &str,
        expected_hash: &str,
    ) -> Result<bool> {
        if cancelled() {
            return Err(Error::sentinel(Sentinel::Cancelled));
        }
        let relative = self.relative_path(path)?;
        let root = self.open_root()?;
        reject_symlinks(&root, &parent(&relative)).map_err(|error| {
            Error::wrap(
                Sentinel::UnsafeStoragePath,
                format!("cannot resolve managed Track directory: {error}"),
            )
        })?;
        match root.symlink_metadata(&relative) {
            Err(error) if not_found(&error) => return Ok(true),
            Ok(metadata) if metadata.is_file() => {}
            _ => {
                return Err(Error::wrap(
                    Sentinel::UnsafeStoragePath,
                    "managed Track source is not a regular file",
                ));
            }
        }
        if let Err(error) = verify_rooted_file_hash(&root, &relative, expected_hash, cancelled) {
            if cancelled() {
                return Err(Error::sentinel(Sentinel::Cancelled));
            }
            return Err(Error::wrap(
                Sentinel::UnsafeStoragePath,
                format!("managed Track source identity changed: {error}"),
            ));
        }
        if cancelled() {
            return Err(Error::sentinel(Sentinel::Cancelled));
        }
        root.remove_file(&relative)
            .map_err(|error| io("remove Managed Track source", error))?;
        Ok(true)
    }

    pub fn plan_replacement_placement(
        &self,
        staged_path: &str,
        inspection: &MediaInspection,
        identity: &CommitIdentity,
        target: &ReplacementTarget,
    ) -> Result<ReplacementPlacement> {
        let album_relative = if identity.existing_artwork_path.is_empty() {
            self.album_directory(inspection, identity)
        } else {
            self.existing_canonical_album(identity)?.1
        };
        let extension = source_extension(&inspection.audio.format)?;
        let staged_relative = self.relative_path(staged_path)?;
        let planned = self.plan_placement_in_album(
            staged_path,
            inspection,
            &identity.track_id,
            &album_relative,
            &format!(
                "{album_relative}/cover{}",
                artwork_extension(&inspection.album_artwork.mime_type)
            ),
        )?;
        let mut placement = ReplacementPlacement {
            audio_path: planned.audio_path.clone(),
            staged_relative,
            pending_audio_relative: format!(
                "{album_relative}/.replacement-{}{extension}",
                identity.track_id
            ),
            audio_relative: planned.audio_relative.clone(),
            previous_audio_relative: target.relative_path.clone(),
            retired_audio_relative: join_dir(
                &parent(&target.relative_path),
                &format!(
                    ".retired-{}{}",
                    identity.track_id,
                    path_extension(&target.relative_path)
                ),
            ),
            ..ReplacementPlacement::default()
        };
        self.plan_replacement_artwork(
            &mut placement,
            &planned,
            inspection,
            identity,
            target,
            &album_relative,
        )?;
        Ok(placement)
    }

    fn plan_replacement_artwork(
        &self,
        placement: &mut ReplacementPlacement,
        planned: &PlacedFiles,
        inspection: &MediaInspection,
        identity: &CommitIdentity,
        target: &ReplacementTarget,
        album_relative: &str,
    ) -> Result<()> {
        placement.artwork_path = planned.artwork_path.clone();
        placement.artwork_relative = planned.artwork_relative.clone();
        let is_moving_album = identity.album_id != target.album_id;
        if is_moving_album && target.is_sole_track && !target.artwork_path.is_empty() {
            placement.previous_artwork_relative = self.relative_path(&target.artwork_path)?;
        }
        if identity.existing_artwork_path.is_empty() && inspection.album_artwork.sha256.is_empty() {
            placement.artwork_mode = REPLACEMENT_ARTWORK_MODE_NONE.to_owned();
            placement.artwork_path.clear();
            placement.artwork_relative.clear();
            return Ok(());
        }
        if identity.existing_artwork_path.is_empty() {
            placement.artwork_mode = REPLACEMENT_ARTWORK_MODE_CREATE.to_owned();
            return Ok(());
        }
        let existing_relative = self.relative_path(&identity.existing_artwork_path)?;
        if identity.existing_artwork_sha256 == inspection.album_artwork.sha256 {
            placement.artwork_mode = REPLACEMENT_ARTWORK_MODE_EXISTING.to_owned();
            placement.artwork_path = identity.existing_artwork_path.clone();
            placement.artwork_relative = existing_relative;
            return Ok(());
        }
        if is_moving_album || !target.is_sole_track {
            return Err(album_artwork_conflict(
                "embedded Album Artwork differs from the existing Album",
            ));
        }
        placement.artwork_mode = REPLACEMENT_ARTWORK_MODE_REPLACE.to_owned();
        placement.pending_artwork_relative = format!(
            "{album_relative}/.replacement-cover{}",
            artwork_extension(&inspection.album_artwork.mime_type)
        );
        placement.retired_artwork_relative = format!(
            "{album_relative}/.retired-cover{}",
            path_extension(&existing_relative)
        );
        placement.previous_artwork_relative = existing_relative;
        Ok(())
    }

    pub fn replacement_placement_from_journal(
        &self,
        journal: &ReplacementJournal,
    ) -> Result<ReplacementPlacement> {
        Ok(ReplacementPlacement {
            audio_path: journal.audio_file_path.clone(),
            artwork_path: journal.artwork_file_path.clone(),
            artwork_mode: journal.artwork_mode.clone(),
            artwork_created: journal.is_artwork_created,
            staged_relative: self.optional_relative_path(&journal.staged_file_path)?,
            pending_audio_relative: self.optional_relative_path(&journal.pending_audio_path)?,
            audio_relative: self.optional_relative_path(&journal.audio_file_path)?,
            previous_audio_relative: self.optional_relative_path(&journal.previous_audio_path)?,
            retired_audio_relative: self.optional_relative_path(&journal.retired_audio_path)?,
            artwork_relative: self.optional_relative_path(&journal.artwork_file_path)?,
            pending_artwork_relative: self.optional_relative_path(&journal.pending_artwork_path)?,
            previous_artwork_relative: self
                .optional_relative_path(&journal.previous_artwork_path)?,
            retired_artwork_relative: self.optional_relative_path(&journal.retired_artwork_path)?,
        })
    }

    pub fn replacement_journal_paths(&self, placement: &ReplacementPlacement) -> ReplacementPaths {
        ReplacementPaths {
            pending_audio: self.absolute_path(&placement.pending_audio_relative),
            previous_audio: self.absolute_path(&placement.previous_audio_relative),
            retired_audio: self.absolute_path(&placement.retired_audio_relative),
            pending_artwork: self.optional_absolute_path(&placement.pending_artwork_relative),
            previous_artwork: self.optional_absolute_path(&placement.previous_artwork_relative),
            retired_artwork: self.optional_absolute_path(&placement.retired_artwork_relative),
        }
    }

    /// Moves the upload onto its pending path and prepares the artwork
    /// without touching the previous managed file.
    pub fn place_replacement(
        &self,
        mut placement: ReplacementPlacement,
        inspection: &MediaInspection,
        identity: &CommitIdentity,
        artwork_created: Option<&dyn Fn() -> Result<()>>,
    ) -> Result<ReplacementPlacement> {
        let root = self.open_root()?;
        ensure_directory(&root, &parent(&placement.audio_relative), 0o750)?;
        root.rename(
            &placement.staged_relative,
            &root,
            &placement.pending_audio_relative,
        )
        .map_err(|error| io("place replacement Managed Track on pending path", error))?;
        let artwork = &inspection.album_artwork;
        let created = match placement.artwork_mode.as_str() {
            REPLACEMENT_ARTWORK_MODE_NONE => Ok(false),
            REPLACEMENT_ARTWORK_MODE_EXISTING => verify_rooted_file_hash(
                &root,
                &placement.artwork_relative,
                &identity.existing_artwork_sha256,
                &|| false,
            )
            .map(|_| false)
            .map_err(|error| error.context("verify existing Album Artwork")),
            REPLACEMENT_ARTWORK_MODE_REPLACE => write_rooted_artwork(
                &root,
                &placement.pending_artwork_relative,
                &artwork.data,
                &artwork.sha256,
                artwork_created,
            ),
            _ => write_rooted_artwork(
                &root,
                &placement.artwork_relative,
                &artwork.data,
                &artwork.sha256,
                artwork_created,
            ),
        };
        match created {
            Ok(created) => {
                placement.artwork_created = created;
                Ok(placement)
            }
            Err(error) => {
                let restored = restore_rooted_file(
                    &root,
                    &placement.pending_audio_relative,
                    &placement.staged_relative,
                )
                .err();
                Err(Error::join([Some(error), restored]).unwrap())
            }
        }
    }

    /// Hashes the pending audio and artwork before anything visible changes.
    pub fn verify_replacement(
        &self,
        placement: &ReplacementPlacement,
        audio_sha256: &str,
        artwork_sha256: &str,
    ) -> Result<()> {
        let root = self.open_root()?;
        verify_rooted_file_hash(
            &root,
            &placement.pending_audio_relative,
            audio_sha256,
            &|| false,
        )
        .map_err(|error| error.context("verify pending replacement Managed Track"))?;
        let artwork_relative = if placement.artwork_mode == REPLACEMENT_ARTWORK_MODE_REPLACE {
            &placement.pending_artwork_relative
        } else {
            &placement.artwork_relative
        };
        if artwork_relative.is_empty() {
            return Ok(());
        }
        verify_rooted_file_hash(&root, artwork_relative, artwork_sha256, &|| false)
            .map_err(|error| error.context("verify replacement Album Artwork"))
    }

    /// Retires the previous file and moves the verified replacement onto
    /// its canonical path.
    pub fn swap_replacement(&self, placement: &ReplacementPlacement) -> Result<()> {
        let root = self.open_root()?;
        root.rename(
            &placement.previous_audio_relative,
            &root,
            &placement.retired_audio_relative,
        )
        .map_err(|error| io("retire previous Managed Track", error))?;
        if let Err(error) = root.rename(
            &placement.pending_audio_relative,
            &root,
            &placement.audio_relative,
        ) {
            let restored = restore_rooted_file(
                &root,
                &placement.retired_audio_relative,
                &placement.previous_audio_relative,
            )
            .err();
            return result_of(Error::join([
                Some(io(
                    "place replacement Managed Track at Canonical Library Path",
                    error,
                )),
                restored,
            ]));
        }
        if placement.artwork_mode != REPLACEMENT_ARTWORK_MODE_REPLACE {
            return Ok(());
        }
        if let Err(error) = root.rename(
            &placement.previous_artwork_relative,
            &root,
            &placement.retired_artwork_relative,
        ) {
            return result_of(Error::join([
                Some(io("retire previous Album Artwork", error)),
                undo_audio_swap(&root, placement).err(),
            ]));
        }
        if let Err(error) = root.rename(
            &placement.pending_artwork_relative,
            &root,
            &placement.artwork_relative,
        ) {
            let restored = restore_rooted_file(
                &root,
                &placement.retired_artwork_relative,
                &placement.previous_artwork_relative,
            )
            .err();
            return result_of(Error::join([
                Some(io("place replacement Album Artwork", error)),
                restored,
                undo_audio_swap(&root, placement).err(),
            ]));
        }
        Ok(())
    }

    /// Reads the canonical replacement back to its end before the previous
    /// file is deleted.
    pub fn stream_swapped_replacement(
        &self,
        placement: &ReplacementPlacement,
        audio_sha256: &str,
    ) -> Result<()> {
        let root = self.open_root()?;
        verify_rooted_file_hash(&root, &placement.audio_relative, audio_sha256, &|| false)
            .map_err(|error| error.context("verify canonical replacement Managed Track"))?;
        let mut stream = root
            .open(&placement.audio_relative)
            .map_err(|error| io("open canonical replacement stream", error))?;
        std::io::copy(&mut stream, &mut std::io::sink())
            .map_err(|error| io("stream canonical replacement Managed Track", error))?;
        Ok(())
    }

    /// Restores the previous file and returns the replacement to staging,
    /// judging from the files rather than the journaled phase.
    pub fn rollback_replacement(&self, placement: &ReplacementPlacement) -> Result<()> {
        let root = self.open_root()?;
        let artwork_swap = if placement.artwork_mode == REPLACEMENT_ARTWORK_MODE_REPLACE {
            undo_retired_swap(
                &root,
                &placement.retired_artwork_relative,
                &placement.previous_artwork_relative,
                &placement.artwork_relative,
                &placement.pending_artwork_relative,
            )
            .err()
        } else {
            None
        };
        let audio_swap = undo_retired_swap(
            &root,
            &placement.retired_audio_relative,
            &placement.previous_audio_relative,
            &placement.audio_relative,
            &placement.pending_audio_relative,
        )
        .err();
        let pending = restore_if_present(
            &root,
            &placement.pending_audio_relative,
            &placement.staged_relative,
        )
        .err();
        let artwork = if placement.artwork_created {
            let created = if placement.artwork_mode == REPLACEMENT_ARTWORK_MODE_REPLACE {
                &placement.pending_artwork_relative
            } else {
                &placement.artwork_relative
            };
            remove_rooted_file(&root, created, "uncommitted replacement Album Artwork").err()
        } else {
            None
        };
        let directories = remove_empty_directories_under(
            &root,
            &parent(&placement.audio_relative),
            CANONICAL_LIBRARY_ROOT,
        )
        .err();
        result_of(Error::join([
            artwork_swap,
            audio_swap,
            pending,
            artwork,
            directories,
        ]))
    }

    /// Deletes the retired previous file and artwork that lost its last
    /// reference, after the database made the replacement authoritative.
    pub fn complete_replacement_files(
        &self,
        cancelled: &dyn Fn() -> bool,
        journal: &ReplacementJournal,
        placement: &ReplacementPlacement,
    ) -> (i64, Result<()>) {
        let audio = self.remove_managed_file(
            cancelled,
            &journal.retired_audio_path,
            &journal.previous_audio_sha256,
        );
        let (removed, mut failure) = match audio {
            Ok(removed) => (removed, None),
            Err(error) => (false, Some(error)),
        };
        if !removed {
            return (0, result_of(failure));
        }
        let mut deleted = 1;
        let artwork_path = if journal.retired_artwork_path.is_empty() {
            &journal.previous_artwork_path
        } else {
            &journal.retired_artwork_path
        };
        if !artwork_path.is_empty() {
            match self.remove_managed_file(
                cancelled,
                artwork_path,
                &journal.previous_artwork_sha256,
            ) {
                Ok(true) => deleted += 1,
                Ok(false) => return (deleted, result_of(failure)),
                Err(error) => return (deleted, result_of(Error::join([failure, Some(error)]))),
            }
        }
        match self.open_root() {
            Err(error) => failure = Error::join([failure, Some(error)]),
            Ok(root) => {
                let directories = remove_empty_directories_under(
                    &root,
                    &parent(&placement.previous_audio_relative),
                    CANONICAL_LIBRARY_ROOT,
                )
                .err();
                failure = Error::join([failure, directories]);
            }
        }
        (deleted, result_of(failure))
    }
}

fn result_of(error: Option<Error>) -> Result<()> {
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

pub fn album_artwork_conflict(reason: &str) -> Error {
    Error::validation(
        ValidationError::new(ERROR_CODE_ALBUM_ARTWORK_CONFLICT, "artwork", reason),
        reason,
    )
}

fn undo_audio_swap(root: &Dir, placement: &ReplacementPlacement) -> Result<()> {
    result_of(Error::join([
        restore_rooted_file(
            root,
            &placement.audio_relative,
            &placement.pending_audio_relative,
        )
        .err(),
        restore_rooted_file(
            root,
            &placement.retired_audio_relative,
            &placement.previous_audio_relative,
        )
        .err(),
    ]))
}

/// Moves canonical files back only when the retired copy proves the swap
/// started; before it the canonical path may hold the previous file.
fn undo_retired_swap(
    root: &Dir,
    retired: &str,
    previous: &str,
    canonical: &str,
    pending: &str,
) -> Result<()> {
    match root.symlink_metadata(retired) {
        Err(error) if not_found(&error) => return Ok(()),
        Err(error) => {
            return Err(io(
                format!("inspect retired Managed Storage file {retired:?}"),
                error,
            ));
        }
        Ok(_) => {}
    }
    result_of(Error::join([
        restore_if_present(root, canonical, pending).err(),
        restore_if_present(root, retired, previous).err(),
    ]))
}

fn restore_if_present(root: &Dir, from: &str, to: &str) -> Result<()> {
    match root.symlink_metadata(from) {
        Err(error) if not_found(&error) => return Ok(()),
        Err(error) => return Err(io(format!("inspect {from:?} before restore"), error)),
        Ok(_) => {}
    }
    if root.symlink_metadata(to).is_ok() {
        return Err(Error::wrap(
            Sentinel::UnsafeStoragePath,
            format!("restore target {to:?} already exists"),
        ));
    }
    restore_rooted_file(root, from, to)
}

/// Creates each missing directory of `path` with `mode`; no component may be
/// a link or a file.
fn ensure_directory(root: &Dir, path: &str, mode: u32) -> Result<()> {
    let mut current = String::new();
    for component in path
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
    {
        current = join(&[&current, component]);
        match root.symlink_metadata(&current) {
            Err(error) if not_found(&error) => {
                let mut builder = DirBuilder::new();
                builder.mode(mode);
                root.create_dir_with(&current, &builder).map_err(|error| {
                    io(
                        format!("create Managed Storage directory {current:?}"),
                        error,
                    )
                })?;
            }
            Err(error) => {
                return Err(io(
                    format!("inspect Managed Storage directory {current:?}"),
                    error,
                ));
            }
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::wrap(
                    Sentinel::UnsafeStoragePath,
                    format!("directory {current:?} is a symbolic link"),
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(Error::new(format!(
                    "managed storage path {current:?} is not a directory"
                )));
            }
            Ok(_) => {}
        }
    }
    root.set_permissions(path, Permissions::from_mode(mode))
        .map_err(|error| io(format!("protect Managed Storage directory {path:?}"), error))
}

fn reject_symlinks(root: &Dir, path: &str) -> Result<()> {
    let mut current = String::new();
    for component in path
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
    {
        current = join(&[&current, component]);
        let metadata = root
            .symlink_metadata(&current)
            .map_err(|error| io(format!("inspect Managed Storage path {current:?}"), error))?;
        if metadata.file_type().is_symlink() {
            return Err(Error::wrap(
                Sentinel::UnsafeStoragePath,
                format!("path {current:?} is a symbolic link"),
            ));
        }
    }
    Ok(())
}

fn verify_rooted_file_hash(
    root: &Dir,
    path: &str,
    expected: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    reject_symlinks(root, path)?;
    let mut file = root
        .open(path)
        .map_err(|error| io("open Managed Storage file for verification", error))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        if cancelled() {
            return Err(Error::sentinel(Sentinel::Cancelled).context("hash Managed Storage file"));
        }
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buffer[..n]),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(io("hash Managed Storage file", error)),
        }
    }
    let actual = hex::encode(hasher.finalize());
    if actual != expected {
        return Err(Error::new(format!(
            "verify Managed Storage file hash: got {actual}, want {expected}"
        )));
    }
    Ok(())
}

/// Creates the artwork exclusively; an existing file must match. Returns
/// whether this call created it.
fn write_rooted_artwork(
    root: &Dir,
    path: &str,
    data: &[u8],
    expected: &str,
    created: Option<&dyn Fn() -> Result<()>>,
) -> Result<bool> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    let mut artwork = match root.open_with(path, &options) {
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            return verify_matching_artwork(root, path, expected).map(|_| false);
        }
        Err(error) => return Err(io("create canonical Album Artwork", error)),
        Ok(file) => file,
    };
    let remove = |error: Error| {
        result_of(Error::join([
            Some(error),
            remove_rooted_file(root, path, "Canonical Album Artwork").err(),
        ]))
    };
    if let Some(created) = created
        && let Err(error) = created()
    {
        drop(artwork);
        return remove(error).map(|_| false);
    }
    if let Err(error) = artwork.write_all(data) {
        drop(artwork);
        return remove(io("write Album Artwork", error)).map(|_| false);
    }
    drop(artwork);
    if let Err(error) = verify_rooted_file_hash(root, path, expected, &|| false) {
        return remove(error).map(|_| false);
    }
    Ok(true)
}

fn verify_matching_artwork(root: &Dir, path: &str, expected: &str) -> Result<()> {
    verify_rooted_file_hash(root, path, expected, &|| false).map_err(|error| {
        Error::validation(
            ValidationError::new(
                ERROR_CODE_ALBUM_ARTWORK_CONFLICT,
                "artwork",
                "canonical Album Artwork differs from the selected Album",
            ),
            error,
        )
    })
}

/// Removes every empty directory from `directory` upwards, stopping at the
/// root or at `boundary`.
fn remove_empty_directories_under(root: &Dir, directory: &str, boundary: &str) -> Result<()> {
    let mut directory = directory.to_owned();
    while directory != "." && !directory.is_empty() && directory != boundary {
        match root.read_dir(&directory) {
            Err(error) if not_found(&error) => {}
            Err(error) => {
                return Err(io(
                    format!("open Managed Storage directory {directory:?} for cleanup"),
                    error,
                ));
            }
            Ok(mut entries) => {
                if entries.next().is_some() {
                    return Ok(());
                }
                if let Err(error) = root.remove_dir(&directory)
                    && !not_found(&error)
                {
                    return Err(io(
                        format!("remove empty Managed Storage directory {directory:?}"),
                        error,
                    ));
                }
            }
        }
        directory = parent(&directory);
    }
    Ok(())
}

fn remove_rooted_file(root: &Dir, path: &str, description: &str) -> Result<()> {
    match root.remove_file(path) {
        Err(error) if !not_found(&error) => {
            Err(io(format!("remove {description} {path:?}"), error))
        }
        _ => Ok(()),
    }
}

fn restore_rooted_file(root: &Dir, from: &str, to: &str) -> Result<()> {
    root.rename(from, root, to).map_err(|error| {
        io(
            format!("restore Managed Import staging file from {from:?} to {to:?}"),
            error,
        )
    })
}

pub fn source_extension(format: &str) -> Result<&'static str> {
    Ok(match format {
        "flac" => ".flac",
        "wav" => ".wav",
        "m4a" => ".m4a",
        "mp3" => ".mp3",
        "ogg" => ".ogg",
        "opus" => ".opus",
        _ => {
            return Err(Error::validation(
                ValidationError::new("unsupported_format", "format", ""),
                format!("validated Source Audio Format {format:?} has no canonical extension"),
            ));
        }
    })
}

pub fn artwork_extension(media_type: &str) -> &'static str {
    match media_type {
        "image/jpeg" => ".jpg",
        "image/webp" => ".webp",
        _ => ".png",
    }
}

/// Lower-case ASCII words joined by hyphens, at most 80 bytes.
pub fn slug(value: &str) -> String {
    let lowered: String = crate::inspect::metadata::go_lower(value)
        .chars()
        .map(|c| if c.is_ascii() { c } else { '-' })
        .collect();
    let replaced = SLUG_SEPARATORS.replace_all(&lowered, "-");
    let mut slug = replaced.trim_matches('-').to_owned();
    if slug.len() > MAX_SLUG_BYTES {
        slug = slug[..MAX_SLUG_BYTES].trim_matches('-').to_owned();
    }
    if slug.is_empty() {
        "untitled".to_owned()
    } else {
        slug
    }
}
