# Earthly Audio Desktop

Linux Electron development client (see [ADR 0024](../docs/adr/0024-electron-shell-with-rust-core.md)). It renders the shared React source from `web/` and serves it from `app://earthly`; it never loads UI assets from a Music Server.

```text
src/main/      Electron main process: windows, tray, pickers, app:// protocol, CSP, IPC
src/preload/   sandboxed preload exposing window.earthlyDesktop (@repo/desktop-bridge)
core/          Rust core, started as a child process: mpv playback, media proxy,
               Music Server HTTP bridge, Managed Import, MPRIS, output modes
```

The shell and the core speak line-delimited JSON-RPC over the core's stdin and stdout (`core/src/rpc.rs`, `src/main/core-process.ts`). The renderer sends the same commands it always has; the main process forwards those on the allowlist in `packages/desktop-bridge` to the core and answers window, picker and link commands itself. Closing the core's stdin makes it save the Playback Session and stop mpv, so quitting the shell (tray, Ctrl+C, session end) always goes through it.

Run the Music Server and Desktop Client together (Vite HMR, debug core):

```bash
mise run desktop:dev
```

Build and run the release Desktop Client with the Music Server:

```bash
mise run build
mise run start
```

`desktop:test` runs the core's Rust tests and the shell's Vite+ tests; `desktop:check` runs rustfmt and Clippy (the shell's TypeScript is checked by `web:check`).

Native playback requires pinned mpv `0.41.0`. The desktop scripts validate `/usr/bin/mpv` (or `EARTHLY_AUDIO_MPV_PATH`) and stage it as an ignored sidecar in `core/binaries`; the release build puts it next to the core binary. No mpv binary is committed to Git. Release binary provenance, licensing review, and distribution bundles remain deferred.

The core starts one audio-only `--idle=yes` child with `--no-config`, so user mpv configuration is never loaded. It resolves an explicitly configured executable first, then the sidecar beside the core, then the staged one, then `/usr/bin/mpv`. It owns the private JSON IPC socket inside a random `0700` directory, changes the socket to `0600`, and removes the whole endpoint when the core exits.

The renderer runs sandboxed with context isolation and no Node.js, under a CSP that allows only its own bundle and the media proxy origin. The connection screen accepts `http://localhost`, `http://127.0.0.1`, or IPv6 loopback origins. Desktop JSON requests go through bounded core commands. Covers, track, radio, and HLS audio use a Rust-owned streaming proxy bound to the dedicated `127.0.0.1:43129` origin. Its URL contains a random 256-bit capability token, serves only streaming and cover API paths, and forwards only to the saved exact Music Server origin. It preserves Range responses, rewrites signed HLS child paths through the tokenized proxy, and streams audio without buffering it in renderer IPC.

The proxy starts with the core, rejects redirects and non-media methods, and shuts down with it. A second Desktop Client instance shows the running one's window instead; another process occupying port `43129` makes startup fail closed with an actionable error. Bounded command responses retain the 32 MiB safety limit; only HLS manifests are buffered, with a separate 1 MiB limit, so their child URIs can be rewritten safely.

Settings live in `~/.config/audio.earthly.desktop` (server connection, Playback Session snapshot, processing settings, mini player placement), the same directory the Tauri client used.

The Desktop Client does not bundle a Go server, database, or server lifecycle.

On Linux, the Player Bar output menu offers Normal, Exclusive, and Adaptive. Normal follows the operating system's active PipeWire output. Exclusive automatically resolves that active output to raw ALSA hardware; it does not show a separate device picker. Outputs without a raw ALSA mapping, including Bluetooth sinks, remain on Normal with an actionable error. Adaptive changes the PipeWire graph rate system-wide and therefore requires confirmation.
