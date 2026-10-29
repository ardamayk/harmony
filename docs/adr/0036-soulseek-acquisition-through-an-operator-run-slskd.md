---
status: accepted
---

# Soulseek Acquisition through an operator-run slskd

A listener who hears an External Track and likes it wants to keep it in the library at the best quality it exists in, often as lossless FLAC for a whole Album. [ADR 0032](0032-external-source-through-an-operator-installed-yt-dlp.md) keeps the External Source to playback only: nothing is stored, and adding a Track stays the listener's own import. That import still needs a file the listener does not have.

So the Music Server gets an optional **Soulseek Acquisition**. The operator runs [slskd](https://github.com/slskd/slskd), a Soulseek client with a REST API, and configures the Music Server with its address, its API key and the Soulseek account it uses. Without that configuration the feature is off: the server never contacts slskd, and the clients show no Install action, as with the External Source and the Last.fm key in [ADR 0030](0030-play-counts-from-lastfm-with-an-operator-key.md). Soulseek Acquisition does not require the External Source to be configured.

## How it works

- **Install** starts from an External Track, in its search row or in the Player Bar while it plays. The listener chooses the Track alone or its whole Album (the default) and may edit the Soulseek query built from its artist and title or Album.
- **Search** goes through slskd. The results are grouped into **Acquisition Versions**: the **Acquisition Sources** (one user's folder each) that offer the same format, bit depth and sample rate. Each version shows its quality, its file count and how many sources offer it; an incomplete Album source is shown as such and listed below complete ones. The listener can open a version to see its sources.
- **No automatic quality ranking or choice.** The listener decides which version to take, typically after checking the release's quality elsewhere. This follows Import File Alternatives in Managed Import.
- **Download** is done by slskd into its own download directory. The acquisition appears on the Import page as an Import Batch in progress; slskd keeps the transfer queue, so the Music Server reads the state back from slskd after a restart. When a source fails, the next source of the same version is tried; when none is left, the batch fails with whatever files arrived.
- **Import** is the existing Managed Import. The downloaded files go through the Strict Import Profile and an Import Preview, and enter the library only when the listener confirms. Badly tagged files are rejected as for any upload; the listener can try another version. After commit, rejection or cancellation the copies in slskd's download directory are deleted, so Managed Storage holds the only copy.
- **Soulseek Sharing** is off by default. The operator turns it on by giving slskd read-only access to Managed Storage and listing it in slskd's `shares.directories`. The settings show whether Managed Storage is shared, read from slskd's shares API, but cannot change it.
- The server advertises a capability only when slskd is configured, so clients hide the feature otherwise.

## Why it is shaped this way

Soulseek is an open peer-to-peer network with lawful uses, and slskd, like Nicotine+, is a general-purpose client for it. As with yt-dlp, liability for what is downloaded or shared falls on the person doing it, and this repository is public, so the feature stays as neutral as the External Source:

- **Not distributed.** The project does not ship slskd in a release or published image and never installs or updates it. The operator runs it, with their own Soulseek account.
- **Off by default**, and switched on by the operator for their own server. Sharing is off by default again, and switched on separately by the operator.
- **No quality claims.** The project does not rank sources or present any one as the "original" or "official" release; it shows what each source offers.
- **Nothing bypasses the library's rules.** An acquisition is a Managed Import with the listener's confirmation, never an automatic import into Managed Storage.

This lowers the risk; it does not remove it. Downloading or sharing copyrighted music without permission is unlawful in many places, and the README and the settings say so where the feature is enabled.

## Considered Options

- A Soulseek client written in Rust inside the Music Server: rejected. The protocol is undocumented and large (peer connections, distributed search, NAT and listening ports), and slskd already does it well.
- Looking up the release's quality on Qobuz or another store to mark the matching version: rejected. Qobuz has no public API for new applications, scraping its pages breaks its terms, and other catalogs do not give exact sample rates. The listener checks the quality themself.
- Downloading from Qobuz or another streaming service: rejected. It breaks those services' terms and relies on credentials taken from their players.
- Choosing the best version automatically: rejected, for the reasons above and because tags on the network can be wrong.
- A looser import profile for Soulseek files: rejected. It would break the Strict Import Profile ([ADR 0011](0011-managed-import-and-bit-preserving-storage.md), [ADR 0019](0019-tag-based-import-and-album-artwork.md)).
- A sharing switch in the settings: rejected for now. slskd changes `shares.directories` only through its configuration file, and its remote configuration exposes that file's secrets and needs a restart that interrupts transfers. A share directory of symbolic links does not work, because slskd's scanner skips reparse points; a directory of hard links would need Managed Storage and the share on one filesystem and an update after every import and deletion. The switch can return if slskd can change shares at run time.
- Detecting fake quality (upsampled or transcoded files) by spectral analysis: deferred to a later decision.

## Consequences

- The operator is responsible for what is downloaded and shared, and for their Soulseek account.
- Soulseek users often refuse or slow down peers who share nothing, so acquisitions may be slow while sharing is off.
- Files on the network may claim a quality they do not have. Until fake-quality detection exists, the Import Preview shows only what the file's header says.
