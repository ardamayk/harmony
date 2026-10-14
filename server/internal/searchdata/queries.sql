-- name: ListDocumentFields :many
SELECT field, related_id, position, text_json FROM library_search_fields
WHERE kind = ? AND id = ? AND (user_id = '' OR user_id = sqlc.arg(user_id))
ORDER BY CASE field WHEN 'primary' THEN 0 ELSE 1 END, field, position, related_id;

-- name: HasCurrentTexts :one
SELECT NOT EXISTS (
    SELECT 1 FROM library_search_sources source
    LEFT JOIN library_search_texts prepared ON prepared.kind = source.kind AND prepared.id = source.id
    WHERE prepared.id IS NULL OR prepared.rules_version != sqlc.arg(rules_version)
) AS is_current;

-- name: DeleteTexts :exec
DELETE FROM library_search_texts;

-- name: InsertTextsFromSources :exec
-- library_search_text() and library_search_version() are registered by this package.
INSERT INTO library_search_texts
SELECT kind, id, library_search_text(name, kind), library_search_version() FROM library_search_sources;
