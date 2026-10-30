---
status: accepted
---

# Listen Together through Rooms on the Owner's Music Server

Friends want to listen to one person's library at the same time: the same Track at the same playhead, where either side can pause, seek or pick the next Track, and opening the lyrics opens them for everyone. Protected hosting alone does not give this. A friend who can reach the Music Server can browse and play, but each Playback Session stays independent, so nothing keeps them together.

So the Owner opens a **Room** on their own Music Server and shares an **Invite Link**. A **Guest** who opens the link in a browser gets the Owner's Web Client, sees the Owner's library, and shares the Room's playback. There is no central service and no server-to-server link: a Guest needs no Music Server of their own.

This first version covers joining from the Web Client only. The Desktop Client does not join Rooms yet.

## How it works

- **Listen Together page.** A new entry in the left bar opens it, for the Owner only. The "Listen with friends in my library" action opens a Room and shows the Invite Link, the Guests, and a way to close the Room.
- **Joining.** The Invite Link has the form `https://<server>/join#<token>`. A Guest does not paste it into an app; they open it in the browser, and "Join" on that page gives the browser the click it needs to allow audio. The token sits in the fragment so it never reaches server or proxy logs. The client exchanges it for a Guest session in an `HttpOnly` cookie and removes it from the address bar. The cookie also covers `<audio>` and `EventSource`, which cannot send an `Authorization` header. The page and the API share one origin, so CORS does not change.
- **Guest interface.** The whole Web Client shows the Owner's library, and a persistent bar names whose library it is and offers "Leave". Actions a Guest may not take are hidden. There is no second, Room-only library UI.
- **Guest permissions.** A Guest may read the library, stream Tracks, read lyrics, and send Room commands. Playlists stay private, so Library Search leaves them out for a Guest. Everything else, such as Managed Import, deletion, Radio and settings, is refused by the Music Server through a deny-by-default allowlist; hiding the controls is only a convenience. Preferences a Guest changes stay on their device.
- **Room State.** The Music Server holds the current Track, the playhead at a server timestamp, play or pause, whether the lyrics are open, and the Room Queue, with a revision. Rooms live in memory: a server restart ends them. A Personal Music Server has one Room at a time.
- **Sync.** Commands are `POST` requests and the Room State reaches clients as full snapshots over server-sent events, the same transport as Queue Synchronization. The Desktop Client, whose renderer cannot open a stream to the Music Server, polls the Room State twice a second instead, so a change reaches it up to 500 ms late. A snapshot lets a late joiner or a reconnecting client start from the truth, and missed intermediate events do not matter. Each client estimates its clock offset to the server, derives the expected playhead, and seeks its own Player when it drifts too far.
- **Players stay local.** Every Player still produces sound on its own device, as in [ADR 0004](0004-separate-server-and-device-local-playback.md), and the Music Server never commands one. One client-side sync layer in `packages/ui` compares the Room State with the local playhead and drives the existing `PlaybackEngine` interface, so the Web Client's audio element and the Desktop Client's mpv follow the same code and mpv needs no Room-specific protocol. Correction is by seeking only: changing the speed would break the Direct Profile's promise, and sample-accurate sync is not a goal. Output latency, such as Bluetooth, is not compensated.
- **Local controls go through the Room.** During a Room, pause, seek and Track changes become Room commands, including those from MPRIS, Media Session and media keys, which reach the Player without passing through the UI. The sync layer treats a local change it did not make as a command. Playback speed stays at 1.
- **Room Queue.** A Room has its own Queue, so a Guest never changes the Owner's. When the Room closes, the Owner's Queue is as it was. Playing from an Album, Playlist or list replaces the Room Queue; there is no command to add, remove or reorder Tracks in it, and a Room has no shuffle or repeat.
- **Playback Sources.** A Room plays library Tracks only. Radio is live and specific to a user, and External Tracks and Catalog Previews are outside this first version.
- **Lyrics.** Whether they are open moves from local state in the Player Bar to the Room State during a Room. Clicking a synced line is a Room seek.
- **Reaching the server.** The Owner's listener stays exactly as it is: loopback only, unauthenticated, and never exposed. When `GUEST_ADDR` and `PUBLIC_URL` are both set, the Music Server opens a second loopback listener for Guests, and the operator's reverse proxy or tunnel points there and carries outside traffic over HTTPS. `PUBLIC_URL` names the public address: the Guest listener accepts only that host and origin, and the Invite Link is built from it. The Guest listener serves the Web Client, joining, and the Guest allowlist, and it has no Owner routes at all. Without both settings there is no reachable address, so the Music Server does not advertise `listen-together.v1`, and the page explains why instead of producing a link nobody can open.
- **Invite Links expire**, and closing the Room revokes the link and every Guest session at once. A Room closes itself after 24 hours; `LISTEN_TOGETHER_ROOM_HOURS` sets that from 1 to 168, because a link that lasts longer than a week stops being a link for one occasion.
- **Guest limit.** A Room admits at most 8 Guests, and `LISTEN_TOGETHER_MAX_GUESTS` may lower that. The limit keeps a Room within a circle of friends, which is also why Rooms are never listed or discoverable, and it bounds the Owner's upload bandwidth. A Guest who arrives at a full Room is refused, and a Guest who leaves frees their place.
- **Rate limits.** The Guest Listener faces the internet, so anyone holding an Invite Link could otherwise run ffmpeg and outside lookups on the Owner's server without end. Each Guest has budgets that answer `429 rate_limited` when spent, as Room commands do, and the Owner's listener has none. Each budget is a burst and then a steady rate, sized so a person in a browser never meets it:

  | Budget | Burst | Then, per second | Covers |
  | --- | --- | --- | --- |
  | Library | 1000 | 60 | Library lists and details, covers, search, `me`, preferences, and stream requests, so a seek costs one read |
  | Artist photos | 120 | 10 | One grid asks for 100 photos at once, and each is looked up online at most once |
  | Lookups | 40 | 2 | Waveforms, lyrics, biographies, popular Tracks, play counts and online genres: the reads that run ffmpeg or call an outside service |
  | Room | 20 | 20 | Room State, the event stream and the clock, kept apart so that browsing cannot cost a Guest their sync |

  Popular Tracks and play counts are Lookups although they read like library routes, because they call Deezer and Last.fm. Room commands keep their own limit of 20 a second, a read not named here counts as a library read, and photos have a budget of their own so that an Artist grid does not spend the waveform of the Track that is playing.
- **Audio streams.** A Guest holds at most 4 open audio streams; a further Track is refused with `429 rate_limited`, and a stream gives its place back when it ends or its connection closes. A seek is a new range request while the player or the proxy may still hold the previous one, so refusing at the limit would stop the music. At the limit, a request for a Track that is already streaming therefore replaces that Track's oldest stream and is never refused. `HEAD` sends no audio and takes no place.
- **Joining is limited for everyone together.** `POST join` takes a burst of 20 and then one attempt every 5 seconds, valid or not, whether or not a Room is open. Behind a proxy the Music Server cannot trust a client address, so the limit cannot be per client: a stranger hammering the route can keep friends out until they stop. The tokens are 256 random bits, so the limit is not what stops guessing. It is slow because a new session starts with full budgets, and leaving and joining again must not be a way to refill them.

## The Desktop Client

The Desktop Client can open a Room and take part as the Owner, because it plays the Owner's library anyway. Joining someone else's Room is not built. The Listen Together page shows "Connect to a friend's library" there as a disabled, greyed-out placeholder that cannot be clicked, and no Desktop-side join code exists behind it.

The sync layer lives in the renderer, while the Rust core owns playback and continues after the main window closes ([ADR 0009](0009-rust-owned-desktop-playback-session.md)). If the renderer is not running during Background Playback, that Player stops following the Room until the window returns, and it catches up from the next snapshot.

Joining from the Desktop Client later needs a temporary Server Connection that leaves the saved one intact, and a Playback Session Snapshot that never records another server's Track. The Web Client shows no such button: opening the link is how it joins.

## Considered Options

- Server-to-server federation, where the Guest's Music Server talks to the Owner's: rejected. It requires every Guest to run a server, and it solves nothing that a browser opening the Owner's address does not.
- A central relay run by the project: rejected. It would make a self-hosted product depend on a service, and put the project between listeners and their music.
- Protected hosting with accounts and no Room: rejected. It gives access but no shared playback, and asks friends to create accounts for one evening.
- Owner sign-in on one shared listener: rejected. A proxied request also comes from loopback, so every local client, including the Desktop core, would need credentials, and a password would face the internet. A separate Guest listener gives the same protection with no login surface.
- Showing the friend's library only inside the Listen Together page: rejected. It rebuilds every library screen, while switching the whole interface reuses them.
- A paste-a-link field in the Web Client: rejected. It would either navigate to the link, which the address bar already does, or call another origin, which needs CORS changes for no gain.
- WebSocket: rejected for now, although `contracts.md` used to expect it for party mode. Room commands are infrequent, and server-sent events with `POST` already fit the server's patterns and need no second realtime transport.

## Consequences

- This extends [ADR 0004](0004-separate-server-and-device-local-playback.md) and [ADR 0010](0010-shared-queue-revisions-and-events.md) rather than contradicting them. Outside a Room, Playback Sessions stay independent; inside one, each Player follows the Room State.
- Exposing the Music Server is the main risk, and the two listeners contain it: loopback is what makes a request the Owner's, and that stays true because the tunnel reaches only the Guest listener. An operator who points a tunnel at `SERVER_ADDR` instead hands the whole server to the internet, so the documentation has to say so plainly.
- The Owner opens Rooms from the machine that runs the Music Server, or through their own SSH tunnel to it. Owner sign-in is not needed for this and is left to a later decision.
- The operator must make the server reachable. A home connection behind CGNAT cannot use port forwarding and needs a tunnel, and Listen Together uses the Owner's upload bandwidth once per Guest, at the Source Audio Format's bitrate.
- Anyone holding the Invite Link is a Guest until it expires or the Room closes, so it has to be shared like a password.
- The rate limits bound what one Guest costs, not what eight do, and they live in memory like the Room. The budgets are constants, not settings: nobody should have to tune them for a circle of friends.
