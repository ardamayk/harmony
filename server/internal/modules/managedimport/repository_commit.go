package managedimport

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/ardam/navidrome-replacement/server/internal/modules/managedimport/db"
	"github.com/google/uuid"
	"golang.org/x/text/cases"
	"golang.org/x/text/unicode/norm"
)

type commitData struct {
	Job        importJob
	Identity   commitIdentity
	Placement  placedFiles
	Inspection library.MediaInspection
	AlbumKey   string
}

func (r *Repository) ResolveCommitIdentity(ctx context.Context, metadata library.NormalizedMediaMetadata, albumKeys ...string) (commitIdentity, error) {
	albumKey := albumIdentityKey(metadata)
	if len(albumKeys) > 0 {
		albumKey = albumKeys[0]
	}
	identity := commitIdentity{TrackID: uuid.NewString()}
	artistID, err := r.q.GetArtistIDByName(ctx, ptr(normalizeIdentity(metadata.AlbumArtists[0])))
	switch {
	case errors.Is(err, sql.ErrNoRows):
		identity.AlbumArtistID = uuid.NewString()
	case err != nil:
		return commitIdentity{}, fmt.Errorf("resolve primary Album Artist identity: %w", err)
	default:
		identity.AlbumArtistID = artistID
	}
	album, err := r.q.ResolveAlbumByKey(ctx, &albumKey)
	if errors.Is(err, sql.ErrNoRows) {
		identity.AlbumID = uuid.NewString()
		return identity, nil
	}
	if err != nil {
		return commitIdentity{}, fmt.Errorf("resolve Managed Import Album identity: %w", err)
	}
	identity.AlbumID = album.ID
	identity.ExistingArtworkPath = deref(album.FilePath)
	identity.ExistingArtworkSHA256 = deref(album.ContentSha256)
	return identity, nil
}

// ResolveReplacementIdentity resolves the Album and Album Artist a replacement file belongs to while keeping the Track ID.
func (r *Repository) ResolveReplacementIdentity(ctx context.Context, metadata library.NormalizedMediaMetadata, albumKey, trackID string) (commitIdentity, error) {
	identity, err := r.ResolveCommitIdentity(ctx, metadata, albumKey)
	if err != nil {
		return commitIdentity{}, err
	}
	identity.TrackID = trackID
	return identity, nil
}

func (r *Repository) Commit(ctx context.Context, data commitData) (Result, error) {
	var result Result
	err := r.inTx(ctx, "Managed Import transaction", func(q *db.Queries) error {
		if err := writeCommitData(ctx, q, data); err != nil {
			return err
		}
		var err error
		if result, err = markCommitted(ctx, q, data.Job, data.Identity.TrackID); err != nil {
			return err
		}
		return archiveStandaloneHistory(ctx, q, data.Job.ID, HISTORY_RESULT_COMPLETED)
	})
	if err != nil {
		return Result{}, err
	}
	return result, nil
}

// CommitPending writes the Track hidden and journals the database commit, so
// recovery can finish or undo it after a crash.
func (r *Repository) CommitPending(ctx context.Context, data commitData, journalID string) error {
	return r.inTx(ctx, "pending Managed Import commit", func(q *db.Queries) error {
		if err := writeCommitData(ctx, q, data); err != nil {
			return err
		}
		if err := q.SetTrackPendingCommit(ctx, data.Identity.TrackID); err != nil {
			return fmt.Errorf("hide pending Managed Track: %w", err)
		}
		advanced, err := q.AdvanceCommitJournal(ctx, db.AdvanceCommitJournalParams{
			ToPhase: string(COMMIT_PHASE_DATABASE_COMMITTED), ID: journalID, FromPhase: string(COMMIT_PHASE_VERIFIED),
		})
		if err != nil {
			return fmt.Errorf("journal Managed Import database commit: %w", err)
		}
		return requireOne(advanced, nil)
	})
}

func (r *Repository) FinalizeCommit(ctx context.Context, job importJob, trackID, journalID string) (Result, error) {
	var result Result
	err := r.inTx(ctx, "Managed Import finalization", func(q *db.Queries) error {
		published, err := q.PublishPendingTrack(ctx, trackID)
		if err != nil {
			return fmt.Errorf("publish committed Managed Track: %w", err)
		}
		if err = requireOne(published, nil); err != nil {
			return err
		}
		if result, err = markCommitted(ctx, q, job, trackID); err != nil {
			return err
		}
		completed, err := q.CompleteCommitJournal(ctx, journalID)
		if err != nil {
			return fmt.Errorf("complete Managed Import commit journal: %w", err)
		}
		if err := requireOne(completed, nil); err != nil {
			return err
		}
		return archiveStandaloneHistory(ctx, q, job.ID, HISTORY_RESULT_COMPLETED)
	})
	if err != nil {
		return Result{}, err
	}
	return result, nil
}

func (r *Repository) RollbackPendingCommit(ctx context.Context, journal commitJournal, reason string) error {
	return r.inTx(ctx, "pending Managed Import rollback", func(q *db.Queries) error {
		if err := q.DeleteArtworkFromSourceTrack(ctx, journal.TrackID); err != nil {
			return fmt.Errorf("delete pending Album Artwork: %w", err)
		}
		if err := q.DeletePendingTrack(ctx, journal.TrackID); err != nil {
			return fmt.Errorf("delete pending Managed Track: %w", err)
		}
		reset, err := q.ResetOpenCommitJournal(ctx, db.ResetOpenCommitJournalParams{
			Phase: string(COMMIT_PHASE_VERIFIED), RecoveryReason: &reason, ID: journal.ID,
		})
		if err != nil {
			return fmt.Errorf("journal pending Managed Import rollback: %w", err)
		}
		return requireOne(reset, nil)
	})
}

func writeCommitData(ctx context.Context, q *db.Queries, data commitData) error {
	artistIDs, err := upsertArtists(ctx, q, data.Inspection.Metadata, data.Identity)
	if err != nil {
		return err
	}
	if err := upsertAlbum(ctx, q, data, artistIDs); err != nil {
		return err
	}
	if err := insertTrack(ctx, q, data); err != nil {
		return err
	}
	if err := insertRelationships(ctx, q, data, artistIDs); err != nil {
		return err
	}
	return insertArtwork(ctx, q, data)
}

func upsertArtists(ctx context.Context, q *db.Queries, metadata library.NormalizedMediaMetadata, identity commitIdentity) (map[string]string, error) {
	artistIDs := make(map[string]string)
	primaryAlbumArtist := normalizeIdentity(metadata.AlbumArtists[0])
	for _, name := range append(append([]string{}, metadata.AlbumArtists...), metadata.Artists...) {
		normalizedName := normalizeIdentity(name)
		if _, exists := artistIDs[normalizedName]; exists {
			continue
		}
		preferredID := ""
		if normalizedName == primaryAlbumArtist {
			preferredID = identity.AlbumArtistID
		}
		artistID, err := upsertArtist(ctx, q, name, normalizedName, preferredID)
		if err != nil {
			return nil, err
		}
		artistIDs[normalizedName] = artistID
	}
	return artistIDs, nil
}

func upsertArtist(ctx context.Context, q *db.Queries, name, normalizedName, preferredID string) (string, error) {
	artistID, err := q.GetArtistIDByName(ctx, &normalizedName)
	if err == nil {
		if preferredID != "" && artistID != preferredID {
			return "", fmt.Errorf("resolved Artist identity changed during Managed Import commit")
		}
		return artistID, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return "", fmt.Errorf("lookup Artist %q: %w", name, err)
	}
	artistID = preferredID
	if artistID == "" {
		artistID = uuid.NewString()
	}
	if err := q.InsertArtist(ctx, db.InsertArtistParams{
		ID: artistID, Name: name, NameSort: normalizedName, NameNormalized: &normalizedName,
	}); err != nil {
		return "", fmt.Errorf("create Artist %q: %w", name, err)
	}
	return artistID, nil
}

func upsertAlbum(ctx context.Context, q *db.Queries, data commitData, artistIDs map[string]string) error {
	metadata := data.Inspection.Metadata
	existingID, err := q.GetAlbumIDByKey(ctx, &data.AlbumKey)
	if err == nil {
		if existingID != data.Identity.AlbumID {
			return fmt.Errorf("resolved Album identity changed during Managed Import commit")
		}
		return nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return fmt.Errorf("lookup Managed Import Album: %w", err)
	}
	genres, err := json.Marshal(metadata.Genres)
	if err != nil {
		return fmt.Errorf("encode Managed Import Album Genres: %w", err)
	}
	if err := q.InsertAlbum(ctx, db.InsertAlbumParams{
		ID:          data.Identity.AlbumID,
		ArtistID:    artistIDs[normalizeIdentity(metadata.AlbumArtists[0])],
		Title:       metadata.Album,
		TitleSort:   normalizeIdentity(metadata.Album),
		Year:        nullablePositive(metadata.Year),
		ReleaseDate: releaseDate(metadata.Year),
		Genres:      string(genres),
		IdentityKey: &data.AlbumKey,
	}); err != nil {
		return fmt.Errorf("create Managed Import Album: %w", err)
	}
	return nil
}

// trackColumns holds the tracks-row values shared by a new commit and a replacement.
type trackColumns struct {
	insert   db.InsertTrackParams
	fileInfo os.FileInfo
}

func newTrackColumns(data commitData) (trackColumns, error) {
	metadata := data.Inspection.Metadata
	audio := data.Inspection.Audio
	fileInfo, err := os.Stat(data.Placement.AudioPath)
	if err != nil {
		return trackColumns{}, err
	}
	return trackColumns{fileInfo: fileInfo, insert: db.InsertTrackParams{
		ID:                    data.Identity.TrackID,
		AlbumID:               data.Identity.AlbumID,
		Title:                 metadata.Title,
		TitleSort:             normalizeIdentity(metadata.Title),
		ArtistName:            strings.Join(metadata.Artists, ", "),
		TrackNo:               ptr(int64(metadata.TrackPosition.Number)),
		DurationMs:            int64(audio.DurationMs),
		Format:                audio.Format,
		SizeBytes:             fileInfo.Size(),
		FilePath:              data.Placement.AudioPath,
		FileMtime:             fileInfo.ModTime().Unix(),
		Genre:                 ptr(firstGenre(metadata.Genres)),
		SampleRateHz:          ptr(int64(audio.SampleRateHz)),
		BitDepth:              nullablePositive(audio.BitDepth),
		DiscNo:                ptr(int64(metadata.DiscPosition.Number)),
		TrackTotal:            nullablePositive(metadata.TrackPosition.Total),
		DiscTotal:             nullablePositive(metadata.DiscPosition.Total),
		ChannelCount:          ptr(int64(audio.ChannelCount)),
		BitrateBps:            ptr(int64(audio.BitrateKbps * BITS_PER_KILOBIT)),
		Codec:                 ptr(audio.Codec),
		Container:             ptr(audio.Container),
		ReplaygainTrackGainDb: metadata.ReplayGain.TrackGainDB,
		ReplaygainTrackPeak:   metadata.ReplayGain.TrackPeak,
		ReplaygainAlbumGainDb: metadata.ReplayGain.AlbumGainDB,
		ReplaygainAlbumPeak:   metadata.ReplayGain.AlbumPeak,
		IdentityKey:           ptr(trackIdentityKey(metadata)),
		Lyrics:                metadata.Lyrics,
	}}, nil
}

func insertTrack(ctx context.Context, q *db.Queries, data commitData) error {
	columns, err := newTrackColumns(data)
	if err != nil {
		return fmt.Errorf("stat canonical Managed Track: %w", err)
	}
	if err := q.InsertTrack(ctx, columns.insert); err != nil {
		return fmt.Errorf("create Managed Track: %w", err)
	}
	if err := q.InsertTrackSource(ctx, db.InsertTrackSourceParams{
		ID:            uuid.NewString(),
		TrackID:       data.Identity.TrackID,
		FilePath:      data.Placement.AudioPath,
		ContentSha256: &data.Inspection.FileSHA256,
		SourceFormat:  data.Inspection.Audio.Format,
		SizeBytes:     columns.fileInfo.Size(),
	}); err != nil {
		return fmt.Errorf("create Managed Track source: %w", err)
	}
	return nil
}

func insertRelationships(ctx context.Context, q *db.Queries, data commitData, artistIDs map[string]string) error {
	metadata := data.Inspection.Metadata
	for position, name := range metadata.AlbumArtists {
		if err := q.InsertAlbumArtistCredit(ctx, db.InsertAlbumArtistCreditParams{
			AlbumID: data.Identity.AlbumID, ArtistID: artistIDs[normalizeIdentity(name)], Position: int64(position),
		}); err != nil {
			return fmt.Errorf("create ordered Artist credit %q: %w", name, err)
		}
	}
	if err := insertTrackArtistCredits(ctx, q, data.Identity.TrackID, metadata.Artists, artistIDs); err != nil {
		return err
	}
	for position, genreName := range metadata.Genres {
		genreID, err := upsertGenre(ctx, q, genreName)
		if err != nil {
			return err
		}
		if err := q.InsertTrackGenre(ctx, db.InsertTrackGenreParams{
			TrackID: data.Identity.TrackID, GenreID: genreID, Position: int64(position),
		}); err != nil {
			return fmt.Errorf("create Track Genre %q: %w", genreName, err)
		}
	}
	return nil
}

func insertTrackArtistCredits(ctx context.Context, q *db.Queries, trackID string, names []string, artistIDs map[string]string) error {
	for position, name := range names {
		if err := q.InsertTrackArtistCredit(ctx, db.InsertTrackArtistCreditParams{
			TrackID: trackID, ArtistID: artistIDs[normalizeIdentity(name)], Position: int64(position),
		}); err != nil {
			return fmt.Errorf("create ordered Artist credit %q: %w", name, err)
		}
	}
	return nil
}

func insertAlbumArtistCredits(ctx context.Context, q *db.Queries, albumID string, names []string, artistIDs map[string]string) error {
	for position, name := range names {
		if err := q.InsertAlbumArtistCredit(ctx, db.InsertAlbumArtistCreditParams{
			AlbumID: albumID, ArtistID: artistIDs[normalizeIdentity(name)], Position: int64(position),
		}); err != nil {
			return fmt.Errorf("create ordered Artist credit %q: %w", name, err)
		}
	}
	return nil
}

func upsertGenre(ctx context.Context, q *db.Queries, name string) (string, error) {
	normalizedName := normalizeIdentity(name)
	genreID, err := q.GetGenreIDByName(ctx, normalizedName)
	if err == nil {
		return genreID, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return "", fmt.Errorf("lookup Genre %q: %w", name, err)
	}
	genreID = uuid.NewString()
	if err := q.InsertGenre(ctx, db.InsertGenreParams{ID: genreID, Name: name, NameNormalized: normalizedName}); err != nil {
		return "", fmt.Errorf("create Genre %q: %w", name, err)
	}
	return genreID, nil
}

func insertArtwork(ctx context.Context, q *db.Queries, data commitData) error {
	if data.Identity.ExistingArtworkPath != "" || data.Inspection.AlbumArtwork.SHA256 == "" {
		return nil
	}
	artwork := data.Inspection.AlbumArtwork
	if err := q.InsertAlbumArtwork(ctx, db.InsertAlbumArtworkParams{
		ID:               uuid.NewString(),
		AlbumID:          data.Identity.AlbumID,
		SourceTrackID:    data.Identity.TrackID,
		ContentSha256:    artwork.SHA256,
		MediaType:        artwork.MIMEType,
		Width:            int64(artwork.Width),
		Height:           int64(artwork.Height),
		EncodedSizeBytes: int64(len(artwork.Data)),
		FilePath:         data.Placement.ArtworkPath,
	}); err != nil {
		return fmt.Errorf("create Album Artwork: %w", err)
	}
	return nil
}

func markCommitted(ctx context.Context, q *db.Queries, job importJob, trackID string) (Result, error) {
	marked, err := q.MarkJobCommitted(ctx, db.MarkJobCommittedParams{TrackID: &trackID, ID: job.ID, Revision: int64(job.Revision)})
	if err != nil {
		return Result{}, fmt.Errorf("mark Managed Import committed: %w", err)
	}
	if err := requireOne(marked, nil); err != nil {
		return Result{}, err
	}
	return Result{JobID: job.ID, Status: STATUS_COMMITTED, Revision: job.Revision + 1, TrackID: trackID}, nil
}

func (r *Repository) CreateCommitJournal(ctx context.Context, journal commitJournal) error {
	if err := r.q.InsertCommitJournal(ctx, db.InsertCommitJournalParams{
		ID: journal.ID, JobID: journal.JobID, TrackID: journal.TrackID, Phase: string(journal.Phase),
		StagedFilePath: journal.StagedFilePath, AudioFilePath: journal.AudioFilePath,
		ArtworkFilePath: journal.ArtworkFilePath, AudioSha256: journal.AudioSHA256,
		ArtworkSha256: journal.ArtworkSHA256, ArtworkCreated: boolInt(journal.IsArtworkCreated),
	}); err != nil {
		return fmt.Errorf("create Managed Import commit journal: %w", err)
	}
	return nil
}

func (r *Repository) MarkCommitPlaced(ctx context.Context, journalID string, isArtworkCreated bool) error {
	placed, err := r.q.MarkCommitPlaced(ctx, db.MarkCommitPlacedParams{ArtworkCreated: boolInt(isArtworkCreated), ID: journalID})
	if err != nil {
		return fmt.Errorf("journal canonical Managed Import placement: %w", err)
	}
	return requireOne(placed, nil)
}

func (r *Repository) MarkCommitArtworkCreated(ctx context.Context, journalID string) error {
	marked, err := r.q.MarkCommitArtworkCreated(ctx, journalID)
	if err != nil {
		return fmt.Errorf("journal Canonical Album Artwork creation: %w", err)
	}
	return requireOne(marked, nil)
}

func (r *Repository) UpdateCommitPhase(ctx context.Context, journalID string, phase commitPhase) error {
	updated, err := r.q.UpdateOpenCommitJournalPhase(ctx, db.UpdateOpenCommitJournalPhaseParams{Phase: string(phase), ID: journalID})
	if err != nil {
		return fmt.Errorf("update Managed Import commit journal to %q: %w", phase, err)
	}
	return requireOne(updated, nil)
}

func (r *Repository) RollbackCommitJournal(ctx context.Context, journalID, reason string) error {
	rolledBack, err := r.q.ResetOpenCommitJournal(ctx, db.ResetOpenCommitJournalParams{
		Phase: string(COMMIT_PHASE_ROLLED_BACK), RecoveryReason: &reason, ID: journalID,
	})
	if err != nil {
		return fmt.Errorf("roll back Managed Import commit journal: %w", err)
	}
	return requireOne(rolledBack, nil)
}

func (r *Repository) RecordCommitRecoveryReason(ctx context.Context, journalID, reason string) error {
	recorded, err := r.q.RecordCommitRecoveryReason(ctx, db.RecordCommitRecoveryReasonParams{RecoveryReason: &reason, ID: journalID})
	if err != nil {
		return fmt.Errorf("record Managed Import recovery reason: %w", err)
	}
	return requireOne(recorded, nil)
}

func (r *Repository) ListIncompleteCommitJournals(ctx context.Context) ([]commitJournal, error) {
	rows, err := r.q.ListIncompleteCommitJournals(ctx)
	if err != nil {
		return nil, fmt.Errorf("list incomplete Managed Import commits: %w", err)
	}
	var journals []commitJournal
	for _, row := range rows {
		journals = append(journals, toCommitJournal(row))
	}
	return journals, nil
}

func (r *Repository) GetCommitJournal(ctx context.Context, journalID string) (commitJournal, error) {
	row, err := r.q.GetCommitJournal(ctx, journalID)
	if errors.Is(err, sql.ErrNoRows) {
		return commitJournal{}, ErrNotFound
	}
	if err != nil {
		return commitJournal{}, fmt.Errorf("read Managed Import commit journal: %w", err)
	}
	return toCommitJournal(row), nil
}

func (r *Repository) FindIncompleteCommitJournal(ctx context.Context, jobID string) (commitJournal, bool, error) {
	row, err := r.q.FindIncompleteCommitJournal(ctx, jobID)
	if errors.Is(err, sql.ErrNoRows) {
		return commitJournal{}, false, nil
	}
	if err != nil {
		return commitJournal{}, false, fmt.Errorf("read Managed Import commit journal: %w", err)
	}
	return toCommitJournal(row), true, nil
}

func toCommitJournal(row db.ManagedImportCommitJournal) commitJournal {
	return commitJournal{
		ID: row.ID, JobID: row.JobID, TrackID: row.TrackID, Phase: commitPhase(row.Phase),
		StagedFilePath: row.StagedFilePath, AudioFilePath: row.AudioFilePath, ArtworkFilePath: row.ArtworkFilePath,
		AudioSHA256: row.AudioSha256, ArtworkSHA256: row.ArtworkSha256, IsArtworkCreated: row.ArtworkCreated != 0,
		RecoveryReason: deref(row.RecoveryReason),
	}
}

func normalizeIdentity(value string) string {
	return cases.Fold().String(strings.Join(strings.Fields(norm.NFC.String(value)), " "))
}

func albumIdentityKey(metadata library.NormalizedMediaMetadata) string {
	if metadata.AlbumIdentityKey != "" {
		return metadata.AlbumIdentityKey
	}
	credits := make([]string, len(metadata.AlbumArtists))
	for index, name := range metadata.AlbumArtists {
		credits[index] = normalizeIdentity(name)
	}
	return strings.Join(credits, "\x1e") + "\x1f" + normalizeIdentity(metadata.Album)
}

func trackIdentityKey(metadata library.NormalizedMediaMetadata) string {
	return fmt.Sprintf("%d\x1f%d\x1f%s", metadata.DiscPosition.Number, metadata.TrackPosition.Number, normalizeIdentity(metadata.Title))
}

func releaseDate(year int) *string {
	if year <= 0 {
		return nil
	}
	return ptr(fmt.Sprint(year))
}

func firstGenre(genres []string) string {
	if len(genres) == 0 {
		return ""
	}
	return genres[0]
}
