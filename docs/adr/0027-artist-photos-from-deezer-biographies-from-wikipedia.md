---
status: accepted
---

# Artist photos from Deezer, biographies from Wikipedia

Artists get a detail page with a photo and a short biography. Neither lives in the files Managed Import accepts, so the Music Server looks them up online, the second deliberate exception to the rule against hosted third-party services after [ADR 0017](0017-recording-identification-via-acoustid-and-musicbrainz.md), on the same terms: no operator needs an account or a key, the lookup can be switched off, and every failure degrades silently to the Artist's name and initial.

## Sources

- **Photo: Deezer.** Its public API needs no key and returns a 500 px artist picture. The Artist is found through one of their own Albums (`/search/album` for the Artist and Album names as plain words, the first result whose Artist name matches case-insensitively and has a picture; Deezer's `artist:"…"` syntax leaves out some well-known Artists), so two Artists sharing a name resolve to the one on the user's Album; only when no Album finds them does a name search (`/search/artist`) decide, preferring an exact-case name, then a case-insensitive one, then the most fans. Deezer's empty-picture placeholder counts as no photo.
- **Popular Tracks: Deezer.** The same Artist match gives Deezer's top 50 Tracks (`/artist/{id}/top`). Only their titles are stored; the Artist page shows the Artist's library Tracks among them, in Deezer's order, matched without case, bracketed versions ("Live", "Remastered") or " - …" suffixes, the plain edition first when the library has several. Deezer gives a ranking, not play counts, so none are shown.
- **Biography: Wikipedia, reached through MusicBrainz and Wikidata.** A MusicBrainz release-group search for one of the Artist's Albums names the Artist's MBID (a name search with a single 100-score match is the fallback); the MBID's Wikidata link gives the Wikipedia page title in `ARTIST_INFO_LANGUAGE` (default `en`, falling back to English), and Wikipedia's page summary gives the text. The page shows the source link, as Wikipedia's CC BY-SA licence requires.

Spotify (a developer account with Premium since February 2026), Last.fm (no artist photos since 2019, a key for biographies), Fanart.tv and TheAudioDB (keys) were rejected because every operator would need an account.

## Storage and traffic

Lookups run lazily, the photo when a client first asks for it and the biography when the Artist page asks for it, so a library is never crawled. Results, including "nothing found", are stored per Artist in the database: a photo's bytes are served from the Music Server, so clients never contact Deezer and the Desktop Client's media proxy treats it like an Album cover. Nothing found is retried after 30 days, a failed request after an hour. A found top-Track list is refreshed weekly, since popularity moves; a failed refresh keeps the previous list. MusicBrainz is held to one request per second server-wide, and its 503 "try again" answers (its shared budget for all clients running low) are retried after 2 s and 4 s; at most three Deezer lookups run at once. `ARTIST_INFO_ENABLED=false` makes no outbound request at all.

## Considered Options

- Hot-link Deezer's image URLs: rejected because every client would contact Deezer, and the Desktop Client's CSP allows images only from its media proxy.
- Crawl every Artist after an import: rejected because it sends the whole library to three services up front; lazy lookups only touch Artists someone opens.
- Let the user pick the matching Artist when names collide: deferred; the Album-based search resolves the common case.
