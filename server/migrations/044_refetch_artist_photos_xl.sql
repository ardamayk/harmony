-- +goose Up
-- Artist photos are now Deezer's 1000px picture, for the Artist page's
-- full-width banner (ADR 0027); the 500px ones stored so far are dropped so
-- the next request fetches the larger one.
DELETE FROM artist_photos WHERE outcome = 'found';

-- +goose Down
SELECT 1;
