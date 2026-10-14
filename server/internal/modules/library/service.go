package library

import "context"

// Service exposes read operations over the library. Ingestion, deletion and
// replacement are owned by Managed Import.
type Service struct {
	repo *Repository
}

func NewService(repo *Repository) *Service {
	return &Service{repo: repo}
}

func (s *Service) ListArtists(ctx context.Context, limit, offset int, q string) (ArtistList, error) {
	return s.repo.ListArtists(ctx, limit, offset, q)
}

func (s *Service) ListAlbums(ctx context.Context, limit, offset int, artistID, q string) (AlbumList, error) {
	return s.repo.ListAlbums(ctx, limit, offset, artistID, q)
}

func (s *Service) GetAlbum(ctx context.Context, albumID string) (AlbumDetail, error) {
	return s.repo.GetAlbum(ctx, albumID)
}

func (s *Service) ListTracks(ctx context.Context, limit, offset int, artistID, q string) (TrackList, error) {
	return s.repo.ListTracks(ctx, limit, offset, artistID, q)
}

func (s *Service) GetAlbumCover(ctx context.Context, albumID string) (string, []byte, error) {
	return s.repo.GetAlbumCover(ctx, albumID)
}

func (s *Service) GetTrack(ctx context.Context, trackID string) (Track, error) {
	return s.repo.GetTrack(ctx, trackID)
}

func (s *Service) GetTrackLyrics(ctx context.Context, trackID string) (TrackLyrics, error) {
	lyrics, err := s.repo.GetTrackLyrics(ctx, trackID)
	if err != nil {
		return TrackLyrics{}, err
	}
	return TrackLyrics{TrackID: trackID, Lyrics: lyrics}, nil
}

func (s *Service) GetTrackFilePath(ctx context.Context, trackID string) (string, error) {
	return s.repo.GetTrackFilePath(ctx, trackID)
}
