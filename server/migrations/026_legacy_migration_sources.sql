-- +goose Up
-- Durable record of every Legacy Track source that a Library Migration cutover
-- activated as a Managed Track. The cutover deletes both the Legacy Track and
-- its migration copy, so this table is the only proof that a legacy source
-- file corresponds to a successfully migrated Managed Track. Only rows here
-- may ever be offered for source cleanup; removing the Managed Track removes
-- the proof, which keeps the legacy source untouchable once it is the last
-- remaining copy.
CREATE TABLE legacy_migration_sources (
    track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    source_track_id TEXT NOT NULL,
    source_file_path TEXT NOT NULL,
    source_sha256 TEXT NOT NULL CHECK (
        length(source_sha256) = 64 AND source_sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    migrated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    cleaned_at DATETIME
);

CREATE INDEX idx_legacy_migration_sources_pending_cleanup
    ON legacy_migration_sources(source_file_path)
    WHERE cleaned_at IS NULL;

-- +goose Down
DROP INDEX IF EXISTS idx_legacy_migration_sources_pending_cleanup;
DROP TABLE IF EXISTS legacy_migration_sources;
