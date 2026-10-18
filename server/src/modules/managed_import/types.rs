//! Managed Import records shared by storage, the repository and the
//! service (managedimport/types.go, track_replacement_types.go).

pub const REPLACEMENT_ARTWORK_MODE_NONE: &str = "none";
pub const REPLACEMENT_ARTWORK_MODE_EXISTING: &str = "existing";
pub const REPLACEMENT_ARTWORK_MODE_CREATE: &str = "create";
pub const REPLACEMENT_ARTWORK_MODE_REPLACE: &str = "replace";
pub const ERROR_CODE_ALBUM_ARTWORK_CONFLICT: &str = "album_artwork_conflict";
pub const ERROR_CODE_ALBUM_POSITION_CONFLICT: &str = "album_position_conflict";

/// The rows a commit places files for.
#[derive(Clone, Debug, Default)]
pub struct CommitIdentity {
    pub album_artist_id: String,
    pub album_id: String,
    pub track_id: String,
    pub existing_artwork_path: String,
    pub existing_artwork_sha256: String,
}

/// Every durable phase of one commit.
#[derive(Clone, Debug, Default)]
pub struct CommitJournal {
    pub id: String,
    pub job_id: String,
    pub track_id: String,
    pub phase: String,
    pub staged_file_path: String,
    pub audio_file_path: String,
    pub artwork_file_path: String,
    pub audio_sha256: String,
    pub artwork_sha256: String,
    pub is_artwork_created: bool,
    pub recovery_reason: String,
}

/// Every durable phase of a Track Replacement; a crash at any point leaves
/// either the previous managed file or the verified replacement usable.
#[derive(Clone, Debug, Default)]
pub struct ReplacementJournal {
    pub id: String,
    pub job_id: String,
    pub track_id: String,
    pub phase: String,
    pub staged_file_path: String,
    pub pending_audio_path: String,
    pub audio_file_path: String,
    pub previous_audio_path: String,
    pub retired_audio_path: String,
    pub audio_sha256: String,
    pub previous_audio_sha256: String,
    pub artwork_mode: String,
    pub pending_artwork_path: String,
    pub artwork_file_path: String,
    pub previous_artwork_path: String,
    pub retired_artwork_path: String,
    pub artwork_sha256: String,
    pub previous_artwork_sha256: String,
    pub is_artwork_created: bool,
    pub previous_album_id: String,
    pub recovery_reason: String,
}

/// The Managed Track a Track Replacement overwrites, as storage needs it.
#[derive(Clone, Debug, Default)]
pub struct ReplacementTarget {
    pub track_id: String,
    pub album_id: String,
    pub relative_path: String,
    pub artwork_path: String,
    pub is_sole_track: bool,
}
