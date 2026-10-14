package managedimport

import (
	"context"
	"errors"
	"fmt"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
)

func (service *Service) prepareBatchPlans(ctx context.Context, batch Batch, jobs []importJob, selected map[string]bool, confirmation BatchConfirmation) error {
	selectedBatch := batch
	selectedBatch.Files = nil
	for _, file := range batch.Files {
		if selected[file.JobID] {
			selectedBatch.Files = append(selectedBatch.Files, file)
		}
	}
	albums, err := service.resolveAlbumPlans(ctx, selectedBatch, confirmation.AlbumDecisions)
	if err != nil {
		return err
	}
	decisions := map[string]DuplicateDecision{}
	known := map[string]bool{}
	for _, job := range jobs {
		known[job.ID] = true
	}
	for id := range selected {
		if !known[id] {
			return fmt.Errorf("%w: selected file does not belong to this batch", ErrInvalidUpload)
		}
	}
	for _, decision := range confirmation.DuplicateDecisions {
		if !known[decision.JobID] || decisions[decision.JobID].JobID != "" {
			return fmt.Errorf("%w: file decisions require unique batch job IDs", ErrInvalidUpload)
		}
		decisions[decision.JobID] = decision
	}
	plans := map[string]importPlan{}
	positions, hashes := map[string]bool{}, map[string]bool{}
	for _, job := range jobs {
		if !selected[job.ID] {
			continue
		}
		if job.Status != STATUS_AWAITING_CONFIRMATION {
			return ErrInvalidState
		}
		if hashes[job.ContentSHA256] {
			delete(selected, job.ID)
			plans[job.ID] = importPlan{SkipExact: true}
			continue
		}
		hashes[job.ContentSHA256] = true
		preview, err := decodeStoredPreview(job)
		if err != nil {
			return err
		}
		plan := albums[albumPreviewKey(preview.File)]
		position := fmt.Sprintf("%s\x1f%d\x1f%d", plan.AlbumKey, preview.File.DiscNo, preview.File.TrackNo)
		if positions[position] {
			return fmt.Errorf("%w: choose one file for each Album position", ErrInvalidUpload)
		}
		positions[position] = true
		plan, err = service.planTrack(ctx, preview.File, plan, decisions[job.ID])
		if err != nil {
			return err
		}
		plans[job.ID] = plan
	}
	if err := validatePlannedDiscNumbers(jobs, plans); err != nil {
		return err
	}
	return service.repo.saveImportPlans(ctx, batch, plans)
}

func (service *Service) resolveAlbumPlans(ctx context.Context, batch Batch, decisions []AlbumDecision) (map[string]importPlan, error) {
	if err := service.populateAlbumPreviews(ctx, &batch); err != nil {
		return nil, err
	}
	byKey := map[string]AlbumDecision{}
	for _, decision := range decisions {
		if decision.AlbumKey == "" || byKey[decision.AlbumKey].AlbumKey != "" {
			return nil, fmt.Errorf("%w: Album decisions require unique keys", ErrInvalidUpload)
		}
		byKey[decision.AlbumKey] = decision
	}
	plans := map[string]importPlan{}
	for _, album := range batch.Albums {
		baseKey := ""
		for _, file := range batch.Files {
			if file.Preview != nil && albumPreviewKey(file.Preview.File) == album.Key {
				baseKey = previewAlbumKey(file.Preview.File)
				break
			}
		}
		plan, err := resolveAlbumPlan(batch.ID, baseKey, album, byKey[album.Key])
		if err != nil {
			return nil, err
		}
		plans[album.Key] = plan
		delete(byKey, album.Key)
	}
	// Decisions for skipped Albums are harmless; they create no content.
	return plans, nil
}

func resolveAlbumPlan(batchID, baseKey string, album AlbumPreview, decision AlbumDecision) (importPlan, error) {
	plan := importPlan{AlbumKey: baseKey}
	hasArtwork := false
	if decision.CreateSeparate {
		if decision.AlbumID != "" {
			return plan, fmt.Errorf("%w: a separate Album cannot target an existing Album", ErrInvalidUpload)
		}
		plan.AlbumKey += "\x1fmanaged-import-edition:" + batchID
	} else {
		if decision.AlbumID == "" && len(album.ExistingAlbums) > 1 {
			return plan, fmt.Errorf("%w: choose an existing Album or create a separate Album", ErrInvalidUpload)
		}
		if decision.AlbumID == "" && len(album.ExistingAlbums) == 1 {
			decision.AlbumID = album.ExistingAlbums[0].ID
		}
		found := decision.AlbumID == ""
		for _, match := range album.ExistingAlbums {
			if match.ID == decision.AlbumID {
				plan.AlbumKey = match.IdentityKey
				hasArtwork = match.HasArtwork
				found = true
			}
		}
		if !found {
			return plan, fmt.Errorf("%w: selected Album does not match the file tags", ErrInvalidUpload)
		}
	}
	if hasArtwork {
		return plan, nil
	}
	if decision.ArtworkMode == "" && decision.AlbumID != "" {
		return plan, nil
	}
	switch decision.ArtworkMode {
	case "none":
		return plan, nil
	case "", "auto":
		if len(album.Artworks) > 1 {
			return plan, fmt.Errorf("%w: choose an Album cover or continue without artwork", ErrInvalidUpload)
		}
		if len(album.Artworks) == 1 {
			plan.ArtworkID = album.Artworks[0].ID
		}
	case "selected":
		for _, artwork := range album.Artworks {
			if artwork.ID == decision.ArtworkID {
				plan.ArtworkID = artwork.ID
				return plan, nil
			}
		}
		return plan, fmt.Errorf("%w: selected artwork does not belong to this Album", ErrInvalidUpload)
	default:
		return plan, fmt.Errorf("%w: invalid artwork mode", ErrInvalidUpload)
	}
	return plan, nil
}

func (service *Service) planTrack(ctx context.Context, file PreviewFile, plan importPlan, decision DuplicateDecision) (importPlan, error) {
	candidates, err := service.repo.positionCandidates(ctx, metadataFromPreview(file, plan.AlbumKey), "")
	if err != nil {
		return plan, err
	}
	if len(candidates) == 0 {
		if decision.Action != "" {
			return plan, fmt.Errorf("%w: this file does not replace a Track", ErrInvalidUpload)
		}
		return plan, nil
	}
	if len(candidates) != 1 || normalizeIdentity(candidates[0].Title) != normalizeIdentity(file.Title) {
		return plan, albumPositionConflict()
	}
	if decision.Action != DUPLICATE_ACTION_REPLACE_EXISTING || decision.TrackID != candidates[0].TrackID || decision.TargetRevision != candidates[0].Revision {
		return plan, fmt.Errorf("%w: confirm replacement of the matching Track or skip this file", ErrRevisionConflict)
	}
	plan.Action, plan.TargetID, plan.TargetRevision = decision.Action, candidates[0].TrackID, candidates[0].Revision
	return plan, nil
}

func metadataFromPreview(file PreviewFile, albumKey string) library.NormalizedMediaMetadata {
	return library.NormalizedMediaMetadata{Title: file.Title, Artists: file.Artists, AlbumArtists: file.AlbumArtists, Album: file.Album,
		AlbumIdentityKey: albumKey, TrackPosition: library.MediaPosition{Number: file.TrackNo, Total: file.TrackTotal},
		DiscPosition: library.MediaPosition{Number: file.DiscNo, Total: file.DiscTotal}, Genres: file.Genres, Year: file.Year}
}

func albumPositionConflict() error {
	reason := "another title occupies this Album position; resolve the conflict or choose a separate Album"
	return &ValidationError{Code: ERROR_CODE_ALBUM_POSITION_CONFLICT, Field: "TRACKNUMBER", Reason: reason, Err: errors.New(reason)}
}

func validatePlannedDiscNumbers(jobs []importJob, plans map[string]importPlan) error {
	multiDiscAlbums := map[string]bool{}
	missingDiscAlbums := map[string]bool{}
	for _, job := range jobs {
		plan, ok := plans[job.ID]
		if !ok || plan.SkipExact {
			continue
		}
		preview, err := decodeStoredPreview(job)
		if err != nil {
			return err
		}
		if preview.File.DiscNo > 1 || preview.File.DiscTotal > 1 {
			multiDiscAlbums[plan.AlbumKey] = true
		}
		if !preview.File.HasDiscNumber {
			missingDiscAlbums[plan.AlbumKey] = true
		}
	}
	for key := range multiDiscAlbums {
		if missingDiscAlbums[key] {
			return &ValidationError{Code: string(library.INSPECTION_ERROR_INVALID_METADATA), Field: "DISCNUMBER", Reason: "DISCNUMBER is required for every selected file in a multi-disc Album", Err: errors.New("missing explicit disc number")}
		}
	}
	return nil
}
