package managedimport

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
)

// AlbumDeletionPreview is the Album-level review of one Permanent Track
// Deletion per Track. Deleting an Album is not its own operation: the preview
// aggregates the per-Track previews so the user confirms once, and the
// confirmation token binds that exact set of Tracks and references.
type AlbumDeletionPreview struct {
	AlbumID            string                           `json:"albumId"`
	AlbumTitle         string                           `json:"albumTitle"`
	TrackCount         int                              `json:"trackCount"`
	TotalSizeBytes     int64                            `json:"totalSizeBytes"`
	Tracks             []AlbumDeletionTrack             `json:"tracks"`
	PlaylistReferences []TrackDeletionPlaylistReference `json:"playlistReferences"`
	QueueReferences    []TrackDeletionQueueReference    `json:"queueReferences"`
	ConfirmationToken  string                           `json:"confirmationToken"`
}

type AlbumDeletionTrack struct {
	TrackID    string `json:"trackId"`
	TrackTitle string `json:"trackTitle"`
	SizeBytes  int64  `json:"sizeBytes"`
}

// AlbumDeletionResult reports each Track separately because every Track
// commits on its own: a failure stops the run and leaves the remaining Tracks
// untouched rather than undoing the ones already deleted. StoppedAt is nil
// when every Track was deleted.
type AlbumDeletionResult struct {
	Deleted      []AlbumDeletionTrack `json:"deleted"`
	StoppedAt    *AlbumDeletionFailed `json:"stoppedAt"`
	DeletedFiles int                  `json:"deletedFiles"`
}

type AlbumDeletionFailed struct {
	TrackID    string `json:"trackId"`
	TrackTitle string `json:"trackTitle"`
	Reason     string `json:"reason"`
}

type AlbumDeletionRequest struct {
	AlbumID           string
	ConfirmationToken string
}

type albumDeletionState struct {
	Preview AlbumDeletionPreview
	Tracks  []trackDeletionState
}

func (service *Service) PreviewAlbumDeletion(ctx context.Context, albumID string) (AlbumDeletionPreview, error) {
	state, err := service.repo.LoadAlbumDeletion(ctx, service.storage, albumID)
	if err != nil {
		return AlbumDeletionPreview{}, err
	}
	return state.Preview, nil
}

// DeleteAlbum re-derives the preview, refuses a stale token, then runs one
// Permanent Track Deletion per Track in album order. The album token is
// checked once, before the loop; no lock spans the run. Each Track's
// deletion is protected by DeleteTrack itself, which re-derives and compares
// the Track's state inside its own transaction. The per-Track token minted
// here only feeds that check; it is re-derived right before each deletion
// because deleting a sibling changes what the next one means (for example
// which Track carries the final album artwork).
func (service *Service) DeleteAlbum(ctx context.Context, request AlbumDeletionRequest) (AlbumDeletionResult, error) {
	state, err := service.repo.LoadAlbumDeletion(ctx, service.storage, request.AlbumID)
	if err != nil {
		return AlbumDeletionResult{}, err
	}
	if !tokensEqual(state.Preview.ConfirmationToken, request.ConfirmationToken) {
		return AlbumDeletionResult{}, fmt.Errorf("%w: album deletion preview changed", ErrDeletionConflict)
	}
	result := AlbumDeletionResult{Deleted: []AlbumDeletionTrack{}}
	for _, track := range state.Preview.Tracks {
		var deleteErr error
		var deleted TrackDeletionResult
		if service.albumDeletionTrackHook != nil {
			deleteErr = service.albumDeletionTrackHook(track.TrackID)
		}
		var trackState trackDeletionState
		if deleteErr == nil {
			trackState, deleteErr = service.repo.LoadTrackDeletion(ctx, service.storage, track.TrackID)
		}
		if deleteErr == nil {
			deleted, deleteErr = service.DeleteTrack(ctx, TrackDeletionRequest{TrackID: track.TrackID, ConfirmationToken: trackState.Preview.ConfirmationToken})
		}
		if deleteErr != nil {
			result.StoppedAt = &AlbumDeletionFailed{TrackID: track.TrackID, TrackTitle: track.TrackTitle, Reason: deleteErr.Error()}
			break
		}
		result.Deleted = append(result.Deleted, track)
		result.DeletedFiles += deleted.DeletedFiles
	}
	return result, nil
}

// albumDeletionToken binds the confirmation to the album row and to every
// per-Track token, so any change to a Track, its file, or its references
// invalidates the whole album confirmation.
func albumDeletionToken(albumID string, albumRevision int, tracks []trackDeletionState) (string, error) {
	trackTokens := make([]string, len(tracks))
	for index, track := range tracks {
		trackTokens[index] = track.Preview.ConfirmationToken
	}
	payload := struct {
		AlbumID       string
		AlbumRevision int
		TrackTokens   []string
	}{AlbumID: albumID, AlbumRevision: albumRevision, TrackTokens: trackTokens}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return "", fmt.Errorf("encode Album deletion preview: %w", err)
	}
	hash := sha256.Sum256(encoded)
	return hex.EncodeToString(hash[:]), nil
}
