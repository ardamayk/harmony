# Earthly Audio Desktop

Linux Tauri development client. It builds the shared React source from `web/` and embeds those assets; it never loads UI assets from a Music Server.

Native playback requires pinned mpv `0.41.0`. `mise run desktop:dev` validates `/usr/bin/mpv` (or `EARTHLY_AUDIO_MPV_PATH`), stages an ignored target-specific sidecar, and launches Tauri with `tauri.sidecar.conf.json`. `mise run desktop:build` uses the same exact-version staging path for a release build. No mpv binary is committed to Git. Release binary provenance, licensing review, and distribution bundles remain deferred.

On Linux, the Desktop scripts select the WebKitGTK display mode before Tauri starts. `auto` uses native Wayland with NVIDIA explicit sync disabled on a Wayland/NVIDIA system, native Wayland on other Wayland systems, and X11 otherwise. Override detection with `EARTHLY_AUDIO_DISPLAY_MODE=wayland-nvidia`, `wayland`, `wayland-shm`, or `x11`. The `wayland-shm` mode is a slower fallback for DMA-BUF failures.

The Desktop Client starts one audio-only `--idle=yes` child with `--no-config`, so user mpv configuration is never loaded. Rust resolves an explicitly configured executable first, then a packaged or staged sidecar, then `/usr/bin/mpv`. It owns the private JSON IPC socket inside a random `0700` directory, changes the socket to `0600`, and removes the whole endpoint when the application exits.

Run the Music Server and Desktop Client together:

```bash
mise run desktop:dev
```

Build and run the release Desktop Client with the Music Server:

```bash
mise run build
mise run start
```

Force X11/XWayland or the shared-memory Wayland fallback when needed:

```bash
EARTHLY_AUDIO_DISPLAY_MODE=x11 mise run start
EARTHLY_AUDIO_DISPLAY_MODE=wayland-shm mise run start
```

The connection screen accepts `http://localhost`, `http://127.0.0.1`, or IPv6 loopback origins. Desktop JSON requests use bounded Tauri commands, and covers use a bounded cover-only custom protocol. Track, radio, and HLS audio use a Rust-owned streaming proxy bound to the dedicated `127.0.0.1:43129` origin allowed by the Desktop Content Security Policy. Its URL contains a random 256-bit capability token, serves only streaming API paths, and forwards only to the saved exact Music Server origin. It preserves Range responses, rewrites signed HLS child paths through the tokenized proxy, and streams audio without buffering it in renderer IPC.

The proxy starts with the Desktop Client, rejects redirects and non-media methods, and shuts down with Tauri application state. A second Desktop Client instance or another process occupying port `43129` makes startup fail closed with an actionable error. Bounded command responses retain the 32 MiB safety limit; only HLS manifests are buffered, with a separate 1 MiB limit, so their child URIs can be rewritten safely.

The Desktop Client does not bundle a Go server, database, or server lifecycle.

On Linux, the Player Bar output menu offers Normal, Exclusive, and Adaptive. Normal follows the operating system's active PipeWire output. Exclusive automatically resolves that active output to raw ALSA hardware; it does not show a separate device picker. Outputs without a raw ALSA mapping, including Bluetooth sinks, remain on Normal with an actionable error. Adaptive changes the PipeWire graph rate system-wide and therefore requires confirmation.
