package playback

import (
	"context"
	"database/sql"
	"fmt"
	"slices"

	"github.com/ardam/navidrome-replacement/server/internal/modules/playback/db"
	"github.com/google/uuid"
)

// Repository persists each user's queue. Every mutation runs in one
// transaction that first advances the queue revision, so a stale
// expectedRevision fails with ErrRevisionConflict before anything changes.
type Repository struct {
	sqlDB *sql.DB
	q     *db.Queries
}

type queueItemRow struct {
	ID       string
	TrackID  string
	Position int
	Source   string
}

type queueSnapshot struct {
	Items         []queueItemRow
	Revision      string
	EventSequence string
}

func NewRepository(sqlDB *sql.DB) *Repository {
	return &Repository{sqlDB: sqlDB, q: db.New(sqlDB)}
}

func (r *Repository) Read(ctx context.Context, userID string) (queueSnapshot, error) {
	tx, err := r.sqlDB.BeginTx(ctx, &sql.TxOptions{ReadOnly: true})
	if err != nil {
		return queueSnapshot{}, fmt.Errorf("begin queue read: %w", err)
	}
	defer func() { _ = tx.Rollback() }()
	q := r.q.WithTx(tx)

	state, err := q.GetQueueState(ctx, userID)
	if err != nil {
		return queueSnapshot{}, fmt.Errorf("get queue revision: %w", err)
	}
	rows, err := q.ListQueueItems(ctx, userID)
	if err != nil {
		return queueSnapshot{}, fmt.Errorf("get queue: %w", err)
	}
	if err := tx.Commit(); err != nil {
		return queueSnapshot{}, err
	}
	items := make([]queueItemRow, 0, len(rows))
	for _, row := range rows {
		items = append(items, queueItemRow{ID: row.ID, TrackID: row.TrackID, Position: int(row.Position), Source: row.Source})
	}
	return queueSnapshot{Items: items, Revision: state.Revision, EventSequence: state.EventSequence}, nil
}

func (r *Repository) Replace(ctx context.Context, userID, expectedRevision string, trackIDs []string, sourceJSON string) error {
	return r.mutate(ctx, userID, expectedRevision, func(q *db.Queries) error {
		if err := q.ClearQueue(ctx, userID); err != nil {
			return err
		}
		for position, trackID := range trackIDs {
			if err := insertItem(ctx, q, userID, position, trackID, sourceJSON); err != nil {
				return err
			}
		}
		return nil
	})
}

func (r *Repository) Append(ctx context.Context, userID, expectedRevision, trackID, sourceJSON string, maxItems int) error {
	return r.mutate(ctx, userID, expectedRevision, func(q *db.Queries) error {
		slot, err := q.QueueAppendSlot(ctx, userID)
		if err != nil {
			return err
		}
		if slot.ItemCount >= int64(maxItems) {
			return ErrQueueLimitExceeded
		}
		return insertItem(ctx, q, userID, int(slot.NextPosition), trackID, sourceJSON)
	})
}

// Remove deletes the item and closes the gap in positions.
func (r *Repository) Remove(ctx context.Context, userID, expectedRevision, itemID string) error {
	return r.mutate(ctx, userID, expectedRevision, func(q *db.Queries) error {
		deleted, err := q.DeleteQueueItem(ctx, db.DeleteQueueItemParams{ID: itemID, UserID: userID})
		if err != nil {
			return err
		}
		if deleted == 0 {
			return ErrNotFound
		}
		remaining, err := q.ListQueueItemIDs(ctx, userID)
		if err != nil {
			return err
		}
		return setPositions(ctx, q, userID, remaining)
	})
}

// Clear deletes every item except keepItemID, which keeps its ID and moves to
// position 0. An empty keepItemID clears the whole queue.
func (r *Repository) Clear(ctx context.Context, userID, expectedRevision, keepItemID string) error {
	return r.mutate(ctx, userID, expectedRevision, func(q *db.Queries) error {
		if keepItemID == "" {
			return q.ClearQueue(ctx, userID)
		}
		current, err := q.ListQueueItemIDs(ctx, userID)
		if err != nil {
			return err
		}
		if !slices.Contains(current, keepItemID) {
			return ErrNotFound
		}
		if err := q.DeleteQueueItemsExcept(ctx, db.DeleteQueueItemsExceptParams{UserID: userID, ID: keepItemID}); err != nil {
			return err
		}
		return setPositions(ctx, q, userID, []string{keepItemID})
	})
}

// Reorder applies itemIDs as the new order once accept approves the current IDs.
func (r *Repository) Reorder(ctx context.Context, userID, expectedRevision string, itemIDs []string, accept func(current []string) bool) error {
	return r.mutate(ctx, userID, expectedRevision, func(q *db.Queries) error {
		current, err := q.ListQueueItemIDs(ctx, userID)
		if err != nil {
			return err
		}
		if !accept(current) {
			return ErrInvalidQueueOrder
		}
		if err := q.ParkQueuePositions(ctx, userID); err != nil {
			return err
		}
		return setPositions(ctx, q, userID, itemIDs)
	})
}

func (r *Repository) mutate(ctx context.Context, userID, expectedRevision string, apply func(*db.Queries) error) error {
	tx, err := r.sqlDB.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback() }()
	q := r.q.WithTx(tx)

	if err = q.EnsureQueueState(ctx, userID); err != nil {
		return err
	}
	advanced, err := q.AdvanceQueueRevision(ctx, db.AdvanceQueueRevisionParams{UserID: userID, ExpectedRevision: expectedRevision})
	if err != nil {
		return err
	}
	if advanced == 0 {
		return ErrRevisionConflict
	}
	if err := apply(q); err != nil {
		return err
	}
	return tx.Commit()
}

func insertItem(ctx context.Context, q *db.Queries, userID string, position int, trackID, sourceJSON string) error {
	return q.InsertQueueItem(ctx, db.InsertQueueItemParams{
		ID: uuid.NewString(), UserID: userID, Position: int64(position), TrackID: trackID, Source: sourceJSON,
	})
}

func setPositions(ctx context.Context, q *db.Queries, userID string, itemIDs []string) error {
	for position, itemID := range itemIDs {
		if err := q.SetQueueItemPosition(ctx, db.SetQueueItemPositionParams{
			Position: int64(position), ID: itemID, UserID: userID,
		}); err != nil {
			return err
		}
	}
	return nil
}
