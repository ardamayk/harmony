-- +goose Up
-- A dismissed Soulseek Acquisition is gone for the listener but kept while
-- it still holds slskd transfers, so the worker can remove them; it is
-- deleted once none is left.
ALTER TABLE soulseek_acquisitions ADD COLUMN dismissed INTEGER NOT NULL DEFAULT 0
    CHECK (dismissed IN (0, 1));

-- +goose Down
ALTER TABLE soulseek_acquisitions DROP COLUMN dismissed;
