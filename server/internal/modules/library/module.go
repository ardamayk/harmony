package library

import (
	"database/sql"

	"github.com/go-chi/chi/v5"
)

type Module struct {
	handler *Handler
	service *Service
	repo    *Repository
}

// NewModule wires library reads. Managed Import is the only ingestion path.
func NewModule(sqlDB *sql.DB) *Module {
	repo := NewRepository(sqlDB)
	service := NewService(repo)
	return &Module{handler: NewHandler(service), service: service, repo: repo}
}

func (m *Module) Name() string {
	return "library"
}

func (m *Module) RegisterRoutes(r chi.Router) {
	r.Get("/api/v1/library/artists", m.handler.ListArtists)
	r.Get("/api/v1/library/albums", m.handler.ListAlbums)
	r.Get("/api/v1/library/albums/{albumId}/cover", m.handler.GetAlbumCover)
	r.Get("/api/v1/library/albums/{albumId}", m.handler.GetAlbum)
	r.Get("/api/v1/library/tracks", m.handler.ListTracks)
	r.Get("/api/v1/library/tracks/{trackId}", m.handler.GetTrack)
	r.Get("/api/v1/library/tracks/{trackId}/lyrics", m.handler.GetTrackLyrics)
	r.Get("/api/v1/library/tracks/{trackId}/waveform", m.handler.GetTrackWaveform)
}

// EnableWaveforms wires lazy waveform generation; `available` reflects the
// ffmpeg Server Dependency and gates both generation and the capability.
func (m *Module) EnableWaveforms(available bool) {
	m.handler.waveforms = NewWaveformService(m.repo, FFmpegWaveformGenerator, available)
}

func (m *Module) TrackAccess() TrackAccess {
	return m.service
}
