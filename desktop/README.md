# Harmony Desktop

Linux Electron development client (see [ADR 0024](../docs/adr/0024-electron-shell-with-rust-core.md)). It renders the shared React source from `web/` and serves it from `app://earthly`; it never loads UI assets from a Music Server.

```text
src/main/      Electron main process: windows, tray, pickers, app:// protocol, CSP, IPC
src/preload/   sandboxed preload exposing window.earthlyDesktop (@repo/desktop-bridge)
core/          Rust core, started as a child process: mpv playback, media proxy,
               Music Server HTTP bridge, Managed Import, MPRIS, output modes
```

The shell and the core speak line-delimited JSON-RPC over the core's stdin and stdout (`core/src/rpc.rs`, `src/main/core-process.ts`). The renderer sends the same commands it always has; the main process forwards those on the allowlist in `packages/desktop-bridge` to the core and answers window, picker and link commands itself. Closing the core's stdin makes it save the Playback Session and stop mpv, so quitting the shell (tray, Ctrl+C, session end) always goes through it.

Run the Music Server and Desktop Client together (Vite HMR, debug core); `mise run desktop:dev` runs the client alone against a Music Server that is already running:

```bash
mise run dev
```

Build and run the release Desktop Client with the Music Server (`mise run desktop:start` starts the client alone):

```bash
mise run build
mise run start
```

`desktop:test` runs the core's Rust tests and the shell's Vite+ tests; `desktop:check` runs rustfmt and Clippy (the shell's TypeScript is checked by `web:check`).

Native playback on Linux requires mpv `0.38.0` or newer (`core/mpv-minimum-version.txt`), because 0.38 added the `loadfile` index argument the core sends. The desktop scripts validate `/usr/bin/mpv` (or `EARTHLY_AUDIO_MPV_PATH`) and stage it as an ignored sidecar in `core/binaries`; the release build puts it next to the core binary. No mpv binary is committed to Git. Packages and updates: `.github/workflows/release.yml` and [ADR 0039](../docs/adr/0039-desktop-client-updates-through-github-releases.md). The Linux AppImage uses the system mpv. CI tests against the version in `core/mpv-version.txt`.

The core starts one audio-only `--idle=yes` child with `--no-config`, so user mpv configuration is never loaded. It resolves an explicitly configured executable first, then the sidecar beside the core, then the staged one, then `/usr/bin/mpv`. It owns the private JSON IPC socket inside a random `0700` directory, changes the socket to `0600`, and removes the whole endpoint when the core exits.

The renderer runs sandboxed with context isolation and no Node.js, under a CSP that allows only its own bundle and the media proxy origin. The connection screen accepts an `https` origin, or `http://localhost`, `http://127.0.0.1`, or IPv6 loopback origins. For an `https` origin it also takes custom request headers, for a proxy that asks for them ([ADR 0038](../docs/adr/0038-owner-access-through-a-signing-proxy.md)). The shell keeps them in `server-headers.bin`, encrypted with Electron `safeStorage` under the operating system's keychain, and refuses to save them if only plain text storage is available; the core adds them to requests for that one origin. The renderer never reads them back. Desktop JSON requests go through bounded core commands. Covers, track, radio, and HLS audio use a Rust-owned streaming proxy bound to the dedicated `127.0.0.1:43129` origin. Its URL contains a random 256-bit capability token, serves only streaming and cover API paths, and forwards only to the saved exact Music Server origin. It preserves Range responses, rewrites signed HLS child paths through the tokenized proxy, and streams audio without buffering it in renderer IPC.

The proxy starts with the core, rejects redirects and non-media methods, and shuts down with it. A second Desktop Client instance shows the running one's window instead; another process occupying port `43129` makes startup fail closed with an actionable error. Bounded command responses retain the 32 MiB safety limit; only HLS manifests are buffered, with a separate 1 MiB limit, so their child URIs can be rewritten safely.

Settings live in `~/.config/audio.earthly.desktop` (server connection, Playback Session snapshot, processing settings), the same directory the Tauri client used.

The Desktop Client does not bundle a Go server, database, or server lifecycle.

On Linux, the Player Bar output menu offers Normal, Exclusive, and Adaptive. Normal follows the operating system's active PipeWire output. Exclusive automatically resolves that active output to raw ALSA hardware; it does not show a separate device picker. Outputs without a raw ALSA mapping, including Bluetooth sinks, remain on Normal with an actionable error. Adaptive changes the PipeWire graph rate system-wide and therefore requires confirmation.

Exclusive and Adaptive keep mpv at full volume and move the listener's volume onto the device (the raw ALSA card, or the default PipeWire sink). Each hand-off is recorded in `device-volume-hand-off.json` until it is returned, so a client that was killed has it undone at the next start: mpv comes back at the listener's level first, then the sink gets its volume back (PipeWire restores a card's own volume itself).

## Windows

The same shell and core run on Windows; only the platform layer differs, and Linux keeps everything above.

- mpv IPC is a private named pipe (`\\.\pipe\earthly-audio-mpv-<random>`) instead of a Unix socket, and a kill-on-close Job Object stands in for the `fd://0` lifetime link, so mpv still ends with the core (`core/src/mpv_pipe.rs`).
- Output modes use WASAPI (`core/src/wasapi.rs`). Normal follows the default Windows output. Exclusive resolves the default render endpoint and opens it with mpv's `wasapi/<endpoint>` device and `audio-exclusive`,. Exclusive and Adaptive move the listener's volume onto the endpoint only when the endpoint applies it in hardware (`IAudioEndpointVolume::QueryHardwareSupport`); a software endpoint volume would change the samples like mpv's, so mpv keeps it there. The endpoint volume is system-wide, so the hand-off record above undoes it at the next start if the client was killed. Adaptive sets the default endpoint's shared-mode format (its "Default Format" in Sound settings) to each Track's sample rate, system-wide like the PipeWire graph rate, through `IPolicyConfig` (`core/src/policy_config.rs`). That interface is the one the Sound control panel uses, but Microsoft does not document it, so a Windows release could change it; Adaptive then reports itself unavailable while Normal and Exclusive keep working. The original format is saved in `adaptive-system-rate.windows-format.json` before the first change and restored when Adaptive turns off, the client quits, or it starts again after a crash. A rate the device does not support keeps the original format.
- MPRIS is Linux-only; on Windows media keys and the system media overlay are not wired yet.
- Settings live in `%APPDATA%\audio.earthly.desktop`.

The Windows client is built entirely inside WSL; there is no Windows clone or Windows toolchain.

- The core is cross-built for `x86_64-pc-windows-gnu` with WSL's mingw-w64 and needs no extra DLLs at run time.
- Pinned mpv is the official `mpv-<version>-x86_64-w64-mingw32.zip` release build; `mpv-update.yml` moves it to a new release. It is downloaded once into `~/.cache/earthly-audio-windows` and checked against its SHA-256. It reports `0.41.0-dev-g41f6a6450`, pinned in `core/mpv-windows-version.txt`, and `mpv.exe` ships with the DLLs beside it.
- electron-builder packages the shell with its `dir` target (`electron-builder.yml`). It neither signs nor edits the executable; `scripts/desktop-windows/icon.ts` sets the icon with resedit. A release adds `--installer` for the NSIS target, whose uninstaller needs Wine.
- The result is copied to `%LOCALAPPDATA%\EarthlyAudio` (`EARTHLY_AUDIO_WINDOWS_OUT` overrides it) and started from there through WSL interop.

```text
Harmony.exe
resources/app/       package.json, dist/ (shell bundle), resources/ (icons)
resources/web/dist/  the Web Client
resources/core/      earthly-audio-core.exe, mpv.exe and its DLLs
```

Requirements: WSL with mingw-w64 (`sudo apt install mingw-w64`), the Rust target (`rustup target add x86_64-pc-windows-gnu`), and the repository's mise toolchain. The Music Server runs in WSL or elsewhere; the connection screen takes its loopback origin, such as WSL's forwarded `http://127.0.0.1:8090`.

In WSL the `desktop:*` tasks, and `dev`, `build` and `start` with them, build and run this Windows client; elsewhere they use the Linux one. `EARTHLY_DESKTOP_TARGET=linux` picks the Linux client in WSL too; `windows` works only in WSL.

```bash
mise run desktop:build   # web/dist, shell, release core, pinned mpv; package and install
mise run desktop:start   # launch the installed client
mise run desktop:dev     # installed client with a fresh shell, debug core, and Vite HMR from WSL
mise run desktop:clean   # remove target/desktop-windows and the installed client
```

`mise run dev` and `mise run start` also run the Music Server in WSL and stop it when the client exits. `desktop:dev` needs a previous `desktop:build`. It replaces the shell bundle and the core inside the installed client and passes `EARTHLY_AUDIO_DEV_SERVER_URL` to it through `WSLENV`. Quit the client before rebuilding: Windows locks the files of a running program.

Installers are not built yet; an electron-builder NSIS target would come later.

### Releases

A `v*` tag matching `desktop/package.json`'s version publishes this package as a GitHub Release once CI passes (`.github/workflows/release.yml`): an unsigned, portable `Harmony-<version>-windows-x64.zip` and its SHA-256, without automatic updates. Bump the version, commit, then tag:

```bash
git tag v0.1.0 && git push origin v0.1.0
```

Running the workflow by hand builds the same zip as a workflow artifact and publishes nothing.

## Sentry (optional)

The core reports panics, mpv process failures, each Playback Source load (source type and trigger, never its URL) and each audio stream through the media proxy (source type, byte range and what it waited on) to your own Sentry only when `SENTRY_DSN` is set; `SENTRY_ENVIRONMENT` and `SENTRY_TRACES_SAMPLE_RATE` work as for the Music Server ([server/README.md](../server/README.md#sentry-optional), [ADR 0033](../docs/adr/0033-opt-in-sentry-for-the-rust-programs.md)). The shell starts the core with its own environment, unfiltered:

- **Linux:** set them where the client starts, for example in `mise.local.toml` for `mise run desktop:dev`. With `mise run dev` the Music Server shares that file, so set `DESKTOP_SENTRY_DSN` too: `scripts/desktop.sh` then gives the core that DSN and leaves `SENTRY_DSN` to the server.
- **Windows:** set them as user environment variables for the installed client (`mise run desktop:start` passes none from WSL). `mise run desktop:dev` forwards those set in WSL through `WSLENV`.

An invalid value leaves Sentry off and the core running; it says so on stderr.
