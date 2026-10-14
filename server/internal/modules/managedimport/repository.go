package managedimport

import (
	"context"
	"database/sql"
	"errors"
	"fmt"

	"github.com/ardam/navidrome-replacement/server/internal/modules/managedimport/db"
)

// Repository owns every Managed Import database read and write. Multi-step
// state transitions run in one transaction; the database begins transactions
// IMMEDIATE, so each holds the write lock from its first statement.
type Repository struct {
	sqlDB *sql.DB
	q     *db.Queries
}

func NewRepository(sqlDB *sql.DB) *Repository {
	return &Repository{sqlDB: sqlDB, q: db.New(sqlDB)}
}

// inTx runs apply in a transaction and joins a failed rollback into the error.
func (r *Repository) inTx(ctx context.Context, operation string, apply func(q *db.Queries) error) (returnErr error) {
	transaction, err := r.sqlDB.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("begin %s: %w", operation, err)
	}
	defer func() { returnErr = errors.Join(returnErr, rollbackTransaction(transaction, operation)) }()
	if err := apply(r.q.WithTx(transaction)); err != nil {
		return err
	}
	if err := transaction.Commit(); err != nil {
		return fmt.Errorf("commit %s: %w", operation, err)
	}
	return nil
}

// inReadTx runs read in a read-only snapshot.
func (r *Repository) inReadTx(ctx context.Context, operation string, read func(q *db.Queries) error) (returnErr error) {
	transaction, err := r.sqlDB.BeginTx(ctx, &sql.TxOptions{ReadOnly: true})
	if err != nil {
		return fmt.Errorf("begin %s: %w", operation, err)
	}
	defer func() { returnErr = errors.Join(returnErr, rollbackTransaction(transaction, operation)) }()
	if err := read(r.q.WithTx(transaction)); err != nil {
		return err
	}
	if err := transaction.Commit(); err != nil {
		return fmt.Errorf("commit %s: %w", operation, err)
	}
	return nil
}

func rollbackTransaction(transaction *sql.Tx, operation string) error {
	err := transaction.Rollback()
	if err == nil || errors.Is(err, sql.ErrTxDone) {
		return nil
	}
	return fmt.Errorf("rollback %s transaction: %w", operation, err)
}

// requireOne turns a guarded UPDATE/DELETE that matched no row into ErrInvalidState.
func requireOne(affected int64, err error) error {
	if err != nil {
		return err
	}
	if affected != 1 {
		return ErrInvalidState
	}
	return nil
}

func ptr[T any](value T) *T {
	return &value
}

func deref[T any](value *T) T {
	var zero T
	if value == nil {
		return zero
	}
	return *value
}

func boolInt(value bool) int64 {
	if value {
		return 1
	}
	return 0
}

// nullablePositive stores zero or negative counts as NULL.
func nullablePositive(value int) *int64 {
	if value <= 0 {
		return nil
	}
	return ptr(int64(value))
}
