# Library Search data

This package implements the normalized-data spec. It prepares data for the
matching/ranking module; it does not add a Library Search HTTP endpoint or change
existing browse filters.

`PrepareQuery` prepares one incoming query. `Repository.GetDocument` exposes prepared
primary and related fields with stable identities and ordered credits. Playlist
reads require the owning user ID. Hidden/pending Tracks and their otherwise
unreferenced Albums, Artists, and Genres are excluded. Track-only Artists remain
eligible, independently of the Album Artist browse filter.

Each entity name is prepared once when it is written. SQLite triggers call the
registered deterministic normalizer within the original mutation transaction.
Preparation failure rejects that write. Relationships join prepared names;
changing an Artist name or a credit does not require copying derived fields to
every related Track. Queries never normalize stored library names again.

Representations preserve original text, case-folded accent/punctuation evidence,
folded words, and a punctuation-joined alternative. Joining never removes ordinary
whitespace boundaries. Version qualifiers are recognized only in parenthesized or
` - ` suffix sections of stored Track/Album titles; complete titles remain
searchable. Query qualifiers may occur anywhere. Plain titles do not establish an
original recording. Ranking, typo budgets, and version preference belong to the
companion ranking spec.

Migration 036 backfills existing entities. At startup, `EnsureCurrent` rebuilds
missing or outdated data before handlers receive the database. Increment
`RULES_VERSION` whenever normalization changes. Triggers obtain the version from
the binary through `library_search_version()`, so migration SQL does not need to
be edited for future rules. `Rebuild` explicitly regenerates all derived data in
one transaction and leaves source records/audio untouched. Failure preserves the
previous snapshot and returns a contextual error. Direct SQL writers must use a
connection with the registered functions; missing functions fail writes rather
than silently leaving stale data.

The shared `testutil.LibrarySearchNormalizationCases` fixtures supply positive
normalization and negative word-boundary examples for the `librarysearch`
module's ranking HTTP suite. Data tests use this package's public interface and real migrated SQLite;
HTTP tests cover Managed Import, replacement, deletion, byte preservation, and
failed preparation. End-to-end write-to-search ranking assertions live with the
`librarysearch` module, which owns the search HTTP read boundary. No search latency
or ranking-quality result is claimed by this implementation.
