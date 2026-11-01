# Harmony

Harmony is a self-hosted music app for browsing local library content and listening to live radio streams.

## Language

### Library

**Library Search**:
A query across library Tracks, Albums, Artists, Genres, and Playlists. Every result must match at least one query word in its own title or name; remaining words may match its searchable credits or other related fields. Relationships alone never add results. Radio search is separate.
_Avoid_: Radio search, global catalog search, per-page filter

**External Source**:
An optional, operator-configured search of one site through the operator's own yt-dlp (ADR 0032). It is off unless the operator configures it, and it is separate from Library Search.
_Avoid_: YouTube search, online library, catalog

**External Track**:
A result of the External Source that is not in the library. It can be played in full through the Music Server, but it is never stored, and adding it to the library is the listener's own import.
_Avoid_: Library Track, preview clip, download

**Best Match**:
The single library result highlighted above search groups because it strongly matches the query directly. It also appears first in its own category, counting toward that category's five-result limit. A result found only through typo correction is not a Best Match.
_Avoid_: First Track, most popular result, related result

**Personal Music Server**:
A self-hosted Music Server operated for one person's library in the initial product. Future user profiles may isolate multiple personal libraries, but the initial product is not a shared or multi-tenant music host.
_Avoid_: Public music server, multi-tenant library

**Managed Import**:
A user-initiated transfer in which a Playback Client uploads an audio file and the Music Server retains an authoritative managed copy after validation and explicit confirmation.
_Avoid_: Folder scan, referenced file, automatic import

**Local Folder Import**:
A Managed Import initiated by selecting a directory on a Playback Client and recursively uploading its supported contents. The client directory and its hierarchy are not retained as a library source or storage structure.
_Avoid_: folder scan, preserved folder hierarchy

**Managed Storage**:
The Music Server-owned location that holds one authoritative audio file per Track in its Source Audio Format and, when available, one artwork file in each Album's directory alongside its music files.
_Avoid_: Source folder, Playlist folder, symlink view

**Bit-Preserving Storage**:
Managed Storage that retains the exact uploaded audio-file bytes and verifies them by full-file SHA-256 before and after canonical placement. It does not imply an end-to-end bit-perfect Playback guarantee.
_Avoid_: Transcoded storage, rewritten tags, bit-perfect Playback guarantee

**Import Preview**:
The validation result shown before a Managed Import is committed. It identifies accepted and rejected files without creating library content until the user confirms.
_Avoid_: Completed import, scan result

**Strict Import Profile**:
The acceptance contract for Managed Import covering audio integrity and file-tag metadata: Title, Artist, Album Artist, Album, and Track number are required, with a Disc number required for multi-disc Albums and assumed to be one for single-disc Albums. Genre, year, and Album Artwork are optional; missing required metadata is rejected rather than inferred from filenames or an online service.
_Avoid_: Best-effort scan, legacy exception, filename fallback, tag rewriting

**Import Batch**:
A user-selected group of files processed through one Import Preview, potentially spanning multiple automatically grouped Albums, single Tracks, or incomplete Albums. Each selected file commits independently, so one failure does not roll back files already imported successfully.
_Avoid_: Atomic batch, folder scan, Playlist

**Import History**:
The retained results of the most recent Managed Imports without their staged audio files. It supports reviewing outcomes but not resuming or restoring an import.
_Avoid_: Staging storage, retry queue, library history

**Exact Duplicate**:
A proposed import whose full-file content hash matches an existing library Track. It is not imported again, regardless of its filename, metadata, source location, or Album.
_Avoid_: Possible Duplicate, same title, same recording

**Track Replacement Candidate**:
A proposed import with a different full-file content hash that matches an existing Track's Album, Disc number, Track number, and normalized Title. Differences in Artist credits or Source Audio Format do not prevent a match and require review before a Track Replacement.
_Avoid_: Possible Duplicate, Recording Duplicate, automatic upgrade

**Import File Alternatives**:
Files in one Import Batch with different full-file content hashes that propose the same Album, Disc number, Track number, and normalized Title. The user chooses one file for that Album position and the others are skipped, without an automatic quality ranking.
_Avoid_: Exact Duplicate, automatic upgrade, first-upload-wins

**Album Position Conflict**:
A proposed import occupying the same Album, Disc number, and Track number as another Track but carrying a different normalized Title. It prevents that file from being imported until resolved and is distinct from an Exact Duplicate.
_Avoid_: Duplicate, automatic replacement

**Source Audio Format**:
The audio file's existing supported format, which a Managed Import preserves without transcoding.
_Avoid_: Import format, normalized format

**Artist**:
A credited performer associated with a Track, an Album, or both. Structured multi-value credits create ordered Artist relationships without guessing from punctuation inside a display credit.
_Avoid_: Album Artist, free-text artist label, Composer

**Album Artist**:
The Artist credit under which an Album is grouped. It is required independently from each Track's Artist credit and is never inferred from it.
_Avoid_: Track Artist, fallback Artist, Composer

**Artist Photo**:
An Artist's picture, looked up on Deezer through one of their Albums on first request and stored on the Music Server; without one the Artist shows their initial.
_Avoid_: artist image, avatar, Album Artwork

**Artist Biography**:
An Artist's Wikipedia summary, reached through MusicBrainz and Wikidata on first request, stored, and always shown with its source link.
_Avoid_: bio, artist description

**Album**:
A grouping of Tracks matched by Album Artist credits and the full Album title, independent of year and each Track's Artist credits, with case and redundant whitespace ignored. Edition qualifiers remain part of the title, so "Album X" and "Album X (Deluxe)" are separate Albums; users may explicitly create a separate Album for a differently intended edition with otherwise matching tags.
_Avoid_: Playlist, folder, import batch, grouping by Track Artist

**Album Artwork**:
The optional validated front-cover image for an Album, chosen from valid embedded covers or supplied by the user as JPEG or PNG. An existing cover is preserved during Managed Import, a coverless Album may receive a cover with user confirmation, and an Album without artwork uses a default cover in clients.
_Avoid_: Required embedded artwork, per-track cover copy

**Track**:
A library item backed by one authoritative playable audio source. Playlist and genre membership reference the Track without creating additional audio-file copies.
_Avoid_: File copy, playlist entry, Radio Station

**Managed Track**:
A Track backed by a Music Server-owned audio file in Managed Storage.
_Avoid_: client-local file

**Genre**:
A normalized classification referenced by one or more Tracks. An Album's displayed Genres are derived from its active Tracks without copying audio files.
_Avoid_: Playlist, folder, symlink view

**Track Replacement**:
An explicit user-confirmed change of a Track's managed audio file that preserves the Track's identity and adopts the replacement file's validated metadata, whether its Source Audio Format is the same or different. Declining replacement skips the incoming file and preserves the existing Track; retaining both versions requires a separate Album.
_Avoid_: Exact duplicate, automatic replacement, separate import

**Canonical Library Path**:
A Music Server-generated, human-readable Managed Storage path whose stable IDs determine identity while metadata-derived slugs aid inspection. Client filenames and folder hierarchies never determine it.
_Avoid_: Client path, identity-by-filename, source hierarchy

**Playlist**:
A user-created named collection of library Tracks. Managed Imports do not create Playlists automatically.
_Avoid_: Album, genre, import batch, Queue

**Permanent Track Deletion**:
An irreversible removal of both a Track and its managed audio file from the Music Server. Deleting an Album is not a separate concept: it is one Permanent Track Deletion per Track in that Album, previewed and confirmed once at the Album level, with each Track committing independently so one failure stops the run without undoing the Tracks already deleted. The Album record and its artwork disappear when their last Track does.
_Avoid_: Remove from Playlist, hide Track, trash, Album delete as an atomic unit

### Playback

**Music Server**:
The authoritative source for library content and user-shared data. It serves Playback Clients but does not produce sound for them.
_Avoid_: Player, Playback Client, audio device

**Server Connection**:
The Desktop Client's configured relationship to one Music Server URL. The initial product does not discover servers or retain an offline library.
_Avoid_: Server Profile, local server, offline library

**Local-only Deployment**:
A Music Server deployment bound to the local machine's loopback interface. The initial product does not authenticate Playback Clients and cannot be reached from other devices.
_Avoid_: LAN deployment, internet-ready deployment, trusted client identity

**Guest Listener**:
A second loopback address of a Music Server that exposes only what a Guest may use, and that an operator-chosen reverse proxy or tunnel carries to the internet over HTTPS.
_Avoid_: Public music server, port forwarding, LAN deployment

**Server Capability**:
A named behavior advertised by a Music Server so a Playback Client can adapt when their release versions differ.
_Avoid_: Client version, feature assumption

**Server Dependency**:
An external program or configured service the Music Server relies on at runtime, such as ffmpeg or fpcalc, reported with whether it is required, present, and which version was found. It describes the deployment environment, not a Server Capability, and its absence never changes the advertised Server Capabilities.
_Avoid_: Server Capability, feature flag, plugin

**Playback Client**:
An app instance on a user device that browses the Music Server and plays audio on that device. Web Client and Desktop Client are Playback Client variants.
_Avoid_: Music Server, UI, frontend

**Web Client**:
A Playback Client used through a web browser, or installed from one to a phone's Home Screen. Its playback remains local to that device.
_Avoid_: Desktop Client, server player

**Desktop Client**:
An installed Playback Client with native playback capability. It connects to a separate Music Server and does not contain or start one.
_Avoid_: Music Server, standalone server, web wrapper

**Player**:
The audio engine local to a Playback Client. Each Playback Client owns its Player and playback position independently.
_Avoid_: Music Server, Playback Client, queue

**Native Playback**:
Playback performed by the Desktop Client's local Player through the host operating system's audio facilities.
_Avoid_: Browser playback, server playback

**Output Mode**:
The device-local routing policy used by a Native Player: System Output, Direct ALSA Output, or Adaptive System Rate.
_Avoid_: Processing Profile, Output Device

**Processing Profile**:
The device-local signal-processing policy used independently from Output Mode: Direct Profile or Processed Profile.
_Avoid_: Output Mode, audio route

**Direct Profile**:
A Native Playback profile that keeps software volume at unity and disables ReplayGain, EQ, and other signal processing while seeking a source-to-output format match.
_Avoid_: Bit-perfect guarantee, Processed Profile

**Processed Profile**:
A Native Playback profile that permits software volume, ReplayGain, EQ, or other transformations of the decoded signal.
_Avoid_: Direct Profile, format-preserving playback

**Format Match**:
An observed match between decoded source parameters and parameters written to the operating system audio interface, with no known signal processing enabled. It is evidence of a direct path, not proof of mathematically bit-identical DAC output.
_Avoid_: Bit-perfect guarantee, source quality

**System Output**:
The default Native Playback output mode in which the Player submits source parameters to PipeWire and PipeWire decides the graph and device formats.
_Avoid_: Direct ALSA Output, guaranteed format match

**Direct ALSA Output**:
An advanced Native Playback output mode that opens a selected ALSA `hw:` Output Device without PipeWire or ALSA conversion plugins. Device availability and exact source-format support determine whether playback can start.
_Avoid_: PipeWire exclusive mode, guaranteed device availability

**Adaptive System Rate**:
A Native Playback output mode that temporarily forces the device's system audio rate (the PipeWire graph on Linux, the default output's shared-mode format on Windows) to each Playback Source's sample rate and therefore changes processing for every application on the device.
_Avoid_: Bitrate matching, bit-depth matching, isolated application setting

**Playback Telemetry**:
The separately reported source, decoder, system graph, Output Device, and active processing parameters used to explain whether conversion or processing is known to occur. Unknown device information remains explicitly unknown rather than being inferred from source metadata.
_Avoid_: Generic quality badge, bit-perfect proof

**ReplayGain Metadata**:
Track or album loudness adjustment metadata owned by a library Track and indexed by the Music Server. Its availability is separate from whether a Playback Client enables ReplayGain.
_Avoid_: Player preference, volume setting, inferred loudness

**Playback Source**:
Content a Player can load, currently a library Track, saved Radio Station, Catalog Preview, or External Track.
_Avoid_: Queue item, playlist, Output Device

**Playback Parity**:
The promise that Web and Desktop Clients can play the same Playback Sources even though their Players use different audio engines.
_Avoid_: Identical audio engine, shared Playback Session

**Playback Session**:
The transient, device-local state of one Player, including the current item, playhead, play/pause state, volume, shuffle, and repeat mode.
_Avoid_: Queue, playlist, shared playback state

**Playback Session Snapshot**:
The device-local record used to restore the last item and playhead in a paused state after the Desktop Client relaunches or to recover once from a Player crash.
_Avoid_: Shared playback state, automatic playback

**Background Playback**:
Continuation of the Desktop Client's Playback Session after its main window closes. An explicit application quit ends the session.
_Avoid_: Server playback, remote playback

**Queue**:
A user-owned ordered list of library Tracks shared across that user's Playback Clients. Each Player consumes the shared Queue through its own independent Playback Session.
_Avoid_: Playlist, Playback Session, play history

**Queue Synchronization**:
Propagation of Queue changes to every active Playback Client without synchronizing their independent Playback Sessions.
_Avoid_: Playback synchronization, shared playhead

**Queue Revision**:
The version of a Queue state used to detect and reject mutations based on stale data.
_Avoid_: Track position, Playback Session version

**Output Device**:
The audio endpoint on the Playback Client's device through which its Player produces sound.
_Avoid_: Playback Client, Music Server, stream source

### Listen Together

**Listen Together**:
Shared listening in which several Playback Clients follow one Room on the Owner's Music Server, each producing sound on its own device.
_Avoid_: Party mode, federation, multi-room, Queue Synchronization

**Owner**:
The person whose library a Music Server holds and who alone can open a Room or change the library.
_Avoid_: Admin, host, operator

**Guest**:
A person temporarily admitted to a Room through an Invite Link, who can browse the Owner's library and control the Room but cannot change anything else. A Guest has no account and needs no Music Server.
_Avoid_: User, friend, member, account

**Room**:
A temporary shared listening space on one Music Server, opened and closed by the Owner.
_Avoid_: Party, session, channel

**Invite Link**:
A revocable, expiring link that admits its holder to a Room as a Guest.
_Avoid_: Share link, public link, Server Connection

**Room State**:
The Music Server's record of what a Room is playing, including the current Track, playhead, play or pause, and whether lyrics are open. Each Player follows it through its own Playback Session.
_Avoid_: Playback Session, shared Player, Queue

**Room Queue**:
The ordered list of library Tracks belonging to a Room, separate from the Owner's Queue.
_Avoid_: Queue, Playlist

### Radio

**Radio Station**:
A user-specific, user-managed or imported live internet audio stream. It is not a local library track and does not have stable per-track ownership in the app.
_Avoid_: Channel, radio track, stream item

**Imported Radio Station**:
A Radio Station copied from an external directory into the local collection. After import, the local copy is the source of truth.
_Avoid_: Remote station, live catalog item

**Radio Browser Catalog Entry**:
A discoverable external internet radio listing returned by Radio Browser. It can be previewed or imported, but it is not a local Radio Station until import succeeds.
_Avoid_: Radio Station, saved station, local stream

**Radio Browser Catalog Option**:
A country or tag/genre option proxied from Radio Browser for discover filtering. Country options use Radio Browser's ISO country code when available so the UI can render a flag next to the country name.
_Avoid_: Local filter preset, saved preference

**Catalog Preview**:
Temporary Live Playback of a Radio Browser Catalog Entry before import. It lets the user audition the stream without creating a local Radio Station.
_Avoid_: Auto-import, saved playback, library playback

**AAC Family**:
Discover filter meaning AAC and AAC+ catalog entries. Users expect AAC to include AAC+ because Radio Browser reports both as closely related codec labels.
_Avoid_: Exact AAC-only filter

**Local Station Health**:
Reachability status measured by this app for a saved Radio Station. It is separate from Radio Browser's catalog health because manual stations do not have external directory metadata.
_Avoid_: Radio Browser health, stream quality, Now Playing

**Radio Browser Health**:
Directory metadata from Radio Browser such as last successful check. It is useful for ranking confidence but does not guarantee that the stream will play through the browser or this app at preview time.
_Avoid_: Playback guarantee, local station health

**Live Playback**:
Playback of an unbounded radio stream. It is separate from queue playback because it has no fixed duration, stable track list, or seek target.
_Avoid_: Queue playback, radio queue item

**Now Playing**:
Current program or song metadata reported by a Radio Station while its live stream is playing. It is advisory metadata, not a local library Track, and may be stale or absent.
_Avoid_: Track, queued track, library metadata
