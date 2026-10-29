---
status: accepted
---

# One library search dialog instead of per-page search fields

> Opened from the top bar's search field since [ADR 0028](0028-sidebar-and-search-top-bar.md), over which the card now drops down.

The Albums, Artists and Tracks pages each had a search field in their header, every one scoped to its own page. They are gone; the Top Nav's "Search" entry (also Ctrl/⌘+K, or "/" outside a text field) opens a single dialog that searches Tracks, Albums, Artists, Genres and Playlists at once. Radio keeps its own station search: it filters a saved-station list and the Radio Discover catalog, which are not part of the library.

## Best Match, then grouped results

The Library Search experience specification of 2026-09-12 supersedes the original tracks-first highlight and client-side matching decisions below; unrelated dialog and navigation decisions remain unchanged.

Results sit in a "Library" section; when the Music Server has an External Source ([ADR 0032](0032-external-source-through-an-operator-installed-yt-dlp.md)), a parallel "External – <site>" section follows it. Each section carries its own one-line empty, loading and error state. In the Library section, one eligible Best Match appears first, followed by groups in the order Track, Album, Artist, Genre, Playlist. Only strong direct results qualify for Best Match; typo-only results do not. The highlighted record also appears first in its category and counts toward that category's five-result limit. Its two presentations use distinct row keys and DOM IDs; keyboard selection marks only one presentation, while activation uses the same real record identity.

Choosing a Track plays it; an Album, Genre or Playlist retains its existing navigation. An Artist result opens the Artist page (`/library/artists/<id>`, [ADR 0027](0027-artist-photos-from-deezer-biographies-from-wikipedia.md)); before Artists had one it opened `/library/tracks?artistId=<id>`. Search includes all visible credited Artists, including Track-only contributors; it is not restricted to the Album Artists browse list.

## Where the matches come from

The generated API client's `/api/v1/library/search` response is the authority for all five types, Best Match, ordering and deduplication. Normalization and ranking belong to the Music Server, not client-side name filters, prelimited browse pages or a full-library Genre scan.

Every result must contribute at least one query word from its own title or name. Remaining words may match its searchable related fields, preserving title-plus-Artist queries and bounded typo correction. No Track expands nonmatching Albums or Artists: `ar` may find Ariana Grande, but not Bang Bang solely through its Ariana credit or Taylor Swift through a shared Album. Original names and relevant credits remain visible; scores and index details do not.

Empty or punctuation-only input sends no search request. Loading, no matches and failure are distinct; failure offers retry. Results and activation must belong to the current query, and relevant library mutations invalidate cached Library Search results.

## The dialog

It is a Radix Dialog centred in the viewport with a transparent overlay: the page behind stays as it is instead of dimming to black. The input is a combobox over a listbox; the arrow keys move the active row and Enter activates it, Escape closes. Result rows are real buttons so they stay clickable and focusable without custom key handling. The shared `useDebouncedValue` hook (moved out of the Tracks page) holds requests back for 200 ms while typing.

The 2026-09-13 refinement keeps the theme and Best Match behavior. Track and Album rows show Album covers, titles and credits use separate lines, and a theme-accent border marks keyboard selection. A separate Track action (also Shift+Enter from the input) appends to the existing `user` source, displayed as “Added by you”, without starting playback or closing search. Pending additions are guarded; success and failure use existing toast feedback.

The header shows the server's total distinct matches before the five-per-category limit; Best Match is not counted twice. A fixed footer contains keyboard hints and expands all ranked results inside the same scrolling modal. Editing the query restores the five-per-category preview. Older servers without totals fall back to the distinct returned-result count.
