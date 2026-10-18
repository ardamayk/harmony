-- +goose Up
-- Durable per-phase marker for the Library Migration cutover, mirroring the
-- Managed Import commit journal: 'promoted' records that the copy is being
-- moved to the Canonical Library Path so a restart can restore it instead of
-- duplicating or losing managed bytes.
ALTER TABLE legacy_migration_copies ADD COLUMN phase TEXT NOT NULL DEFAULT 'prepared'
    CHECK (phase IN ('prepared', 'verified', 'promoted'));

UPDATE legacy_migration_copies SET phase = 'verified' WHERE status = 'verified';

CREATE INDEX idx_legacy_migration_copies_promoted
    ON legacy_migration_copies(source_track_id)
    WHERE phase = 'promoted';

-- +goose Down
DROP INDEX IF EXISTS idx_legacy_migration_copies_promoted;
ALTER TABLE legacy_migration_copies DROP COLUMN phase;
