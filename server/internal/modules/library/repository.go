package library

import (
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"errors"
	"fmt"
	"os"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library/db"
)

// Repository reads library records. Every Album and Track it returns carries
// its ordered Artist credits, Genres, release identifiers, and artwork.
type Repository struct {
	q *db.Queries
}

func NewRepository(sqlDB *sql.DB) *Repository {
	return &Repository{q: db.New(sqlDB)}
}

func (r *Repository) ListArtists(ctx context.Context, limit, offset int, q string) (ArtistList, error) {
	total, err := r.q.CountArtists(ctx, q)
	if err != nil {
		return ArtistList{}, fmt.Errorf("count Artists for query %q: %w", q, err)
	}
	rows, err := r.q.ListArtists(ctx, db.ListArtistsParams{Q: q, RowLimit: int64(limit), RowOffset: int64(offset)})
	if err != nil {
		return ArtistList{}, fmt.Errorf("list Artists for query %q: %w", q, err)
	}
	items := make([]Artist, 0, len(rows))
	for _, row := range rows {
		items = append(items, Artist{ID: row.ID, Name: row.Name, AlbumCount: int(row.AlbumCount)})
	}
	return ArtistList{Items: items, Total: int(total)}, nil
}

func (r *Repository) ListAlbums(ctx context.Context, limit, offset int, artistID, q string) (AlbumList, error) {
	total, err := r.q.CountAlbums(ctx, db.CountAlbumsParams{ArtistID: artistID, Q: q})
	if err != nil {
		return AlbumList{}, fmt.Errorf("count Albums for Artist %q and query %q: %w", artistID, q, err)
	}
	rows, err := r.q.ListAlbums(ctx, db.ListAlbumsParams{
		ArtistID: artistID, Q: q, RowLimit: int64(limit), RowOffset: int64(offset),
	})
	if err != nil {
		return AlbumList{}, fmt.Errorf("list Albums for Artist %q and query %q: %w", artistID, q, err)
	}
	items := make([]Album, 0, len(rows))
	for _, row := range rows {
		album := toAlbum(row.ID, row.Title, row.ArtistID, row.ArtistName, row.Year, row.ReleaseDate, row.Genres)
		album.TrackCount = int(row.TrackCount)
		items = append(items, album)
	}
	if err := r.enrichAlbums(ctx, items); err != nil {
		return AlbumList{}, fmt.Errorf("enrich Album list for Artist %q and query %q: %w", artistID, q, err)
	}
	return AlbumList{Items: items, Total: int(total)}, nil
}

func (r *Repository) GetAlbum(ctx context.Context, albumID string) (AlbumDetail, error) {
	row, err := r.q.GetAlbum(ctx, albumID)
	if errors.Is(err, sql.ErrNoRows) {
		return AlbumDetail{}, ErrNotFound
	}
	if err != nil {
		return AlbumDetail{}, fmt.Errorf("get Album %q: %w", albumID, err)
	}
	trackRows, err := r.q.ListAlbumTracks(ctx, albumID)
	if err != nil {
		return AlbumDetail{}, fmt.Errorf("list Album %q Tracks: %w", albumID, err)
	}
	albums := []Album{toAlbum(row.ID, row.Title, row.ArtistID, row.ArtistName, row.Year, row.ReleaseDate, row.Genres)}
	if err := r.enrichAlbums(ctx, albums); err != nil {
		return AlbumDetail{}, fmt.Errorf("enrich Album %q: %w", albumID, err)
	}
	tracks := toTracks(trackRows)
	if err := r.enrichTracks(ctx, tracks); err != nil {
		return AlbumDetail{}, fmt.Errorf("enrich Album %q Tracks: %w", albumID, err)
	}
	detail := AlbumDetail{Album: albums[0], Tracks: tracks}
	detail.TrackCount = len(tracks)
	return detail, nil
}

func (r *Repository) ListTracks(ctx context.Context, limit, offset int, artistID, q string) (TrackList, error) {
	total, err := r.q.CountTracks(ctx, db.CountTracksParams{ArtistID: artistID, Q: q})
	if err != nil {
		return TrackList{}, fmt.Errorf("count Tracks for query %q: %w", q, err)
	}
	rows, err := r.q.ListTracks(ctx, db.ListTracksParams{
		ArtistID: artistID, Q: q, RowLimit: int64(limit), RowOffset: int64(offset),
	})
	if err != nil {
		return TrackList{}, fmt.Errorf("list Tracks for query %q: %w", q, err)
	}
	items := toTracks(rows)
	if err := r.enrichTracks(ctx, items); err != nil {
		return TrackList{}, fmt.Errorf("enrich Track list for query %q: %w", q, err)
	}
	return TrackList{Items: items, Total: int(total)}, nil
}

func (r *Repository) GetTrack(ctx context.Context, trackID string) (Track, error) {
	row, err := r.q.GetTrack(ctx, trackID)
	if errors.Is(err, sql.ErrNoRows) {
		return Track{}, ErrNotFound
	}
	if err != nil {
		return Track{}, fmt.Errorf("get Track %q: %w", trackID, err)
	}
	tracks := []Track{toTrack(row)}
	if err := r.enrichTracks(ctx, tracks); err != nil {
		return Track{}, fmt.Errorf("enrich Track %q: %w", trackID, err)
	}
	return tracks[0], nil
}

// GetTrackLyrics returns the stored lyrics text for a Track; empty when the
// file carried none. Lyrics stay out of Track listings because they are
// large and only the Lyrics view needs them.
func (r *Repository) GetTrackLyrics(ctx context.Context, trackID string) (string, error) {
	lyrics, err := r.q.GetTrackLyrics(ctx, trackID)
	if errors.Is(err, sql.ErrNoRows) {
		return "", ErrNotFound
	}
	if err != nil {
		return "", fmt.Errorf("get Track lyrics %q: %w", trackID, err)
	}
	return lyrics, nil
}

func (r *Repository) GetTrackFilePath(ctx context.Context, trackID string) (string, error) {
	source, err := r.GetTrackWaveformSource(ctx, trackID)
	return source.FilePath, err
}

// AlbumCover is an Album's original cover and the SHA-256 of its bytes, which
// identifies it for scaled-down variants.
type AlbumCover struct {
	MediaType    string
	Data         []byte
	SourceSHA256 string
}

// GetAlbumCover prefers managed Album Artwork and falls back to a cover
// embedded in the album row.
func (r *Repository) GetAlbumCover(ctx context.Context, albumID string) (mime string, data []byte, err error) {
	cover, err := r.GetAlbumCoverSource(ctx, albumID)
	return cover.MediaType, cover.Data, err
}

// GetAlbumCoverSource is GetAlbumCover plus the cover's content hash.
func (r *Repository) GetAlbumCoverSource(ctx context.Context, albumID string) (AlbumCover, error) {
	artwork, err := r.q.GetVisibleAlbumArtwork(ctx, albumID)
	if err == nil {
		data, err := os.ReadFile(artwork.FilePath)
		if err != nil {
			return AlbumCover{}, fmt.Errorf("read Album Artwork %q: %w", albumID, err)
		}
		return AlbumCover{MediaType: artwork.MediaType, Data: data, SourceSHA256: artwork.ContentSha256}, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return AlbumCover{}, fmt.Errorf("get Album Artwork %q: %w", albumID, err)
	}
	cover, err := r.q.GetEmbeddedAlbumCover(ctx, albumID)
	if errors.Is(err, sql.ErrNoRows) {
		return AlbumCover{}, ErrNotFound
	}
	if err != nil {
		return AlbumCover{}, err
	}
	if len(cover.CoverData) == 0 {
		return AlbumCover{}, ErrNotFound
	}
	mime := "image/jpeg"
	if cover.CoverMime != nil && *cover.CoverMime != "" {
		mime = *cover.CoverMime
	}
	sum := sha256.Sum256(cover.CoverData)
	return AlbumCover{MediaType: mime, Data: cover.CoverData, SourceSHA256: hex.EncodeToString(sum[:])}, nil
}

// GetAlbumCoverVariant returns a stored scaled-down cover; found is false when
// none was generated for this size yet.
func (r *Repository) GetAlbumCoverVariant(ctx context.Context, albumID string, size int) (variant AlbumCover, found bool, err error) {
	row, err := r.q.GetAlbumCoverVariant(ctx, db.GetAlbumCoverVariantParams{AlbumID: albumID, Size: int64(size)})
	if errors.Is(err, sql.ErrNoRows) {
		return AlbumCover{}, false, nil
	}
	if err != nil {
		return AlbumCover{}, false, fmt.Errorf("get cover variant %q at %d: %w", albumID, size, err)
	}
	return AlbumCover{MediaType: row.MediaType, Data: row.Data, SourceSHA256: row.SourceSha256}, true, nil
}

func (r *Repository) PutAlbumCoverVariant(ctx context.Context, albumID string, size int, variant AlbumCover) error {
	if err := r.q.PutAlbumCoverVariant(ctx, db.PutAlbumCoverVariantParams{
		AlbumID:      albumID,
		Size:         int64(size),
		SourceSha256: variant.SourceSHA256,
		MediaType:    variant.MediaType,
		Data:         variant.Data,
	}); err != nil {
		return fmt.Errorf("put cover variant %q at %d: %w", albumID, size, err)
	}
	return nil
}

func (r *Repository) GetTrackWaveformSource(ctx context.Context, trackID string) (WaveformSource, error) {
	source, err := r.q.GetTrackPlaybackSource(ctx, trackID)
	if errors.Is(err, sql.ErrNoRows) {
		return WaveformSource{}, ErrNotFound
	}
	if err != nil {
		return WaveformSource{}, fmt.Errorf("get Track source %q: %w", trackID, err)
	}
	return WaveformSource{FilePath: source.FilePath, DurationMs: int(source.DurationMs)}, nil
}

func (r *Repository) GetTrackWaveform(ctx context.Context, trackID string) (WaveformRecord, bool, error) {
	row, err := r.q.GetTrackWaveform(ctx, trackID)
	if errors.Is(err, sql.ErrNoRows) {
		return WaveformRecord{}, false, nil
	}
	if err != nil {
		return WaveformRecord{}, false, fmt.Errorf("get waveform %q: %w", trackID, err)
	}
	return WaveformRecord{Peaks: row.Peaks, SourceSizeBytes: row.SourceSizeBytes, SourceModifiedAt: row.SourceModifiedAt}, true, nil
}

func (r *Repository) PutTrackWaveform(ctx context.Context, trackID string, record WaveformRecord) error {
	if err := r.q.PutTrackWaveform(ctx, db.PutTrackWaveformParams{
		TrackID:          trackID,
		PeakCount:        int64(len(record.Peaks)),
		Peaks:            record.Peaks,
		SourceSizeBytes:  record.SourceSizeBytes,
		SourceModifiedAt: record.SourceModifiedAt,
	}); err != nil {
		return fmt.Errorf("put waveform %q: %w", trackID, err)
	}
	return nil
}

func (r *Repository) enrichAlbums(ctx context.Context, albums []Album) error {
	if len(albums) == 0 {
		return nil
	}
	ids := make([]string, len(albums))
	for index, album := range albums {
		ids[index] = album.ID
	}
	credits, err := r.albumArtistCredits(ctx, ids)
	if err != nil {
		return err
	}
	genreRows, err := r.q.ListAlbumGenres(ctx, ids)
	if err != nil {
		return fmt.Errorf("read Album Genres for IDs %q: %w", ids, err)
	}
	genres := groupBy(genreRows, func(row db.ListAlbumGenresRow) (string, Genre) {
		return row.AlbumID, Genre{ID: row.ID, Name: row.Name}
	})
	identifierRows, err := r.q.ListAlbumReleaseIdentifiers(ctx, ids)
	if err != nil {
		return fmt.Errorf("read Album release identifiers for IDs %q: %w", ids, err)
	}
	identifiers := groupBy(identifierRows, func(row db.AlbumReleaseIdentifier) (string, ReleaseIdentifier) {
		return row.AlbumID, ReleaseIdentifier{Scheme: row.Scheme, Value: row.Value}
	})
	artworkRows, err := r.q.ListAlbumArtworkMetadata(ctx, ids)
	if err != nil {
		return fmt.Errorf("read Album Artwork for IDs %q: %w", ids, err)
	}
	artwork := make(map[string]*AlbumArtworkMetadata, len(artworkRows))
	for _, row := range artworkRows {
		artwork[row.AlbumID] = &AlbumArtworkMetadata{
			SourceTrackID: row.SourceTrackID,
			ContentSHA256: row.ContentSha256,
			MediaType:     row.MediaType,
			Width:         int(row.Width),
			Height:        int(row.Height),
			SizeBytes:     row.EncodedSizeBytes,
		}
	}
	for index := range albums {
		album := &albums[index]
		album.AlbumArtists = credits[album.ID]
		if len(album.AlbumArtists) == 0 {
			album.AlbumArtists = []ArtistCredit{{ID: album.ArtistID, Name: album.ArtistName}}
		}
		album.ArtistID = album.AlbumArtists[0].ID
		album.ArtistName = album.AlbumArtists[0].Name
		album.GenreItems = nonNil(genres[album.ID])
		album.ReleaseIdentifiers = nonNil(identifiers[album.ID])
		album.Artwork = artwork[album.ID]
	}
	return nil
}

func (r *Repository) enrichTracks(ctx context.Context, tracks []Track) error {
	if len(tracks) == 0 {
		return nil
	}
	trackIDs := make([]string, len(tracks))
	albumIDs := make([]string, 0, len(tracks))
	seenAlbums := make(map[string]bool, len(tracks))
	for index, track := range tracks {
		trackIDs[index] = track.ID
		if !seenAlbums[track.AlbumID] {
			albumIDs = append(albumIDs, track.AlbumID)
			seenAlbums[track.AlbumID] = true
		}
	}
	artistRows, err := r.q.ListTrackArtistCredits(ctx, trackIDs)
	if err != nil {
		return fmt.Errorf("read Track Artists for IDs %q: %w", trackIDs, err)
	}
	artists := groupBy(artistRows, func(row db.ListTrackArtistCreditsRow) (string, ArtistCredit) {
		return row.TrackID, ArtistCredit{ID: row.ID, Name: row.Name}
	})
	albumArtists, err := r.albumArtistCredits(ctx, albumIDs)
	if err != nil {
		return err
	}
	genreRows, err := r.q.ListTrackGenres(ctx, trackIDs)
	if err != nil {
		return fmt.Errorf("read Track Genres for IDs %q: %w", trackIDs, err)
	}
	genres := groupBy(genreRows, func(row db.ListTrackGenresRow) (string, Genre) {
		return row.TrackID, Genre{ID: row.ID, Name: row.Name}
	})
	for index := range tracks {
		track := &tracks[index]
		track.Artists = artists[track.ID]
		if len(track.Artists) == 0 {
			track.Artists = []ArtistCredit{{Name: track.ArtistName}}
		}
		track.AlbumArtists = nonNil(albumArtists[track.AlbumID])
		track.Genres = nonNil(genres[track.ID])
	}
	return nil
}

func (r *Repository) albumArtistCredits(ctx context.Context, albumIDs []string) (map[string][]ArtistCredit, error) {
	rows, err := r.q.ListAlbumArtistCredits(ctx, albumIDs)
	if err != nil {
		return nil, fmt.Errorf("read Album Artists for IDs %q: %w", albumIDs, err)
	}
	return groupBy(rows, func(row db.ListAlbumArtistCreditsRow) (string, ArtistCredit) {
		return row.AlbumID, ArtistCredit{ID: row.ID, Name: row.Name}
	}), nil
}

func toAlbum(id, title, artistID, artistName string, year *int64, releaseDate *string, genres string) Album {
	album := Album{ID: id, Title: title, ArtistID: artistID, ArtistName: artistName, ReleaseDate: releaseDate}
	album.Year = intPtr(year)
	album.Genres = decodeGenres(genres)
	return album
}

func toTracks(rows []db.LibraryTrackRead) []Track {
	tracks := make([]Track, 0, len(rows))
	for _, row := range rows {
		tracks = append(tracks, toTrack(row))
	}
	return tracks
}

func toTrack(row db.LibraryTrackRead) Track {
	track := Track{
		ID:           row.ID,
		Title:        row.Title,
		ArtistName:   row.ArtistName,
		AlbumID:      row.AlbumID,
		AlbumTitle:   row.AlbumTitle,
		DiscNo:       int(row.DiscNo),
		TrackNo:      intPtr(row.TrackNo),
		TrackTotal:   intPtr(row.TrackTotal),
		DiscTotal:    intPtr(row.DiscTotal),
		DurationMs:   int(row.DurationMs),
		Format:       row.Format,
		Codec:        deref(row.Codec),
		Container:    deref(row.Container),
		SampleFormat: deref(row.SampleFormat),
		SizeBytes:    row.SizeBytes,
		Genre:        row.Genre,
		SampleRateHz: int(row.SampleRateHz),
		BitDepth:     int(row.BitDepth),
		ChannelCount: int(row.ChannelCount),
		BitrateBps:   int(row.BitrateBps),
		ReplayGain: ReplayGainMetadata{
			TrackGainDB: row.ReplaygainTrackGainDb,
			TrackPeak:   row.ReplaygainTrackPeak,
			AlbumGainDB: row.ReplaygainAlbumGainDb,
			AlbumPeak:   row.ReplaygainAlbumPeak,
		},
		TitleSort:     row.TitleSort,
		FilePath:      row.FilePath,
		FileMtime:     row.FileMtime,
		ContentSHA256: deref(row.ContentSha256),
		IdentityKey:   deref(row.IdentityKey),
		Revision:      int(row.Revision),
		CreatedAt:     row.CreatedAt,
		UpdatedAt:     row.UpdatedAt,
	}
	if track.BitrateBps > 0 {
		track.BitrateKbps = track.BitrateBps / 1000
	} else {
		enrichTrackBitrate(&track)
	}
	return track
}

func groupBy[R, T any](rows []R, split func(R) (string, T)) map[string][]T {
	grouped := make(map[string][]T)
	for _, row := range rows {
		key, value := split(row)
		grouped[key] = append(grouped[key], value)
	}
	return grouped
}

func nonNil[T any](items []T) []T {
	if items == nil {
		return []T{}
	}
	return items
}

func intPtr(value *int64) *int {
	if value == nil {
		return nil
	}
	result := int(*value)
	return &result
}

func deref(value *string) string {
	if value == nil {
		return ""
	}
	return *value
}
