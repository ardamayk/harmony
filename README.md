# Navidrome Replacement

Self-hosted music server: own your library, stream anywhere on your network, customize the UI with movable panels and widgets.

Not a Navidrome fork — modern OpenAPI backend (Go), React SPA, modular monolith architecture, and room for opt-in plugins/sidecars (discover, party mode, sharing) without legacy Subsonic API lock-in.

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
| Server | Go, chi, SQLite, goose migrations, sqlc queries |
| Web | React 19, TanStack Router, Vite, Tailwind 4, shadcn |
| Monorepo | bun workspaces, Vite+ (`vp`: Oxlint, Oxfmt, Vitest, task runner), mise |
| API | OpenAPI spec-first (`packages/contracts`) |
| Docs | MDX (`packages/docs`) + Swagger UI (`/api/docs`) |

## Quick start

```bash
mise install
bun install
mise run generate   # OpenAPI codegen
mise run dev    # Go :8090 + Vite :3000
```

Open http://localhost:3000/library/tracks and use **Import Music** to upload files or a local folder. The Music Server validates every file, shows an Import Preview, and stores accepted files in Managed Storage only after you confirm.

## Server dependencies

The Music Server calls these programs at runtime and reports each of them, with its version, in the health response and on the Settings page.

| Program | Required | Used for |
|---------|----------|----------|
| `ffmpeg` / `ffprobe` | yes | media inspection and full-stream decode during Managed Import |

Install FFmpeg from your distribution; the container image includes it. Settings reports FFmpeg and ffprobe availability.

Managed Import uses file tags locally. TITLE, ARTIST, ALBUMARTIST, ALBUM, and TRACKNUMBER are required. DISCNUMBER is required for multi-disc albums and defaults to one otherwise. GENRE, DATE, artwork, totals, and ReplayGain are optional. Album matching uses album artist and the full album title, preserving edition qualifiers. Only identical full-file SHA-256 values are duplicates. Different bytes at the same album/disc/track/title require explicit replacement approval. New albums may use an embedded cover, a JPEG/PNG upload, or no cover. See [ADR 0019](docs/adr/0019-tag-based-import-and-album-artwork.md).

## Build

```bash
mise run build
```

Mise is the canonical command interface. Use `mise run web:build`, `mise run server:build`, or `mise run desktop:build` for one artifact. `mise tasks` lists everything.

## Linux Desktop Client

Run the Music Server and Tauri Desktop Client together in development mode:

```bash
mise run desktop:dev
```

Build and start the release Desktop Client:

```bash
mise run build
mise run start
```

The launcher automatically selects native Wayland with the NVIDIA explicit-sync workaround when required. Use `EARTHLY_AUDIO_DISPLAY_MODE=x11` to force X11/XWayland or `EARTHLY_AUDIO_DISPLAY_MODE=wayland-shm` for the slower shared-memory Wayland fallback. Docker Compose runs only the Music Server and does not launch the Desktop Client.

## Test

```bash
mise run ci                # static checks, unit tests, generated-client drift (what PR CI runs)
mise run ci:integration    # Playwright, HLS, and pinned-mpv tests
mise run check             # bun --bun vp check (Oxfmt + Oxlint + types), gofmt + golangci-lint, rustfmt + Clippy
mise run test              # TypeScript, Go, and Rust unit tests
```

Targeted commands include `mise run web:test`, `mise run server:test`, and `mise run desktop:test`. Inside the TypeScript workspace, `bun --bun vp check --fix`, `bun --bun vp test`, and `bun --bun vp run <task>` work directly; `--bun` keeps Vite+ on the Bun runtime instead of Node.

CI is a single workflow (`.github/workflows/ci.yml`): web, server, desktop, and E2E jobs on every pull request; integration and release builds on `main`, weekly, or on demand.

## Git hooks

`bun install` installs the Vite+ hook dispatcher (`.vite-hooks/`). Pre-commit runs `bun --bun vp staged` (Oxfmt/Oxlint on staged TypeScript and JSON, gofmt on Go, rustfmt check on Rust) and OpenAPI drift verification only when generation inputs are staged. Use `--no-verify` to bypass; CI remains authoritative.

If `graphify-out/` exists and `graphify` is available on `PATH`, post-commit and branch-switch post-checkout hooks update the graph in the background.

## Developer docs

- [AGENTS.md](AGENTS.md) — conventions for contributors and AI agents
- [contracts.md](contracts.md) — API contract rules
