---
status: accepted
---

# Collection cards stretch beside a fixed Queue Drawer

Amends [ADR 0021](0021-top-nav-shell-with-queue-drawer.md), which fixed collection cards at 250px and let the Queue Drawer fill what they left over.

On a 2560px window that rule made the drawer about 520px wide, and it jumped by a whole card as the window crossed each column break. A wide drawer spends room the page could use, and a fixed cards-and-gap row cannot share leftover space without either a moving drawer or a gap before it.

From `lg` (1024px) the Queue Drawer is now 400px at every window width, and collection cards are at least 250px and stretch equally to fill their row, so the last card still sits one page inset from the drawer. Below `lg` the drawer goes from 386px to 350px, so it never grows as the window narrows, and the 275px width below `md` is gone: the shell is never narrower than 768px, and a narrower browser window scrolls sideways.

## Considered Options

- Keep ADR 0021: no gap and fixed cards, but the drawer's width keeps varying between about 288px and 550px.
- A fixed drawer with fixed 250px cards: leaves a gap of up to a card's width before the drawer.

## Consequences

- Cards differ in size between windows, and a row with few cards leaves its empty tracks empty rather than growing into them.
- `docs/design-rules.md` still says collection cards are fixed from `lg` up and that widths start at 640px with a 900px Desktop window, and needs the maintainer's update.
