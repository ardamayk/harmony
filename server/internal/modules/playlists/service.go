package playlists

import (
	"context"
	"fmt"
	"strings"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
)

// Service owns playlist rules: every user has a default Favorites playlist,
// and a user playlist disappears when its last track is removed.
type Service struct {
	repo   *Repository
	tracks library.TrackReader
}

func NewService(repo *Repository, tracks library.TrackReader) *Service {
	return &Service{repo: repo, tracks: tracks}
}

func (s *Service) GetDefaultFavorites(ctx context.Context, userID string) (Playlist, error) {
	return s.repo.EnsureDefault(ctx, userID)
}

func (s *Service) ListPlaylists(ctx context.Context, userID string) (PlaylistList, error) {
	if _, err := s.repo.EnsureDefault(ctx, userID); err != nil {
		return PlaylistList{}, err
	}
	items, err := s.repo.List(ctx, userID)
	if err != nil {
		return PlaylistList{}, err
	}
	return PlaylistList{Items: items, Total: len(items)}, nil
}

func (s *Service) CreatePlaylist(ctx context.Context, userID, name string) (Playlist, error) {
	name = strings.TrimSpace(name)
	if name == "" {
		return Playlist{}, fmt.Errorf("playlist name is required")
	}
	if _, err := s.repo.EnsureDefault(ctx, userID); err != nil {
		return Playlist{}, err
	}
	id, err := s.repo.Create(ctx, userID, name)
	if err != nil {
		return Playlist{}, err
	}
	return s.repo.Get(ctx, userID, id)
}

func (s *Service) GetPlaylist(ctx context.Context, userID, playlistID string) (PlaylistDetail, error) {
	if _, err := s.repo.EnsureDefault(ctx, userID); err != nil {
		return PlaylistDetail{}, err
	}
	playlist, err := s.repo.Get(ctx, userID, playlistID)
	if err != nil {
		return PlaylistDetail{}, err
	}
	trackIDs, err := s.repo.TrackIDs(ctx, playlistID)
	if err != nil {
		return PlaylistDetail{}, err
	}
	tracks := make([]library.Track, 0, len(trackIDs))
	for _, trackID := range trackIDs {
		track, err := s.tracks.GetTrack(ctx, trackID)
		if err != nil {
			continue
		}
		tracks = append(tracks, track)
	}
	return PlaylistDetail{Playlist: playlist, Tracks: tracks}, nil
}

func (s *Service) AddTrack(ctx context.Context, userID, playlistID, trackID string) (PlaylistDetail, error) {
	if _, err := s.GetPlaylist(ctx, userID, playlistID); err != nil {
		return PlaylistDetail{}, err
	}
	if _, err := s.tracks.GetTrack(ctx, trackID); err != nil {
		return PlaylistDetail{}, library.ErrNotFound
	}
	if err := s.repo.AppendTrack(ctx, playlistID, trackID); err != nil {
		return PlaylistDetail{}, err
	}
	return s.GetPlaylist(ctx, userID, playlistID)
}

func (s *Service) RemoveTrack(ctx context.Context, userID, playlistID, trackID string) (PlaylistDetail, error) {
	detail, err := s.GetPlaylist(ctx, userID, playlistID)
	if err != nil {
		return PlaylistDetail{}, err
	}
	if err = s.repo.RemoveTrack(ctx, playlistID, trackID); err != nil {
		return PlaylistDetail{}, err
	}
	if detail.IsDefault {
		return s.GetPlaylist(ctx, userID, playlistID)
	}
	remaining, err := s.repo.CountTracks(ctx, playlistID)
	if err != nil {
		return PlaylistDetail{}, err
	}
	if remaining > 0 {
		return s.GetPlaylist(ctx, userID, playlistID)
	}
	if err := s.repo.DeleteUserPlaylist(ctx, userID, playlistID); err != nil {
		return PlaylistDetail{}, err
	}
	detail.TrackCount = 0
	detail.Tracks = []library.Track{}
	return detail, nil
}
