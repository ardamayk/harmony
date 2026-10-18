-- +goose Up
ALTER TABLE managed_import_jobs ADD COLUMN validation_issues TEXT NOT NULL DEFAULT '[]';
ALTER TABLE managed_import_history_files ADD COLUMN validation_issues TEXT NOT NULL DEFAULT '[]';

-- +goose Down
ALTER TABLE managed_import_history_files DROP COLUMN validation_issues;
ALTER TABLE managed_import_jobs DROP COLUMN validation_issues;
