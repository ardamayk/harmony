package managedimport

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"time"

	"github.com/ardam/navidrome-replacement/server/internal/modules/managedimport/db"
)

func (r *Repository) ListHistory(ctx context.Context) (HistoryList, error) {
	var history HistoryList
	err := r.inReadTx(ctx, "Import History snapshot", func(q *db.Queries) error {
		rows, err := q.ListHistory(ctx, IMPORT_HISTORY_LIMIT)
		if err != nil {
			return fmt.Errorf("list Import History: %w", err)
		}
		history.Items = make([]HistoryItem, 0, len(rows))
		for _, row := range rows {
			item := HistoryItem{
				ImportID: row.ImportID, StartedAt: row.StartedAt, CompletedAt: row.CompletedAt,
				ResultCode: HistoryResultCode(row.ResultCode),
				Counts: HistoryCounts{
					Total: int(row.TotalCount), Imported: int(row.ImportedCount), Rejected: int(row.RejectedCount),
					Failed: int(row.FailedCount), Replaced: int(row.ReplacedCount),
					NotAttempted: int(row.NotAttemptedCount), Canceled: int(row.CanceledCount),
				},
			}
			if item.Files, err = listHistoryFiles(ctx, q, item.ImportID); err != nil {
				return err
			}
			history.Items = append(history.Items, item)
		}
		return nil
	})
	return history, err
}

func listHistoryFiles(ctx context.Context, q *db.Queries, importID string) ([]HistoryFile, error) {
	rows, err := q.ListHistoryFiles(ctx, importID)
	if err != nil {
		return nil, fmt.Errorf("list Import History files for %q: %w", importID, err)
	}
	files := make([]HistoryFile, 0, len(rows))
	for _, row := range rows {
		issues, err := decodeValidationIssues(row.ValidationIssues)
		if err != nil {
			return nil, fmt.Errorf("read Import History file: %w", err)
		}
		files = append(files, HistoryFile{
			Issues: issues, FileID: row.FileID, JobID: row.JobID, SafeFilename: deref(row.SafeFilename),
			StartedAt: row.StartedAt, CompletedAt: row.CompletedAt, ContentSHA256: deref(row.ContentSha256),
			ResultCode: row.ResultCode, CreatedTrackID: deref(row.CreatedTrackID), ReplacedTrackID: deref(row.ReplacedTrackID),
		})
	}
	return files, nil
}

// historySource is a Job or canceled file as it stood when its import ended.
type historySource struct {
	file      HistoryFile
	status    ImportStatus
	outcome   ImportOutcome
	errorCode string
	trackID   string
}

func newHistorySource(fileID, jobID string, safeFilename *string, startedAt, completedAt time.Time, contentSHA256 *string,
	status string, outcome, errorCode, trackID *string, issues string) (historySource, error) {
	decoded, err := decodeValidationIssues(issues)
	if err != nil {
		return historySource{}, fmt.Errorf("read terminal Managed Import file: %w", err)
	}
	return historySource{
		file: HistoryFile{
			Issues: decoded, FileID: fileID, JobID: jobID, SafeFilename: deref(safeFilename),
			StartedAt: startedAt, CompletedAt: completedAt, ContentSHA256: deref(contentSHA256),
		},
		status: ImportStatus(status), outcome: ImportOutcome(deref(outcome)),
		errorCode: deref(errorCode), trackID: deref(trackID),
	}, nil
}

func archiveBatchHistory(ctx context.Context, q *db.Queries, batchID string, terminalCode HistoryResultCode) error {
	item := HistoryItem{ImportID: batchID, CompletedAt: time.Now().UTC()}
	startedAt, err := q.GetBatchCreatedAt(ctx, batchID)
	if err != nil {
		return fmt.Errorf("read terminal Managed Import Batch %q: %w", batchID, err)
	}
	item.StartedAt = startedAt
	rows, err := q.ListBatchHistorySources(ctx, &batchID)
	if err != nil {
		return fmt.Errorf("read terminal Managed Import Batch files: %w", err)
	}
	item.Files = []HistoryFile{}
	for _, row := range rows {
		source, err := newHistorySource(row.FileID, row.JobID, row.SafeFilename, row.StartedAt, row.CompletedAt,
			row.ContentSha256, row.Status, row.Outcome, row.ErrorCode, row.TrackID, row.ValidationIssues)
		if err != nil {
			return err
		}
		applyHistoryFileResult(&source, terminalCode, item.CompletedAt)
		incrementHistoryCounts(&item.Counts, source, terminalCode)
		item.Files = append(item.Files, source.file)
	}
	item.ResultCode = historyResultCode(item.Counts, terminalCode)
	return insertHistory(ctx, q, item)
}

func archiveStandaloneHistory(ctx context.Context, q *db.Queries, jobID string, terminalCode HistoryResultCode) error {
	row, err := q.GetStandaloneHistorySource(ctx, jobID)
	if errors.Is(err, sql.ErrNoRows) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("read terminal Managed Import file: %w", err)
	}
	source, err := newHistorySource(row.FileID, row.JobID, row.SafeFilename, row.StartedAt, row.CompletedAt,
		row.ContentSha256, row.Status, row.Outcome, row.ErrorCode, row.TrackID, row.ValidationIssues)
	if err != nil {
		return err
	}
	applyHistoryFileResult(&source, terminalCode, source.file.CompletedAt)
	counts := HistoryCounts{}
	incrementHistoryCounts(&counts, source, terminalCode)
	item := HistoryItem{
		ImportID: jobID, StartedAt: source.file.StartedAt, CompletedAt: source.file.CompletedAt,
		ResultCode: historyResultCode(counts, terminalCode), Counts: counts, Files: []HistoryFile{source.file},
	}
	if err := insertHistory(ctx, q, item); err != nil {
		return err
	}
	// A canceled standalone job is deleted by the caller in the same transaction; clearing its staged path
	// here would violate the awaiting_confirmation CHECK constraint, so only terminal rows are cleared.
	if err := q.ClearStandaloneTerminalPayloads(ctx, jobID); err != nil {
		return fmt.Errorf("clear terminal standalone Import payloads: %w", err)
	}
	return nil
}

func applyHistoryFileResult(source *historySource, terminalCode HistoryResultCode, terminalAt time.Time) {
	file := &source.file
	if isCanceledHistoryFile(*source, terminalCode) {
		file.ResultCode = string(HISTORY_RESULT_CANCELED)
		if source.errorCode != IMPORT_CANCELED_RESULT_CODE {
			file.CompletedAt = terminalAt
		}
		return
	}
	file.ResultCode = string(source.outcome)
	if source.status == STATUS_COMMITTED && file.ResultCode == "" {
		file.ResultCode = string(OUTCOME_IMPORTED)
	} else if file.ResultCode == "" {
		file.ResultCode = string(source.status)
	}
	if source.errorCode != "" && source.outcome != OUTCOME_IMPORTED && source.outcome != OUTCOME_REPLACED {
		file.ResultCode = source.errorCode
	}
	if source.outcome == OUTCOME_REPLACED {
		file.ReplacedTrackID = source.trackID
	} else if source.status == STATUS_COMMITTED {
		file.CreatedTrackID = source.trackID
	}
}

func incrementHistoryCounts(counts *HistoryCounts, source historySource, terminalCode HistoryResultCode) {
	counts.Total++
	if isCanceledHistoryFile(source, terminalCode) {
		counts.Canceled++
		return
	}
	switch outcome := source.outcome; {
	case outcome == OUTCOME_IMPORTED || outcome == "" && source.status == STATUS_COMMITTED:
		counts.Imported++
	case outcome == OUTCOME_REJECTED:
		counts.Rejected++
	case outcome == OUTCOME_REPLACED:
		counts.Replaced++
	case outcome == OUTCOME_NOT_ATTEMPTED:
		counts.NotAttempted++
	default:
		counts.Failed++
	}
}

func isCanceledHistoryFile(source historySource, terminalCode HistoryResultCode) bool {
	if source.errorCode == IMPORT_CANCELED_RESULT_CODE {
		return true
	}
	return terminalCode == HISTORY_RESULT_CANCELED && source.outcome == "" &&
		source.status != STATUS_COMMITTED && source.status != STATUS_FAILED
}

func historyResultCode(counts HistoryCounts, terminalCode HistoryResultCode) HistoryResultCode {
	if terminalCode == HISTORY_RESULT_CANCELED {
		return terminalCode
	}
	succeeded := counts.Imported + counts.Replaced
	if succeeded == counts.Total {
		return HISTORY_RESULT_COMPLETED
	}
	if succeeded > 0 {
		return HISTORY_RESULT_PARTIALLY_COMPLETED
	}
	return HISTORY_RESULT_FAILED
}

// insertHistory keeps the first archive of an import and prunes history to its limit.
func insertHistory(ctx context.Context, q *db.Queries, item HistoryItem) error {
	inserted, err := q.InsertHistory(ctx, db.InsertHistoryParams{
		ImportID: item.ImportID, StartedAt: item.StartedAt, CompletedAt: item.CompletedAt,
		ResultCode: string(item.ResultCode), TotalCount: int64(item.Counts.Total),
		ImportedCount: int64(item.Counts.Imported), RejectedCount: int64(item.Counts.Rejected),
		FailedCount: int64(item.Counts.Failed), ReplacedCount: int64(item.Counts.Replaced),
		NotAttemptedCount: int64(item.Counts.NotAttempted), CanceledCount: int64(item.Counts.Canceled),
	})
	if err != nil {
		return fmt.Errorf("store Import History %q: %w", item.ImportID, err)
	}
	if inserted == 0 {
		return nil
	}
	for position, file := range item.Files {
		issues, err := file.Issues.encode()
		if err != nil {
			return fmt.Errorf("store Import History file %q: %w", file.FileID, err)
		}
		if err := q.InsertHistoryFile(ctx, db.InsertHistoryFileParams{
			ImportID: item.ImportID, FileID: file.FileID, JobID: file.JobID, SafeFilename: file.SafeFilename,
			StartedAt: file.StartedAt, CompletedAt: file.CompletedAt, ContentSha256: file.ContentSHA256,
			ResultCode: file.ResultCode, CreatedTrackID: file.CreatedTrackID, ReplacedTrackID: file.ReplacedTrackID,
			Position: int64(position), ValidationIssues: issues,
		}); err != nil {
			return fmt.Errorf("store Import History file %q: %w", file.FileID, err)
		}
	}
	if err := q.PruneHistory(ctx, IMPORT_HISTORY_LIMIT); err != nil {
		return fmt.Errorf("prune Import History: %w", err)
	}
	return nil
}
