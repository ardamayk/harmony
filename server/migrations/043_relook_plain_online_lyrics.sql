-- +goose Up
-- Lookups now also search LRCLIB for a synced edition of the same length
-- (ADR 0029); plain lyrics stored before that are dropped so the next
-- request looks again. Instrumentals (empty) and synced lyrics stay.
DELETE FROM track_online_lyrics
WHERE outcome = 'found' AND lyrics <> '' AND lyrics NOT GLOB '*[[][0-9]*';

-- +goose Down
SELECT 1;
