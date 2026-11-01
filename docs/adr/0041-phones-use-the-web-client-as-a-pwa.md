---
status: accepted
---

# Phones use the Web Client as a PWA

Harmony had one unwritten rule for phones: the Web Client skips them, and a separate app comes later. That left an iPhone with nothing. Harmony's React pages and playback stack already run in a phone browser; only the layout stops it, because `AppShell` refuses to be narrower than 768px and clips at phone width.

So the Web Client covers phones, installed to the Home Screen as a PWA.

## How it works

- **One app, two shells.** Below 768px, or on a touch screen under 500px tall, `AppShell` renders a phone shell in place of the Top Nav, Sidebar, Player Bar and Queue Drawer. Routes, pages, queries and playback stay the same.
- **Desktop layouts are untouched.** The desktop shell is never narrower than 768px and the Desktop Client's window has that minimum, so the phone shell only runs where the desktop shell could not.
- **Preferences stay per shell.** The phone shell never writes layout preferences, because they live on the Music Server and the desktop reads them.
- **No service worker.** iOS installs from the manifest alone, and a worker would hide the signing proxy's login ([ADR 0038](0038-owner-access-through-a-signing-proxy.md)). It comes with offline playback, if that is ever wanted.

## Considered Options

- A separate app in `mobile/`, served at `/m/`: rejected. It repeats every page, and needs its own server route, Docker step and shared packages, to reach the same screens.
- Breakpoints inside the desktop shell: rejected. The Queue Drawer reserves its width at every size, and the Player Bar has no room for its controls at 402px.
- Capacitor now: rejected. It needs a Mac, and a PWA has to fail on the real iPhone first. `mobile/` keeps that plan.

## Consequences

- Hover-only actions and fixed-width pages need touch and phone layouts, in `web/src` and `packages/ui/src`.
- iOS gaps in `BrowserPlaybackEngine` are handled there, behind iOS detection, since Safari's lock-screen behavior differs from every other browser's.
- `docs/design-rules.md` still says no phone layouts. The maintainer owns that file and updates it.
- Lighthouse measures a mobile profile too.
