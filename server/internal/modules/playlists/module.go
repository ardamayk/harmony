package playlists

import (
	"database/sql"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/go-chi/chi/v5"
)

type Module struct {
	handler *Handler
}

func NewModule(sqlDB *sql.DB, tracks library.TrackReader) *Module {
	service := NewService(NewRepository(sqlDB), tracks)
	return &Module{handler: NewHandler(service)}
}

func (m *Module) Name() string {
	return "playlists"
}

func (m *Module) RegisterRoutes(r chi.Router) {
	r.Get("/api/v1/playlists", m.handler.ListPlaylists)
	r.Post("/api/v1/playlists", m.handler.CreatePlaylist)
	r.Get("/api/v1/playlists/{playlistId}", m.handler.GetPlaylist)
	r.Post("/api/v1/playlists/{playlistId}/tracks", m.handler.AddTrack)
	r.Delete("/api/v1/playlists/{playlistId}/tracks/{trackId}", m.handler.RemoveTrack)
}
