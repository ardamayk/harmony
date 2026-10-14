package radio

import (
	"database/sql"
	"net/http"

	"github.com/ardam/navidrome-replacement/server/internal/config"
	"github.com/go-chi/chi/v5"
)

type Module struct {
	handler *Handler
}

func NewModule(sqlDB *sql.DB, cfg config.Config) *Module {
	service := NewService(NewRepository(sqlDB))
	cache := NewNowPlayingCache()
	searcher := NewRadioBrowserClient(cfg.RadioBrowserBaseURL, http.DefaultClient)
	streamer := NewStreamProxy(service, cache)
	return &Module{handler: NewHandler(service, searcher, streamer, cache)}
}

func (m *Module) Name() string {
	return "radio"
}

func (m *Module) RegisterRoutes(r chi.Router) {
	r.Get("/api/v1/radio/stations", m.handler.ListStations)
	r.Post("/api/v1/radio/stations", m.handler.CreateStation)
	r.Get("/api/v1/radio/stations/{stationId}", m.handler.GetStation)
	r.Patch("/api/v1/radio/stations/{stationId}", m.handler.PatchStation)
	r.Delete("/api/v1/radio/stations/{stationId}", m.handler.DeleteStation)
	r.Get("/api/v1/radio/search", m.handler.SearchStations)
	r.Get("/api/v1/radio/catalog/countries", m.handler.ListCatalogCountries)
	r.Get("/api/v1/radio/catalog/tags", m.handler.ListCatalogTags)
	r.Post("/api/v1/radio/import", m.handler.ImportStation)
	r.Post("/api/v1/radio/preview", m.handler.PreviewStation)
	r.Get("/api/v1/radio/preview/{stationUuid}/stream", m.handler.StreamPreviewStation)
	r.Get("/api/v1/radio/stations/{stationId}/stream", m.handler.StreamStation)
	r.Get("/api/v1/radio/stations/{stationId}/now-playing", m.handler.GetNowPlaying)
}
