# Earthly Audio Music Server

The Music Server, written in Rust (crate `earthly-music-server`; axum on tokio, sqlx over SQLite). It serves the HTTP contract in `packages/contracts/openapi.yaml`, applies the goose migrations in `migrations/` at startup, and embeds the built Web Client and docs (`static/`, filled by `mise run server:build`).

```bash
mise run server:build      # web + docs, then the release binary at bin/server
mise run server:test       # unit tests and the contract tests (tests/contract.rs)
mise run server:check      # rustfmt, clippy, sqlx offline query data
mise run server:prepare    # regenerate .sqlx after changing a query or migration
```

`cargo run -p earthly-music-server` runs it in development; `src/bin/repair-artist-credits.rs` is the Artist credit repair command (`docs/operations/repair-artist-credits.md`).

## Layout

```text
src/main.rs, app.rs     config, logging, router, middleware
src/db.rs, migrate.rs   SQLite (WAL, BEGIN IMMEDIATE writes), goose-compatible migrator
src/respond.rs          ApiError and the JSON error envelope
src/inspect/            Strict Import Profile media inspection (symphonia; ffmpeg for Opus and M4A)
src/modules/<module>/   one module per domain: routes/handlers, service, repository
testdata/               audio fixtures shared with the Web E2E and Desktop import parity tests
```

`tests/search_text_parity.rs` recomputes every Library Search text a database holds (`EARTHLY_PARITY_DB=<copy> cargo test -p earthly-music-server --test search_text_parity -- --ignored`).
