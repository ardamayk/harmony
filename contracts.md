# API Contracts

## Source of truth

`packages/contracts/openapi.yaml` is the single source of truth for the HTTP API.

The TypeScript client types are generated from this file, and the Music Server serves it as is at `/api/openapi.yaml`. Do not hand-edit generated output in:

- `packages/api-client/src/generated/schema.ts`

## Versioning

- Base path: `/api/v1/`
- Breaking changes require a new major version (`/api/v2/`)
- Non-breaking additions (new optional fields, new endpoints) stay in v1

## Naming

| Item | Convention |
|------|------------|
| JSON fields | camelCase |
| Operation IDs | camelCase (`getHealth`, `patchPreferences`) |
| Error codes | snake_case strings |

## Error format

All error responses use:

```json
{
  "error": "bad_request",
  "code": "bad_request",
  "message": "Human-readable description"
}
```

`code` is the stable machine-readable identifier clients branch on; `message` is for people and may change. `error` duplicates `code` for older clients. Strict Import Profile rejections add `field` (the offending tag or stream, such as `TITLE` or `artwork`) and `reason` (the actionable repair hint) so a Playback Client can render precise per-file feedback.

HTTP status codes follow standard semantics (400, 401, 403, 404, 408, 409, 410, 413, 422, 500, 507).

## Auth (v1 stub)

- Security scheme: Bearer JWT (`bearerAuth`)
- v1 scaffold uses a fixed default user; real auth comes in a later plan
- Protected routes: `GET /api/v1/me`, `GET/PATCH /api/v1/preferences`, playback queue endpoints

## Library (v1)

- Ingestion: Managed Import only (`/api/v1/imports/...`); the server never scans a server-side folder (ADR 0015, ADR 0016)
- Browse: artists, albums, tracks list/detail endpoints under `/api/v1/library/`
- Library Search: `GET /api/v1/library/search?q=` (`searchLibrary`) evaluates one coherent query across Tracks, Albums, Artists, Genres, and Playlists on the Music Server and returns an optional Best Match plus five capped groups; each result carries `match` (`direct`, `related`, `corrected`). Browse `q` filters remain the older substring behavior, not the ranking contract.
- Supported formats: mp3, flac, ogg, m4a, opus, wav

## Playback (v1)

- Queue persisted in SQLite per stub user (`playback_queue` table)
- Stream: `GET /api/v1/tracks/{trackId}/stream` with HTTP Range (`206 Partial Content`)
- No transcoding in v1 — direct file serve

## Preferences / layout contract

`UserPreferences.layout` drives the modular widget shell:

```json
{
  "sidebarPosition": "left",
  "panels": {
    "left": ["now-playing", "queue"],
    "right": ["discover"]
  },
  "collapsed": { "left": false, "right": true }
}
```

Widget IDs must be registered in `packages/ui/src/widgets/registry.tsx`.

## Live updates

Shared Queue invalidations use the OpenAPI-documented server-sent event stream. Party mode remains a future WebSocket feature.

## Codegen

```bash
mise run generate
```

| Target | Tool | Output |
|--------|------|--------|
| TS schema | openapi-typescript | `packages/api-client/src/generated/schema.ts` |

`mise run generate` also refreshes `server/.sqlx`, the Music Server's offline query data.

`packages/api-client/src/index.ts` may define convenience wrappers, but exported domain types should alias generated schemas rather than re-declaring fields.

## Server Capabilities

`GET /api/v1/health` advertises named behaviors (ADR 0006). Clients gate optional features on the exact name and ignore unknown entries; the Desktop Client refuses to connect when a required capability is absent.

| Capability | Behavior |
| --- | --- |
| `api.v1` | The versioned `/api/v1` surface |
| `playback.queue-events.v1` | Shared Queue server-sent events |
| `managed-import.v1` | Single-file Managed Import (upload, Import Preview, confirm) |
| `managed-import-batches.v1` | Multi-file Import Batches with per-file status and duplicate decisions |
| `managed-track-deletion.v1` | Permanent Track Deletion preview and confirmation |
| `managed-album-deletion.v1` | Album deletion: one Permanent Track Deletion per Track, previewed and confirmed once at the Album level |
| `managed-track-replacement.v1` | Explicit Track Replacement |

`@repo/api-client` exports one constant per capability plus `hasServerCapability` and `missingServerCapabilities`; the Web Client's `useServerCapabilityState` hook wraps them.

## Contract verification

Generated files are committed (ADR 0013). `mise run generate:check` regenerates into a temporary directory and fails when the committed client or query data is stale. Server tests verify the contract without UI journeys:

- `server/tests/contract.rs` sends every documented operation to the assembled router and fails when one is not mounted, fails when the source registers an `/api/v1` path the spec does not document, checks that every `operationId` is unique, that every advertised capability is described in `HealthResponse`, and that every documented request header passes CORS preflight. The server embeds `packages/contracts/openapi.yaml` itself, so the served spec cannot drift from the file.
- The Managed Import, Track Replacement and Permanent Track Deletion service tests (`server/src/modules/managed_import/service_tests.rs`) drive uploads, previews, structured errors and crash recovery end to end.
- `packages/api-client/src/contract.test.ts` pins the generated schemas, upload media types, structured errors, and capability negotiation on the client side.

The Go server also validated responses against the schema in its tests; the Rust server was instead checked answer for answer against the Go server while it was ported (ADR 0025).

## API reference

- Spec URL: `/api/openapi.yaml`
- UI: `/api/docs`, Scalar's API reference (ADR 0035)

The page (`server/api-reference.html` and `server/api-reference.js`) and Scalar's standalone bundle are served by the Music Server; nothing loads from a CDN. The bundle comes from `@scalar/api-reference`, pinned in `packages/docs/package.json`; `scripts/sync-static.sh api-reference` copies it into `server/static/api-reference`, and `mise run server:build`, `mise run dev` and the Docker build run that copy. `server/tests/contract.rs` checks that the page and its scripts are served under a Content-Security-Policy that allows no other origin.

## Product documentation

Human-readable docs live in `packages/docs/content/*.mdx` and are served at `/docs/`. API reference stays in the OpenAPI spec and `/api/docs`; do not duplicate endpoint lists in MDX.

After building docs only, run:

```bash
./scripts/sync-static.sh docs
```
