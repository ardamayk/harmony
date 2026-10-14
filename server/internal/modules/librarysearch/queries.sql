-- name: GetSearchRevision :one
SELECT revision FROM library_search_revision WHERE singleton = 1;

-- name: ListSearchFieldSources :many
SELECT source.kind, source.id, source.field, source.related_kind, source.related_id
FROM library_search_field_sources source
JOIN library_search_entities entity ON entity.kind = source.kind AND entity.id = source.id
WHERE entity.user_id = '' OR entity.user_id = ?
ORDER BY source.kind, source.id, CASE source.field WHEN 'primary' THEN 0 ELSE 1 END,
    source.field, source.position, source.related_id;

-- name: ListSearchTexts :many
SELECT kind, id, text_json FROM library_search_texts;
