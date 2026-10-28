---
status: accepted
---

# An External Source through an operator-installed yt-dlp

Listeners want to hear a Track they do not own before deciding whether to add it, played in full rather than as a 30-second clip. No keyless source offers full-length playback: Deezer and iTunes give 30-second previews, and YouTube's terms allow full playback only through its embedded player.

So the Music Server gets an optional **External Source**. The operator installs [yt-dlp](https://github.com/yt-dlp/yt-dlp) and sets `EXTERNAL_SOURCE_SEARCH` to one of yt-dlp's search prefixes (for example `scsearch` for SoundCloud); `YT_DLP` names the binary when it is not on `PATH`. Without `EXTERNAL_SOURCE_SEARCH` the feature is off: the server never runs yt-dlp, and the clients show no External results, as with the Last.fm key in [ADR 0030](0030-play-counts-from-lastfm-with-an-operator-key.md).

## How it works

- **Search** runs `yt-dlp --flat-playlist --dump-json "<prefix><n>:<query>"` on the server. The results are **External Tracks**, shown in the search UI as their own group next to Library Search results; they never enter the library.
- **Playback** goes through the server. The server resolves an External Track's audio with yt-dlp and streams the bytes itself, with Range support, on a stream endpoint. The site's resolved URLs are tied to the address that resolved them and expire, so a client cannot use them directly. The Web Client plays that endpoint like a Radio Station stream, and the Desktop Client plays it through its media proxy and mpv like any other Playback Source.
- Nothing is stored. There is no download to disk, no cache of audio, and no import into Managed Storage. Adding a Track to the library stays the listener's own import.
- The server advertises `external-source.v1` only when the source is configured, so clients hide the feature otherwise.

## Why it is shaped this way

yt-dlp stands as a general-purpose tool with many lawful uses, and liability for a use falls on the person using it. Two things set a "search for any song and play it in full" feature apart: circumventing a site's technical protection is a risk of its own (DMCA §1201, EU InfoSoc Article 6), and a tool built or presented for infringement draws liability that a neutral one does not. This repository is public, so the feature is kept as close to yt-dlp's own position as possible:

- **General, not YouTube-specific.** The source is whatever yt-dlp search prefix the operator chooses. Nothing defaults to or is named after YouTube, and lawful sources such as Internet Archive, Bandcamp's free Tracks and Creative Commons uploads work the same way.
- **Not distributed.** The project does not ship yt-dlp in any release or published image, and the server never downloads or updates it. The operator installs it and keeps it current (see Installing yt-dlp).
- **Off by default**, and switched on by the operator for their own server.

This lowers the risk; it does not remove it. The realistic outcome for the project is a takedown notice to GitHub, not a lawsuit. GitHub reviews §1201 notices and gives developers a chance to respond.

## Installing yt-dlp

The install is always the operator's explicit choice, from yt-dlp's own channels (PyPI, pipx, or the distribution's package):

- **On a host**, the operator installs yt-dlp the way yt-dlp documents. A future server install script may offer it only behind an explicit opt-in flag, never by default, and only from those channels; it never vendors or pins a copy.
- **In Docker**, the image is built locally from the repository. `WITH_YT_DLP` is a build argument, `false` by default; `docker build --build-arg WITH_YT_DLP=true .` installs yt-dlp from PyPI into the operator's own image. Updating it means rebuilding with `--no-cache` or running the venv's `pip install -U yt-dlp`.
- **Published images** (any registry) are built with the default and never contain yt-dlp, and there is no `with-yt-dlp` variant. An operator who uses a published image derives their own image from it, or installs yt-dlp into a mounted volume and points `YT_DLP` at it. Publishing a yt-dlp image would be the project distributing it, which the bullet above rules out.

## Considered Options

- YouTube's embedded IFrame player: rejected for now. It is within YouTube's terms and plays full Tracks, but audio stays inside the page, outside mpv, WASAPI and the Output Modes, and the player must stay visible. It could be added later as a separate, YouTube-only preview.
- Bundling or auto-installing yt-dlp: rejected. The project would then be distributing a circumvention tool, and a pinned copy goes stale as sites change.
- An install script that pulls yt-dlp by default (`curl … | bash`): rejected. The default would be the project installing it on every server; an explicit opt-in flag is the most such a script may offer.
- A published image variant with yt-dlp (`:with-yt-dlp`): rejected, for the same reason. `WITH_YT_DLP` exists for the operator's local build only.
- A YouTube default or YouTube-specific UI: rejected, for the neutrality above.
- Running yt-dlp in the clients (mpv's built-in yt-dlp hook in the Desktop Client): rejected. The Web Client cannot run it, and the Windows Desktop Client would need its own yt-dlp.

## Consequences

- The operator is responsible for what the source is used for, and for the site's terms. The README and the settings say so where the feature is enabled.
- Sites change often. When an update to yt-dlp is needed, searches or playback fail with an actionable error until the operator updates it.
- Sites may rate-limit or challenge server addresses. A home server is rarely affected; a datacenter one may be.
- Full-length playback of a Track the listener does not own happens only on the operator's own server, never through a service the project runs.
