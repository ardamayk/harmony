---
status: accepted
---

# Desktop Client updates through GitHub Releases

Every push to `main` that changes the Desktop Client publishes it to GitHub Releases as `0.1.<workflow run number>`, once CI has passed. Installed clients use electron-updater: they check at start and every six hours, download a newer version in the background, show a system notification, and install when Harmony quits. This ends the distribution deferral of [ADR 0024](0024-electron-shell-with-rust-core.md). The repository is public, so clients need no token.

Windows ships an unsigned per-user NSIS installer with the pinned mpv inside. Linux ships an AppImage without mpv, because the pinned build links the distribution's FFmpeg; `resolve_mpv_binary` finds the system mpv, which must be 0.41.0. electron-builder writes the NSIS uninstaller through Wine, so the Windows job installs it. A `v*` tag still publishes `desktop/package.json`'s version and the Music Server image under it.

## Considered Options

- Release only on tags: rejected because each version would need a manual bump and tag, and the maintainer works directly on `main`.
- Bundle mpv in the AppImage: rejected for now; it would mean building and shipping its FFmpeg stack with it.

## Consequences

Whatever passes CI on `main` reaches installed clients; there is no manual gate. Windows shows a SmartScreen warning until the installer is signed. A system mpv upgrade past 0.41.0 makes Settings report it as unpinned on Linux. Harmony stays in the tray, so an update waits for an explicit quit.
