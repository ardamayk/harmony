-- +goose Up
-- A server-owned Import Batch (a Soulseek Acquisition's) has no Playback
-- Client: it is exempt from client-liveness and restart cleanup and lives
-- until it is confirmed or cancelled (ADR 0036, an exception to ADR 0018).
ALTER TABLE managed_import_batches ADD COLUMN owner TEXT NOT NULL DEFAULT 'client'
    CHECK (owner IN ('client', 'server'));

-- +goose Down
ALTER TABLE managed_import_batches DROP COLUMN owner;
