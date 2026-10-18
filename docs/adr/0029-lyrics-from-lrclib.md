---
status: accepted
---

# Lyrics from LRCLIB when the file has none

A Track's lyrics come from its file tags at import. Most files carry none, so when they are empty the Music Server asks [LRCLIB](https://lrclib.net/docs), the third deliberate exception to the rule against hosted third-party services after [ADR 0017](0017-recording-identification-via-acoustid-and-musicbrainz.md) and [ADR 0027](0027-artist-photos-from-deezer-biographies-from-wikipedia.md), on the same terms: no operator needs an account or a key, the lookup can be switched off, and every failure degrades silently to "no lyrics".

## Lookup

`GET /api/get` with the Track's title, its first credited Artist, its Album title and its duration in whole seconds; LRCLIB answers only when the duration is within 2 s of its record, so another edition's lyrics do not appear. Synced lyrics (LRC) are preferred over plain ones; the clients already render both. LRCLIB's best match for that signature may hold only plain lyrics (a compilation's edition) while another record of the same song is synced, so when `/api/get` gives plain lyrics or nothing, `/api/search` by title and Artist is tried for a synced, non-instrumental record within the same 2 s of the duration, the closest first; that search failing keeps what `/api/get` found. A record marked instrumental is stored as found with no text, so it is not asked again. Lyrics from the file always win and are never overwritten; nothing is written to the audio file, keeping [ADR 0011](0011-managed-import-and-bit-preserving-storage.md)'s guarantee.

## Storage and traffic

The lookup runs lazily, when a client asks for a Track's lyrics, so a library is never crawled. Results are stored per Track, including "nothing found" and plain lyrics, which are retried after three days in case synced ones appear because LRCLIB starts fetching a missing Track in the background on the first request; a failed request after five minutes, since LRCLIB is often briefly overloaded. Requests name the server in `User-Agent` as LRCLIB requires, run one at a time with at least 300 ms between them, a 503 ("briefly busy") is retried once after its `Retry-After` (1 s when it names none) when that is at most 2 s, and any other error, 429 included, counts as a failure. `LYRICS_LOOKUP_ENABLED=false` makes no outbound request; stored results are still served.

Lyrics are copyrighted. LRCLIB's records are contributed by its users; the Music Server shows them only to its own users and redistributes nothing, as other self-hosted players using LRCLIB do.

## Considered Options

- Musixmatch: rejected because it needs a key and returns only part of the lyrics without a paid plan.
- Genius: rejected because its API returns no lyrics text and scraping its pages breaks its terms.
- Deezer: rejected because lyrics are only in its private, signed-in API.
- Register an LRCLIB server-side key for higher limits: rejected because the key is secret and cannot ship in an open-source server; anonymous limits suffice for lazy lookups.
