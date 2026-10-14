package playback

import (
	"database/sql"

	"github.com/ardam/navidrome-replacement/server/internal/modules/library"
	"github.com/go-chi/chi/v5"
)

type Module struct {
	handler *Handler
}

func NewModule(sqlDB *sql.DB, tracks library.TrackAccess, queueEventBrokers ...*QueueEventBroker) *Module {
	service := NewService(NewRepository(sqlDB), tracks)
	queueEvents := NewQueueEventBroker()
	if len(queueEventBrokers) > 0 {
		queueEvents = queueEventBrokers[0]
	}
	return &Module{handler: NewHandler(service, tracks, queueEvents)}
}

func (m *Module) Name() string {
	return "playback"
}

func (m *Module) RegisterRoutes(r chi.Router) {
	r.Get("/api/v1/playback/queue", m.handler.GetQueue)
	r.Put("/api/v1/playback/queue", m.handler.ReplaceQueue)
	r.Patch("/api/v1/playback/queue", m.handler.ReorderQueue)
	r.Post("/api/v1/playback/queue/items", m.handler.AppendItem)
	r.Delete("/api/v1/playback/queue/items/{itemId}", m.handler.RemoveItem)
	r.Get("/api/v1/playback/queue/events", m.handler.StreamQueueEvents)
	r.Get("/api/v1/tracks/{trackId}/stream", m.handler.StreamTrack)
	r.Head("/api/v1/tracks/{trackId}/stream", m.handler.StreamTrack)
}
