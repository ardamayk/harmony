// Package librarysearch evaluates one coherent Library Search behind the
// Music Server HTTP boundary. It consumes the prepared representations of the
// searchdata package and owns matching, ranking, typo fallback, related
// results, and Best Match selection.
package librarysearch

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
	return "librarysearch"
}

func (m *Module) RegisterRoutes(r chi.Router) {
	r.Get("/api/v1/library/search", m.handler.Search)
}
