# Earthly Audio Music Server (Rust)

The Music Server ported from `server/` to Rust, module by module (ADR 0025). Same HTTP contract (`packages/contracts/openapi.yaml`), same SQLite database and goose migrations (`server/migrations`), same environment variables.

```bash
mise run server-rs:build    # release binary with embedded Web Client and docs
mise run server-rs:test
mise run server-rs:parity   # Go vs Rust answers on copies of server/data/app.db
```

Run it like the Go server, from `server/` (or the repository root): `../target/release/earthly-music-server`.

## Porting status

| Step | Scope | Status |
|---|---|---|
| 0 | Config, logging, SQLite (WAL, IMMEDIATE writes), goose-compatible migrator, Library Search SQLite functions, dependency probe, `health`, `me`, CORS, timeouts, embedded Web Client and docs, parity harness | done |
| 1 | preferences | done |
| 2 | playlists (after library: it returns library Tracks) | done |
| 3 | library (albums, artists, tracks, covers, lyrics, waveform) | done |
| 4 | library search (and startup rebuild of search data) | done |
| 5 | radio (stations, Radio Browser catalog, HLS proxy, now playing) | done |
| 6 | playback (queue, queue events over SSE, Track streaming) | done |
| 7 | managed import (imports, batches, artwork, replacement, recording identification) | |
| 8 | switch: build, Docker, CI and docs use the Rust server; `server/` is removed | |

`tests/search_text_parity.rs` recomputes every Library Search text a Go-written database holds (`EARTHLY_PARITY_DB=<copy> cargo test -p earthly-music-server --test search_text_parity -- --ignored`).
