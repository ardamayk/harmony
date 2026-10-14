package preferences

import (
	"database/sql"

	"github.com/go-chi/chi/v5"
)

type Module struct {
	handler *Handler
}

func NewModule(sqlDB *sql.DB) *Module {
	return &Module{handler: NewHandler(NewService(NewRepository(sqlDB)))}
}

func (m *Module) Name() string {
	return "preferences"
}

func (m *Module) RegisterRoutes(r chi.Router) {
	r.Get("/api/v1/preferences", m.handler.Get)
	r.Patch("/api/v1/preferences", m.handler.Patch)
}
