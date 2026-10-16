//! Track Replacement (track_replacement.go, repository_replacement.go).
//! Only the pieces Managed Import's jobs need exist so far; the
//! replacement flow itself is ported with Permanent Track Deletion.

use super::errors::{Error, Result};
use super::model::{ImportJob, ImportResult, Preview};
use super::repository::{Repository, context};
use super::service::Service;
use super::types::ReplacementJournal;
use crate::inspect::MediaInspection;

/// What a confirmed replacement needs (`replacementState`).
pub struct ReplacementState {
    pub preview: Box<serde_json::value::RawValue>,
}

fn not_ported() -> Error {
    Error::new("Track Replacement is not available in the Rust Music Server yet")
}

impl Repository {
    pub async fn find_incomplete_replacement_journal(
        &self,
        job_id: &str,
    ) -> Result<Option<ReplacementJournal>> {
        let row = sqlx::query!(
            r#"SELECT id AS "id!", job_id, track_id, phase FROM managed_track_replacements
            WHERE job_id = ? AND phase NOT IN ('completed', 'rolled_back')"#,
            job_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(context("read Track Replacement journal"))?;
        Ok(row.map(|row| ReplacementJournal {
            id: row.id,
            job_id: row.job_id,
            track_id: row.track_id,
            phase: row.phase,
            ..ReplacementJournal::default()
        }))
    }
}

impl Service {
    pub(super) async fn recover_replacement(&self, _journal: &ReplacementJournal) -> Result<()> {
        Err(not_ported())
    }

    pub(super) async fn build_replacement_state(
        &self,
        _job: &ImportJob,
        _inspection: &MediaInspection,
        _staged_path: &str,
    ) -> Result<ReplacementState> {
        Err(not_ported())
    }

    pub(super) async fn commit_replacement(
        &self,
        _job: &ImportJob,
        _state: ReplacementState,
    ) -> Result<ImportResult> {
        Err(not_ported())
    }
}

pub fn attach(preview: &mut Preview, state: &ReplacementState) {
    preview.replacement = Some(state.preview.clone());
}
