# Native iPhone client (parked)

Phones use the Web Client as a PWA (ADR 0041), so phone UI and iOS playback work belong in `web/` and `packages/ui`. This folder holds no code. It keeps the plan for the day the PWA is not enough, for the maintainer's iPhone 17 on iOS 26 or 27.

## Ladder

Each rung replaces the one above it only when that rung's trigger is met on the real iPhone. The rung in use today is PWA.

| Rung | Stack | Survives from the rung above | Climb when |
| --- | --- | --- | --- |
| PWA | Web Client in `web/`, `<audio>` | - | In use |
| Capacitor | `web/dist` in a WKWebView, native Player plugin on AVFoundation | All UI and logic | The lock-screen gate in `PWA.md` fails, or offline Tracks outgrow browser storage |
| React Native | Native views in TypeScript | `@repo/api-client`, playback logic | The WebView's scrolling or gestures fail on the real library, and Android is wanted too |
| Swift | SwiftUI, AVFoundation | `packages/contracts/openapi.yaml` | The same WebView failure with iPhone as the only target, or CarPlay or widgets are wanted |

Rungs from Capacitor down need Xcode on a Mac and an Apple signing identity. A build signed for free expires after 7 days, so daily use needs the paid Apple Developer Program.

## Rules for when a rung starts

- **The seam is `PlaybackEngine`** in `packages/ui/src/playback/PlaybackEngine.ts`. Capacitor replaces the implementation behind it, as `DesktopPlaybackEngine` does on the desktop.
- **The Music Server stays client-neutral.** A server change goes through `openapi.yaml` and, where clients must detect it, a Server Capability (ADR 0006).
- **Sound moves only on Play** (ADR 0040).
- **The iPhone decides "done".** Anything touching audio, lock screen, install or sign-in is finished once checked on the installed app, with the exact iOS version recorded. Playwright's WebKit on Linux checks layout only.
- **Diagnostics stay self-hosted**, through the Music Server's `client_metrics` module.
