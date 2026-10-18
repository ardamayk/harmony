---
status: accepted
---

# Electron shell around the web UI, with the Rust layer as a headless core

The Desktop Client's shell moves from Tauri to Electron. Electron's main process (TypeScript on Node 24, bundled with `vp pack`) owns the windows, tray, native file pickers, external links, single-instance lock and application lifetime, and serves the built web UI (`web/dist`, the same TanStack Router SPA as the Web Client) from `app://earthly`. The former Tauri crate becomes `desktop/core`, a Tauri-free binary the shell starts as a child process. It keeps everything [ADR 0009](0009-rust-owned-desktop-playback-session.md) gave the Rust layer: the Playback Session and mpv lifecycle, the tokenized media proxy, the Music Server HTTP bridge ([ADR 0008](0008-platform-specific-desktop-transport.md)), Managed Import uploads, MPRIS, Exclusive and Adaptive outputs.

Shell and core speak line-delimited JSON-RPC over the core's stdin and stdout. Requests carry the same command names and camelCase arguments the renderer always used; responses carry the command's own result or error value; events (`desktop-playback-state`, Queue events, upload progress) are forwarded to every renderer window. Window, tray and lifetime decisions stay in the core's playback logic and reach the shell as `shell:*` events; tray clicks and the close button come back as `shell_action`. Closing stdin quits like the tray's Quit: the Playback Session is saved and mpv stops before the core exits, so a crashed shell never leaves the core running.

The renderer is untrusted content: sandboxed, context-isolated, without Node.js, under the previous CSP, with one preload API (`window.earthlyDesktop`) whose command allowlist lives in `@repo/desktop-bridge`. Core methods only the shell may call (registering picked file paths, shell actions) are not on it, so renderer content still handles only opaque import selection IDs.

## Considered Options

- Port the Rust layer to TypeScript in the main process: rejected for now; about 18k lines of tested playback, output-device and import code would be rewritten for no user-visible gain.
- Load the Rust layer into Electron as a napi-rs addon: rejected because it ties the core to Electron's ABI and rebuilds, and a crash in native code would take the shell down with it.
- Stay on Tauri: rejected because WebKitGTK on Linux kept costing workarounds (the NVIDIA/Wayland DMA-BUF crash in `docs/research/tauri-wayland-dmabuf.md`, custom-scheme image limits, repaint costs) and renders differently from the Chromium browsers the Web Client is checked in.

## Consequences

The Desktop Client now ships Chromium, which costs disk and memory compared with the system webview but renders like the Web Client. Electron frameless windows resize from their edges, so the page keeps only its rounded frame; drag regions use `-webkit-app-region`. The settings directory (`audio.earthly.desktop`), its files and the fixed media proxy port are unchanged. Distribution packaging remains deferred, as in [ADR 0005](0005-tauri-client-with-bundled-mpv.md).

Supersedes the Tauri shell choice in [ADR 0005](0005-tauri-client-with-bundled-mpv.md); its bundled UI, pinned mpv and separate Music Server decisions stand.
