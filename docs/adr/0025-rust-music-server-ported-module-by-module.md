---
status: accepted
---

# The Music Server moves to Rust, module by module

The Music Server is rewritten in Rust (crate `earthly-music-server`) with axum on tokio and sqlx over SQLite, in the same Cargo workspace as the Desktop Client's core (ADR 0024). The Go server kept running until every module was ported; then the build, Docker image and CI switched to the Rust binary, which now lives in `server/`.

The Rust server keeps everything a client or the database can observe: the HTTP contract in `packages/contracts/openapi.yaml`, the error shape, the environment variables and defaults, the SQLite database file and its goose migrations (applied through the same `goose_db_version` table, so either server can open the same database), the Library Search texts its triggers write (the `library_search_text` SQLite function is reimplemented byte for byte), and the embedded Web Client and docs. Modules keep the Go layering: handler, service, repository, and module wiring.

Each module was ported, unit tested, and checked against the Go server with a parity harness that started both servers on separate copies of one database, sent the same requests and compared status, content type, body and the stored data. `mise run perf:server-load` measures either binary.

## Considered Options

- Rewrite everything, then switch at once: rejected because nothing could be checked against the Go server until the end.
- Keep Go: it is already small (about 23 MB idle, 78 ms startup); the move is for one language across server and desktop core, not for resource use alone.
- rusqlite instead of sqlx: rejected for losing compile-time checked queries, the closest match to today's sqlc.
- A new schema: rejected because the existing database must open unchanged and both servers must be comparable on the same data.

## Outcome

The switch is done. The last parity run matched on all 376 requests and all 30 tables; the harness and the Go code were then removed, and `server/tests/contract.rs` checks the router against the OpenAPI document instead. Comments that name a Go file refer to the Go tree as of commit `434238c`.

Known, deliberate differences in media inspection: MPEG-2.5 MP3 frames are rejected (as the Go decoder did), CELT-mode Opus is accepted, trailing bytes after a FLAC stream are tolerated, and ffmpeg progress lines reporting `N/A` are skipped.

## Consequences

During the port the repository carried two server implementations, and a schema change landed as a goose migration both applied. Measured on the skeleton, the Rust binary starts in about 60 ms with about 7 MB idle and is half the size; these numbers are rechecked as modules land.
