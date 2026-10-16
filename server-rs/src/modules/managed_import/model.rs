//! Managed Import API and stored JSON shapes (managedimport/types.go,
//! album-preview.go). Field order, `omitempty` and null-versus-empty lists
//! follow Go's encoding, since Import Previews are stored as JSON and served
//! again.

use super::errors::Issue;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

pub const STATUS_UPLOADING: &str = "uploading";
pub const STATUS_AWAITING_CONFIRMATION: &str = "awaiting_confirmation";
pub const STATUS_COMMITTED: &str = "committed";
pub const STATUS_FAILED: &str = "failed";

pub const BATCH_STATUS_UPLOADING: &str = "uploading";
pub const BATCH_STATUS_CONFIRMING: &str = "confirming";
pub const BATCH_STATUS_COMPLETED: &str = "completed";

pub const BATCH_FILE_ACCEPTED: &str = "accepted";
pub const BATCH_FILE_REJECTED: &str = "rejected";
pub const BATCH_FILE_UNRESOLVED: &str = "unresolved";
pub const BATCH_FILE_COMPLETED: &str = "completed";

pub const OUTCOME_IMPORTED: &str = "imported";
pub const OUTCOME_REJECTED: &str = "rejected";
pub const OUTCOME_FAILED: &str = "failed";
pub const OUTCOME_REPLACED: &str = "replaced";
pub const OUTCOME_NOT_ATTEMPTED: &str = "not_attempted";

pub const DUPLICATE_NONE: &str = "none";
pub const DUPLICATE_EXACT: &str = "exact_duplicate";

pub const DUPLICATE_ACTION_IMPORT_SEPARATELY: &str = "import_separately";
pub const DUPLICATE_ACTION_REPLACE_EXISTING: &str = "replace_existing";

pub const COMMIT_PHASE_PREPARED: &str = "prepared";
pub const COMMIT_PHASE_PLACED: &str = "placed";
pub const COMMIT_PHASE_VERIFIED: &str = "verified";
pub const COMMIT_PHASE_DATABASE_COMMITTED: &str = "database_committed";
pub const COMMIT_PHASE_CLEANED: &str = "cleaned";
pub const COMMIT_PHASE_ROLLED_BACK: &str = "rolled_back";

pub const HISTORY_RESULT_COMPLETED: &str = "completed";
pub const HISTORY_RESULT_PARTIALLY_COMPLETED: &str = "partially_completed";
pub const HISTORY_RESULT_FAILED: &str = "failed";
pub const HISTORY_RESULT_CANCELED: &str = "canceled";

pub const ERROR_CODE_EXACT_DUPLICATE: &str = "exact_duplicate";
pub const ERROR_CODE_REVISION_CONFLICT: &str = "import_revision_conflict";
pub const UPLOAD_INTERRUPTED_ERROR_CODE: &str = "upload_interrupted";
pub const IMPORT_CANCELED_RESULT_CODE: &str = "canceled";
pub const IMPORT_HISTORY_LIMIT: i64 = 20;
pub const MAX_ORIGINAL_FILENAME_BYTES: usize = 255;
pub const BITS_PER_KILOBIT: i64 = 1000;

fn is_zero(value: &i64) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub status: String,
    pub revision: i64,
    pub validation_progress: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error_code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub track_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub replaces_track_id: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Batch {
    pub albums: Vec<AlbumPreview>,
    pub id: String,
    pub status: String,
    pub revision: i64,
    pub files: Vec<BatchFile>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchFile {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub phase: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub transferred_bytes: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub total_bytes: i64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<Issue>,
    pub job_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub client_file_id: String,
    pub state: String,
    pub status: String,
    pub revision: i64,
    pub validation_progress: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub original_filename: String,
    pub selected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Preview>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error_code: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error_field: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error_reason: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub outcome: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub track_id: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct BatchConfirmation {
    #[serde(deserialize_with = "crate::gojson::nullable")]
    pub album_decisions: Vec<AlbumDecision>,
    pub revision: i64,
    pub selected_file_ids: Option<Vec<String>>,
    #[serde(deserialize_with = "crate::gojson::nullable")]
    pub duplicate_decisions: Vec<DuplicateDecision>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DuplicateDecision {
    pub target_revision: i64,
    #[serde(deserialize_with = "crate::gojson::nullable")]
    pub track_id: String,
    #[serde(deserialize_with = "crate::gojson::nullable")]
    pub job_id: String,
    #[serde(deserialize_with = "crate::gojson::nullable")]
    pub action: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preview {
    #[serde(skip_serializing_if = "NoneOrEmpty::is_none_or_empty")]
    pub matching_tracks: Option<Vec<DuplicateCandidate>>,
    pub job_id: String,
    pub status: String,
    pub revision: i64,
    pub file: PreviewFile,
    pub duplicate_classification: String,
    #[serde(skip_serializing_if = "NoneOrEmpty::is_none_or_empty")]
    pub duplicate_candidates: Option<Vec<DuplicateCandidate>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement: Option<Box<RawValue>>,
}

/// `omitempty` on a Go slice: absent when nil or empty.
trait NoneOrEmpty {
    fn is_none_or_empty(&self) -> bool;
}

impl<T> NoneOrEmpty for Option<Vec<T>> {
    fn is_none_or_empty(&self) -> bool {
        self.as_ref().is_none_or(Vec::is_empty)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DuplicateCandidate {
    pub revision: i64,
    pub title_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_file: Option<Box<PreviewFile>>,
    pub track_id: String,
    pub title: String,
    pub artists: Option<Vec<String>>,
    pub album: String,
    pub disc_no: i64,
    pub track_no: i64,
    pub format: String,
    pub duration_ms: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PreviewFile {
    pub has_disc_number: bool,
    pub title_key: String,
    pub album_key: String,
    #[serde(rename = "contentSha256")]
    pub content_sha256: String,
    pub size_bytes: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub artwork_warning: String,
    pub original_filename: String,
    pub title: String,
    pub artists: Option<Vec<String>>,
    pub album_artists: Option<Vec<String>>,
    pub album: String,
    pub genres: Option<Vec<String>>,
    pub track_no: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub track_total: i64,
    pub disc_no: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub disc_total: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub year: i64,
    pub duration_ms: i64,
    pub format: String,
    pub container: String,
    pub codec: String,
    pub sample_rate_hz: i64,
    pub channel_count: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub bit_depth: i64,
    pub bitrate_kbps: i64,
    pub artwork_media_type: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Confirmation {
    pub revision: i64,
    #[serde(deserialize_with = "crate::gojson::nullable")]
    pub duplicate_decision: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub job_id: String,
    pub status: String,
    pub revision: i64,
    pub track_id: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct HistoryList {
    pub items: Vec<HistoryItem>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub import_id: String,
    pub started_at: String,
    pub completed_at: String,
    pub result_code: String,
    pub counts: HistoryCounts,
    pub files: Vec<HistoryFile>,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryCounts {
    pub total: i64,
    pub imported: i64,
    pub rejected: i64,
    pub failed: i64,
    pub replaced: i64,
    pub not_attempted: i64,
    pub canceled: i64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryFile {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<Issue>,
    pub file_id: String,
    pub job_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub safe_filename: String,
    pub started_at: String,
    pub completed_at: String,
    #[serde(rename = "contentSha256", skip_serializing_if = "String::is_empty")]
    pub content_sha256: String,
    pub result_code: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created_track_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub replaced_track_id: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumPreview {
    pub key: String,
    pub title: String,
    pub album_artists: Option<Vec<String>>,
    pub existing_albums: Vec<AlbumMatch>,
    pub artworks: Vec<ArtworkOption>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumMatch {
    pub id: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub year: i64,
    pub has_artwork: bool,
    pub tracks: Vec<DuplicateCandidate>,
    #[serde(skip)]
    pub identity_key: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkOption {
    pub id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub job_id: String,
    pub media_type: String,
    #[serde(rename = "contentSha256")]
    pub content_sha256: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AlbumDecision {
    #[serde(deserialize_with = "crate::gojson::nullable")]
    pub album_key: String,
    #[serde(deserialize_with = "crate::gojson::nullable")]
    pub album_id: String,
    pub create_separate: bool,
    #[serde(deserialize_with = "crate::gojson::nullable")]
    pub artwork_id: String,
    #[serde(deserialize_with = "crate::gojson::nullable")]
    pub artwork_mode: String,
}

/// The per-file plan a Batch confirmation stores (`importPlan`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportPlan {
    #[serde(skip_serializing_if = "is_zero")]
    pub target_revision: i64,
    #[serde(skip_serializing_if = "is_false")]
    pub skip_exact: bool,
    pub album_key: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub artwork_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub action: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub target_id: String,
}

/// A Job row with everything the service needs (`importJob`).
#[derive(Clone, Debug, Default)]
pub struct ImportJob {
    pub job: Job,
    pub import_plan_json: String,
    pub issues: Vec<Issue>,
    pub batch_id: String,
    pub client_file_id: String,
    pub original_filename: String,
    pub staged_file_path: String,
    pub content_sha256: String,
    pub preview_json: String,
    pub error_field: String,
    pub error_reason: String,
    pub outcome: String,
    pub selected: bool,
    pub replace_track_id: String,
}

impl ImportJob {
    pub fn id(&self) -> &str {
        &self.job.id
    }

    pub fn status(&self) -> &str {
        &self.job.status
    }

    pub fn revision(&self) -> i64 {
        self.job.revision
    }

    pub fn error_code(&self) -> &str {
        &self.job.error_code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_round_trip_null_and_empty_lists_like_go() {
        let stored = r#"{"jobId":"j","status":"awaiting_confirmation","revision":2,"file":{"hasDiscNumber":false,"titleKey":"t","albumKey":"k","contentSha256":"h","sizeBytes":3,"originalFilename":"a.flac","title":"T","artists":["A"],"albumArtists":["B"],"album":"Al","genres":[],"trackNo":1,"discNo":1,"durationMs":1000,"format":"flac","container":"flac","codec":"flac","sampleRateHz":44100,"channelCount":2,"bitrateKbps":900,"artworkMediaType":""},"duplicateClassification":"none","duplicateCandidates":[{"revision":1,"titleKey":"t","trackId":"x","title":"T","artists":null,"album":"Al","discNo":1,"trackNo":1,"format":"flac","durationMs":1}],"replacement":{"b":1,"a":[2]}}"#;
        let preview: Preview = serde_json::from_str(stored).unwrap();
        assert_eq!(serde_json::to_string(&preview).unwrap(), stored);
    }
}
