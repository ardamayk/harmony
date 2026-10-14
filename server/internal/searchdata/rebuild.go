package searchdata

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
)

// Rebuild atomically replaces derived data. Original records and audio are never
// written, and readers retain the previous snapshot until the commit succeeds.
func (r *Repository) Rebuild(ctx context.Context) error {
	return r.rebuild(ctx, false)
}

// EnsureCurrent backfills missing records and upgrades normalization rules at
// startup, before the database is handed to request handlers.
func (r *Repository) EnsureCurrent(ctx context.Context) error {
	return r.rebuild(ctx, true)
}

func (r *Repository) rebuild(ctx context.Context, onlyIfNeeded bool) (returnErr error) {
	transaction, err := r.sqlDB.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("begin Library Search rebuild: %w", err)
	}
	defer func() {
		if err := transaction.Rollback(); err != nil && !errors.Is(err, sql.ErrTxDone) {
			returnErr = errors.Join(returnErr, fmt.Errorf("rollback Library Search rebuild: %w", err))
		}
	}()
	q := r.q.WithTx(transaction)
	if onlyIfNeeded {
		isCurrent, err := q.HasCurrentTexts(ctx, RULES_VERSION)
		if err != nil {
			return fmt.Errorf("check Library Search rules version: %w", err)
		}
		if isCurrent {
			return nil
		}
	}
	if err := q.DeleteTexts(ctx); err != nil {
		return fmt.Errorf("rebuild Library Search data: %w", err)
	}
	if err := q.InsertTextsFromSources(ctx); err != nil {
		return fmt.Errorf("rebuild Library Search data: %w", err)
	}
	if err := transaction.Commit(); err != nil {
		return fmt.Errorf("commit Library Search rebuild: %w", err)
	}
	return nil
}
