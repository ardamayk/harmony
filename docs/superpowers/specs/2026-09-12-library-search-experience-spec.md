# Library Search: result experience and measured acceptance

> Scope update — 2026-09-12: the user replaced the detailed performance requirements
> below with a small total/p50/p95 summary from existing HTTP correctness tests.
> The 50,000-Track workload, dedicated browser timing, hardware/raw-data reports and
> latency acceptance gates are no longer required. Functional/quality requirements
> remain; browser coverage uses the ordinary integration suite. See
> [current timing checks](../../testing/library-search-performance.md).

## Problem Statement

The current search dialog shows fixed groups assembled from multiple sources,
without a cross-type Best Match. A correct server ranking is not sufficient if
the intended result is hidden, accidentally duplicated, replaced by an older response, or
unusable with the keyboard. The product also needs measured quality and latency
targets rather than an unverified claim that search is better or faster.

## Solution

Integrate the agreed Library Search behavior into the existing dialog. Highlight
one eligible Best Match, show up to five results in each category, preserve
playback and navigation, and distinguish loading, failure, and empty states.
Validate the complete experience with a labeled query set and a representative
50,000-Track performance workload.

## User Stories

1. As a listener, I want the same search dialog for all five library types, so that I do not need to choose a page first.
2. As a listener, I want the strongest eligible direct result highlighted, so that I can quickly reach my intended item.
3. As a listener, I want the Best Match type to follow the query, so that Tracks, Albums, Artists, Genres, and Playlists can each lead when appropriate.
4. As a listener, I want the highlight omitted for typo-only or related-only results, so that the interface does not imply stronger confidence than the search supports.
5. As a listener, I want the highlighted record first in its own category too, so that the category remains complete within its five-result limit.
6. As a listener, I want bounded category groups, so that the dialog stays easy to scan.
7. As a listener, I want every result to match its own title or name, so that Artist credits alone do not fill the dialog with unrelated titles.
8. As a listener, I want original names and useful credit information displayed, so that I can distinguish recordings.
9. As a listener, I want selecting a Track to play it, so that search leads directly to listening.
10. As a listener, I want other record types to retain their navigation behavior, so that search remains consistent with library browsing.
11. As a keyboard user, I want the existing shortcuts, arrow navigation, Enter, Escape, and focus return to work, so that I can search without a pointer.
12. As a listener, I want clicks and keyboard activation to act on the visible current result, so that stale queries cannot play the wrong Track.
13. As a listener, I want a brief typing delay before requests, so that each keystroke does not cause unnecessary work.
14. As a listener, I want empty input and no-match results handled clearly, so that I know whether to type or revise my query.
15. As a listener, I want failures and retry distinguishable from no matches, so that a service problem does not look like missing music.
16. As a listener, I want newly imported, replaced, and deleted content reflected when I search again, so that cached results do not misrepresent my library.
17. As a library owner, I want exact and approximate searches evaluated separately, so that good results in one category do not hide failures in another.
18. As a library owner, I want ambiguous titles evaluated against all acceptable records, so that quality measurements reflect real user intent.
19. As a library owner, I want performance tested against a diverse 50,000-Track library, so that latency claims apply beyond tiny fixtures.
20. As a library owner, I want first-search timing reported separately, so that repeated cached queries cannot conceal startup cost.
21. As a maintainer, I want documented acceptance results, so that implementation completion is supported by evidence.

## Implementation Decisions

- Depend on the normalized-data and matching/ranking specs. Consume their coherent Library Search response instead of reconstructing relevance from separate prelimited browse pages or a client-side full-library Genre scan.
- Keep the existing single dialog and its invocation from the Top Nav, Ctrl/Command+K, and `/` outside text fields. Preserve its established styling, accessible combobox/listbox behavior, and focus handling.
- Show Best Match above the existing category groups. Only strong direct results are eligible; omit the section when none exists. Keep the selected record first in its category. Use distinct presentation keys and DOM IDs for its highlight and category row; keyboard selection marks only one presentation, while activation uses the same real identity.
- Keep the existing group order: Track, Album, Artist, Genre, Playlist. Only nonempty groups appear. Each contains at most five records, including Best Match when it belongs to that category. Do not add relationship-only Albums or Artists.
- Display original names with relevant Artist/Album information. Keep match scores and index implementation details out of product UI.
- Preserve Track playback and other result navigation. Artist selection opens `/library/tracks?artistId=<id>`, covering Track or Album credits by identity without requiring an Artist detail page. Ensure the selected search record remains reachable through that navigation.
- Retain the 200 ms debounce. Do not request search for empty or punctuation-only input. One-character exact-name behavior and subsequent matching rules come from the ranking contract.
- Keep loading, no matches, and failure distinguishable. Provide retry, prevent stale responses from replacing the current query's results, and invalidate relevant cached search results after library mutations. Preserve existing useful failure handling when multiple response sources remain; do not mandate an otherwise unnecessary partial-response architecture.
- Reconcile ADR-0022 explicitly when integrating the feature: Best Match now precedes fixed groups; normalization and ranking are coherent; every result requires own-title/name contribution and Track relationship expansion is removed; client-side name filtering is no longer the authority. Preserve unrelated decisions from that ADR and the existing product navigation.
- Existing OpenAPI contracts remain the source of truth. Use the generated API client from the companion spec and retain generated-artifact consistency checks.

## Testing Decisions

- Primary behavioral seam: real migrated SQLite plus the Music Server HTTP API for search outcomes, including write-to-search lifecycle cases shared with the data spec. Do not duplicate ranking expectations in tests of private helpers.
- UI seam: extend the existing Vitest/Testing Library dialog tests, which already cover grouped results, Album results, Genre results, Enter playback, navigation, arrow keys, empty states, failure/retry, and successful results during source failures.
- Use those interaction tests to verify Best Match placement, cross-type eligibility, Best Match repetition with unique DOM IDs, no other duplicate identities, the inclusive five-result group cap, selection/focus behavior, debounce, stale responses, and distinct loading/empty/error states. Test observable roles, labels, identities, and user actions rather than internal component state.
- Browser seam: reuse the existing Playwright setup that starts the real Go Music Server and Web Client. Add a small set of end-to-end journeys through search, playback/navigation, and a library mutation, plus real end-to-end timing. Do not run the entire labeled ranking matrix exclusively as slow browser tests.
- Build a shared labeled evaluation set with at least 100 queries across full names, combined fields, word order, prefixes, punctuation, accents, Turkish letters, allowed typos, versions, short queries, and deliberately nonmatching input. Include every hard boundary and regression example from the accepted design. Report per-category sample counts and results.
- Define acceptable target identities before evaluating each query. When multiple same-name records legitimately answer the query, use an acceptable target set rather than an arbitrary single record.
- Visibility means Best Match or the first five results in the target's category, not the first five rows across the entire dialog.

| Quality measurement | Acceptance threshold |
| --- | --- |
| Correct Best Match for full-name queries with one correct target | 100% |
| Target visibility for error-free word/combined-field queries | At least 95% |
| Target visibility for queries within the allowed typo limits | At least 90% |
| False results on deliberately nonmatching queries | 0 |
| Violations of hard ordering rules or all-word coverage | 0 |

- Evaluate negative cases against the actual fixture catalog: incidental valid matches must not be mislabeled as false positives. No global false-positive rate is implied by the finite negative test set.
- Measure a diverse catalog of 50,000 Tracks and associated Albums and Artists, with one active search user. Record hardware, catalog composition, query mix, run/sample counts, and cache conditions so results can be reproduced.

| Performance measurement | Acceptance target |
| --- | --- |
| Typing debounce | 200 ms |
| Music Server search latency | p95 <= 100 ms |
| Final keystroke to rendered current results | p95 <= 400 ms |

- Use varied queries rather than only warmed repetitions. Report first-search latency separately. The end-to-end measurement includes debounce, request/response time, and rendering; a mocked API or server-only timer cannot establish it.
- Keep correctness checks deterministic and report performance against the stated environment. Do not weaken targets silently if the first implementation misses them; report the measured gap and optimize within scope.
- Completion requires the labeled quality report, performance results, relevant contract checks, and the existing affected interaction/integration checks. These are future deliverables, not results claimed by this spec.

## Out of Scope

- A general UI redesign, a new Artist detail page, per-page search fields, or Radio search changes.
- New popularity or personalization signals, external catalogs, alias/lyrics/file-path search.
- High-concurrency hosting promises, catalogs larger than the agreed benchmark as a release requirement, or production analytics infrastructure.
- A full ranking matrix duplicated across unit, component, and browser suites.
- Shipping solely on mocked timing measurements or silently relaxing the quality targets.

## Further Notes

This is the third of three broad specs and depends on both companion specs. It
owns final product integration and measured acceptance, while each preceding spec
still owns tests for the behavior it introduces. The three specs intentionally
avoid one ticket per normalization rule, result type, or test case.
