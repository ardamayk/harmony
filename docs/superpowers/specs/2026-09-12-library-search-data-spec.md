# Library Search: normalized data and metadata lifecycle

## Problem Statement

A listener cannot reliably find library content when their spelling differs in
case, accents, Turkish characters, apostrophes, or punctuation. Reprocessing the
whole library on every keystroke also repeats work as the library grows. Search
preparation must not alter displayed metadata, merge distinct records, or change
the managed audio files.

## Solution

Prepare reusable search representations when library metadata is stored or
changed. Preserve original names and relationships, support the agreed tolerant
forms, and recognize explicit version qualifiers. Use the same normalization
rules for queries. Keep existing and newly imported content equally searchable.

## User Stories

1. As a listener, I want case-insensitive search, so that capitalization does not prevent discovery.
2. As a listener, I want redundant spaces ignored, so that minor typing differences do not hide a record.
3. As a listener, I want `beyonce` to find `Beyoncé`, so that an accent-free keyboard is sufficient.
4. As a listener, I want `sebnem` to find `Şebnem`, so that Turkish-specific characters are optional in a query.
5. As a listener, I want tolerant matching across `I`, `İ`, `ı`, and `i`, so that keyboard and casing differences are supported.
6. As a listener, I want more faithful spellings distinguishable from folded alternatives, so that precise input can receive priority.
7. As a listener, I want straight and curly apostrophes and omitted apostrophes supported, so that `don't`, `don’t`, and `dont` can find the same title.
8. As a listener, I want `AC/DC`, `ac dc`, and `acdc` supported, so that punctuation conventions do not block search.
9. As a listener, I want ordinary word boundaries preserved, so that `a b` is not treated as `ab` just by deleting spaces.
10. As a listener, I want non-Latin names and digits preserved, so that my library is not restricted to English text.
11. As a listener, I want original names shown in results, so that search preparation does not rewrite my collection.
12. As a listener, I want title words combined with credits, Album, and Genres to find Tracks, so that I can use the information I remember without credit-only results.
13. As a listener, I want Album Artists distinguished from Track Artists, so that searches respect my library's actual credits.
14. As a listener, I want live, remix, and other recognized editions distinguishable, so that I can search for the version I intend.
15. As a listener, I want `Live Forever` treated as an ordinary title, so that a version keyword does not misclassify the recording.
16. As a library owner, I want successful imports and metadata changes reflected in search, so that results represent my current collection.
17. As a library owner, I want deleted records removed from search, so that results do not lead to unavailable content.
18. As a library owner, I want existing content prepared after an upgrade, so that I do not need to reimport my library.
19. As a library owner, I want search rules rebuildable without changing audio or identity, so that search improvements are safe to adopt.
20. As a library owner, I want failed updates reported accurately, so that search cannot silently appear current after a preparation failure.

## Implementation Decisions

- This spec owns derived search data, its lifecycle, and version recognition. Ranking and presentation are covered by the companion specs.
- The existing Music Server uses Go and SQLite, with normalized Artist, Album Artist, and Genre relationships. Prefer the existing storage and Unicode capabilities; no separate search engine or new dependency was selected in the interview.
- Prepare stored search representations on successful creation and metadata changes. Normalize the query at search time using the same rules; do not normalize every stored record again for every query.
- Preserve authoritative names, Track identity, Album grouping, credit order, and source audio bytes. Search representations are derived data, not identity keys.
- Cover successful Managed Import, Track Replacement, deletion, and existing name/relationship mutations that affect search. Keep dependent search data current and make failures observable. Do not introduce new metadata-editing product flows merely to support indexing.
- Backfill existing library content and support rebuilding derived data when rules change. Preserve source data on a failed rebuild and use existing error-reporting conventions. The physical schema and rebuild mechanism remain implementation choices constrained by the companion acceptance targets.

| Result type | Primary field | Secondary fields |
| --- | --- | --- |
| Track | Title | Track Artists, Album Artists, Album title, Genres |
| Album | Title | Album Artists |
| Artist | Name | None |
| Genre | Name | None |
| Playlist | Name | None |

The ranking contract requires at least one query word to contribute through the
primary field. Secondary fields supplement it; relationships alone never add
results. Data preparation retains all fields for combined queries.

- Treat case and redundant whitespace as equivalent. Retain accent-preserving and tolerant alternatives, including Turkish-letter folding. Preserve evidence needed to prefer the representation closer to the query's letters and punctuation.
- Support the positive punctuation examples in the user stories without globally joining whitespace-separated words. Preserve non-Latin letters and digits. No translation or cross-script transliteration is required.
- Search all five agreed result types. A Playlist does not become a match solely because one of its Tracks matches. Radio remains separate.
- Recognize version qualifiers from an explicit allowlist: `live`, `remix`, `acoustic`, `instrumental`, `demo`, `remaster` / `remastered`, and `radio edit`.
- In stored titles, recognize version terms only in parentheses or a version section separated by ` - `, including dated forms such as `2011 Remaster`. Do not strip every parenthetical phrase or classify `Live Forever` as a live version. A plain title does not prove an original recording.
- Version terms can occur anywhere in a query; the ranking spec owns their effect. Keep the complete searchable title and version information so that no query word needs to be discarded.
- Reuse the existing domain relationships instead of inferring credits by splitting punctuation in display names.

## Testing Decisions

- Test observable behavior through the highest practical boundary: successful import or mutation followed by Library Search over the Music Server HTTP API with a real migrated SQLite database. Do not expose a test-only endpoint or assert private normalization helpers as the feature contract.
- The companion ranking spec supplies the complete Library Search read boundary. This spec supplies reusable mutation fixtures and normalization acceptance cases; run the integrated write-to-search cases when that boundary is connected. Existing library reads can independently verify that source metadata and identity remain intact.
- Prior art includes the existing normalized-credit HTTP handler integration tests, Managed Import and Track Replacement router tests, migrated-database fixtures, and deletion/recovery tests.
- Cover every positive normalization example and negative cases such as ordinary-word concatenation. Include competing original and folded spellings to preserve ranking evidence.
- Test version recognition with ordinary titles, parenthetical text that is not a version, explicit suffixes, and dated remasters.
- Verify import, replacement, related credit changes, deletion, existing-library backfill, and rebuild outcomes. Include failures and confirm that errors are visible and authoritative metadata is preserved.
- Share acceptance fixtures with the ranking spec rather than building separate, conflicting definitions of normalization.

## Out of Scope

- Ranking classes, fuzzy scoring, Best Match selection, and dialog layout.
- Aliases, lyrics, file-path search, external catalogs, and Radio search.
- Translation, automatic cross-script transliteration, and metadata enrichment.
- Rewriting audio tags or changing Track identity and Album grouping.
- New metadata editing screens or a mandated external search service.

## Further Notes

This is the first of three broad Library Search specs. Its consumer is the
matching and ranking spec; final UI and measured acceptance belong to the third
spec. The separation is an implementation dependency, not three independent
search algorithms. The accepted Library Search design remains the shared product
contract. Existing ADR-0022 describes the earlier search behavior; the integration
spec reconciles the resulting changes explicitly.
