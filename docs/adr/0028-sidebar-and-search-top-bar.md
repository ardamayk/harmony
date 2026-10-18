---
status: accepted
---

# Sidebar for the library sections, a search top bar

Supersedes the Top Nav part of [ADR 0021](0021-top-nav-shell-with-queue-drawer.md) and where the search card opens in [ADR 0022](0022-library-search-dialog.md). The library sections (Albums, Artists, Genres, Radio, Tracks, Playlists) and Settings move to a Sidebar on the left; the top bar keeps only the brand, a Home button and the library search field in the middle (modelled on Spotify's), plus page actions and, in the Desktop Client, the window controls on the right.

- **Sidebar.** 15rem wide, docked like the Queue Drawer: one line toward the page, no box, the open section lit as before. Below `lg` only the icons stay, so the page keeps its room at the 768px main target.
- **Search field.** A button that looks like a field. Clicking it (or Ctrl/⌘+K, or "/") opens the existing search card exactly over it and as wide as it, so the field appears to drop down; the App Shell measures the field for the card's position. Results, ranking and keyboard use are unchanged.
- **Insets.** `--shell-inset` now spaces page content beside the Sidebar (`clamp(1.5rem, (100vw - 15rem - var(--shell-max-content)) / 2, 17rem)`); the top bar and the Player Bar pad with a fixed 1.5rem (0.75rem below `lg`). The Queue Drawer's width leaves the Sidebar out of what cards and drawer share.

## Considered Options

- Keep the sections in the top bar next to a search field: rejected because six links, a field and the page actions do not fit at the main target width.
- A real input in the top bar with a separate results popover: rejected for now; the existing card already has the input, focus handling and keyboard model, and opening it over the field looks the same.
