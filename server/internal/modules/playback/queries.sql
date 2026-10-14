-- name: GetQueueState :one
SELECT
    CAST(COALESCE((SELECT revision FROM playback_queue_state s WHERE s.user_id = sqlc.arg(user_id)), 0) AS TEXT) AS revision,
    CAST(COALESCE((SELECT event_sequence FROM playback_queue_state s WHERE s.user_id = sqlc.arg(user_id)), 0) AS TEXT) AS event_sequence;

-- name: EnsureQueueState :exec
INSERT OR IGNORE INTO playback_queue_state (user_id, revision) VALUES (?, 0);

-- name: AdvanceQueueRevision :execrows
UPDATE playback_queue_state
SET revision = revision + 1, event_sequence = event_sequence + 1
WHERE user_id = ? AND CAST(revision AS TEXT) = CAST(sqlc.arg(expected_revision) AS TEXT);

-- name: ListQueueItems :many
SELECT id, track_id, position, source FROM playback_queue
WHERE user_id = ?
ORDER BY position;

-- name: ListQueueItemIDs :many
SELECT id FROM playback_queue WHERE user_id = ? ORDER BY position;

-- name: QueueAppendSlot :one
SELECT COUNT(*) AS item_count, CAST(COALESCE(MAX(position) + 1, 0) AS INTEGER) AS next_position
FROM playback_queue WHERE user_id = ?;

-- name: InsertQueueItem :exec
INSERT INTO playback_queue (id, user_id, position, track_id, source) VALUES (?, ?, ?, ?, ?);

-- name: ClearQueue :exec
DELETE FROM playback_queue WHERE user_id = ?;

-- name: DeleteQueueItem :execrows
DELETE FROM playback_queue WHERE id = ? AND user_id = ?;

-- name: ParkQueuePositions :exec
-- Moves every position below zero so renumbering cannot hit UNIQUE(user_id, position).
UPDATE playback_queue SET position = -position - 1 WHERE user_id = ?;

-- name: SetQueueItemPosition :exec
UPDATE playback_queue SET position = ? WHERE id = ? AND user_id = ?;
