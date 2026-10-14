package searchdata

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"

	"github.com/ardam/navidrome-replacement/server/internal/searchdata/db"
)

type Field struct {
	Name      string
	RelatedID string
	Position  int
	Text      Text
}

type Document struct {
	Kind   string
	ID     string
	Fields []Field
}

// Repository reads prepared Library Search documents and rebuilds derived
// text data. It never writes original library records.
type Repository struct {
	sqlDB *sql.DB
	q     *db.Queries
}

func NewRepository(sqlDB *sql.DB) *Repository {
	return &Repository{sqlDB: sqlDB, q: db.New(sqlDB)}
}

// GetDocument reads prepared fields with live visibility and ownership rules.
// Relationships retain their source identity and credit order; no name splitting
// or query-time normalization of library records occurs here.
func (r *Repository) GetDocument(ctx context.Context, kind, id, userID string) (Document, error) {
	rows, err := r.q.ListDocumentFields(ctx, db.ListDocumentFieldsParams{Kind: kind, ID: id, UserID: userID})
	if err != nil {
		return Document{}, fmt.Errorf("read Library Search %s %q: %w", kind, id, err)
	}
	if len(rows) == 0 {
		return Document{}, sql.ErrNoRows
	}
	document := Document{Kind: kind, ID: id, Fields: make([]Field, 0, len(rows))}
	for _, row := range rows {
		field := Field{Name: row.Field, RelatedID: row.RelatedID, Position: int(row.Position)}
		if err := json.Unmarshal([]byte(row.TextJson), &field.Text); err != nil {
			return Document{}, fmt.Errorf("read Library Search %s %q field: decode prepared field: %w", kind, id, err)
		}
		document.Fields = append(document.Fields, field)
	}
	return document, nil
}
