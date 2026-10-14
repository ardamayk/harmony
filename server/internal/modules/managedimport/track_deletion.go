package managedimport

import (
	"context"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"sync"
	"time"
)

const PERMANENT_DELETE_CONFIRMATION_HEADER = "X-Permanent-Delete"

var permanentTrackDeletionMu sync.Mutex

type TrackDeletionManagedFile struct {
	Path      string `json:"path"`
	SizeBytes int64  `json:"sizeBytes"`
}

type TrackDeletionPlaylistReference struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

type TrackDeletionQueueReference struct {
	UserID    string `json:"userId"`
	ItemCount int    `json:"itemCount"`
}

type TrackDeletionPreview struct {
	TrackID            string                           `json:"trackId"`
	TrackTitle         string                           `json:"trackTitle"`
	ManagedFile        TrackDeletionManagedFile         `json:"managedFile"`
	PlaylistReferences []TrackDeletionPlaylistReference `json:"playlistReferences"`
	QueueReferences    []TrackDeletionQueueReference    `json:"queueReferences"`
	ConfirmationToken  string                           `json:"confirmationToken"`
}

type TrackDeletionConfirmation struct {
	ConfirmationToken string `json:"confirmationToken"`
}

type TrackDeletionResult struct {
	DeletedFiles int `json:"deletedFiles"`
}

type trackDeletionState struct {
	Preview        TrackDeletionPreview
	AlbumID        string
	FilePath       string
	ContentSHA256  string
	ArtworkPath    string
	ArtworkSHA256  string
	TrackRevision  int
	SourceRevision int
}

type QueueInvalidationPublisher interface {
	Publish(userID, revision, sequence string)
}

type discardQueueInvalidations struct{}

func (discardQueueInvalidations) Publish(string, string, string) {}

type queueInvalidation struct {
	userID   string
	revision string
	sequence string
}

func (service *Service) PreviewTrackDeletion(ctx context.Context, trackID string) (TrackDeletionPreview, error) {
	state, err := service.repo.LoadTrackDeletion(ctx, service.storage, trackID)
	if err != nil {
		return TrackDeletionPreview{}, err
	}
	return state.Preview, nil
}

func (service *Service) DeleteTrack(ctx context.Context, confirmation TrackDeletionRequest) (TrackDeletionResult, error) {
	permanentTrackDeletionMu.Lock()
	defer permanentTrackDeletionMu.Unlock()
	managedImportCommitMu.Lock()
	defer managedImportCommitMu.Unlock()

	state, err := service.repo.PrepareTrackDeletion(ctx, service.storage, confirmation)
	if err != nil {
		return TrackDeletionResult{}, err
	}
	hasRemoved, allRemoved, removeErr := service.removePreparedDeletionFiles(ctx, state)
	completionContext, cancelCompletion := context.WithTimeout(context.WithoutCancel(ctx), 60*time.Second)
	defer cancelCompletion()
	if removeErr != nil && !hasRemoved {
		return TrackDeletionResult{}, errors.Join(removeErr, service.repo.CancelTrackDeletion(completionContext, state.Preview.TrackID))
	}
	if !allRemoved {
		return TrackDeletionResult{}, removeErr
	}
	invalidations, finalizeErr := service.repo.FinalizeTrackDeletion(completionContext, state.Preview.TrackID)
	if finalizeErr != nil {
		return TrackDeletionResult{}, errors.Join(removeErr, finalizeErr)
	}
	service.publishQueueInvalidations(invalidations)
	deletedFiles := 1
	if state.ArtworkPath != "" {
		deletedFiles++
	}
	return TrackDeletionResult{DeletedFiles: deletedFiles}, removeErr
}

type TrackDeletionRequest struct {
	TrackID           string
	ConfirmationToken string
}

func (service *Service) publishQueueInvalidations(invalidations []queueInvalidation) {
	for _, invalidation := range invalidations {
		service.queueEvents.Publish(invalidation.userID, invalidation.revision, invalidation.sequence)
	}
}

func (service *Service) removePreparedDeletionFiles(ctx context.Context, state trackDeletionState) (hasRemoved, allRemoved bool, returnErr error) {
	audioRemoved, audioErr := service.storage.RemoveManagedFile(ctx, state.FilePath, state.ContentSHA256)
	if !audioRemoved {
		return false, false, audioErr
	}
	if state.ArtworkPath == "" {
		return true, true, audioErr
	}
	artworkRemoved, artworkErr := service.storage.RemoveManagedFile(ctx, state.ArtworkPath, state.ArtworkSHA256)
	return true, artworkRemoved, errors.Join(audioErr, artworkErr)
}

func (service *Service) RecoverPendingTrackDeletions(ctx context.Context) error {
	pending, err := service.repo.ListPendingDeletions(ctx)
	if err != nil {
		return err
	}
	for _, deletion := range pending {
		state := trackDeletionState{
			Preview:  TrackDeletionPreview{TrackID: deletion.trackID},
			FilePath: deletion.filePath, ContentSHA256: deletion.contentSHA256,
			ArtworkPath: deletion.artworkPath, ArtworkSHA256: deletion.artworkSHA256,
		}
		_, allRemoved, removeErr := service.removePreparedDeletionFiles(ctx, state)
		if !allRemoved {
			return fmt.Errorf("recover pending Permanent Track Deletion %q: %w", deletion.trackID, removeErr)
		}
		invalidations, finalizeErr := service.repo.FinalizeTrackDeletion(ctx, deletion.trackID)
		if finalizeErr != nil {
			return fmt.Errorf("finalize recovered Permanent Track Deletion %q: %w", deletion.trackID, errors.Join(removeErr, finalizeErr))
		}
		service.publishQueueInvalidations(invalidations)
		if removeErr != nil {
			return fmt.Errorf("close Managed Storage after recovering Permanent Track Deletion %q: %w", deletion.trackID, removeErr)
		}
	}
	return nil
}

func deletionToken(state trackDeletionState) (string, error) {
	payload := struct {
		TrackID            string
		TrackTitle         string
		AlbumID            string
		FilePath           string
		ContentSHA256      string
		ArtworkPath        string
		ArtworkSHA256      string
		TrackRevision      int
		SourceRevision     int
		PlaylistReferences []TrackDeletionPlaylistReference
		QueueReferences    []TrackDeletionQueueReference
	}{
		TrackID: state.Preview.TrackID, TrackTitle: state.Preview.TrackTitle, AlbumID: state.AlbumID,
		FilePath: state.FilePath, ContentSHA256: state.ContentSHA256, TrackRevision: state.TrackRevision,
		ArtworkPath: state.ArtworkPath, ArtworkSHA256: state.ArtworkSHA256,
		SourceRevision: state.SourceRevision, PlaylistReferences: state.Preview.PlaylistReferences,
		QueueReferences: state.Preview.QueueReferences,
	}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return "", fmt.Errorf("encode Permanent Track Deletion preview: %w", err)
	}
	hash := sha256.Sum256(encoded)
	return hex.EncodeToString(hash[:]), nil
}

func tokensEqual(expected, actual string) bool {
	if len(expected) != len(actual) {
		return false
	}
	return subtle.ConstantTimeCompare([]byte(expected), []byte(actual)) == 1
}
