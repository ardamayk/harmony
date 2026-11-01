---
status: accepted
---

# Opt-in Sentry for the Rust programs

The owner wants errors, slow paths and the External Source's timings from their own Music Server and their own Desktop Client in one place, without reading JSON log lines. They run a self-hosted [Sentry](https://sentry.io).

So the two Rust programs, the Music Server (`server/`) and the Desktop core (`desktop/core`), can report to Sentry, and **only when `SENTRY_DSN` is set**. The repository is public and people download it; it must never report from their installations. There is no DSN in the code, no default DSN, and no build flag that turns it on: without `SENTRY_DSN` no Sentry client, transport or tracing layer exists and nothing leaves the process, as with the Last.fm key in [ADR 0030](0030-play-counts-from-lastfm-with-an-operator-key.md). The DSN is the owner's, for the owner's self-hosted Sentry, set in their own environment (`mise.local.toml`, the service unit, the Windows user environment), never in a tracked file.

| Variable | Default | Meaning |
| --- | --- | --- |
| `SENTRY_DSN` | none | The project's DSN, `https://<public-key>@<your-sentry-host>/<project-id>`. Unset, Sentry is off. |
| `SENTRY_ENVIRONMENT` | `development` for a debug build, `production` for a release build | 1 to 64 letters, digits, `.`, `_` or `-`. |
| `SENTRY_TRACES_SAMPLE_RATE` | `1.0` | The share of transactions sent, 0 to 1. One owner's traffic is small, so all of it by default. |
| `DESKTOP_SENTRY_DSN` | none | Read only by `scripts/desktop.sh`, which gives the core this DSN instead of `SENTRY_DSN`, so `mise run dev` keeps the two programs in their own Sentry projects. |

The Music Server refuses to start with an invalid value, like its other settings. The Desktop core reports the problem on stderr and runs without Sentry: a telemetry setting must not stop playback.

The release is `earthly-music-server@<APP_VERSION>` and `earthly-audio-core@<crate version>`. Both use the `sentry` crate 0.49.3 with its default features off: `reqwest` with `rustls` as the transport (no native-tls or OpenSSL; reqwest and rustls are already in the workspace and cross-build for `x86_64-pc-windows-gnu`), plus `backtrace`, `contexts`, `panic`, `metrics`, and on the server `logs` and `tracing`. `debug-images` (the loaded libraries' paths) and `release-health` (sessions) are left out.

## Signals

| Signal | Rust SDK | Music Server | Desktop core |
| --- | --- | --- | --- |
| Errors and panics | yes | `error` tracing lines and panics, including those `CatchPanicLayer` answers with 500 | panics, the core failing, the mpv process failing (one issue), a Playback Source failing to start |
| Tracing | yes | one transaction per request named `<method> <route template>`; External Source search, resolve, stream and artwork as child spans with yt-dlp's queue, run and parse, the resolve and upstream first byte as their children; a resolve that joins another request's yt-dlp run has only its wait (`external_source.resolve.wait`), and the run is recorded once, by the request that started it; every query on the pool as `db.sql.query` with its SQL text; `db.transaction.write`, `db.transaction.read` and `db.connection` for several statements on one connection, with the wait for it (`db.wait`, for a write the wait on SQLite's write lock) and `db.commit` as children; `http.client` for each outside service, by host; `radio.upstream`, `cover.scale`, `waveform.generate` with `waveform.queue` and `waveform.ffmpeg`, `import.inspect`, `artist_info.wait` for a rate limit's queue; `track.stream.body` as a transaction of its own in the request's trace | one transaction per Playback Source load, `loadfile` to playing, failed or replaced, with `playback.close_previous`, `playback.file_loaded` and `playback.open_file` as child spans; one `media.stream <source type>` per audio stream in the load's trace, with the wait for the Music Server's headers and first byte as child spans; one `playback.output_mode <mode>` per Output Mode command, as long as the user waits for it, rollback included |
| Logs | yes (0.42+) | `info` and `warn` tracing lines, also kept as breadcrumbs | not sent: the core writes plain stderr lines, not tracing |
| Metrics | yes (0.48+) | distributions `external_source.{search,resolve,artwork,stream}.duration`, `external_source.yt_dlp.{queue,duration}`, `external_source.stream.ttfb`, `external_source.stream.bytes`, `import.{upload,inspect,confirm,confirm_batch}.duration`, `import.upload.bytes`, `track.stream.{ttfb,duration,bytes}`, `cover.scale.duration`, `waveform.generate.duration`, `outbound.request.duration` by host, and the `client.*` metrics below; counter `external_source.resolve.cache` | distributions `playback.load.duration`, `playback.load.file_started`, `playback.load.file_loaded`, `playback.output_mode.duration` by the modes, whether a Player was sounding and the outcome, `media.stream.duration`, `media.stream.server_wait_longest`, `media.stream.player_wait_longest` |
| Profiling | **no** | not available | not available |

Sentry has no profiler for Rust: its profiling covers Python, Node.js, mobile, browser and .NET SDKs, and the Rust SDK removed its experimental profiling in 0.31.6. Native crash minidumps (`sentry-minidump`, new in 0.49.3) are not used: they need a separate crash-handler process.

The request transaction ends when the response starts, so a stream's transaction measures up to its first byte; `track.stream.body` covers the rest, recorded when the body ends. A long stream's question is who it waited on, so the body's transaction, like `media.stream` in the Desktop core, splits its time between the source's next chunk and the reader taking the last one, with the longest of each, and says how it ended; a stream left open counts as the reader's wait. Together they tell a stalled Music Server or network from an mpv that stopped reading, and `playback.close_previous` tells mpv still closing the last file from mpv opening this one. The core sends `sentry-trace` with each audio stream, without a sampling decision, and the Music Server continues that trace, so a load, its streams and their requests read as one trace. A stream opened after a load belongs to that load's trace, because mpv opens streams on its own and no request names its load. Each request polls in its own Sentry hub, so concurrent requests do not share a scope. The transport sends one envelope at a time, so the Music Server queues up to 1024 of them instead of the SDK's 30, which one page load can fill.

## The Web and Desktop Clients

Neither client talks to Sentry. Their renderer measures the page with the browser's own Performance API (`client.page.{ttfb,fcp,lcp,cls}`, the longest interaction as `client.interaction.inp`, `client.playback.start` in the browser, and `client.playback.takeover` in both: the time from pressing Play on a Following client until its playhead moves, labelled with the client the sound moved to) and posts the numbers to `POST /api/v1/client-metrics` on the Owner's listener, which passes known names on as metrics. So there is no browser SDK, no DSN in a client and no third party, and a server without `SENTRY_DSN` drops them. Guests have no such route and stop reporting at the first refusal.

## What is never sent

`send_default_pii` stays off and the request body is never read. On top of that:

- **Requests:** the transaction holds the method, the route template (`/api/v1/external/tracks/{external_track_id}/stream`), the status and nothing else: no URL, query string, path ids, headers, cookies or client address. Events carry no request or user.
- **Free text:** before an event or log leaves the process, every URL becomes `<url>` and every word holding `/` or `\` becomes `<path>`, in messages, exception values, breadcrumbs, tags, extra data, tracing fields and log attributes. A path's directories and file name can hold spaces and punctuation (`C:\Users\Jane Mary Smith\...`, `Smith, Jane - Guns N' Roses.flac`) and no word tells where it ends, so `<path>` runs from the first word holding a separator to the end of the text, stopping only before a URL or an API path: whatever follows a path in the same text is lost, no part of a path is left. An `/api/` path loses its query and keeps only its route words (`api`, `v1`, `tracks`, `stream` and the others in the contract); every other segment becomes `<id>`, because External Track ids and HLS resource tokens decode to page and media URLs, so `/api/v1/external/tracks/<id>/stream` is what remains. This covers the yt-dlp, stream and artwork URLs, media proxy URLs with their tokens, and music library paths.
- **External Source:** search queries are never recorded, only their length, as in its log lines (ADR 0032). The free-text `error` of an External Source line becomes `<redacted>` in Sentry, because yt-dlp's output can repeat the query or a media id; the server's own log keeps it.
- **Host:** events name `music-server` or `desktop-core` instead of the host's name.
- **Spans:** only explicit spans and the Music Server's own tracing spans are sent, never a dependency's, whose fields could hold URLs. A query's SQL text is sent because it cannot hold a value: sqlx takes only literal SQL outside the migrations, and values are `?`. Statements inside a transaction are not spans of their own, only the transaction is: they run on sqlx's connection, past the pool. `radio.upstream` names no host, since a station's names what is playing.

The Rust SDK has no `before_send_transaction`, so transactions are kept clean at the source: they are built only from the data above.

## Considered Options

- `sentry-tower`'s `SentryHttpLayer`: rejected, it attaches the full URL with its query and the request headers to the transaction, and no hook can strip them.
- OpenTelemetry to Sentry: rejected, more moving parts for one owner's server; the SDK's own tracing layer covers it.
- Sentry in the Electron shell and the Web Client: out of scope here; the JavaScript SDKs would need their own opt-in.
