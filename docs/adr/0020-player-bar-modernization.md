---
status: accepted
---

# Player Bar modernization: shared Playback Preferences, native OS integration, lazy waveforms

The Player Bar gained keyboard shortcuts, inline error recovery, a scrubbing seek bar with buffered range and waveform, a sleep timer, playback speed, A-B repeat, queue actions, a Now Playing view with cover-derived accent, OS media integration, and an always-on-top mini player. Three decisions shaped how those pieces fit the self-hosted, two-client architecture.

## Playback Preferences live on the Music Server

Seek steps, default speed, transition fade, waveform, cover accent, up-next, error auto-skip and hover timestamp are stored in a `playback_json` column of `user_preferences` and exposed as a `playback` section of `GET`/`PATCH /api/v1/preferences`, next to theme and layout. The PATCH body uses a sparse form with explicit pointers so a boolean set to false is distinguishable from one that was not sent, and numeric ranges are validated server-side. Both the Web Client and the Desktop Client read the same values, and a client talking to an older server fills the section from defaults.

The one exception is the software volume per output route. ALSA device ids are machine-local, so the Desktop Client remembers the last volume for "system" and for each ALSA device in its own processing settings file and restores it when the output route changes.

## OS integration uses MPRIS on the desktop and the Media Session API on the web

The Desktop Client serves `org.mpris.MediaPlayer2.earthly_audio` through the `mpris-server` crate: pure Rust over `zbus`, which Tauri already pulls in on Linux, so no libdbus C dependency is added. A pure `MprisView` is derived from the playback session and diffed, so only changed properties are announced and large position jumps emit `Seeked`. Incoming commands become `DesktopPlaybackAction`s and are dispatched on a blocking thread because a transition fade may sleep. Album covers are allowed through the private media proxy so desktop widgets can fetch `mpris:artUrl`; the proxy still serves no other library path.

The Web Client drives `navigator.mediaSession` from the browser engine instead. The Desktop Client never registers a Media Session, so there is no double registration.

A real crossfade was rejected: mpv plays one file at a time, and `acrossfade` needs two inputs. The "transition fade" is a volume ramp around user-initiated pause, resume, previous and next; natural gapless boundaries are never faded.

## Waveform peaks are generated lazily and cached by file identity

`GET /api/v1/library/tracks/{trackId}/waveform` decodes the managed file with ffmpeg to mono 8 kHz PCM and folds it into 400 normalized peaks in one streaming pass, so memory stays flat for long tracks. Peaks are cached in `track_waveforms` together with the source file's size and modification time; a replaced file simply misses the cache and is decoded again, without a hook into Managed Import. Generation is single-flight per track with at most two decoders at once. A request that outlasts two seconds answers `202` with `Retry-After` and the client polls. The `track-waveform.v1` capability is advertised only when the ffmpeg Server Dependency is present, so clients hide the feature cleanly on servers without it.

## Mini player is a second app window under the same policy

The mini player is a dynamically created, undecorated, always-on-top Tauri window that loads the bundled app at `/mini`. It shares playback truth through the existing `desktop-playback-state` broadcast and needs no extra playback plumbing. The renderer capability now targets both app windows with the same minimal permissions, and the permissions test pins that deliberately.
