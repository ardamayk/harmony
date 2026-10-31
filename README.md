# Harmony

Self-hosted music server: own your library, stream anywhere on your network, customize the UI with movable panels and widgets.

Not a Navidrome fork — modern OpenAPI backend (Rust), React SPA, modular monolith architecture, and room for opt-in plugins/sidecars (discover, party mode, sharing) without legacy Subsonic API lock-in.

## What it does today

- **Import:** Managed Import. The Web and Desktop Clients upload files or a local folder; the Music Server validates them strictly and keeps bit-preserving managed copies.
- **Library:** albums, artists, genres, tracks, playlists and favorites, with one search across them from the top bar.
- **Artist pages:** photo, biography, popular Tracks and discography; albums without genre tags get Deezer's genres (see [External services](#external-services)).
- **Playback:** a server-side queue grouped by source (album, playlist, added by you), seek, volume, sleep timer, and Last.fm play counts when the operator sets a key.
- **Lyrics:** synced or plain lyrics from the file, or from LRCLIB when the file has none, in a full-screen Lyrics view.
- **Radio:** stations from the Radio Browser catalog, streamed through the Music Server's proxy.
- **Clients:** the Web Client in a browser, and the Linux Desktop Client (Electron) with native mpv playback and a mini player.
- **Themes:** seven presets in light, dark or system mode. In dark mode each preset is built from a neutral and an opposite accent. Vintage Harbor can take its accent from the playing cover.

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
| Docs | MDX (`packages/docs`) + Scalar API reference (`/api/docs`) |

## Quick start

```bash
mise install
pnpm install
mise run generate   # OpenAPI client and sqlx query data
mise run dev:web    # Music Server :8090 + Vite :3000
```

Open http://localhost:3000/settings and use **Import Music** to upload files or a local folder. The Music Server validates every file, shows an Import Preview, and stores accepted files in Managed Storage only after you confirm.

## Server dependencies

The Music Server calls these programs at runtime and reports each of them, with its version, in the health response and on the Settings page.

| Program | Required | Used for |
|---------|----------|----------|
| `ffmpeg` / `ffprobe` | yes | media inspection and full-stream decode during Managed Import |

Install FFmpeg from your distribution; the container image includes it. Settings reports FFmpeg and ffprobe availability.

Managed Import uses file tags locally. TITLE, ARTIST, ALBUMARTIST, ALBUM, and TRACKNUMBER are required. DISCNUMBER is required for multi-disc albums and defaults to one otherwise. GENRE, DATE, artwork, totals, and ReplayGain are optional. Album matching uses album artist and the full album title, preserving edition qualifiers. Only identical full-file SHA-256 values are duplicates. Different bytes at the same album/disc/track/title require explicit replacement approval. New albums may use an embedded cover, a JPEG/PNG upload, or no cover. See [ADR 0019](docs/adr/0019-tag-based-import-and-album-artwork.md).

## External services

The Music Server calls these services itself, so clients never contact them directly. Artist info, play counts and lyrics are looked up when a client first asks for them and then stored; a library is never crawled. Only Last.fm needs a key, and Sentry your own DSN (the Desktop core reports to it too). yt-dlp is not a web service: it is a program you install yourself (see below).

| Service | Key | Used for | Switch |
|---------|-----|----------|--------|
| [Deezer](https://developers.deezer.com/api) | no | Artist photo, popular Tracks, genres for Albums whose tags name none, sharper Album covers for covers under 1000 px | `ARTIST_INFO_ENABLED=false` |
| [MusicBrainz](https://musicbrainz.org/doc/MusicBrainz_API) → [Wikidata](https://www.wikidata.org) → [Wikipedia](https://en.wikipedia.org/api/rest_v1/) | no | Artist biography, in `ARTIST_INFO_LANGUAGE` (default `en`) | `ARTIST_INFO_ENABLED=false` |
| [Last.fm](https://www.last.fm/api) | `LASTFM_API_KEY` | Track play counts | off without a key |
| [LRCLIB](https://lrclib.net) | no | Lyrics for Tracks whose files have none | `LYRICS_LOOKUP_ENABLED=false` |
| [Radio Browser](https://www.radio-browser.info) | no | Radio station catalog | `RADIO_BROWSER_BASE_URL` |
| [yt-dlp](https://github.com/yt-dlp/yt-dlp) (a program you install) | no | External Source: search and play Tracks from a site you choose | off unless `EXTERNAL_SOURCE_SEARCH` is set |
| Radio stations | no | Streams, through the Music Server's proxy | play no radio |
| Your own [Sentry](https://sentry.io) | `SENTRY_DSN` | Errors, traces, logs and metrics from the Music Server and Desktop core | off without a DSN |

Two more are reached by the clients rather than the server:

- **Radio station icons** load in the Web Client straight from each station's own `faviconUrl`.
- **Google Fonts** serves the Web Client's fonts (`web/src/styles.css`).

The External Source runs yt-dlp on the Music Server's host. You install yt-dlp and you choose the site (for example `EXTERNAL_SOURCE_SEARCH=scsearch`); you are responsible for that source and for the site's terms. Nothing is bundled or enabled by default. See [server/README.md](server/README.md#external-source-yt-dlp).

The album and artist links to Spotify, Qobuz, Last.fm and ListenBrainz are plain links; nothing is sent to these services until you click one.

ADRs: [0003](docs/adr/0003-radio-browser-catalog-and-health.md) (Radio Browser), [0027](docs/adr/0027-artist-photos-from-deezer-biographies-from-wikipedia.md) (Deezer, Wikipedia), [0029](docs/adr/0029-lyrics-from-lrclib.md) (LRCLIB), [0030](docs/adr/0030-play-counts-from-lastfm-with-an-operator-key.md) (Last.fm), [0032](docs/adr/0032-external-source-through-an-operator-installed-yt-dlp.md) (External Source, yt-dlp), [0033](docs/adr/0033-opt-in-sentry-for-the-rust-programs.md) (Sentry).

## Configuration

The Music Server reads its settings from environment variables only; it does not load a `.env` file.

| Variable | Default | Meaning |
|----------|---------|---------|
| `SERVER_ADDR` | `127.0.0.1:8090` | Listen address. Must be a loopback address: the server has no authentication yet. |
| `DATABASE_PATH` | `./data/app.db` | SQLite database. |
| `MANAGED_STORAGE_PATH` | `./data/managed` | Where imported music is kept. |
| `MANAGED_STORAGE_RESERVE_BYTES` | 2 GiB | Free space an import must leave on the storage disk. |
| `MANAGED_IMPORT_FILE_LIMIT_BYTES` | 2 GiB | Largest single file an import accepts. |
| `MANAGED_IMPORT_BATCH_LIMIT_BYTES` | 4 GiB | Largest import batch. |
| `ARTIST_INFO_ENABLED` | `true` | `false` or `0` stops every Deezer, MusicBrainz, Wikidata and Wikipedia request. |
| `ARTIST_INFO_LANGUAGE` | `en` | Wikipedia language for biographies (a language code such as `tr`); falls back to English. |
| `LYRICS_LOOKUP_ENABLED` | `true` | `false` or `0` stops LRCLIB lookups. |
| `LASTFM_API_KEY` | none | Your own Last.fm API key; turns on Track play counts. |
| `EXTERNAL_SOURCE_SEARCH` | none | A yt-dlp search prefix such as `scsearch`; turns on the External Source. |
| `YT_DLP` | `yt-dlp` | The yt-dlp binary, when it is not on `PATH`. |
| `EXTERNAL_SOURCE_EXTRACTOR_ARGS` | none | yt-dlp extractor arguments such as `youtube:skip=hls,dash`; separate several with spaces. |
| `EXTERNAL_SOURCE_NAME` | from the prefix, else `External` | The site's name in the clients' search, such as `Internet Archive`; up to 40 characters. |
| `RADIO_BROWSER_BASE_URL` | `https://de1.api.radio-browser.info` | Radio Browser mirror for the station catalog. |
| `APP_VERSION` | `0.1.0` | Version shown in the health response and Settings. |
| `SENTRY_DSN` | none | Your own Sentry project's DSN; turns on error, trace, log and metric reporting for the Music Server and the Desktop core. See [server/README.md](server/README.md#sentry-optional). |
| `SENTRY_ENVIRONMENT` | `development` (debug build), `production` (release build) | Sentry environment name. |
| `SENTRY_TRACES_SAMPLE_RATE` | `1.0` | Share of transactions sent to Sentry, 0 to 1. |

Where to set them:

- **Development** (`mise run dev`, `dev:web`, `server:dev`, `desktop:dev`): put them in `mise.local.toml`, which is git-ignored and which mise applies to every task. Keep keys such as `LASTFM_API_KEY` there, never in a tracked file:

  ```toml
  [env]
  LASTFM_API_KEY = "your-key"
  ARTIST_INFO_LANGUAGE = "tr"
  ```

- **Docker:** under `environment:` in `docker-compose.yml` (see below).
- **Anything else:** export them in the shell or service unit that starts `bin/server`.

Appearance and playback preferences (theme, layout, seek steps, transition fade) are not environment variables; they are set on the Settings page and stored by the Music Server.

## Docker

`docker-compose.yml` runs the Music Server image (with FFmpeg), which serves the Web Client too. Pull the published one, `ghcr.io/ardamayk/harmony-server` (`HARMONY_IMAGE_TAG` picks `main`, `latest` or a version; unset = `main`), or build your own from `server/Dockerfile`:

```bash
docker compose pull && docker compose up -d   # the published image
docker compose up -d --build                  # your own build
```

Open http://localhost:8090. The container uses the host's network and binds to `127.0.0.1:8090`, because the server accepts only loopback addresses. The database and imported music live in the `app-data` volume (`/data`); add any variable from [Configuration](#configuration) under `environment:`. The image has no yt-dlp by default; to use the External Source, set the `WITH_YT_DLP` build argument to `true` (in Compose: `build.args`) and rebuild, see [server/README.md](server/README.md#external-source-yt-dlp). Docker Compose runs only the Music Server; the Desktop Client runs on the host and connects to it.

## Build and run

```bash
mise run build    # Music Server (bin/server) and Desktop Client
mise run start    # both, as built
```

Mise is the canonical command interface; `mise tasks` lists everything.

| | dev | build | start |
|---|---|---|---|
| Music Server | `server:dev` | `server:build` | `server:start` |
| Desktop Client | `desktop:dev` | `desktop:build` | `desktop:start` |
| both | `dev` | `build` | `start` |
| Music Server + Web Client (Vite :3000, browser) | `dev:web` | | |

The Music Server tasks run it from `server/`, so development and the built binary share `server/data`. `desktop:dev` and `desktop:start` connect to a Music Server that is already running; `dev`, `dev:web` and `start` start their own and stop it when the client exits (`EARTHLY_REUSE_MUSIC_SERVER=1` attaches to one already running instead). In WSL the Desktop Client is the Windows one, elsewhere the Linux one; `EARTHLY_DESKTOP_TARGET=linux` or `windows` overrides that ([desktop/README.md](desktop/README.md#windows)). `mise run web:build` builds the Web Client alone.

## Linux Desktop Client

Run the Music Server and the Electron Desktop Client together in development mode (Vite HMR, debug Rust core):

```bash
mise run dev
```

Build and start the release Desktop Client:

```bash
mise run build
mise run start
```

The Desktop Client connects to a running Music Server whose loopback origin (for example `http://127.0.0.1:8090`) you enter on its connection screen, and keeps its own settings in `~/.config/audio.earthly.desktop`.

**mpv:** native playback needs mpv **0.41.0** exactly. The desktop scripts check `/usr/bin/mpv`, or the binary named by `EARTHLY_AUDIO_MPV_PATH`, and copy it next to the Rust core. mpv runs with `--no-config`, so your own mpv configuration is never loaded. Install it from your distribution (or build that version) before `mise run desktop:dev`.

**Output modes** (the text button in the Player Bar):

| Mode | What it does |
|------|--------------|
| Normal | Plays through PipeWire to the system's active output. |
| Exclusive | Opens the active output's raw ALSA `hw:` device directly, with no PipeWire or ALSA conversion. Not available for Bluetooth or outputs without a raw ALSA device; the client then stays on Normal and says why. |
| Adaptive | Switches the PipeWire graph to each Track's sample rate while playing, and restores it afterwards. Affects the whole system, so it asks for confirmation. |

In Exclusive and Adaptive, mpv stays at full volume and the volume slider moves the device's own volume instead.

**Processing profiles:** Direct plays the decoded audio untouched (no ReplayGain, EQ or software volume). Processed allows ReplayGain (album or track), a ten-band EQ with presets and software volume; turning any of these on while Direct is active switches to Processed with a short notice. Playback is gapless; a new sample rate between Tracks reopens the output device instead of resampling. See [ADR 0007](docs/adr/0007-native-playback-profiles-and-output-policy.md).

See [desktop/README.md](desktop/README.md) for how the Electron shell and the Rust core fit together.

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

## License

Harmony is licensed under the [GNU Affero General Public License v3.0 or later](LICENSE). A Windows package and the server image also carry mpv, FFmpeg and other third-party software under their own licences; see [licenses/README.md](licenses/README.md).

## Developer docs

- [AGENTS.md](AGENTS.md) — conventions for contributors and AI agents
- [contracts.md](contracts.md) — API contract rules
