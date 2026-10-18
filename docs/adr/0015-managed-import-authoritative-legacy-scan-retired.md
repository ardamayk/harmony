# Managed Import is authoritative and legacy scanning is retired

> Partly superseded by ADR 0016: the deprecated scan routes, `MUSIC_PATHS`, Legacy Tracks, Library Migration and the `legacy_*` tables this ADR retained were removed once the migration had run to completion.

The Music Server no longer discovers, indexes, or reconciles files from `MUSIC_PATHS` at startup or on request. Managed Import is the only way new Tracks enter the library, and existing Legacy Tracks change only through an explicit Library Migration. Copying a file into the former server music directory does not add a Track, and a Legacy Track whose source file disappears is no longer soft-deleted automatically. `MUSIC_PATHS` is retained solely to locate Legacy Tracks for playback, deletion, migration preview, and Legacy Source Cleanup.

The versioned contract keeps `/api/v1` backward compatible per ADR 0006 instead of silently removing the scan routes. `POST /api/v1/library/scan` stays mounted but is marked deprecated and always answers `410` with error code `legacy_scan_retired`, so an older Playback Client fails clearly rather than waiting for a scan that never runs. `GET /api/v1/library/scan/status` stays mounted, is marked deprecated, and always reports `idle` so older clients that poll after a trigger stop gracefully. New Web and Desktop Clients do not expose or invoke either route, and the Folders navigation entry and route are removed because no server-side folder surface remains.

Persistence is contracted only where nothing reads it anymore: the `scan_jobs` table is dropped and the scanner's dual-write path is deleted. The denormalized legacy columns on `tracks` and `albums` and the `legacy_*` identity tables remain because expanded reads, Library Migration cutover, Track Replacement, and Permanent Track Deletion still depend on them, so Legacy Tracks keep their pre-migration playback compatibility.

## Considered Options

- Remove the scan routes outright: rejected because ADR 0006 keeps `/api/v1` backward compatible and requires clear failures instead of missing routes.
- Keep a manual scan trigger without startup scanning: rejected because any scan path keeps synthesizing fallback metadata and path-based identity, which the Strict Import Profile forbids.
- Contract the legacy `tracks` and `albums` columns now: rejected because Legacy Tracks must stay playable until migrated and the migration, replacement, and deletion flows still read those columns.
