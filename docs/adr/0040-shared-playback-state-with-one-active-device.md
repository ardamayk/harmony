---
status: accepted
---

# Shared Playback State with one Active Device

Supersedes the device-local Playback Session of [ADR 0004](0004-separate-server-and-device-local-playback.md) and [ADR 0010](0010-shared-queue-revisions-and-events.md). The Music Server still produces no sound, and the Queue and its revisions stand.

One person using Harmony on a desktop, a browser and a phone expects one Player, not three. Today each Playback Client holds its own Track and playhead: the Web Client forgets both on reload, the Desktop Client restores only its own, and two clients on the same Queue play over each other.

So the Music Server holds one **Shared Playback State** per user, and exactly one Playback Client, the **Active Device**, produces sound. Every other client is **Following**: it shows the same Track and playhead in silence and can control the Active Device.

## How it works

- **Shared Playback State.** The current Queue item and Track, the playhead at a server timestamp, play or pause, the Active Device's ID and name, and a revision. Unlike Room State it is stored in SQLite, so it survives a server restart and a client closing. Volume, Output Mode, processing, shuffle and repeat stay on the device.
- **Becoming Active.** Starting playback on a client makes it the Active Device: pressing Play, or playing a Track, Album or Playlist. A Following client that presses Play starts its own Player at the shared playhead, and the previous Active Device goes silent. Sound moves only this way. Pressing Play is a user gesture, so browsers and phones allow it.
- **Remote control.** Pause, seek and skip from a Following client are commands: the Active Device obeys them and sound stays where it is. Pause from a phone therefore pauses the Desktop Client. With nothing playing, skip has no Player to obey it, so the client that pressed it takes over paused at the next Track.
- **The Active Device reports.** It writes the state when it pauses, seeks or changes Track, and every 5 seconds while playing. A report is based on a revision, and a report older than the latest remote command is ignored, so a heartbeat in flight cannot undo a remote pause.
- **A dead Active Device.** A report older than 20 seconds means the state reads as paused at the last reported playhead. Closing the browser or losing the network therefore leaves the Track and playhead ready for the next client to continue.
- **Sync.** Commands are `POST` requests. Full snapshots reach clients over the existing Queue event stream as a `playback-state` event, as with Room State, so a reconnecting client starts from the truth. The snapshot carries the server's time; a Following client only displays the playhead and takes it over, so one offset without clock sampling is enough.
- **Skip is relayed.** The Active Device owns shuffle and repeat, so the server does not compute the next Track. It passes the request on and the Active Device moves and reports.
- **Other Playback Sources.** Radio, External Tracks and Catalog Previews make their client the Active Device and silence the others. Following clients show that it is playing elsewhere, not what. They are not restored, as before.
- **Listen Together.** A Room suspends this: a client in a Room neither reports nor follows. The Shared Playback State stays as it was for afterwards. A Guest has none: the Guest Listener refuses these routes, and Room behavior in [ADR 0037](0037-listen-together-rooms-on-the-owners-music-server.md) is unchanged.
- **Compatibility.** The Music Server advertises `playback.shared-session.v1`. Against a server without it, clients behave as before ([ADR 0006](0006-capability-based-client-server-compatibility.md)).

## Client structure

One `PlaybackEngine` wraps the local engine. While Active it passes everything through; while Following it presents the Shared Playback State as its session and turns controls into commands. `PlaybackProvider`, the Player Bar and the Queue need almost no change.

The Desktop core reports and obeys, not the renderer, because Background Playback continues with the main window closed ([ADR 0009](0009-rust-owned-desktop-playback-session.md)): a pause from the phone must stop mpv then too. The renderer of the Desktop Client only follows and claims.

Each browser tab is its own device, because two tabs are two Players. The ID lives in `sessionStorage`, so a reload keeps it.

## Considered Options

- A Room for one user: rejected. In a Room every Player makes sound, and Room State is lost on restart.
- Saving only the last position: rejected. It covers coming back later, but not following, remote pause, or one output.
- Keeping a Following Player loaded and muted: rejected. It streams audio nobody hears, and it needs `play()` without a gesture, which browsers refuse.
- Moving sound automatically to the client just opened: rejected. Opening a tab on the phone must not stop the music on the desktop.
- Computing skip on the server: rejected. The server does not know the Active Device's shuffle.

## Consequences

- A Playback Session is still device-local for what a Player does, but where the Track and playhead are is now the Music Server's. The Playback Session Snapshot remains the fallback when the server cannot be reached.
- Queue behavior from ADR 0010 stands: a playing Track continues if its Queue item is removed elsewhere.
- Every playing client writes one row every 5 seconds. That is small for SQLite and only the Active Device does it.
- A Following client's volume and shuffle controls change only that device, and take effect when it becomes Active.
- OS media keys and the tray on a Desktop Client that is Following do nothing: only the app itself can take the sound over.
