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

Exclusive and Adaptive keep mpv at full volume and move the listener's volume onto the device (the raw ALSA card, or the default PipeWire sink). Each hand-off is recorded in `device-volume-hand-off.json` until it is returned, so a client that was killed has it undone at the next start: mpv comes back at the listener's level first, then the sink gets its volume back (PipeWire restores a card's own volume itself).

## Windows

The same shell and core run on Windows; only the platform layer differs, and Linux keeps everything above.

- mpv IPC is a private named pipe (`\\.\pipe\earthly-audio-mpv-<random>`) instead of a Unix socket, and a kill-on-close Job Object stands in for the `fd://0` lifetime link, so mpv still ends with the core (`core/src/mpv_pipe.rs`).
- Output modes use WASAPI (`core/src/wasapi.rs`). Normal follows the default Windows output. Exclusive resolves the default render endpoint and opens it with mpv's `wasapi/<endpoint>` device and `audio-exclusive`,. Exclusive and Adaptive move the listener's volume onto the endpoint only when the endpoint applies it in hardware (`IAudioEndpointVolume::QueryHardwareSupport`); a software endpoint volume would change the samples like mpv's, so mpv keeps it there. The endpoint volume is system-wide, so the hand-off record above undoes it at the next start if the client was killed. Adaptive sets the default endpoint's shared-mode format (its "Default Format" in Sound settings) to each Track's sample rate, system-wide like the PipeWire graph rate, through `IPolicyConfig` (`core/src/policy_config.rs`). That interface is the one the Sound control panel uses, but Microsoft does not document it, so a Windows release could change it; Adaptive then reports itself unavailable while Normal and Exclusive keep working. The original format is saved in `adaptive-system-rate.windows-format.json` before the first change and restored when Adaptive turns off, the client quits, or it starts again after a crash. A rate the device does not support keeps the original format.
- MPRIS is Linux-only; on Windows media keys and the system media overlay are not wired yet.
- Settings live in `%APPDATA%\audio.earthly.desktop`.

Requirements: Node 24, pnpm, Rust with the `x86_64-pc-windows-gnullvm` target, MSYS2 with the CLANG64 clang, and pinned mpv: the official `mpv-v0.41.0-x86_64-w64-mingw32.zip` release build (it reports `0.41.0-dev-g41f6a6450`, pinned in `core/mpv-windows-version.txt`). Its `mpv.exe` and the DLLs beside it come from `EARTHLY_AUDIO_MPV_PATH` or `core/binaries/`. The Music Server runs elsewhere (Linux or WSL); the connection screen takes its loopback origin, such as WSL's forwarded `http://127.0.0.1:8090`.

The core's TLS stack (AWS-LC under rustls) compiles C, which the scripts build with MSYS2 CLANG64 (`C:\msys64`, or `MSYS2_ROOT`). `.cargo/config.toml` links the C runtime and libunwind statically for that target, so the core needs no MSYS2 DLLs at run time.

```powershell
winget install MSYS2.MSYS2
C:\msys64\usr\bin\bash.exe -lc "pacman -Syu --noconfirm && pacman -S --needed --noconfirm mingw-w64-clang-x86_64-clang mingw-w64-clang-x86_64-lld"
rustup target add x86_64-pc-windows-gnullvm
```

```powershell
pnpm install
pnpm --dir desktop run win:dev     # debug core, Vite HMR, shell
pnpm --dir desktop run win:build   # web/dist, release core with mpv.exe beside it, shell
pnpm --dir desktop run win:start   # the built client
```

`EARTHLY_AUDIO_CORE_PATH` runs a prebuilt core instead, such as one cross-built in WSL with `cargo build --release -p earthly-audio-core --target x86_64-pc-windows-gnu` (needs mingw-w64).
