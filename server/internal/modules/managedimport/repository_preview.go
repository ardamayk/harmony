package managedimport

import (
	"context"
	"database/sql"
	"errors"
	"fmt"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/ardam/navidrome-replacement/server/internal/modules/managedimport/db"
)

func (r *Repository) FindManagedTrackByHash(ctx context.Context, contentSHA256 string) (string, error) {
	trackID, err := r.q.FindManagedTrackByHash(ctx, &contentSHA256)
	if errors.Is(err, sql.ErrNoRows) {
		return "", nil
	}
	if err != nil {
		return "", fmt.Errorf("find Managed Track by content hash: %w", err)
	}
	return trackID, nil
}

func (r *Repository) FindAlbumArtworkHash(ctx context.Context, metadata library.NormalizedMediaMetadata) (string, error) {
	contentSHA256, err := r.q.FindAlbumArtworkHash(ctx, ptr(albumIdentityKey(metadata)))
	if errors.Is(err, sql.ErrNoRows) {
		return "", nil
	}
	if err != nil {
		return "", fmt.Errorf("find existing Album Artwork hash: %w", err)
	}
	return contentSHA256, nil
}

// ExistingAlbumArtwork returns the artwork already stored for the Album key, if any.
func (r *Repository) ExistingAlbumArtwork(ctx context.Context, albumKey string) (library.AlbumArtwork, bool, error) {
	row, err := r.q.GetAlbumArtworkByKey(ctx, &albumKey)
	if errors.Is(err, sql.ErrNoRows) {
		return library.AlbumArtwork{}, false, nil
	}
	if err != nil {
		return library.AlbumArtwork{}, false, fmt.Errorf("read existing Album artwork: %w", err)
	}
	return library.AlbumArtwork{SHA256: row.ContentSha256, MIMEType: row.MediaType, Width: int(row.Width), Height: int(row.Height)}, true, nil
}

func (r *Repository) AlbumRequiresDiscNumber(ctx context.Context, metadata library.NormalizedMediaMetadata, excludedTrackID string) (bool, error) {
	requires, err := r.q.AlbumRequiresDiscNumber(ctx, db.AlbumRequiresDiscNumberParams{
		IdentityKey: ptr(albumIdentityKey(metadata)), ID: excludedTrackID,
	})
	if err != nil {
		return false, fmt.Errorf("inspect existing Album disc positions: %w", err)
	}
	return requires, nil
}

// AlbumPositionTotalConflict names the tag whose total disagrees with the Album's existing Tracks.
func (r *Repository) AlbumPositionTotalConflict(ctx context.Context, metadata library.NormalizedMediaMetadata, excludedTrackID string) (string, error) {
	conflict, err := r.q.AlbumPositionTotalConflict(ctx, db.AlbumPositionTotalConflictParams{
		DiscTotal:       int64(metadata.DiscPosition.Total),
		DiscNumber:      int64(metadata.DiscPosition.Number),
		TrackTotal:      int64(metadata.TrackPosition.Total),
		AlbumKey:        ptr(albumIdentityKey(metadata)),
		ExcludedTrackID: excludedTrackID,
	})
	if err != nil {
		return "", fmt.Errorf("inspect existing Album position totals: %w", err)
	}
	if conflict.HasDiscConflict {
		return "TOTALDISCS", nil
	}
	if conflict.HasTrackConflict {
		return "TOTALTRACKS", nil
	}
	return "", nil
}

func (r *Repository) FindExactDuplicateTrackID(ctx context.Context, contentSHA256 string) (string, error) {
	trackID, err := r.q.FindTrackByContentHash(ctx, &contentSHA256)
	if errors.Is(err, sql.ErrNoRows) {
		return "", nil
	}
	if err != nil {
		return "", fmt.Errorf("inspect exact Managed Import duplicate: %w", err)
	}
	return trackID, nil
}

func (r *Repository) ClassifyDuplicate(ctx context.Context, inspection library.MediaInspection) (DuplicateClassification, []DuplicateCandidate, error) {
	trackID, err := r.FindExactDuplicateTrackID(ctx, inspection.FileSHA256)
	if err != nil {
		return "", nil, err
	}
	if trackID == "" {
		return DUPLICATE_NONE, nil, nil
	}
	candidate, err := r.readDuplicateCandidate(ctx, trackID)
	if err != nil {
		return "", nil, err
	}
	return DUPLICATE_EXACT, []DuplicateCandidate{candidate}, nil
}

// findDuplicatesByPosition lists Tracks already at the file's disc and track position in its Album.
func (r *Repository) findDuplicatesByPosition(ctx context.Context, metadata library.NormalizedMediaMetadata, excludedTrackID string) ([]string, error) {
	ids, err := r.q.FindTracksAtPosition(ctx, db.FindTracksAtPositionParams{
		ID:          excludedTrackID,
		IdentityKey: ptr(albumIdentityKey(metadata)),
		DiscNo:      ptr(int64(metadata.DiscPosition.Number)),
		TrackNo:     ptr(int64(metadata.TrackPosition.Number)),
	})
	if err != nil {
		return nil, fmt.Errorf("inspect possible Managed Import position duplicates: %w", err)
	}
	return ids, nil
}

func (r *Repository) positionCandidates(ctx context.Context, metadata library.NormalizedMediaMetadata, excludedTrackID string) ([]DuplicateCandidate, error) {
	ids, err := r.findDuplicatesByPosition(ctx, metadata, excludedTrackID)
	if err != nil {
		return nil, err
	}
	return r.readDuplicateCandidates(ctx, ids)
}

func (r *Repository) readDuplicateCandidates(ctx context.Context, trackIDs []string) ([]DuplicateCandidate, error) {
	candidates := make([]DuplicateCandidate, 0, len(trackIDs))
	for _, trackID := range trackIDs {
		candidate, err := r.readDuplicateCandidate(ctx, trackID)
		if err != nil {
			return nil, err
		}
		candidates = append(candidates, candidate)
	}
	return candidates, nil
}

func (r *Repository) readDuplicateCandidate(ctx context.Context, trackID string) (DuplicateCandidate, error) {
	row, err := r.q.GetDuplicateCandidate(ctx, trackID)
	if err != nil {
		return DuplicateCandidate{}, fmt.Errorf("read Managed Import duplicate candidate: %w", err)
	}
	candidate := DuplicateCandidate{
		TrackID: row.ID, Title: row.Title, Album: row.AlbumTitle, DiscNo: int(row.DiscNo),
		TrackNo: int(deref(row.TrackNo)), Format: row.Format, DurationMs: int(row.DurationMs), Revision: int(row.Revision),
	}
	candidate.Artists, err = r.q.ListTrackArtistNames(ctx, trackID)
	if err != nil {
		return DuplicateCandidate{}, fmt.Errorf("read duplicate candidate Artists: %w", err)
	}
	if len(candidate.Artists) == 0 {
		candidate.Artists = nil
	}
	candidate.TitleKey = normalizeIdentity(candidate.Title)
	return candidate, r.populateCandidateMetadata(ctx, &candidate)
}

func (r *Repository) populateCandidateMetadata(ctx context.Context, candidate *DuplicateCandidate) error {
	target, err := readReplacementTargetRow(ctx, r.q, candidate.TrackID)
	if errors.Is(err, ErrTrackNotFound) {
		return nil
	}
	if err != nil {
		return err
	}
	genres, err := r.q.ListTrackGenreNames(ctx, candidate.TrackID)
	if err != nil {
		return fmt.Errorf("list Track Replacement names: %w", err)
	}
	albumArtists, err := r.q.ListAlbumArtistNames(ctx, target.AlbumID)
	if err != nil {
		return fmt.Errorf("list Track Replacement names: %w", err)
	}
	candidate.CurrentFile = &PreviewFile{
		OriginalFilename: target.Title + "." + target.Format, Title: target.Title, TitleKey: candidate.TitleKey,
		Artists: candidate.Artists, AlbumArtists: albumArtists, Album: target.Album, Genres: genres, Year: target.Year,
		TrackNo: target.TrackNo, TrackTotal: target.TrackTotal, DiscNo: target.DiscNo, DiscTotal: target.DiscTotal,
		Format: target.Format, Codec: target.Codec, Container: target.Container, DurationMs: target.DurationMs,
		SizeBytes: target.SizeBytes, SampleRateHz: target.SampleRateHz, ChannelCount: target.ChannelCount,
		BitDepth: target.BitDepth, BitrateKbps: target.BitrateKbps, ArtworkMediaType: target.ArtworkType,
	}
	return nil
}

func (r *Repository) albumPreview(ctx context.Context, batchID string, file PreviewFile) (AlbumPreview, error) {
	album := AlbumPreview{Key: albumPreviewKey(file), Title: file.Album, AlbumArtists: file.AlbumArtists}
	var err error
	album.ExistingAlbums, err = r.matchAlbums(ctx, previewAlbumKey(file))
	if err != nil {
		return album, err
	}
	album.Artworks, err = r.listArtworkOptions(ctx, batchID, album.Key)
	return album, err
}

// matchAlbums lists the Album with this key and every separate edition created from it.
func (r *Repository) matchAlbums(ctx context.Context, key string) ([]AlbumMatch, error) {
	rows, err := r.q.MatchAlbums(ctx, db.MatchAlbumsParams{AlbumKey: key, EditionPrefix: key + "\x1fmanaged-import-edition:"})
	if err != nil {
		return nil, fmt.Errorf("match import Albums: %w", err)
	}
	matches := make([]AlbumMatch, 0, len(rows))
	for _, row := range rows {
		match := AlbumMatch{ID: row.ID, IdentityKey: deref(row.IdentityKey), Year: int(row.Year), HasArtwork: row.HasArtwork}
		trackIDs, err := r.q.ListVisibleAlbumTrackIDs(ctx, row.ID)
		if err != nil {
			return nil, fmt.Errorf("list import Album tracks: %w", err)
		}
		if match.Tracks, err = r.readDuplicateCandidates(ctx, trackIDs); err != nil {
			return nil, err
		}
		matches = append(matches, match)
	}
	return matches, nil
}

func (r *Repository) saveArtwork(ctx context.Context, batchID, id, jobID, albumKey string, artwork library.AlbumArtwork) error {
	saved, err := r.q.SaveImportArtwork(ctx, db.SaveImportArtworkParams{
		ID: id, JobID: jobID, AlbumKey: albumKey, MediaType: artwork.MIMEType, Width: int64(artwork.Width),
		Height: int64(artwork.Height), ContentSha256: artwork.SHA256, Data: artwork.Data, BatchID: batchID,
	})
	if err != nil {
		return fmt.Errorf("stage import Album artwork: %w", err)
	}
	return requireOne(saved, nil)
}

// listArtworkOptions offers each distinct staged image once, uploaded covers first.
func (r *Repository) listArtworkOptions(ctx context.Context, batchID, albumKey string) ([]ArtworkOption, error) {
	rows, err := r.q.ListImportArtworkOptions(ctx, db.ListImportArtworkOptionsParams{BatchID: batchID, AlbumKey: albumKey})
	if err != nil {
		return nil, fmt.Errorf("list import artwork: %w", err)
	}
	options := []ArtworkOption{}
	seen := map[string]bool{}
	for _, row := range rows {
		if seen[row.ContentSha256] {
			continue
		}
		seen[row.ContentSha256] = true
		options = append(options, ArtworkOption{ID: row.ID, JobID: row.JobID, MediaType: row.MediaType, ContentSHA256: row.ContentSha256})
	}
	return options, nil
}

func (r *Repository) loadArtwork(ctx context.Context, batchID, id string) (library.AlbumArtwork, error) {
	row, err := r.q.GetImportArtwork(ctx, db.GetImportArtworkParams{BatchID: batchID, ID: id})
	if errors.Is(err, sql.ErrNoRows) {
		return library.AlbumArtwork{}, ErrNotFound
	}
	if err != nil {
		return library.AlbumArtwork{}, fmt.Errorf("load staged import artwork: %w", err)
	}
	return library.AlbumArtwork{
		MIMEType: row.MediaType, Width: int(row.Width), Height: int(row.Height), SHA256: row.ContentSha256, Data: row.Data,
	}, nil
}
