# Library Search module

`GET /api/v1/library/search?q=` is the Music Server's Library Search boundary.
It consumes the representations prepared by `internal/searchdata` and owns
matching, ranking, typo fallback, and Best Match selection.
Clients receive ordered groups and never reconstruct relevance from browse pages.

Each request checks the transactional `library_search_revision`, reusing one
immutable visibility-filtered snapshot until searchable data changes. A rebuild
reads the revision, `library_search_entities`, `library_search_field_sources`, and
`library_search_texts` in one SQLite transaction. Ranking, deduplication, and group
limits still cover the whole eligible library; Playlists remain owner-filtered.
Name, credit, Genre, Album membership, visibility, and owner changes invalidate
in the same transaction as the write. Failed/rolled-back writes cannot publish a
partial snapshot. Playback-only writes do not invalidate. A storage error returns
an error even with a warmed snapshot. The first search and first search after a
mutation pay the full rebuild cost; no TTL or query-result cache hides freshness.

Matching rules, in order of evaluation:

- The query is prepared once with `searchdata.PrepareQuery`. Its folded words are
  matched against each field's folded and punctuation-joined words; the joined
  query form is tried as an alternative. A one-character query returns only
  exact primary-name matches. Empty and punctuation-only queries are `400`.
- A strong direct match covers every query word as a full word; only the final
  word may prefix-match. Internal substrings never match. At least one word must
  contribute through the result's own title/name, including during typo fallback.
  Related fields can cover remaining words, never make a result eligible alone.
- If any strong direct result exists in any type, typo correction is skipped
  entirely. Otherwise each word may use insertion, deletion, substitution, or an
  adjacent transposition: 0 edits for 1–3 characters, 1 for 4–7, 2 for 8 or
  more, at most 2 across the query. Prefix completion and correction are never
  combined within one word. Corrected results are labeled `corrected`, never
  become Best Match, and never expand related records.
- Classes: full primary name; all words within the primary name; words split
  between primary and related fields. Within a class the
  order is edits, full-word before prefix-dependent coverage, written-form
  fidelity, version preference (requested allowlisted version, or plain title
  when none is requested), name, Artist/Album text, then identity. Popularity
  and type hierarchy contribute nothing.
- Tracks never expand related Albums or Artists. Every category follows the same
  relevance ordering, with no relationship-only entries.
- The Best Match is the top strong direct result across types and also leads
  its category. Every group holds at most five records, including Best Match.

`handler_test.go` is the acceptance seam: the shared
`testutil.SeedLibrarySearchFixture` library served through the router with
every response validated against the OpenAPI contract. The labeled
`testutil.LibrarySearchRankingCases` expectations are reusable by the integrated
quality evaluation. Tests assert identities, order, labels, exclusions, and
errors only. Query text longer than 200 characters is rejected as a request
validation bound.
