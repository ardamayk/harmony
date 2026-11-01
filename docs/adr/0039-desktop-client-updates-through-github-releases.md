---
status: accepted
---

# Desktop Client updates through GitHub Releases

Every push to `main` that changes the Desktop Client publishes it to GitHub Releases as `0.1.<workflow run number>`, once CI has passed. Installed clients use electron-updater: they check at start and every six hours, and everything after the check needs a click. Settings is the full home of the control: the current version, Check for updates, Download update, its progress, Restart to update, and a failed state with a retry. The tray mirrors only the one pending action, because it is the only surface while the window is hidden. No system notification announces an update. Restart to update stops the core first, so the Playback Session is saved before the installer kills what runs from the install folder. A downloaded update stays offered, and no check runs, until the restart; if Harmony quits instead, nothing installs and the next start offers the update again. This ends the distribution deferral of [ADR 0024](0024-electron-shell-with-rust-core.md). The repository is public, so clients need no token.

Windows ships an unsigned per-user NSIS installer with the pinned mpv inside. Linux ships an AppImage without mpv, because a build links the distribution's FFmpeg; `resolve_mpv_binary` finds the system mpv, which only has to be 0.38.0 or newer, since 0.38 added the `loadfile` index argument the core sends. The exact pin from [ADR 0005](0005-tauri-client-with-bundled-mpv.md) stays for the mpv Windows bundles: a pin fits a binary we ship, not one the distribution upgrades. A weekly workflow moves that pin to an mpv release at least a week old: it proves each line of the licence source list from the build, runs the new `mpv.exe` on Windows, runs CI on a branch, pushes to `main` and starts the release. Anything it cannot prove, or any failure, opens an Issue instead. It uses `GITHUB_TOKEN`, whose pushes start no workflows, so it starts CI and Release itself through `workflow_dispatch`. electron-builder writes the NSIS uninstaller through Wine, so the Windows job installs it. A `v*` tag still publishes `desktop/package.json`'s version and the Music Server image under it.

## Considered Options

- Release only on tags: rejected because each version would need a manual bump and tag, and the maintainer works directly on `main`.
- Bundle mpv in the AppImage: rejected for now; it would mean building and shipping its FFmpeg stack with it.
- Download in the background and install on quit, the first version of this decision: replaced. Every push to `main` publishes a version and the Windows installer is unsigned, so the listener decides when one arrives, and no update may interrupt playback or restart the app on its own.

## Consequences

Whatever passes CI on `main` is published with no manual gate on the release side, and the listener is the gate on each client. A new mpv reaches a Windows client that accepts the update without a person or a real audio device having tried it. Clients can stay on an old version for as long as nobody clicks, so a Music Server cannot assume they follow it. Windows shows a SmartScreen warning until the installer is signed. CI tests one mpv version, so a newer distribution mpv can differ from what was tested; Ubuntu 24.04's 0.37 is refused at start. Clients released before this change still install on quit, once, into the first version that asks.
