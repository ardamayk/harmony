package managedimport

import (
	"context"
	"database/sql"
	"errors"
	"fmt"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/ardam/navidrome-replacement/server/internal/modules/managedimport/db"
)

func (r *Repository) LoadReplacementTarget(ctx context.Context, storage *Storage, trackID string) (replacementTarget, error) {
	target, err := readReplacementTargetRow(ctx, r.q, trackID)
	if err != nil {
		return replacementTarget{}, err
	}
	relativePath, actualSize, err := storage.ResolveManagedFile(target.FilePath, target.ContentSHA256)
	if err != nil {
		return replacementTarget{}, err
	}
	if target.SizeBytes != actualSize {
		return replacementTarget{}, fmt.Errorf("%w: managed file size changed", ErrReplacementConflict)
	}
	target.RelativePath = relativePath
	if target.Artists, err = r.q.ListTrackArtistNames(ctx, trackID); err != nil {
		return replacementTarget{}, fmt.Errorf("list Track Replacement names: %w", err)
	}
	if target.AlbumArtists, err = r.q.ListAlbumArtistNames(ctx, target.AlbumID); err != nil {
		return replacementTarget{}, fmt.Errorf("list Track Replacement names: %w", err)
	}
	if target.Genres, err = r.q.ListTrackGenreNames(ctx, trackID); err != nil {
		return replacementTarget{}, fmt.Errorf("list Track Replacement names: %w", err)
	}
	if target.Playlists, err = listDeletionPlaylists(ctx, r.q, trackID); err != nil {
		return replacementTarget{}, err
	}
	if target.Queues, err = listDeletionQueues(ctx, r.q, trackID); err != nil {
		return replacementTarget{}, err
	}
	return target, nil
}

func readReplacementTargetRow(ctx context.Context, q *db.Queries, trackID string) (replacementTarget, error) {
	row, err := q.GetReplacementTarget(ctx, trackID)
	if errors.Is(err, sql.ErrNoRows) {
		return replacementTarget{}, ErrTrackNotFound
	}
	if err != nil {
		return replacementTarget{}, fmt.Errorf("load Track Replacement target: %w", err)
	}
	if row.SourceKind != "managed" {
		return replacementTarget{}, ErrNotManagedTrack
	}
	target := replacementTarget{
		TrackID:        trackID,
		Title:          row.Title,
		AlbumID:        row.AlbumID,
		TrackRevision:  int(row.Revision),
		TrackNo:        int(deref(row.TrackNo)),
		TrackTotal:     int(deref(row.TrackTotal)),
		DiscNo:         int(deref(row.DiscNo)),
		DiscTotal:      int(deref(row.DiscTotal)),
		DurationMs:     int(row.DurationMs),
		SampleRateHz:   int(deref(row.SampleRateHz)),
		BitDepth:       int(deref(row.BitDepth)),
		ChannelCount:   int(deref(row.ChannelCount)),
		BitrateKbps:    int(deref(row.BitrateBps)) / BITS_PER_KILOBIT,
		Codec:          deref(row.Codec),
		Container:      deref(row.Container),
		Album:          row.AlbumTitle,
		Year:           int(deref(row.Year)),
		AlbumKey:       deref(row.AlbumKey),
		FilePath:       row.FilePath,
		ContentSHA256:  deref(row.ContentSha256),
		SizeBytes:      row.SizeBytes,
		Format:         row.SourceFormat,
		SourceRevision: int(row.SourceRevision),
		ArtworkPath:    deref(row.ArtworkPath),
		ArtworkSHA256:  deref(row.ArtworkSha256),
		ArtworkType:    deref(row.ArtworkType),
		IsSoleTrack:    row.IsSoleTrack,
	}
	if target.DiscNo == 0 {
		target.DiscNo = 1
	}
	return target, nil
}

func (r *Repository) AlbumExists(ctx context.Context, albumKey string) (bool, error) {
	exists, err := r.q.AlbumKeyExists(ctx, &albumKey)
	if err != nil {
		return false, fmt.Errorf("inspect replacement Album: %w", err)
	}
	return exists, nil
}

func (r *Repository) MissingArtistNames(ctx context.Context, names []string) ([]string, error) {
	return missingNames(names, func(normalized string) (bool, error) {
		return r.q.ArtistNameExists(ctx, &normalized)
	})
}

func (r *Repository) MissingGenreNames(ctx context.Context, names []string) ([]string, error) {
	return missingNames(names, func(normalized string) (bool, error) {
		return r.q.GenreNameExists(ctx, normalized)
	})
}

func missingNames(names []string, exists func(normalized string) (bool, error)) ([]string, error) {
	missing := []string{}
	seen := map[string]bool{}
	for _, name := range names {
		normalized := normalizeIdentity(name)
		if seen[normalized] {
			continue
		}
		seen[normalized] = true
		found, err := exists(normalized)
		if err != nil {
			return nil, fmt.Errorf("inspect replacement name %q: %w", name, err)
		}
		if !found {
			missing = append(missing, name)
		}
	}
	return missing, nil
}

// OrphanedArtistNames lists Artists that lose every reference once the target Track adopts the replacement credits.
func (r *Repository) OrphanedArtistNames(ctx context.Context, target replacementTarget, metadata library.NormalizedMediaMetadata, removesAlbum bool) ([]string, error) {
	retained := map[string]bool{}
	for _, name := range append(append([]string{}, metadata.Artists...), metadata.AlbumArtists...) {
		retained[normalizeIdentity(name)] = true
	}
	rows, err := r.q.ListReplacementArtistReferences(ctx, db.ListReplacementArtistReferencesParams{
		TrackID: target.TrackID, AlbumID: target.AlbumID, IncludesAlbum: boolInt(removesAlbum),
	})
	if err != nil {
		return nil, fmt.Errorf("list replacement Artist references: %w", err)
	}
	names := []string{}
	for _, row := range rows {
		if retained[deref(row.NameNormalized)] {
			continue
		}
		isReferenced, err := r.q.IsArtistReferencedElsewhere(ctx, db.IsArtistReferencedElsewhereParams{
			ArtistID: row.ID, TrackID: target.TrackID, ExcludesAlbum: boolInt(removesAlbum), AlbumID: target.AlbumID,
		})
		if err != nil {
			return nil, fmt.Errorf("inspect replacement Artist reference: %w", err)
		}
		if !deref(isReferenced) {
			names = append(names, row.Name)
		}
	}
	return names, nil
}

func (r *Repository) CreateReplacementJournal(ctx context.Context, journal replacementJournal) error {
	if err := r.q.InsertReplacementJournal(ctx, db.InsertReplacementJournalParams{
		ID: journal.ID, JobID: journal.JobID, TrackID: journal.TrackID, Phase: string(journal.Phase),
		StagedFilePath: journal.StagedFilePath, PendingAudioPath: journal.PendingAudioPath,
		AudioFilePath: journal.AudioFilePath, PreviousAudioPath: journal.PreviousAudioPath,
		RetiredAudioPath: journal.RetiredAudioPath, AudioSha256: journal.AudioSHA256,
		PreviousAudioSha256: journal.PreviousAudioSHA256, ArtworkMode: journal.ArtworkMode,
		PendingArtworkPath: journal.PendingArtworkPath, ArtworkFilePath: journal.ArtworkFilePath,
		PreviousArtworkPath: journal.PreviousArtworkPath, RetiredArtworkPath: journal.RetiredArtworkPath,
		ArtworkSha256: journal.ArtworkSHA256, PreviousArtworkSha256: journal.PreviousArtworkSHA256,
		ArtworkCreated: boolInt(journal.IsArtworkCreated), PreviousAlbumID: journal.PreviousAlbumID,
	}); err != nil {
		return fmt.Errorf("create Track Replacement journal: %w", err)
	}
	return nil
}

func (r *Repository) UpdateReplacementPhase(ctx context.Context, journalID string, from, to replacementPhase) error {
	advanced, err := r.q.AdvanceReplacementJournal(ctx, db.AdvanceReplacementJournalParams{
		ToPhase: string(to), ID: journalID, FromPhase: string(from),
	})
	if err != nil {
		return fmt.Errorf("journal Track Replacement phase %q: %w", to, err)
	}
	return requireOne(advanced, nil)
}

func (r *Repository) MarkReplacementArtworkCreated(ctx context.Context, journalID string) error {
	marked, err := r.q.MarkReplacementArtworkCreated(ctx, journalID)
	if err != nil {
		return fmt.Errorf("journal Track Replacement artwork creation: %w", err)
	}
	return requireOne(marked, nil)
}

func (r *Repository) RollbackReplacementJournal(ctx context.Context, journalID, reason string) error {
	rolledBack, err := r.q.RollbackReplacementJournal(ctx, db.RollbackReplacementJournalParams{RecoveryReason: &reason, ID: journalID})
	if err != nil {
		return fmt.Errorf("roll back Track Replacement journal: %w", err)
	}
	return requireOne(rolledBack, nil)
}

func (r *Repository) RecordReplacementRecoveryReason(ctx context.Context, journalID, reason string) error {
	recorded, err := r.q.RecordReplacementRecoveryReason(ctx, db.RecordReplacementRecoveryReasonParams{RecoveryReason: &reason, ID: journalID})
	if err != nil {
		return fmt.Errorf("record Track Replacement recovery reason: %w", err)
	}
	return requireOne(recorded, nil)
}

func (r *Repository) ListIncompleteReplacementJournals(ctx context.Context) ([]replacementJournal, error) {
	rows, err := r.q.ListIncompleteReplacementJournals(ctx)
	if err != nil {
		return nil, fmt.Errorf("list incomplete Track Replacements: %w", err)
	}
	var journals []replacementJournal
	for _, row := range rows {
		journals = append(journals, toReplacementJournal(row))
	}
	return journals, nil
}

func (r *Repository) FindIncompleteReplacementJournal(ctx context.Context, jobID string) (replacementJournal, bool, error) {
	row, err := r.q.FindIncompleteReplacementJournal(ctx, jobID)
	if errors.Is(err, sql.ErrNoRows) {
		return replacementJournal{}, false, nil
	}
	if err != nil {
		return replacementJournal{}, false, fmt.Errorf("find Track Replacement journal: %w", err)
	}
	return toReplacementJournal(row), true, nil
}

func (r *Repository) GetReplacementJournal(ctx context.Context, journalID string) (replacementJournal, error) {
	row, err := r.q.GetReplacementJournal(ctx, journalID)
	if errors.Is(err, sql.ErrNoRows) {
		return replacementJournal{}, ErrNotFound
	}
	if err != nil {
		return replacementJournal{}, fmt.Errorf("read Track Replacement journal: %w", err)
	}
	return toReplacementJournal(row), nil
}

func (r *Repository) LatestReplacementJournalForJob(ctx context.Context, jobID string) (replacementJournal, error) {
	row, err := r.q.LatestReplacementJournalForJob(ctx, jobID)
	if errors.Is(err, sql.ErrNoRows) {
		return replacementJournal{}, ErrNotFound
	}
	if err != nil {
		return replacementJournal{}, fmt.Errorf("read latest Track Replacement journal: %w", err)
	}
	return toReplacementJournal(row), nil
}

func toReplacementJournal(row db.ManagedTrackReplacement) replacementJournal {
	return replacementJournal{
		ID: row.ID, JobID: row.JobID, TrackID: row.TrackID, Phase: replacementPhase(row.Phase),
		StagedFilePath: row.StagedFilePath, PendingAudioPath: row.PendingAudioPath, AudioFilePath: row.AudioFilePath,
		PreviousAudioPath: row.PreviousAudioPath, RetiredAudioPath: row.RetiredAudioPath, AudioSHA256: row.AudioSha256,
		PreviousAudioSHA256: row.PreviousAudioSha256, ArtworkMode: row.ArtworkMode,
		PendingArtworkPath: row.PendingArtworkPath, ArtworkFilePath: row.ArtworkFilePath,
		PreviousArtworkPath: row.PreviousArtworkPath, RetiredArtworkPath: row.RetiredArtworkPath,
		ArtworkSHA256: row.ArtworkSha256, PreviousArtworkSHA256: row.PreviousArtworkSha256,
		IsArtworkCreated: row.ArtworkCreated != 0, PreviousAlbumID: row.PreviousAlbumID,
		RecoveryReason: deref(row.RecoveryReason),
	}
}

// CommitReplacement makes the validated replacement metadata authoritative for the existing Track ID in one
// transaction and journals the database commit so a crash afterwards only has file cleanup left to finish.
func (r *Repository) CommitReplacement(ctx context.Context, data replacementCommitData, journalID string) ([]queueInvalidation, error) {
	var invalidations []queueInvalidation
	err := r.inTx(ctx, "Track Replacement commit", func(q *db.Queries) error {
		var err error
		if invalidations, err = writeReplacementData(ctx, q, data); err != nil {
			return err
		}
		advanced, err := q.AdvanceReplacementJournal(ctx, db.AdvanceReplacementJournalParams{
			ToPhase: string(REPLACEMENT_PHASE_DATABASE_COMMITTED), ID: journalID, FromPhase: string(REPLACEMENT_PHASE_SWAPPED),
		})
		if err != nil {
			return fmt.Errorf("journal Track Replacement database commit: %w", err)
		}
		return requireOne(advanced, nil)
	})
	return invalidations, err
}

func writeReplacementData(ctx context.Context, q *db.Queries, data replacementCommitData) ([]queueInvalidation, error) {
	shell := commitData{Job: data.Job, Identity: data.Identity, Inspection: data.Inspection, AlbumKey: data.AlbumKey}
	shell.Placement = placedFiles{AudioPath: data.Placement.AudioPath, ArtworkPath: data.Placement.ArtworkPath}
	artistIDs, err := upsertArtists(ctx, q, data.Inspection.Metadata, data.Identity)
	if err != nil {
		return nil, err
	}
	if err = upsertAlbum(ctx, q, shell, artistIDs); err != nil {
		return nil, err
	}
	previousArtistIDs, err := q.ListTrackAndAlbumArtistIDs(ctx, db.ListTrackAndAlbumArtistIDsParams{
		TrackID: data.Target.TrackID, AlbumID: data.Target.AlbumID,
	})
	if err != nil {
		return nil, fmt.Errorf("list affected Artists: %w", err)
	}
	isMovingAlbum := data.Identity.AlbumID != data.Target.AlbumID
	if isMovingAlbum {
		if err = moveOrDeleteArtworkReference(ctx, q, data.Target.TrackID, data.Target.AlbumID); err != nil {
			return nil, err
		}
	}
	if err = updateReplacedTrack(ctx, q, data, shell); err != nil {
		return nil, err
	}
	if err = q.DeleteTrackArtists(ctx, data.Target.TrackID); err != nil {
		return nil, fmt.Errorf("clear replaced Track relationships: %w", err)
	}
	if err = q.DeleteTrackGenres(ctx, data.Target.TrackID); err != nil {
		return nil, fmt.Errorf("clear replaced Track relationships: %w", err)
	}
	if err = insertRelationships(ctx, q, shell, artistIDs); err != nil {
		return nil, err
	}
	if err = writeReplacementArtwork(ctx, q, data, shell); err != nil {
		return nil, err
	}
	if isMovingAlbum {
		if err = q.DeleteAlbumIfEmpty(ctx, data.Target.AlbumID); err != nil {
			return nil, fmt.Errorf("delete emptied Album after Track Replacement: %w", err)
		}
	}
	if err = deleteUnreferencedArtists(ctx, q, previousArtistIDs); err != nil {
		return nil, fmt.Errorf("delete unreferenced Artist after Track Replacement: %w", err)
	}
	invalidations, err := advanceQueues(ctx, q, data.Target.Queues)
	if err != nil {
		return nil, fmt.Errorf("advance Queue revision after Track Replacement: %w", err)
	}
	return invalidations, nil
}

func updateReplacedTrack(ctx context.Context, q *db.Queries, data replacementCommitData, shell commitData) error {
	columns, err := newTrackColumns(shell)
	if err != nil {
		return fmt.Errorf("stat replacement Managed Track: %w", err)
	}
	track := columns.insert
	updated, err := q.UpdateReplacedTrack(ctx, db.UpdateReplacedTrackParams{
		AlbumID: track.AlbumID, Title: track.Title, TitleSort: track.TitleSort, ArtistName: track.ArtistName,
		TrackNo: track.TrackNo, DurationMs: track.DurationMs, Format: track.Format, SizeBytes: track.SizeBytes,
		FilePath: track.FilePath, FileMtime: track.FileMtime, Genre: track.Genre, SampleRateHz: track.SampleRateHz,
		BitDepth: track.BitDepth, DiscNo: track.DiscNo, TrackTotal: track.TrackTotal, DiscTotal: track.DiscTotal,
		ChannelCount: track.ChannelCount, BitrateBps: track.BitrateBps, Codec: track.Codec, Container: track.Container,
		ReplaygainTrackGainDb: track.ReplaygainTrackGainDb, ReplaygainTrackPeak: track.ReplaygainTrackPeak,
		ReplaygainAlbumGainDb: track.ReplaygainAlbumGainDb, ReplaygainAlbumPeak: track.ReplaygainAlbumPeak,
		IdentityKey: track.IdentityKey, TrackID: data.Target.TrackID, ExpectedRevision: int64(data.Target.TrackRevision),
	})
	if err != nil {
		return fmt.Errorf("update replaced Managed Track: %w", err)
	}
	if updated != 1 {
		return fmt.Errorf("%w: Track changed during replacement", ErrReplacementConflict)
	}
	updated, err = q.UpdateReplacedTrackSource(ctx, db.UpdateReplacedTrackSourceParams{
		FilePath: data.Placement.AudioPath, ContentSha256: &data.Inspection.FileSHA256,
		SourceFormat: data.Inspection.Audio.Format, SizeBytes: columns.fileInfo.Size(),
		TrackID: data.Target.TrackID, ExpectedRevision: int64(data.Target.SourceRevision),
	})
	if err != nil {
		return fmt.Errorf("update replaced Managed Track source: %w", err)
	}
	if updated != 1 {
		return fmt.Errorf("%w: Track source changed during replacement", ErrReplacementConflict)
	}
	return nil
}

func writeReplacementArtwork(ctx context.Context, q *db.Queries, data replacementCommitData, shell commitData) error {
	switch data.Placement.ArtworkMode {
	case REPLACEMENT_ARTWORK_MODE_CREATE:
		shell.Identity.ExistingArtworkPath = ""
		return insertArtwork(ctx, q, shell)
	case REPLACEMENT_ARTWORK_MODE_REPLACE:
		artwork := data.Inspection.AlbumArtwork
		replaced, err := q.ReplaceAlbumArtwork(ctx, db.ReplaceAlbumArtworkParams{
			SourceTrackID: data.Target.TrackID, ContentSha256: artwork.SHA256, MediaType: artwork.MIMEType,
			Width: int64(artwork.Width), Height: int64(artwork.Height), EncodedSizeBytes: int64(len(artwork.Data)),
			FilePath: data.Placement.ArtworkPath, AlbumID: data.Identity.AlbumID,
		})
		if err != nil {
			return fmt.Errorf("replace Album Artwork: %w", err)
		}
		return requireOne(replaced, nil)
	default:
		return nil
	}
}

// FinalizeReplacement records the replaced outcome once the previous managed file has been deleted.
func (r *Repository) FinalizeReplacement(ctx context.Context, job importJob, journalID string) (TrackReplacementResult, error) {
	err := r.inTx(ctx, "Track Replacement finalization", func(q *db.Queries) error {
		marked, err := q.MarkJobReplaced(ctx, db.MarkJobReplacedParams{ID: job.ID, Revision: int64(job.Revision)})
		if err != nil {
			return fmt.Errorf("mark Track Replacement committed: %w", err)
		}
		if err = requireOne(marked, nil); err != nil {
			return err
		}
		completed, err := q.CompleteReplacementJournal(ctx, journalID)
		if err != nil {
			return fmt.Errorf("complete Track Replacement journal: %w", err)
		}
		if err := requireOne(completed, nil); err != nil {
			return err
		}
		return archiveStandaloneHistory(ctx, q, job.ID, HISTORY_RESULT_COMPLETED)
	})
	if err != nil {
		return TrackReplacementResult{}, err
	}
	return TrackReplacementResult{JobID: job.ID, Status: STATUS_COMMITTED, Revision: job.Revision + 1, TrackID: job.ReplaceTrackID}, nil
}
