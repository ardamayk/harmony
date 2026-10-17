# Tauri client with bundled UI and mpv

> The Tauri shell is superseded by [ADR 0024](0024-electron-shell-with-rust-core.md); the bundled UI, pinned mpv and separate Music Server decisions stand.

The Linux-first Desktop Client uses Tauri, bundles the React UI built from the same source as the Web Client, and connects to a configurable, separately deployed Music Server. It also bundles a pinned mpv executable and controls it as an isolated child process through JSON IPC instead of linking libmpv or implementing a custom audio engine. The first release is development-only, restricts the unauthenticated Music Server to the local machine, and targets Playback Parity across library Tracks, saved Radio Stations, and Catalog Previews.

## Considered Options

- Load the Music Server's remotely hosted UI: rejected because remote content must not receive the Desktop Client's native privileges and client/server UI versions would be coupled.
- Link libmpv into the Desktop Client: rejected because native linking and cross-platform ABI management add complexity without enough initial benefit.
- Build a custom native audio engine: rejected because codec, output-device, gapless, ReplayGain, DSP, and passthrough behavior would become application-owned work.

## Consequences

The Music Server and Desktop Client are separate release artifacts. Desktop packaging owns the mpv binary, licensing review, updates, process lifecycle, and Linux audio-output verification. Native playback provides System Output, Direct ALSA Output, and Adaptive System Rate alongside device selection, observable format reporting, weak gapless playback, ReplayGain, and DSP/EQ. Compressed surround passthrough is not part of the first release. Distribution packaging remains deferred until development builds prove the architecture on real hardware.
