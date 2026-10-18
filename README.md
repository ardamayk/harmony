# Navidrome Replacement

Self-hosted music server: own your library, stream anywhere on your network, customize the UI with movable panels and widgets.

Not a Navidrome fork — modern OpenAPI backend (Rust), React SPA, modular monolith architecture, and room for opt-in plugins/sidecars (discover, party mode, sharing) without legacy Subsonic API lock-in.

## What it does today (v1)

- Import music through Managed Import (Web and Desktop Clients upload files or a local folder; the server validates strictly and keeps bit-preserving managed copies)
- Browse albums and tracks
- Play with queue, seek, and volume (HTTP Range streaming)
- Customizable layout: left/right panels, widgets (now playing, queue)
- Theme: light / dark / system

## Product & design

For **Figma**, **Google Stitch**, or visual design work, start here:

**[docs/DESIGN_BRIEF.md](docs/DESIGN_BRIEF.md)** — product vision, screen inventory, widget shell model, future features, visual direction.

## Stack

| Layer | Tech |
|-------|------|
| Server | Rust, axum, SQLite (sqlx), goose-format migrations |
| Web | React 19, TanStack Router, Vite, Tailwind 4, shadcn |
| Monorepo | Node, pnpm workspaces, Vite+ (`vp`: Oxlint, Oxfmt, Vitest, task runner), mise |
| API | OpenAPI spec-first (`packages/contracts`) |
| Docs | MDX (`packages/docs`) + Swagger UI (`/api/docs`) |

## Quick start

```bash
mise install
pnpm install
mise run generate   # OpenAPI client and sqlx query data
mise run dev    # Music Server :8090 + Vite :3000
```

Open http://localhost:3000/library/tracks and use **Import Music** to upload files or a local folder. The Music Server validates every file, shows an Import Preview, and stores accepted files in Managed Storage only after you confirm.

## Server dependencies

The Music Server calls these programs at runtime and reports each of them, with its version, in the health response and on the Settings page.

| Program | Required | Used for |
|---------|----------|----------|
| `ffmpeg` / `ffprobe` | yes | media inspection and full-stream decode during Managed Import |

Install FFmpeg from your distribution; the container image includes it. Settings reports FFmpeg and ffprobe availability.

Managed Import uses file tags locally. TITLE, ARTIST, ALBUMARTIST, ALBUM, and TRACKNUMBER are required. DISCNUMBER is required for multi-disc albums and defaults to one otherwise. GENRE, DATE, artwork, totals, and ReplayGain are optional. Album matching uses album artist and the full album title, preserving edition qualifiers. Only identical full-file SHA-256 values are duplicates. Different bytes at the same album/disc/track/title require explicit replacement approval. New albums may use an embedded cover, a JPEG/PNG upload, or no cover. See [ADR 0019](docs/adr/0019-tag-based-import-and-album-artwork.md).

## External services

The Music Server calls these services itself, so clients never contact them directly. Artist info, play counts and lyrics are looked up when a client first asks for them and then stored; a library is never crawled. Only Last.fm needs a key.

| Service | Key | Used for | Switch |
|---------|-----|----------|--------|
| [Deezer](https://developers.deezer.com/api) | no | Artist photo, popular Tracks, genres for Albums whose tags name none | `ARTIST_INFO_ENABLED=false` |
| [MusicBrainz](https://musicbrainz.org/doc/MusicBrainz_API) → [Wikidata](https://www.wikidata.org) → [Wikipedia](https://en.wikipedia.org/api/rest_v1/) | no | Artist biography, in `ARTIST_INFO_LANGUAGE` (default `en`) | `ARTIST_INFO_ENABLED=false` |
| [Last.fm](https://www.last.fm/api) | `LASTFM_API_KEY` | Track play counts | off without a key |
| [LRCLIB](https://lrclib.net) | no | Lyrics for Tracks whose files have none | `LYRICS_LOOKUP_ENABLED=false` |
| [Radio Browser](https://www.radio-browser.info) | no | Radio station catalog | `RADIO_BROWSER_BASE_URL` |
| Radio stations | no | Streams, through the Music Server's proxy | play no radio |

Two more are reached by the clients rather than the server:

- **Radio station icons** load in the Web Client straight from each station's own `faviconUrl`.
- **Google Fonts** serves the Web Client's fonts (`web/src/styles.css`).

The album and artist links to Spotify, Qobuz, Last.fm and ListenBrainz are plain links; nothing is sent to these services until you click one.

ADRs: [0003](docs/adr/0003-radio-browser-catalog-and-health.md) (Radio Browser), [0027](docs/adr/0027-artist-photos-from-deezer-biographies-from-wikipedia.md) (Deezer, Wikipedia), [0029](docs/adr/0029-lyrics-from-lrclib.md) (LRCLIB), [0030](docs/adr/0030-play-counts-from-lastfm-with-an-operator-key.md) (Last.fm).

## Build

```bash
mise run build
```

Mise is the canonical command interface. Use `mise run web:build`, `mise run server:build`, or `mise run desktop:build` for one artifact. `mise tasks` lists everything.

## Linux Desktop Client

Run the Music Server and the Electron Desktop Client together in development mode (Vite HMR, debug Rust core):

```bash
mise run desktop:dev
```

Build and start the release Desktop Client:

```bash
mise run build
mise run start
```

See [desktop/README.md](desktop/README.md) for how the Electron shell and the Rust core fit together. Docker Compose runs only the Music Server and does not launch the Desktop Client.

## Test

```bash
mise run ci                # static checks, unit tests, generated-client drift (what PR CI runs)
mise run ci:integration    # Playwright, HLS, and pinned-mpv tests
mise run check             # vp check (Oxfmt + Oxlint + types), rustfmt + Clippy
mise run test              # TypeScript and Rust unit tests
```

Targeted commands include `mise run web:test`, `mise run server:test`, and `mise run desktop:test`. Inside the TypeScript workspace, `vp check --fix`, `vp test`, and `vp run <task>` work directly (through mise, or `pnpm exec vp …`).

CI is a single workflow (`.github/workflows/ci.yml`): web, server, desktop, and E2E jobs on every pull request; integration and release builds on `main`, weekly, or on demand.

## Git hooks

`pnpm install` installs the Vite+ hook dispatcher (`.vite-hooks/`). Pre-commit runs `vp staged` (Oxfmt/Oxlint on staged TypeScript and JSON, rustfmt check on Rust) and OpenAPI drift verification only when generation inputs are staged. Use `--no-verify` to bypass; CI remains authoritative.

If `graphify-out/` exists and `graphify` is available on `PATH`, post-commit and branch-switch post-checkout hooks update the graph in the background.

## Developer docs

- [AGENTS.md](AGENTS.md) — conventions for contributors and AI agents
- [contracts.md](contracts.md) — API contract rules
