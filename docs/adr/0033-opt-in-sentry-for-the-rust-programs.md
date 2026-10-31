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

The Music Server refuses to start with an invalid value, like its other settings. The Desktop core reports the problem on stderr and runs without Sentry: a telemetry setting must not stop playback.

The release is `earthly-music-server@<APP_VERSION>` and `earthly-audio-core@<crate version>`. Both use the `sentry` crate 0.49.3 with its default features off: `reqwest` with `rustls` as the transport (no native-tls or OpenSSL; reqwest and rustls are already in the workspace and cross-build for `x86_64-pc-windows-gnu`), plus `backtrace`, `contexts`, `panic`, `metrics`, and on the server `logs` and `tracing`. `debug-images` (the loaded libraries' paths) and `release-health` (sessions) are left out.

## Signals

| Signal | Rust SDK | Music Server | Desktop core |
| --- | --- | --- | --- |
| Errors and panics | yes | `error` tracing lines and panics, including those `CatchPanicLayer` answers with 500 | panics, the core failing, the mpv process failing (one issue), a Playback Source failing to start |
| Tracing | yes | one transaction per request named `<method> <route template>`; External Source search, resolve, stream and artwork as child spans with yt-dlp's queue, run and parse, the resolve and upstream first byte as their children; a resolve that joins another request's yt-dlp run has only its wait (`external_source.resolve.wait`), and the run is recorded once, by the request that started it | one transaction per Playback Source load, `loadfile` to playing, failed or replaced, with `file-loaded` as a child span |
| Logs | yes (0.42+) | `info` and `warn` tracing lines, also kept as breadcrumbs | not sent: the core writes plain stderr lines, not tracing |
| Metrics | yes (0.48+) | distributions `external_source.{search,resolve,artwork,stream}.duration`, `external_source.yt_dlp.{queue,duration}`, `external_source.stream.ttfb`, `external_source.stream.bytes`, `import.{upload,inspect,confirm,confirm_batch}.duration`, `import.upload.bytes`, `track.stream.{ttfb,duration,bytes}`, `cover.scale.duration`, `waveform.generate.duration`, `outbound.request.duration` by host, and the `client.*` metrics below; counter `external_source.resolve.cache` | distributions `playback.load.duration`, `playback.load.file_loaded` |
| Profiling | **no** | not available | not available |

Sentry has no profiler for Rust: its profiling covers Python, Node.js, mobile, browser and .NET SDKs, and the Rust SDK removed its experimental profiling in 0.31.6. Native crash minidumps (`sentry-minidump`, new in 0.49.3) are not used: they need a separate crash-handler process.

The request transaction ends when the response starts, so a stream's transaction measures up to its first byte; the stream's length and bytes are metrics recorded when its body ends. Each request polls in its own Sentry hub, so concurrent requests do not share a scope.

## The Web and Desktop Clients

Neither client talks to Sentry. Their renderer measures the page with the browser's own Performance API (`client.page.{ttfb,fcp,lcp,cls}`, the longest interaction as `client.interaction.inp`, and `client.playback.start` in the browser) and posts the numbers to `POST /api/v1/client-metrics` on the Owner's listener, which passes known names on as metrics. So there is no browser SDK, no DSN in a client and no third party, and a server without `SENTRY_DSN` drops them. Guests have no such route and stop reporting at the first refusal.

## What is never sent

`send_default_pii` stays off and the request body is never read. On top of that:

- **Requests:** the transaction holds the method, the route template (`/api/v1/external/tracks/{external_track_id}/stream`), the status and nothing else: no URL, query string, path ids, headers, cookies or client address. Events carry no request or user.
- **Free text:** before an event or log leaves the process, every URL becomes `<url>` and every word holding `/` or `\` becomes `<path>`, in messages, exception values, breadcrumbs, tags, extra data, tracing fields and log attributes. A path's directories and file name can hold spaces and punctuation (`C:\Users\Jane Mary Smith\...`, `Smith, Jane - Guns N' Roses.flac`) and no word tells where it ends, so `<path>` runs from the first word holding a separator to the end of the text, stopping only before a URL or an API path: whatever follows a path in the same text is lost, no part of a path is left. An `/api/` path loses its query and keeps only its route words (`api`, `v1`, `tracks`, `stream` and the others in the contract); every other segment becomes `<id>`, because External Track ids and HLS resource tokens decode to page and media URLs, so `/api/v1/external/tracks/<id>/stream` is what remains. This covers the yt-dlp, stream and artwork URLs, media proxy URLs with their tokens, and music library paths.
- **External Source:** search queries are never recorded, only their length, as in its log lines (ADR 0032). The free-text `error` of an External Source line becomes `<redacted>` in Sentry, because yt-dlp's output can repeat the query or a media id; the server's own log keeps it.
- **Host:** events name `music-server` or `desktop-core` instead of the host's name.
- **Spans:** only explicit spans and the Music Server's own tracing spans are sent, never a dependency's, whose fields could hold URLs.

The Rust SDK has no `before_send_transaction`, so transactions are kept clean at the source: they are built only from the data above.

## Considered Options

- `sentry-tower`'s `SentryHttpLayer`: rejected, it attaches the full URL with its query and the request headers to the transaction, and no hook can strip them.
- OpenTelemetry to Sentry: rejected, more moving parts for one owner's server; the SDK's own tracing layer covers it.
- Sentry in the Electron shell and the Web Client: out of scope here; the JavaScript SDKs would need their own opt-in.
