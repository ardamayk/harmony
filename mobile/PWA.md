# PWA notes

What an iPhone PWA of Harmony has to get right, and the test that decides whether it is enough. The app itself is the Web Client (ADR 0041). Statements about iOS marked _reported_ come from bug reports and developer write-ups, and nobody has checked them on this iPhone yet.

## Lock-screen gate

Keeping music going on a locked phone is the job a music app cannot fail, and it is where a PWA is weakest. [WebKit bug 261858](https://bugs.webkit.org/show_bug.cgi?id=261858) describes the failure: in a Home Screen app a Track ends, the next one does not start, and lock-screen controls stop answering until the app is reopened. It has been open and unassigned since 2023. The reporter sees it only in the installed app, not in Safari.

Install the deployed Web Client, play an Album, lock the phone, and check:

1. At least 30 minutes of consecutive Tracks advance alone while locked.
2. Lock-screen previous, next, play and pause work, including after 10 minutes paused.
3. The Desktop Client keeps showing the iPhone as the Active Device with a moving playhead. ADR 0040 reads a report older than 20 seconds as paused, so throttled timers would show here.
4. Playback survives a phone call, and a Wi-Fi to cellular handover mid-Track.
5. A Track plays after a cold relaunch, through the signing proxy.

If 1 or 2 still fails after the workarounds below, the rung has failed: climb to Capacitor. If 3 fails alone, fix it here, since it is a Harmony behavior.

## Player on iOS

`web/src/playback/BrowserPlaybackEngine.ts` already has the right shape: one `Audio` element and full Media Session handlers. These are the iPhone gaps to work through during the gate.

```ts
// Declares this page a media player to iOS.
if ("audioSession" in navigator) navigator.audioSession.type = "playback";
```

- **Track change.** _Reported:_ iOS closes the audio session at a Track's natural end in the background and refuses to reopen it until the app is foregrounded. The reported fix sets `media.loop = true`, so `ended` never fires, and advances on detecting the jump to 0:00. `handleEnded` holds repeat and stop-after-current, so that logic must run from the new trigger too. `setMediaSource` also calls `removeAttribute("src")` and `load()` between Tracks; test assigning `src` directly.
- **Lock-screen buttons.** The engine registers `seekbackward` and `seekforward`. _Reported:_ iOS then shows 10-second skips, and ignores handlers registered before the element is Now Playing. The reported fix re-registers on every `playing` event:

  ```ts
  session.setActionHandler("seekbackward", null);
  session.setActionHandler("seekforward", null);
  session.setActionHandler("previoustrack", () => this.previous());
  session.setActionHandler("nexttrack", () => this.next());
  ```

- **Volume.** iOS ignores `audio.volume`; the hardware buttons own it. Leave the slider out.
- **Radio.** `setMediaSource` prefers hls.js when supported, which on iPhone means ManagedMediaSource. Prefer native HLS on iOS, and test a station while locked.
- **Formats.** The server streams original files and does not transcode, so a format iOS cannot decode fails with a playback error, and FLAC on cellular costs what FLAC costs. Check the formats in the real library with `canPlayType`. Transcoding is a separate server decision, taken if cellular use hurts.
- **Silent `play()`.** _Reported:_ on iOS 26 `play()` can resolve in an installed PWA without producing sound ([field report](https://forums.macrumors.com/threads/ios-26-audio-issues-in-pwa-web-apps-not-fixed-in-26-1-or-26-2-but-much-better.2466839/)). Watch for `currentTime` not advancing after a resume.

Silent audio loops and timers that poke the player are out: they fight iOS instead of working with it.

## Install surface

iOS 26 opens every site added to the Home Screen as a web app by default. `web/public/manifest.json` needs:

```json
{
	"id": "/",
	"name": "Harmony",
	"short_name": "Harmony",
	"start_url": "/",
	"scope": "/",
	"display": "standalone",
	"background_color": "#0B0B12",
	"theme_color": "#0B0B12",
	"icons": [
		{ "src": "logo192.png", "sizes": "192x192", "type": "image/png" },
		{ "src": "logo512.png", "sizes": "512x512", "type": "image/png" }
	]
}
```

`web/index.html`:

```html
<meta
	name="viewport"
	content="width=device-width, initial-scale=1, viewport-fit=cover"
/>
<link rel="apple-touch-icon" href="/apple-touch-icon.png" />
<meta name="apple-mobile-web-app-title" content="Harmony" />
<meta
	name="apple-mobile-web-app-status-bar-style"
	content="black-translucent"
/>
```

Layout uses `100dvh` and pads with `env(safe-area-inset-top)` and `env(safe-area-inset-bottom)`. The iPhone 17 viewport is 402 × 874 points. The Now Playing bar sits above the bottom inset.

## Sign-in

Outside loopback the Owner's listener demands a token from the signing proxy (ADR 0038). The Desktop Client sends custom headers, but an `<audio>` element cannot, so the proxy has to work from a browser login and its cookie. Three things to check on the phone:

- An installed app keeps cookies apart from Safari, so sign in inside the installed app. A sign-in page on another host may open in an in-app browser sheet; confirm the cookie lands in the app.
- iOS loads `<audio>` through its media stack, so confirm the stream request carries that cookie. Gate check 5 covers this.
- When the proxy session expires, `/api` calls fail with a cross-origin redirect rather than a clean 401, which looks the same as the server being down. Reload the top-level page once so the proxy can show its login, with a `sessionStorage` flag cleared by the next successful call so an outage cannot loop. Set the proxy session long enough that this is rare.

## Service worker

iOS installs a PWA from its manifest alone, so the first version ships without a service worker. HTTP caching already keeps `assets/` for good, and the persisted Query cache already starts the Library fast. A worker would add real failure modes: a cached shell hiding the proxy's login, and stale builds after a deploy.

It arrives with offline, under these rules:

- Same-origin only and hand-written.
- `/api/` passes through untouched, except downloaded Tracks. It must never see the Queue event stream.
- Safari asks for audio with Range requests, so a saved Track must be answered with a correct `206` from the worker.
- Call `navigator.storage.persist()`, and show what is saved so a user can see if iOS removed it.

If the storage proves unreliable for a real library, that is the Capacitor trigger.

## Testing

- **Logic:** `vp test`, like the rest of the workspace.
- **Layout:** Playwright at 402 × 874 against WebKit. It runs no iOS audio, no lock screen and no install.
- **Real behavior:** the installed app, on a deployed build over the public HTTPS host. `docs/testing/` already holds matrices of this kind; add `mobile-lock-screen-matrix.md` with the iOS version per run.
- **Debugging without a Mac:** Safari's Web Inspector for a phone requires one. Instead, keep a small on-screen ring buffer of `audio` events, `visibilitychange` and Media Session calls behind a Settings toggle, and send timing through `client_metrics`.

## Sources

- [WebKit bug 261858: home-screen audio stops at Track end](https://bugs.webkit.org/show_bug.cgi?id=261858)
- [iOS 26 PWA audio field reports](https://forums.macrumors.com/threads/ios-26-audio-issues-in-pwa-web-apps-not-fixed-in-26-1-or-26-2-but-much-better.2466839/)
- [Lock-screen write-up: audio session, handlers](https://amsyeung.medium.com/getting-my-iphone-music-app-to-keep-playing-on-the-lock-screen-a5fb15311002)
- [Home Music issue 327: debugging the same failure](https://github.com/felipe-urgal/home-music/issues/327)
- [WebKit: every Home Screen site opens as a web app in iOS 26](https://webkit.org/blog/16993/news-from-wwdc25-web-technology-coming-this-fall-in-safari-26-beta/)
- [Safari 27.0 feature notes](https://webkit.org/blog/18325/webkit-features-for-safari-27-0/), [Safari 27.2](https://webkit.org/blog/18515/webkit-features-for-safari-27-2/): no change to this picture found
- [Audio Session support in Safari](https://web-platform-dx.github.io/web-features-explorer/features/audio-session)
