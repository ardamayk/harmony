---
status: accepted
---

# Desktop Client updates through GitHub Releases

Every push to `main` that changes the Desktop Client publishes it to GitHub Releases as `0.1.<workflow run number>`, once CI has passed. Installed clients use electron-updater: they check at start and every six hours, download a newer version in the background, show a system notification, and install when Harmony quits. This ends the distribution deferral of [ADR 0024](0024-electron-shell-with-rust-core.md). The repository is public, so clients need no token.

Windows ships an unsigned per-user NSIS installer with the pinned mpv inside. Linux ships an AppImage without mpv, because a build links the distribution's FFmpeg; `resolve_mpv_binary` finds the system mpv, which only has to be 0.38.0 or newer, since 0.38 added the `loadfile` index argument the core sends. The exact pin from [ADR 0005](0005-tauri-client-with-bundled-mpv.md) stays for the mpv Windows bundles: a pin fits a binary we ship, not one the distribution upgrades. A weekly workflow moves that pin to an mpv release at least a week old: it proves each line of the licence source list from the build, runs the new `mpv.exe` on Windows, runs CI on a branch, pushes to `main` and starts the release. Anything it cannot prove, or any failure, opens an Issue instead. It uses `GITHUB_TOKEN`, whose pushes start no workflows, so it starts CI and Release itself through `workflow_dispatch`. electron-builder writes the NSIS uninstaller through Wine, so the Windows job installs it. A `v*` tag still publishes `desktop/package.json`'s version and the Music Server image under it.

## Considered Options

- Release only on tags: rejected because each version would need a manual bump and tag, and the maintainer works directly on `main`.
- Bundle mpv in the AppImage: rejected for now; it would mean building and shipping its FFmpeg stack with it.

## Consequences

Whatever passes CI on `main` reaches installed clients; there is no manual gate, and a new mpv reaches Windows clients without a person or a real audio device having tried it. Windows shows a SmartScreen warning until the installer is signed. CI tests one mpv version, so a newer distribution mpv can differ from what was tested; Ubuntu 24.04's 0.37 is refused at start. Harmony stays in the tray, so an update waits for an explicit quit.
