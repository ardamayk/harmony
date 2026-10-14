package managedimport

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"path/filepath"

	"github.com/ardam/navidrome-replacement/server/internal/modules/managedimport/db"
)

// pendingDeletion is a journaled Permanent Track Deletion awaiting file removal.
type pendingDeletion struct {
	trackID, filePath, contentSHA256, artworkPath, artworkSHA256 string
}

func (r *Repository) LoadTrackDeletion(ctx context.Context, storage *Storage, trackID string) (trackDeletionState, error) {
	return loadTrackDeletionState(ctx, r.q, storage, trackID)
}

func (r *Repository) LoadAlbumDeletion(ctx context.Context, storage *Storage, albumID string) (albumDeletionState, error) {
	return loadAlbumDeletionState(ctx, r.q, storage, albumID)
}

// PrepareTrackDeletion re-reads the Track, checks the confirmation token, then
// journals the deletion and hides the Track in one transaction.
func (r *Repository) PrepareTrackDeletion(ctx context.Context, storage *Storage, confirmation TrackDeletionRequest) (trackDeletionState, error) {
	var state trackDeletionState
	err := r.inTx(ctx, "Permanent Track Deletion preparation", func(q *db.Queries) error {
		var err error
		state, err = loadTrackDeletionState(ctx, q, storage, confirmation.TrackID)
		if err != nil {
			return err
		}
		if !tokensEqual(state.Preview.ConfirmationToken, confirmation.ConfirmationToken) {
			return ErrDeletionConflict
		}
		if err := q.InsertPermanentDeletion(ctx, db.InsertPermanentDeletionParams{
			TrackID: confirmation.TrackID, FilePath: state.FilePath, ContentSha256: state.ContentSHA256,
			ArtworkFilePath: state.ArtworkPath, ArtworkContentSha256: state.ArtworkSHA256,
		}); err != nil {
			return fmt.Errorf("journal Permanent Track Deletion: %w", err)
		}
		if err := q.HideTrack(ctx, confirmation.TrackID); err != nil {
			return fmt.Errorf("hide pending Permanent Track Deletion: %w", err)
		}
		return nil
	})
	return state, err
}

func (r *Repository) CancelTrackDeletion(ctx context.Context, trackID string) error {
	return r.inTx(ctx, "Permanent Track Deletion cancellation", func(q *db.Queries) error {
		if err := q.UnhideTrack(ctx, trackID); err != nil {
			return fmt.Errorf("restore Managed Track after failed deletion: %w", err)
		}
		if err := q.DeletePermanentDeletion(ctx, trackID); err != nil {
			return fmt.Errorf("clear failed Permanent Track Deletion journal: %w", err)
		}
		return nil
	})
}

// FinalizeTrackDeletion removes the Track and every reference once its files are gone.
func (r *Repository) FinalizeTrackDeletion(ctx context.Context, trackID string) ([]queueInvalidation, error) {
	var invalidations []queueInvalidation
	err := r.inTx(ctx, "Permanent Track Deletion finalization", func(q *db.Queries) error {
		row, err := q.GetPendingDeletion(ctx, trackID)
		if errors.Is(err, sql.ErrNoRows) {
			return ErrTrackNotFound
		}
		if err != nil {
			return fmt.Errorf("load pending Permanent Track Deletion: %w", err)
		}
		state := trackDeletionState{
			AlbumID: row.AlbumID, FilePath: row.FilePath, ContentSHA256: row.ContentSha256,
			ArtworkPath: row.ArtworkFilePath, ArtworkSHA256: row.ArtworkContentSha256,
		}
		state.Preview.TrackID = trackID
		if state.Preview.QueueReferences, err = listDeletionQueues(ctx, q, trackID); err != nil {
			return err
		}
		invalidations, err = deleteTrackRelationships(ctx, q, state)
		return err
	})
	return invalidations, err
}

func (r *Repository) ListPendingDeletions(ctx context.Context) ([]pendingDeletion, error) {
	rows, err := r.q.ListPermanentDeletions(ctx)
	if err != nil {
		return nil, fmt.Errorf("list pending Permanent Track Deletions: %w", err)
	}
	pending := make([]pendingDeletion, 0, len(rows))
	for _, row := range rows {
		pending = append(pending, pendingDeletion{
			trackID: row.TrackID, filePath: row.FilePath, contentSHA256: row.ContentSha256,
			artworkPath: row.ArtworkFilePath, artworkSHA256: row.ArtworkContentSha256,
		})
	}
	return pending, nil
}

func loadTrackDeletionState(ctx context.Context, q *db.Queries, storage *Storage, trackID string) (trackDeletionState, error) {
	row, err := q.GetDeletionTrack(ctx, trackID)
	if errors.Is(err, sql.ErrNoRows) {
		return trackDeletionState{}, ErrTrackNotFound
	}
	if err != nil {
		return trackDeletionState{}, fmt.Errorf("load Managed Track deletion: %w", err)
	}
	if row.SourceKind != "managed" {
		return trackDeletionState{}, ErrNotManagedTrack
	}
	state := trackDeletionState{
		AlbumID: row.AlbumID, FilePath: row.FilePath, ContentSHA256: deref(row.ContentSha256),
		TrackRevision: int(row.Revision), SourceRevision: int(row.SourceRevision),
	}
	state.Preview.TrackTitle = row.Title
	relativePath, actualSize, err := storage.ResolveManagedFile(state.FilePath, state.ContentSHA256)
	if err != nil {
		return trackDeletionState{}, err
	}
	if row.SizeBytes != actualSize {
		return trackDeletionState{}, fmt.Errorf("%w: managed file size changed", ErrDeletionConflict)
	}
	state.Preview.TrackID = trackID
	state.Preview.ManagedFile = TrackDeletionManagedFile{Path: filepath.ToSlash(relativePath), SizeBytes: actualSize}
	if state.Preview.PlaylistReferences, err = listDeletionPlaylists(ctx, q, trackID); err != nil {
		return trackDeletionState{}, err
	}
	if state.Preview.QueueReferences, err = listDeletionQueues(ctx, q, trackID); err != nil {
		return trackDeletionState{}, err
	}
	if state.ArtworkPath, state.ArtworkSHA256, err = loadFinalAlbumArtwork(ctx, q, storage, trackID, state.AlbumID); err != nil {
		return trackDeletionState{}, err
	}
	state.Preview.ConfirmationToken, err = deletionToken(state)
	return state, err
}

// loadFinalAlbumArtwork returns the Album Artwork that is removed with the Album's last Track.
func loadFinalAlbumArtwork(ctx context.Context, q *db.Queries, storage *Storage, trackID, albumID string) (string, string, error) {
	row, err := q.GetFinalAlbumArtwork(ctx, db.GetFinalAlbumArtworkParams{AlbumID: albumID, TrackID: trackID})
	if errors.Is(err, sql.ErrNoRows) {
		return "", "", nil
	}
	if err != nil {
		return "", "", fmt.Errorf("load final-Album Artwork: %w", err)
	}
	if _, _, err := storage.ResolveManagedFile(row.FilePath, row.ContentSha256); err != nil {
		return "", "", fmt.Errorf("validate final-Album Artwork: %w", err)
	}
	return row.FilePath, row.ContentSha256, nil
}

func listDeletionPlaylists(ctx context.Context, q *db.Queries, trackID string) ([]TrackDeletionPlaylistReference, error) {
	rows, err := q.ListTrackPlaylists(ctx, trackID)
	if err != nil {
		return nil, fmt.Errorf("list affected Playlists: %w", err)
	}
	references := make([]TrackDeletionPlaylistReference, 0, len(rows))
	for _, row := range rows {
		references = append(references, TrackDeletionPlaylistReference{ID: row.ID, Name: row.Name})
	}
	return references, nil
}

func listDeletionQueues(ctx context.Context, q *db.Queries, trackID string) ([]TrackDeletionQueueReference, error) {
	rows, err := q.ListTrackQueues(ctx, trackID)
	if err != nil {
		return nil, fmt.Errorf("list affected Queues: %w", err)
	}
	references := make([]TrackDeletionQueueReference, 0, len(rows))
	for _, row := range rows {
		references = append(references, TrackDeletionQueueReference{UserID: row.UserID, ItemCount: int(row.ItemCount)})
	}
	return references, nil
}

func deleteTrackRelationships(ctx context.Context, q *db.Queries, state trackDeletionState) ([]queueInvalidation, error) {
	trackID := state.Preview.TrackID
	artistIDs, err := q.ListTrackAndAlbumArtistIDs(ctx, db.ListTrackAndAlbumArtistIDsParams{TrackID: trackID, AlbumID: state.AlbumID})
	if err != nil {
		return nil, fmt.Errorf("list affected Artists: %w", err)
	}
	invalidations, err := advanceQueues(ctx, q, state.Preview.QueueReferences)
	if err != nil {
		return nil, fmt.Errorf("advance affected Queue revision: %w", err)
	}
	if err := q.DeleteTrackQueueItems(ctx, trackID); err != nil {
		return nil, fmt.Errorf("delete Queue references: %w", err)
	}
	if err := q.DeleteTrackPlaylistItems(ctx, trackID); err != nil {
		return nil, fmt.Errorf("delete Playlist references: %w", err)
	}
	if err := q.DeleteTrackJobs(ctx, &trackID); err != nil {
		return nil, fmt.Errorf("archive terminal Managed Import Jobs before Track deletion: %w", err)
	}
	if err := moveOrDeleteArtworkReference(ctx, q, trackID, state.AlbumID); err != nil {
		return nil, err
	}
	if err := q.DeleteTrack(ctx, trackID); err != nil {
		return nil, fmt.Errorf("delete Managed Track: %w", err)
	}
	if err := q.DeleteAlbumIfEmpty(ctx, state.AlbumID); err != nil {
		return nil, fmt.Errorf("delete empty Album: %w", err)
	}
	if err := deleteUnreferencedArtists(ctx, q, artistIDs); err != nil {
		return nil, fmt.Errorf("delete inactive Artist: %w", err)
	}
	return invalidations, nil
}

// advanceQueues bumps each affected Queue revision so Playback Clients refetch.
func advanceQueues(ctx context.Context, q *db.Queries, queues []TrackDeletionQueueReference) ([]queueInvalidation, error) {
	invalidations := make([]queueInvalidation, 0, len(queues))
	for _, queue := range queues {
		advanced, err := q.AdvanceQueueForUser(ctx, queue.UserID)
		if err != nil {
			return nil, err
		}
		invalidations = append(invalidations, queueInvalidation{userID: queue.UserID, revision: advanced.Revision, sequence: advanced.EventSequence})
	}
	return invalidations, nil
}

// moveOrDeleteArtworkReference hands Album Artwork to another Track, or drops it with the Album's last Track.
func moveOrDeleteArtworkReference(ctx context.Context, q *db.Queries, trackID, albumID string) error {
	replacementTrackID, err := q.FirstOtherAlbumTrack(ctx, db.FirstOtherAlbumTrackParams{AlbumID: albumID, ID: trackID})
	switch {
	case errors.Is(err, sql.ErrNoRows):
		err = q.DeleteAlbumArtwork(ctx, albumID)
	case err == nil:
		err = q.MoveAlbumArtworkSource(ctx, db.MoveAlbumArtworkSourceParams{NewTrackID: replacementTrackID, AlbumID: albumID, OldTrackID: trackID})
	}
	if err != nil {
		return fmt.Errorf("update Album Artwork source after Track deletion: %w", err)
	}
	return nil
}

func deleteUnreferencedArtists(ctx context.Context, q *db.Queries, artistIDs []string) error {
	for _, artistID := range artistIDs {
		if err := q.DeleteArtistIfUnreferenced(ctx, artistID); err != nil {
			return err
		}
	}
	return nil
}

func loadAlbumDeletionState(ctx context.Context, q *db.Queries, storage *Storage, albumID string) (albumDeletionState, error) {
	album, err := q.GetAlbumForDeletion(ctx, albumID)
	if errors.Is(err, sql.ErrNoRows) {
		return albumDeletionState{}, ErrAlbumNotFound
	}
	if err != nil {
		return albumDeletionState{}, fmt.Errorf("load Album deletion: %w", err)
	}
	trackIDs, err := q.ListAlbumDeletionTrackIDs(ctx, albumID)
	if err != nil {
		return albumDeletionState{}, fmt.Errorf("list Album Tracks for deletion: %w", err)
	}
	if len(trackIDs) == 0 {
		return albumDeletionState{}, ErrAlbumNotFound
	}
	var state albumDeletionState
	state.Preview.AlbumID = albumID
	state.Preview.AlbumTitle = album.Title
	state.Preview.Tracks = make([]AlbumDeletionTrack, 0, len(trackIDs))
	state.Preview.PlaylistReferences = []TrackDeletionPlaylistReference{}
	state.Preview.QueueReferences = []TrackDeletionQueueReference{}
	playlistSeen := map[string]bool{}
	queueItems := map[string]int{}
	queueOrder := []string{}
	for _, trackID := range trackIDs {
		trackState, loadErr := loadTrackDeletionState(ctx, q, storage, trackID)
		if loadErr != nil {
			return albumDeletionState{}, loadErr
		}
		state.Tracks = append(state.Tracks, trackState)
		state.Preview.Tracks = append(state.Preview.Tracks, AlbumDeletionTrack{
			TrackID: trackID, TrackTitle: trackState.Preview.TrackTitle, SizeBytes: trackState.Preview.ManagedFile.SizeBytes,
		})
		state.Preview.TotalSizeBytes += trackState.Preview.ManagedFile.SizeBytes
		for _, playlist := range trackState.Preview.PlaylistReferences {
			if !playlistSeen[playlist.ID] {
				playlistSeen[playlist.ID] = true
				state.Preview.PlaylistReferences = append(state.Preview.PlaylistReferences, playlist)
			}
		}
		for _, queue := range trackState.Preview.QueueReferences {
			if _, known := queueItems[queue.UserID]; !known {
				queueOrder = append(queueOrder, queue.UserID)
			}
			queueItems[queue.UserID] += queue.ItemCount
		}
	}
	for _, userID := range queueOrder {
		state.Preview.QueueReferences = append(state.Preview.QueueReferences, TrackDeletionQueueReference{UserID: userID, ItemCount: queueItems[userID]})
	}
	state.Preview.TrackCount = len(state.Preview.Tracks)
	state.Preview.ConfirmationToken, err = albumDeletionToken(albumID, int(album.Revision), state.Tracks)
	return state, err
}
