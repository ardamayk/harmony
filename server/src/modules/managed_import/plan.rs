//! Batch confirmation planning: one Album and artwork choice per Album, one
//! file per position, replacements confirmed explicitly (import-plan.go,
//! album-preview.go).

use super::errors::{Error, Result, Sentinel, ValidationError};
use super::identity::{album_identity_key, normalize_identity};
use super::model::*;
use super::preview::{album_preview_key, preview_album_key};
use super::service::Service;
use super::types::ERROR_CODE_ALBUM_POSITION_CONFLICT;
use crate::inspect::{AlbumArtwork, MediaInspection, MediaPosition, NormalizedMetadata};
use std::collections::{HashMap, HashSet};

fn invalid(detail: &str) -> Error {
    Error::wrap(Sentinel::InvalidUpload, detail)
}

pub fn album_position_conflict() -> Error {
    let reason = "another title occupies this Album position; resolve the conflict or choose a separate Album";
    Error::validation(
        ValidationError::new(ERROR_CODE_ALBUM_POSITION_CONFLICT, "TRACKNUMBER", reason),
        reason,
    )
}

pub fn decode_import_plan(job: &ImportJob) -> Result<ImportPlan> {
    if job.import_plan_json.is_empty() {
        return Ok(ImportPlan::default());
    }
    serde_json::from_str(&job.import_plan_json)
        .map_err(|error| Error::new(format!("decode import plan for {:?}: {error}", job.id())))
}

pub fn decode_stored_preview(job: &ImportJob) -> Result<Preview> {
    serde_json::from_str(&job.preview_json).map_err(|error| {
        Error::new(format!(
            "decode duplicate preview for job {:?}: {error}",
            job.id()
        ))
    })
}

pub fn metadata_from_preview(file: &PreviewFile, album_key: &str) -> NormalizedMetadata {
    NormalizedMetadata {
        title: file.title.clone(),
        artists: file.artists.clone().unwrap_or_default(),
        album_artists: file.album_artists.clone().unwrap_or_default(),
        album_artists_from_artists: !file.album_artist_warning.is_empty(),
        album: file.album.clone(),
        album_identity_key: album_key.to_owned(),
        track_position: MediaPosition {
            number: file.track_no,
            total: file.track_total,
        },
        disc_position: MediaPosition {
            number: file.disc_no,
            total: file.disc_total,
        },
        genres: file.genres.clone().unwrap_or_default(),
        year: file.year,
        ..NormalizedMetadata::default()
    }
}

/// `resolveAlbumPlan`: the Album key and artwork a decision selects.
fn resolve_album_plan(
    batch_id: &str,
    base_key: &str,
    album: &AlbumPreview,
    decision: &AlbumDecision,
) -> Result<ImportPlan> {
    let mut plan = ImportPlan {
        album_key: base_key.to_owned(),
        ..ImportPlan::default()
    };
    let mut album_id = decision.album_id.clone();
    let mut has_artwork = false;
    if decision.create_separate {
        if !album_id.is_empty() {
            return Err(invalid("a separate Album cannot target an existing Album"));
        }
        plan.album_key
            .push_str(&format!("\u{1f}managed-import-edition:{batch_id}"));
    } else {
        if album_id.is_empty() && album.existing_albums.len() > 1 {
            return Err(invalid(
                "choose an existing Album or create a separate Album",
            ));
        }
        if album_id.is_empty() && album.existing_albums.len() == 1 {
            album_id = album.existing_albums[0].id.clone();
        }
        let mut found = album_id.is_empty();
        for existing in &album.existing_albums {
            if existing.id == album_id {
                plan.album_key = existing.identity_key.clone();
                has_artwork = existing.has_artwork;
                found = true;
            }
        }
        if !found {
            return Err(invalid("selected Album does not match the file tags"));
        }
    }
    if has_artwork {
        return Ok(plan);
    }
    if decision.artwork_mode.is_empty() && !album_id.is_empty() {
        return Ok(plan);
    }
    match decision.artwork_mode.as_str() {
        "none" => {}
        "" | "auto" => {
            if album.artworks.len() > 1 {
                return Err(invalid("choose an Album cover or continue without artwork"));
            }
            if let Some(artwork) = album.artworks.first() {
                plan.artwork_id = artwork.id.clone();
            }
        }
        "selected" => {
            return match album
                .artworks
                .iter()
                .find(|artwork| artwork.id == decision.artwork_id)
            {
                Some(artwork) => {
                    plan.artwork_id = artwork.id.clone();
                    Ok(plan)
                }
                None => Err(invalid("selected artwork does not belong to this Album")),
            };
        }
        _ => return Err(invalid("invalid artwork mode")),
    }
    Ok(plan)
}

fn validate_planned_disc_numbers(
    jobs: &[ImportJob],
    plans: &HashMap<String, ImportPlan>,
) -> Result<()> {
    let mut multi_disc = HashSet::new();
    let mut missing_disc = HashSet::new();
    for job in jobs {
        let Some(plan) = plans.get(job.id()) else {
            continue;
        };
        if plan.skip_exact {
            continue;
        }
        let preview = decode_stored_preview(job)?;
        if preview.file.disc_no > 1 || preview.file.disc_total > 1 {
            multi_disc.insert(plan.album_key.clone());
        }
        if !preview.file.has_disc_number {
            missing_disc.insert(plan.album_key.clone());
        }
    }
    if multi_disc.iter().any(|key| missing_disc.contains(key)) {
        return Err(Error::validation(
            ValidationError::new(
                "invalid_metadata",
                "DISCNUMBER",
                "DISCNUMBER is required for every selected file in a multi-disc Album",
            ),
            "missing explicit disc number",
        ));
    }
    Ok(())
}

impl Service {
    /// `populateAlbumPreviews`: one preview per Album among accepted files.
    pub(super) async fn populate_album_previews(&self, batch: &mut Batch) -> Result<()> {
        batch.albums = Vec::new();
        let mut seen = HashSet::new();
        for file in &batch.files {
            let Some(preview) = &file.preview else {
                continue;
            };
            if file.status != STATUS_AWAITING_CONFIRMATION {
                continue;
            }
            if !seen.insert(album_preview_key(&preview.file)) {
                continue;
            }
            let album = self.repo.album_preview(&batch.id, &preview.file).await?;
            batch.albums.push(album);
        }
        Ok(())
    }

    async fn resolve_album_plans(
        &self,
        mut batch: Batch,
        decisions: &[AlbumDecision],
    ) -> Result<HashMap<String, ImportPlan>> {
        self.populate_album_previews(&mut batch).await?;
        let mut by_key: HashMap<String, &AlbumDecision> = HashMap::new();
        for decision in decisions {
            if decision.album_key.is_empty() || by_key.contains_key(&decision.album_key) {
                return Err(invalid("Album decisions require unique keys"));
            }
            by_key.insert(decision.album_key.clone(), decision);
        }
        let none = AlbumDecision::default();
        let mut plans = HashMap::new();
        for album in &batch.albums {
            let base_key = batch
                .files
                .iter()
                .filter_map(|file| file.preview.as_ref())
                .find(|preview| album_preview_key(&preview.file) == album.key)
                .map(|preview| preview_album_key(&preview.file))
                .unwrap_or_default();
            let decision = by_key.get(&album.key).copied().unwrap_or(&none);
            plans.insert(
                album.key.clone(),
                resolve_album_plan(&batch.id, &base_key, album, decision)?,
            );
        }
        // Decisions for skipped Albums are harmless; they create no content.
        Ok(plans)
    }

    async fn plan_track(
        &self,
        file: &PreviewFile,
        mut plan: ImportPlan,
        decision: Option<&DuplicateDecision>,
    ) -> Result<ImportPlan> {
        let candidates = self
            .repo
            .position_candidates(&metadata_from_preview(file, &plan.album_key), "")
            .await?;
        let action = decision.map_or("", |decision| decision.action.as_str());
        if candidates.is_empty() {
            if !action.is_empty() {
                return Err(invalid("this file does not replace a Track"));
            }
            return Ok(plan);
        }
        if candidates.len() != 1
            || normalize_identity(&candidates[0].title) != normalize_identity(&file.title)
        {
            return Err(album_position_conflict());
        }
        let decision = decision.cloned().unwrap_or_default();
        if decision.action != DUPLICATE_ACTION_REPLACE_EXISTING
            || decision.track_id != candidates[0].track_id
            || decision.target_revision != candidates[0].revision
        {
            return Err(Error::wrap(
                Sentinel::RevisionConflict,
                "confirm replacement of the matching Track or skip this file",
            ));
        }
        plan.action = decision.action;
        plan.target_id = candidates[0].track_id.clone();
        plan.target_revision = candidates[0].revision;
        Ok(plan)
    }

    /// `prepareBatchPlans`: validates every choice and stores one plan per
    /// selected file.
    pub(super) async fn prepare_batch_plans(
        &self,
        batch: &Batch,
        jobs: &[ImportJob],
        selected: &mut Vec<String>,
        confirmation: &BatchConfirmation,
    ) -> Result<()> {
        let mut selected_batch = batch.clone();
        selected_batch
            .files
            .retain(|file| selected.contains(&file.job_id));
        let albums = self
            .resolve_album_plans(selected_batch, &confirmation.album_decisions)
            .await?;
        let known: HashSet<&str> = jobs.iter().map(ImportJob::id).collect();
        if selected.iter().any(|id| !known.contains(id.as_str())) {
            return Err(invalid("selected file does not belong to this batch"));
        }
        let mut decisions: HashMap<&str, &DuplicateDecision> = HashMap::new();
        for decision in &confirmation.duplicate_decisions {
            if !known.contains(decision.job_id.as_str())
                || decisions.contains_key(decision.job_id.as_str())
            {
                return Err(invalid("file decisions require unique batch job IDs"));
            }
            decisions.insert(&decision.job_id, decision);
        }
        let mut plans = HashMap::new();
        let mut positions = HashSet::new();
        let mut hashes = HashSet::new();
        for job in jobs {
            if !selected.iter().any(|id| id == job.id()) {
                continue;
            }
            if job.status() != STATUS_AWAITING_CONFIRMATION {
                return Err(Error::sentinel(Sentinel::InvalidState));
            }
            if !hashes.insert(job.content_sha256.clone()) {
                selected.retain(|id| id != job.id());
                plans.insert(
                    job.id().to_owned(),
                    ImportPlan {
                        skip_exact: true,
                        ..ImportPlan::default()
                    },
                );
                continue;
            }
            let preview = decode_stored_preview(job)?;
            let plan = albums
                .get(&album_preview_key(&preview.file))
                .cloned()
                .unwrap_or_default();
            let position = format!(
                "{}\u{1f}{}\u{1f}{}",
                plan.album_key, preview.file.disc_no, preview.file.track_no
            );
            if !positions.insert(position) {
                return Err(invalid("choose one file for each Album position"));
            }
            let plan = self
                .plan_track(&preview.file, plan, decisions.get(job.id()).copied())
                .await?;
            plans.insert(job.id().to_owned(), plan);
        }
        validate_planned_disc_numbers(jobs, &plans)?;
        self.repo.save_import_plans(batch, &plans).await
    }

    /// `applyImportPlan`: the chosen Album and artwork, then the Album's
    /// stored artwork when it has one.
    pub(super) async fn apply_import_plan(
        &self,
        job: &ImportJob,
        mut inspection: MediaInspection,
    ) -> Result<MediaInspection> {
        let plan = decode_import_plan(job)?;
        if !plan.album_key.is_empty() {
            inspection.metadata.album_identity_key = plan.album_key.clone();
            inspection.album_artwork = AlbumArtwork::default();
            if !plan.artwork_id.is_empty() {
                inspection.album_artwork = self
                    .repo
                    .load_artwork(&job.batch_id, &plan.artwork_id)
                    .await?;
            }
        }
        if let Some(artwork) = self
            .repo
            .existing_album_artwork(&album_identity_key(&inspection.metadata))
            .await?
        {
            inspection.album_artwork = artwork;
        }
        Ok(inspection)
    }
}
