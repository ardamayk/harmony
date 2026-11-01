-- +goose Up
-- The Shared Playback State: where the user's playback is, kept on the
-- server so every client follows one Track and playhead (ADR 0040). One row
-- per user. queue_item_id has no foreign key: it may outlive the Queue item
-- it named. track_id keeps one so a deleted Track cannot leave a dangling
-- reference; the devices re-resolve it.
CREATE TABLE IF NOT EXISTS playback_state (
    user_id TEXT PRIMARY KEY,
    queue_item_id TEXT,
    track_id TEXT REFERENCES tracks(id) ON DELETE SET NULL,
    playing INTEGER NOT NULL DEFAULT 0 CHECK (playing IN (0, 1)),
    position_ms INTEGER NOT NULL DEFAULT 0 CHECK (position_ms >= 0),
    position_at_ms INTEGER NOT NULL DEFAULT 0 CHECK (position_at_ms >= 0),
    last_report_at_ms INTEGER NOT NULL DEFAULT 0 CHECK (last_report_at_ms >= 0),
    active_device_id TEXT,
    active_device_name TEXT,
    revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
    remote_command_revision INTEGER NOT NULL DEFAULT 0 CHECK (remote_command_revision >= 0),
    skip_revision INTEGER CHECK (skip_revision IS NULL OR skip_revision >= 0),
    skip_direction TEXT CHECK (skip_direction IS NULL OR skip_direction IN ('next', 'previous'))
);

-- +goose Down
DROP TABLE IF EXISTS playback_state;
