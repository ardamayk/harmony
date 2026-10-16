---
status: accepted
---

# The Music Server moves to Rust, module by module

The Music Server is rewritten in Rust (`server-rs/`, crate `earthly-music-server`) with axum on tokio and sqlx over SQLite, in the same Cargo workspace as the Desktop Client's core (ADR 0024). The Go server in `server/` keeps running until every module is ported; then the build, Docker image and CI switch to the Rust binary and `server/` is removed.

The Rust server keeps everything a client or the database can observe: the HTTP contract in `packages/contracts/openapi.yaml`, the error shape, the environment variables and defaults, the SQLite database file and its goose migrations (applied through the same `goose_db_version` table, so either server can open the same database), the Library Search texts its triggers write (the `library_search_text` SQLite function is reimplemented byte for byte), and the embedded Web Client and docs. Modules keep the Go layering: handler, service, repository, and module wiring.

Each module is ported, unit tested, and checked against the Go server with `mise run server-rs:parity`, which starts both servers on separate copies of one database, sends the same requests and compares status, content type and body. Routes the Rust server does not serve yet fall through to the Web Client and are reported as not ported. `mise run perf:server-load` measures either binary.

## Considered Options

- Rewrite everything, then switch at once: rejected because nothing could be checked against the Go server until the end.
- Keep Go: it is already small (about 23 MB idle, 78 ms startup); the move is for one language across server and desktop core, not for resource use alone.
- rusqlite instead of sqlx: rejected for losing compile-time checked queries, the closest match to today's sqlc.
- A new schema: rejected because the existing database must open unchanged and both servers must be comparable on the same data.

## Consequences

Until the switch the repository carries two server implementations, and a schema change lands as a goose migration both apply. Measured on the skeleton, the Rust binary starts in about 60 ms with about 7 MB idle and is half the size; these numbers are rechecked as modules land.
