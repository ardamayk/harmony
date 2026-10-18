---
status: accepted
---

# Play counts from Last.fm, only with the operator's own key

Track lists on the Album page and the Artist page's Popular can show how often each Track was played. No keyless source gives real counts: Deezer has a rank, not plays; ListenBrainz counts only its own small audience; Spotify's API gives a 0–100 popularity behind a Premium developer account (see [ADR 0027](0027-artist-photos-from-deezer-biographies-from-wikipedia.md)). Last.fm has counts for nearly every Artist but needs an API key, which cannot ship in an open-source server: Last.fm keys are per application owner and its terms forbid sharing them.

So play counts are **opt-in**: the operator sets `LASTFM_API_KEY` to their own key (free with a Last.fm account), unlike ADR 0017, 0027 and 0029, whose lookups need no account. Without it the Music Server makes no Last.fm request and the clients show no play-count column; nothing else changes.

## Lookup

One `artist.getTopTracks` request per Artist (`autocorrect=1`, `limit=1000`, Last.fm's most) gives the Artist's 1000 most played Tracks with their counts. Titles are kept with their versions, compared without case: "Red (Taylor's Version)" is a different recording from the 2012 "Red", and "All Too Well (10 Minute Version)" from "All Too Well". Only when Last.fm lists no Track with the library Track's own title does the most played edition sharing its base title (no bracketed versions or " - …" suffixes, as for Popular) stand in. A library Track takes the count of its first credited Artist's list (an Album Artist when it has no Track Artist); a Track outside the 1000 shows none.

## Storage and traffic

Lazy, like the other Artist lookups: when a client asks for a Track list's counts. Only the title→count map is stored per Artist and refreshed weekly; a failed refresh keeps the previous map, a failure with none is retried after five minutes, nothing found after 30 days. One Last.fm request runs at a time. The key is never logged: lookup errors name the method, not the URL that carries it.

## Considered Options

- Embed a project key: rejected, Last.fm's terms tie a key to its owner, and a key in public code is shared by every installation.
- `track.getInfo` per Track: rejected, an Album of 17 Tracks costs 17 requests where one Artist request covers them.
- Rank Popular by Last.fm when a key is set: deferred; Popular keeps Deezer's ranking so it looks the same with or without a key.
