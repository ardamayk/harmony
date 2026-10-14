package managedimport

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
)

type AlbumPreview struct {
	Key            string          `json:"key"`
	Title          string          `json:"title"`
	AlbumArtists   []string        `json:"albumArtists"`
	ExistingAlbums []AlbumMatch    `json:"existingAlbums"`
	Artworks       []ArtworkOption `json:"artworks"`
}

type AlbumMatch struct {
	ID          string               `json:"id"`
	Year        int                  `json:"year,omitempty"`
	HasArtwork  bool                 `json:"hasArtwork"`
	Tracks      []DuplicateCandidate `json:"tracks"`
	IdentityKey string               `json:"-"`
}

type ArtworkOption struct {
	ID            string `json:"id"`
	JobID         string `json:"jobId,omitempty"`
	MediaType     string `json:"mediaType"`
	ContentSHA256 string `json:"contentSha256"`
}

type AlbumDecision struct {
	AlbumKey       string `json:"albumKey"`
	AlbumID        string `json:"albumId,omitempty"`
	CreateSeparate bool   `json:"createSeparate"`
	ArtworkID      string `json:"artworkId,omitempty"`
	ArtworkMode    string `json:"artworkMode"`
}

type importPlan struct {
	TargetRevision int             `json:"targetRevision,omitempty"`
	SkipExact      bool            `json:"skipExact,omitempty"`
	AlbumKey       string          `json:"albumKey"`
	ArtworkID      string          `json:"artworkId,omitempty"`
	Action         DuplicateAction `json:"action,omitempty"`
	TargetID       string          `json:"targetId,omitempty"`
}

func albumPreviewKey(file PreviewFile) string {
	return fmt.Sprintf("%x", sha256.Sum256([]byte(previewAlbumKey(file))))
}

func (service *Service) populateAlbumPreviews(ctx context.Context, batch *Batch) error {
	batch.Albums = []AlbumPreview{}
	seen := map[string]bool{}
	for _, file := range batch.Files {
		if file.Preview == nil || file.Status != STATUS_AWAITING_CONFIRMATION {
			continue
		}
		key := albumPreviewKey(file.Preview.File)
		if seen[key] {
			continue
		}
		seen[key] = true
		album, err := service.repo.albumPreview(ctx, batch.ID, file.Preview.File)
		if err != nil {
			return err
		}
		batch.Albums = append(batch.Albums, album)
	}
	return nil
}

func decodeImportPlan(job importJob) (importPlan, error) {
	var plan importPlan
	if job.ImportPlanJSON == "" {
		return plan, nil
	}
	if err := json.Unmarshal([]byte(job.ImportPlanJSON), &plan); err != nil {
		return plan, fmt.Errorf("decode import plan for %q: %w", job.ID, err)
	}
	return plan, nil
}

func (service *Service) applyImportPlan(ctx context.Context, job importJob, inspection library.MediaInspection) (library.MediaInspection, error) {
	plan, err := decodeImportPlan(job)
	if err != nil {
		return inspection, err
	}
	if plan.AlbumKey != "" {
		inspection.Metadata.AlbumIdentityKey = plan.AlbumKey
		inspection.AlbumArtwork = library.AlbumArtwork{}
		if plan.ArtworkID != "" {
			inspection.AlbumArtwork, err = service.repo.loadArtwork(ctx, job.BatchID, plan.ArtworkID)
		}
		if err != nil {
			return inspection, err
		}
	}
	artwork, exists, err := service.repo.ExistingAlbumArtwork(ctx, albumIdentityKey(inspection.Metadata))
	if exists {
		inspection.AlbumArtwork = artwork
	}
	return inspection, err
}
