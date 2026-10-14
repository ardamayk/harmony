package librarysearch

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"

	"github.com/ardam/navidrome-replacement/server/internal/modules/librarysearch/db"
)

// Repository reads the prepared Library Search data for one user.
type Repository struct {
	sqlDB *sql.DB
	q     *db.Queries
}

func NewRepository(sqlDB *sql.DB) *Repository {
	return &Repository{sqlDB: sqlDB, q: db.New(sqlDB)}
}

// LoadIndex reads the revision and all fields from one SQLite snapshot, so a
// published index never mixes pre- and post-commit data. When reuse accepts
// the current revision, it returns (nil, revision, nil) without reading fields.
func (r *Repository) LoadIndex(ctx context.Context, userID string, reuse func(revision int64) bool) (*Index, int64, error) {
	tx, err := r.sqlDB.BeginTx(ctx, &sql.TxOptions{ReadOnly: true})
	if err != nil {
		return nil, 0, err
	}
	defer func() { _ = tx.Rollback() }()
	q := r.q.WithTx(tx)

	revision, err := q.GetSearchRevision(ctx)
	if err != nil {
		return nil, 0, err
	}
	if reuse(revision) {
		return nil, revision, nil
	}
	texts, err := loadTexts(ctx, q)
	if err != nil {
		return nil, 0, err
	}
	sources, err := q.ListSearchFieldSources(ctx, userID)
	if err != nil {
		return nil, 0, fmt.Errorf("read Library Search fields: %w", err)
	}
	if err = tx.Commit(); err != nil {
		return nil, 0, err
	}
	index, err := buildIndex(sources, texts)
	return index, revision, err
}

func loadTexts(ctx context.Context, q *db.Queries) (map[entityKey]*preparedText, error) {
	rows, err := q.ListSearchTexts(ctx)
	if err != nil {
		return nil, fmt.Errorf("read Library Search texts: %w", err)
	}
	texts := make(map[entityKey]*preparedText, len(rows))
	for _, row := range rows {
		text := &preparedText{}
		if err := json.Unmarshal([]byte(row.TextJson), &text.Text); err != nil {
			return nil, fmt.Errorf("decode Library Search %s %q text: %w", row.Kind, row.ID, err)
		}
		text.faithfulWords = splitFaithful(text.Faithful)
		text.foldedWords = strings.Fields(text.Folded)
		text.compactWords = strings.Fields(text.Compact)
		texts[entityKey{row.Kind, row.ID}] = text
	}
	return texts, nil
}

func buildIndex(sources []db.ListSearchFieldSourcesRow, texts map[entityKey]*preparedText) (*Index, error) {
	index := &Index{byKey: map[entityKey]*entity{}}
	var current *entity
	for _, source := range sources {
		text, prepared := texts[entityKey{source.RelatedKind, source.RelatedID}]
		if !prepared {
			return nil, fmt.Errorf("prepared Library Search data for %s %q is missing; rebuild derived data", source.RelatedKind, source.RelatedID)
		}
		if current == nil || current.kind != source.Kind || current.id != source.ID {
			current = &entity{kind: source.Kind, id: source.ID}
			index.entities = append(index.entities, current)
			index.byKey[entityKey{source.Kind, source.ID}] = current
		}
		if source.Field == "primary" {
			current.primary = text
			continue
		}
		current.related = append(current.related, fieldRef{name: source.Field, relatedID: source.RelatedID, text: text})
	}
	for _, item := range index.entities {
		if item.primary == nil {
			return nil, fmt.Errorf("searchable %s %q has no primary name", item.kind, item.id)
		}
	}
	return index, nil
}
