package managedimport

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"strings"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/google/uuid"
)

// replacementState is everything a Track Replacement needs from Import Preview through commit.
type replacementState struct {
	Preview    TrackReplacementPreview
	Target     replacementTarget
	Identity   commitIdentity
	Placement  replacementPlacement
	Inspection library.MediaInspection
	AlbumKey   string
	StagedSize int64
}

func (service *Service) CreateReplacementJob(ctx context.Context, trackID string) (Job, error) {
	if _, err := service.repo.LoadReplacementTarget(ctx, service.storage, trackID); err != nil {
		return Job{}, err
	}
	return service.repo.CreateReplacementJob(ctx, trackID)
}

func (service *Service) buildReplacementState(ctx context.Context, job importJob, inspection library.MediaInspection, stagedPath string) (replacementState, error) {
	target, err := service.repo.LoadReplacementTarget(ctx, service.storage, job.ReplaceTrackID)
	if err != nil {
		return replacementState{}, err
	}
	inspection, err = service.applyImportPlan(ctx, job, inspection)
	if err != nil {
		return replacementState{}, err
	}
	metadata := inspection.Metadata
	albumKey := albumIdentityKey(metadata)
	identity, err := service.repo.ResolveReplacementIdentity(ctx, metadata, albumKey, target.TrackID)
	if err != nil {
		return replacementState{}, err
	}
	if positionErr := service.validateReplacementPositions(ctx, job.ID, metadata, target.TrackID); positionErr != nil {
		return replacementState{}, positionErr
	}
	placement, err := service.storage.planReplacementPlacement(stagedPath, inspection, identity, target)
	if err != nil {
		return replacementState{}, err
	}
	stagedSize, err := service.storage.StagedFileSize(stagedPath)
	if err != nil {
		return replacementState{}, err
	}
	libraryChange, err := service.replacementLibraryChange(ctx, target, metadata, identity, albumKey)
	if err != nil {
		return replacementState{}, err
	}
	state := replacementState{Target: target, Identity: identity, Placement: placement, Inspection: inspection, AlbumKey: albumKey, StagedSize: stagedSize}
	state.Preview = buildReplacementPreview(state, libraryChange)
	state.Preview.ConfirmationToken, err = replacementToken(state)
	return state, err
}

func (service *Service) validateReplacementPositions(ctx context.Context, jobID string, metadata library.NormalizedMediaMetadata, trackID string) error {
	occupants, err := service.repo.findDuplicatesByPosition(ctx, metadata, trackID)
	if err != nil {
		return err
	}
	if len(occupants) > 0 {
		reason := "another Track already occupies this Album position"
		return &ValidationError{Code: ERROR_CODE_ALBUM_POSITION_CONFLICT, Field: "TRACKNUMBER", Reason: reason, Err: errors.New(reason)}
	}
	return service.validateAlbumPositionsExcluding(ctx, jobID, metadata, trackID)
}

func (service *Service) replacementLibraryChange(ctx context.Context, target replacementTarget, metadata library.NormalizedMediaMetadata, identity commitIdentity, albumKey string) (TrackReplacementLibraryChange, error) {
	change := TrackReplacementLibraryChange{
		CurrentAlbumID:     target.AlbumID,
		ReplacementAlbumID: identity.AlbumID,
		MovesAlbum:         identity.AlbumID != target.AlbumID,
	}
	albumExists, err := service.repo.AlbumExists(ctx, albumKey)
	if err != nil {
		return change, err
	}
	change.CreatesAlbum = !albumExists
	if change.CreatesAlbum {
		change.ReplacementAlbumID = ""
	}
	change.RemovesEmptyAlbum = change.MovesAlbum && target.IsSoleTrack
	if change.RemovesEmptyArtists, err = service.repo.OrphanedArtistNames(ctx, target, metadata, change.RemovesEmptyAlbum); err != nil {
		return change, err
	}
	if change.CreatesArtists, err = service.repo.MissingArtistNames(ctx, append(append([]string{}, metadata.AlbumArtists...), metadata.Artists...)); err != nil {
		return change, err
	}
	change.CreatesGenres, err = service.repo.MissingGenreNames(ctx, metadata.Genres)
	return change, err
}

func buildReplacementPreview(state replacementState, libraryChange TrackReplacementLibraryChange) TrackReplacementPreview {
	target := state.Target
	metadata := state.Inspection.Metadata
	audio := state.Inspection.Audio
	artwork := state.Inspection.AlbumArtwork

	return TrackReplacementPreview{
		TrackID:      target.TrackID,
		TrackTitle:   target.Title,
		SourceFormat: fieldDiff("format", target.Format, audio.Format),
		TechnicalProperties: []TrackReplacementFieldDiff{
			fieldDiff("container", target.Container, audio.Container),
			fieldDiff("codec", target.Codec, audio.Codec),
			fieldDiff("durationMs", fmt.Sprint(target.DurationMs), fmt.Sprint(audio.DurationMs)),
			fieldDiff("sampleRateHz", fmt.Sprint(target.SampleRateHz), fmt.Sprint(audio.SampleRateHz)),
			fieldDiff("channelCount", fmt.Sprint(target.ChannelCount), fmt.Sprint(audio.ChannelCount)),
			fieldDiff("bitDepth", optionalNumber(target.BitDepth), optionalNumber(audio.BitDepth)),
			fieldDiff("bitrateKbps", fmt.Sprint(target.BitrateKbps), fmt.Sprint(audio.BitrateKbps)),
			fieldDiff("sizeBytes", fmt.Sprint(target.SizeBytes), fmt.Sprint(state.StagedSize)),
		},
		Metadata: []TrackReplacementFieldDiff{
			fieldDiff("title", target.Title, metadata.Title),
			fieldDiff("artists", strings.Join(target.Artists, ", "), strings.Join(metadata.Artists, ", ")),
			fieldDiff("albumArtists", strings.Join(target.AlbumArtists, ", "), strings.Join(metadata.AlbumArtists, ", ")),
			fieldDiff("album", target.Album, metadata.Album),
			fieldDiff("year", optionalNumber(target.Year), optionalNumber(metadata.Year)),
			fieldDiff("genres", strings.Join(target.Genres, ", "), strings.Join(metadata.Genres, ", ")),
			fieldDiff("discNo", fmt.Sprint(target.DiscNo), fmt.Sprint(metadata.DiscPosition.Number)),
			fieldDiff("trackNo", fmt.Sprint(target.TrackNo), fmt.Sprint(metadata.TrackPosition.Number)),
			fieldDiff("discTotal", optionalNumber(target.DiscTotal), optionalNumber(metadata.DiscPosition.Total)),
			fieldDiff("trackTotal", optionalNumber(target.TrackTotal), optionalNumber(metadata.TrackPosition.Total)),
		},
		Library: libraryChange,
		Artwork: TrackReplacementArtworkChange{
			CurrentMediaType:     target.ArtworkType,
			CurrentSHA256:        target.ArtworkSHA256,
			ReplacementMediaType: artwork.MIMEType,
			ReplacementSHA256:    artwork.SHA256,
			IsChanged:            target.ArtworkSHA256 != artwork.SHA256,
			ReplacesAlbumArtwork: state.Placement.ArtworkMode == REPLACEMENT_ARTWORK_MODE_REPLACE,
		},
		CanonicalPath:      fieldDiff("canonicalPath", filepath.ToSlash(target.RelativePath), filepath.ToSlash(state.Placement.audioRelative)),
		OldFile:            TrackReplacementFileDeletion{Path: filepath.ToSlash(target.RelativePath), SizeBytes: target.SizeBytes},
		PlaylistReferences: target.Playlists,
		QueueReferences:    target.Queues,
	}
}

func fieldDiff(field, current, replacement string) TrackReplacementFieldDiff {
	return TrackReplacementFieldDiff{Field: field, Current: current, Replacement: replacement, IsChanged: current != replacement}
}

func optionalNumber(value int) string {
	if value <= 0 {
		return ""
	}
	return fmt.Sprint(value)
}

func replacementToken(state replacementState) (string, error) {
	target := state.Target
	payload := struct {
		TrackID, Title, AlbumID, FilePath, ContentSHA256, ArtworkPath, ArtworkSHA256 string
		SizeBytes                                                                    int64
		TrackRevision, SourceRevision                                                int
		Playlists                                                                    []TrackDeletionPlaylistReference
		Queues                                                                       []TrackDeletionQueueReference
		ReplacementSHA256, ReplacementAlbumKey, ArtworkMode                          string
		IsSoleTrack                                                                  bool
		Library                                                                      TrackReplacementLibraryChange
		PreviousArtworkPath, RetiredArtworkPath                                      string
	}{
		TrackID: target.TrackID, Title: target.Title, AlbumID: target.AlbumID, FilePath: target.FilePath,
		ContentSHA256: target.ContentSHA256, ArtworkPath: target.ArtworkPath, ArtworkSHA256: target.ArtworkSHA256,
		SizeBytes: target.SizeBytes, TrackRevision: target.TrackRevision, SourceRevision: target.SourceRevision,
		Playlists: target.Playlists, Queues: target.Queues, ReplacementSHA256: state.Inspection.FileSHA256,
		ReplacementAlbumKey: state.AlbumKey, ArtworkMode: state.Placement.ArtworkMode,
		IsSoleTrack: target.IsSoleTrack, Library: state.Preview.Library,
		PreviousArtworkPath: state.Placement.previousArtworkRelative, RetiredArtworkPath: state.Placement.retiredArtworkRelative,
	}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return "", fmt.Errorf("encode Track Replacement preview: %w", err)
	}
	hash := sha256.Sum256(encoded)
	return hex.EncodeToString(hash[:]), nil
}

// ConfirmReplacement commits a reviewed Track Replacement. The previous managed file is deleted only after the
// replacement has been placed, hashed, committed to the database, and read back from its canonical path.
func (service *Service) ConfirmReplacement(ctx context.Context, jobID string, confirmation TrackReplacementConfirmation) (TrackReplacementResult, error) {
	managedImportBatchConfirmationMu.Lock()
	defer managedImportBatchConfirmationMu.Unlock()
	permanentTrackDeletionMu.Lock()
	defer permanentTrackDeletionMu.Unlock()
	managedImportCommitMu.Lock()
	defer managedImportCommitMu.Unlock()
	job, err := service.recoveredReplacementJob(ctx, jobID)
	if err != nil {
		return TrackReplacementResult{}, err
	}
	if job.Status == STATUS_COMMITTED {
		if confirmation.Revision != job.Revision-1 {
			return TrackReplacementResult{}, ErrRevisionConflict
		}
		return service.replayedReplacementResult(ctx, job)
	}
	if job.Status == STATUS_FAILED && job.ErrorCode == ERROR_CODE_EXACT_DUPLICATE {
		if confirmation.Revision != job.Revision {
			return TrackReplacementResult{}, ErrRevisionConflict
		}
		if job.StagedFilePath != "" {
			return TrackReplacementResult{}, service.finishExactDuplicateCleanup(ctx, job)
		}
		return TrackReplacementResult{}, ErrExactDuplicate
	}
	if job.Status != STATUS_AWAITING_CONFIRMATION {
		return TrackReplacementResult{}, ErrInvalidState
	}
	if confirmation.Revision != job.Revision {
		return TrackReplacementResult{}, ErrRevisionConflict
	}
	state, err := service.prepareReplacementConfirmation(ctx, job)
	if err != nil {
		return TrackReplacementResult{}, err
	}
	if !tokensEqual(state.Preview.ConfirmationToken, confirmation.ConfirmationToken) {
		return TrackReplacementResult{}, ErrReplacementConflict
	}
	return service.commitReplacement(ctx, job, state)
}

func (service *Service) recoveredReplacementJob(ctx context.Context, jobID string) (importJob, error) {
	job, err := service.repo.GetJob(ctx, jobID)
	if err != nil {
		return importJob{}, err
	}
	if job.ReplaceTrackID == "" {
		return importJob{}, ErrNotReplacementJob
	}
	journal, hasJournal, err := service.repo.FindIncompleteReplacementJournal(ctx, job.ID)
	if err != nil {
		return importJob{}, err
	}
	if !hasJournal {
		return job, nil
	}
	if err := service.recoverReplacement(ctx, journal); err != nil {
		return importJob{}, err
	}
	return service.repo.GetJob(ctx, jobID)
}

func (service *Service) replayedReplacementResult(ctx context.Context, job importJob) (TrackReplacementResult, error) {
	result := TrackReplacementResult{JobID: job.ID, Status: job.Status, Revision: job.Revision, TrackID: job.TrackID, DeletedFiles: 1}
	journal, err := service.repo.LatestReplacementJournalForJob(ctx, job.ID)
	if err != nil {
		return TrackReplacementResult{}, err
	}
	if journal.PreviousArtworkPath != "" {
		result.DeletedFiles++
	}
	return result, nil
}

func (service *Service) prepareReplacementConfirmation(ctx context.Context, job importJob) (replacementState, error) {
	inspection, err := service.inspectStagedJob(ctx, job)
	if err != nil {
		return replacementState{}, err
	}
	if inspection.FileSHA256 != job.ContentSHA256 {
		reason := "staged file changed after Import Preview"
		return replacementState{}, &ValidationError{Code: "staged_file_changed", Field: "file", Reason: reason, Err: errors.New(reason)}
	}
	exactTrackID, err := service.repo.FindExactDuplicateTrackID(ctx, inspection.FileSHA256)
	if err != nil {
		return replacementState{}, err
	}
	if exactTrackID != "" {
		return replacementState{}, service.rejectExactDuplicate(ctx, job)
	}
	state, err := service.buildReplacementState(ctx, job, inspection, job.StagedFilePath)
	if err != nil {
		return replacementState{}, err
	}
	return state, service.preflightCommit(state.StagedSize, inspection)
}

func (service *Service) commitReplacement(ctx context.Context, job importJob, state replacementState) (TrackReplacementResult, error) {
	journal, err := service.prepareReplacementJournal(ctx, job, state)
	if err != nil {
		return TrackReplacementResult{}, err
	}
	placement, err := service.placeAndVerifyReplacement(ctx, journal, state)
	if err != nil {
		return TrackReplacementResult{}, err
	}
	if err := service.swapAndStreamReplacement(ctx, journal, placement, state); err != nil {
		return TrackReplacementResult{}, err
	}
	data := replacementCommitData{Job: job, Target: state.Target, Identity: state.Identity, Placement: placement, Inspection: state.Inspection, AlbumKey: state.AlbumKey}
	invalidations, commitErr := service.repo.CommitReplacement(ctx, data, journal.ID)
	if commitErr == nil && service.commitResultHook != nil {
		commitErr = service.commitResultHook()
	}
	if commitErr != nil {
		persisted, journalErr := service.repo.GetReplacementJournal(context.WithoutCancel(ctx), journal.ID)
		if journalErr != nil {
			return TrackReplacementResult{}, errors.Join(commitErr, journalErr)
		}
		if persisted.Phase == REPLACEMENT_PHASE_SWAPPED {
			return TrackReplacementResult{}, errors.Join(commitErr, service.rollbackReplacementFiles(ctx, journal, placement, "database commit failed"))
		}
		return TrackReplacementResult{}, commitErr
	}
	if err := service.afterReplacementPhase(REPLACEMENT_PHASE_DATABASE_COMMITTED); err != nil {
		return TrackReplacementResult{}, err
	}
	service.publishQueueInvalidations(invalidations)
	return service.completeReplacement(ctx, job, journal, placement)
}

func (service *Service) prepareReplacementJournal(ctx context.Context, job importJob, state replacementState) (replacementJournal, error) {
	placement := state.Placement
	pendingAudio, previousAudio, retiredAudio, pendingArtwork, previousArtwork, retiredArtwork := service.storage.replacementJournalPaths(placement)
	journal := replacementJournal{
		ID: uuid.NewString(), JobID: job.ID, TrackID: state.Target.TrackID, Phase: REPLACEMENT_PHASE_PREPARED,
		StagedFilePath: job.StagedFilePath, PendingAudioPath: pendingAudio, AudioFilePath: placement.AudioPath,
		PreviousAudioPath: previousAudio, RetiredAudioPath: retiredAudio, AudioSHA256: state.Inspection.FileSHA256,
		PreviousAudioSHA256: state.Target.ContentSHA256, ArtworkMode: placement.ArtworkMode,
		PendingArtworkPath: pendingArtwork, ArtworkFilePath: placement.ArtworkPath, PreviousArtworkPath: previousArtwork,
		RetiredArtworkPath: retiredArtwork, ArtworkSHA256: state.Inspection.AlbumArtwork.SHA256,
		PreviousAlbumID: state.Target.AlbumID,
	}
	if previousArtwork != "" {
		journal.PreviousArtworkSHA256 = state.Target.ArtworkSHA256
	}
	if err := service.repo.CreateReplacementJournal(ctx, journal); err != nil {
		return replacementJournal{}, err
	}
	return journal, service.afterReplacementPhase(REPLACEMENT_PHASE_PREPARED)
}

func (service *Service) placeAndVerifyReplacement(ctx context.Context, journal replacementJournal, state replacementState) (replacementPlacement, error) {
	placement, placeErr := service.storage.PlaceReplacement(state.Placement, state.Inspection, state.Identity, func() error {
		return service.repo.MarkReplacementArtworkCreated(ctx, journal.ID)
	})
	if placeErr != nil {
		journalErr := service.repo.RollbackReplacementJournal(context.WithoutCancel(ctx), journal.ID, "pending placement failed")
		return replacementPlacement{}, errors.Join(placeErr, journalErr)
	}
	if err := service.advanceReplacementPhase(ctx, journal, placement, REPLACEMENT_PHASE_PREPARED, REPLACEMENT_PHASE_PLACED); err != nil {
		return replacementPlacement{}, err
	}
	if err := service.storage.VerifyReplacement(placement, journal.AudioSHA256, journal.ArtworkSHA256); err != nil {
		return replacementPlacement{}, errors.Join(err, service.rollbackReplacementFiles(ctx, journal, placement, "pending verification failed"))
	}
	if err := service.advanceReplacementPhase(ctx, journal, placement, REPLACEMENT_PHASE_PLACED, REPLACEMENT_PHASE_VERIFIED); err != nil {
		return replacementPlacement{}, err
	}
	return placement, nil
}

func (service *Service) swapAndStreamReplacement(ctx context.Context, journal replacementJournal, placement replacementPlacement, state replacementState) error {
	if err := service.storage.SwapReplacement(placement); err != nil {
		return errors.Join(err, service.rollbackReplacementFiles(ctx, journal, placement, "canonical swap failed"))
	}
	if err := service.advanceReplacementPhase(ctx, journal, placement, REPLACEMENT_PHASE_VERIFIED, REPLACEMENT_PHASE_SWAPPED); err != nil {
		return err
	}
	if err := service.storage.StreamSwappedReplacement(placement, state.Inspection.FileSHA256); err != nil {
		return errors.Join(err, service.rollbackReplacementFiles(ctx, journal, placement, "canonical stream verification failed"))
	}
	return nil
}

// advanceReplacementPhase journals a durable phase and then runs the injected phase hook. A hook failure
// simulates a crash right after that phase: the journal is left for recovery instead of being rolled back.
func (service *Service) advanceReplacementPhase(ctx context.Context, journal replacementJournal, placement replacementPlacement, from, to replacementPhase) error {
	if err := service.repo.UpdateReplacementPhase(ctx, journal.ID, from, to); err != nil {
		return errors.Join(err, service.rollbackReplacementFiles(ctx, journal, placement, "journal update failed"))
	}
	return service.afterReplacementPhase(to)
}

func (service *Service) afterReplacementPhase(phase replacementPhase) error {
	if service.replacementPhaseHook == nil {
		return nil
	}
	return service.replacementPhaseHook(phase)
}

func (service *Service) rollbackReplacementFiles(ctx context.Context, journal replacementJournal, placement replacementPlacement, reason string) error {
	recoveryCtx := context.WithoutCancel(ctx)
	if err := service.storage.RollbackReplacement(placement); err != nil {
		reasonErr := service.repo.RecordReplacementRecoveryReason(recoveryCtx, journal.ID, reason+"; filesystem rollback failed")
		return errors.Join(err, reasonErr)
	}
	return service.repo.RollbackReplacementJournal(recoveryCtx, journal.ID, reason)
}

// completeReplacement runs without the request deadline: the database already made the replacement authoritative,
// and hashing a retired file up to the configured upload limit must be allowed to finish before it is deleted.
func (service *Service) completeReplacement(ctx context.Context, job importJob, journal replacementJournal, placement replacementPlacement) (TrackReplacementResult, error) {
	completionCtx := context.WithoutCancel(ctx)
	deletedFiles, removeErr := service.storage.CompleteReplacementFiles(completionCtx, journal, placement)
	if removeErr != nil {
		reasonErr := service.repo.RecordReplacementRecoveryReason(completionCtx, journal.ID, "previous file cleanup failed: "+removeErr.Error())
		return TrackReplacementResult{}, errors.Join(removeErr, reasonErr)
	}
	result, err := service.repo.FinalizeReplacement(completionCtx, job, journal.ID)
	if err != nil {
		return TrackReplacementResult{}, err
	}
	result.DeletedFiles = deletedFiles
	return result, nil
}

// RecoverPendingTrackReplacements finishes or undoes every Track Replacement interrupted by a restart.
func (service *Service) RecoverPendingTrackReplacements(ctx context.Context) error {
	managedImportCommitMu.Lock()
	defer managedImportCommitMu.Unlock()
	journals, err := service.repo.ListIncompleteReplacementJournals(ctx)
	if err != nil {
		return err
	}
	var recoveryErr error
	for _, journal := range journals {
		if err := service.recoverReplacement(ctx, journal); err != nil {
			recoveryErr = errors.Join(recoveryErr, fmt.Errorf("recover Track Replacement %q: %w", journal.ID, err))
		}
	}
	return recoveryErr
}

func (service *Service) recoverReplacement(ctx context.Context, journal replacementJournal) error {
	placement, err := service.storage.replacementPlacementFromJournal(journal)
	if err != nil {
		return errors.Join(err, service.repo.RecordReplacementRecoveryReason(ctx, journal.ID, "unsafe journaled storage path"))
	}
	switch journal.Phase {
	case REPLACEMENT_PHASE_PREPARED, REPLACEMENT_PHASE_PLACED, REPLACEMENT_PHASE_VERIFIED, REPLACEMENT_PHASE_SWAPPED:
		return service.rollbackReplacementFiles(ctx, journal, placement, fmt.Sprintf("restart rolled back replacement from %s phase", journal.Phase))
	case REPLACEMENT_PHASE_DATABASE_COMMITTED:
		job, err := service.repo.GetJob(ctx, journal.JobID)
		if err != nil {
			return err
		}
		_, err = service.completeReplacement(ctx, job, journal, placement)
		return err
	default:
		return fmt.Errorf("unsupported Track Replacement phase %q", journal.Phase)
	}
}
