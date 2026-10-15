---
status: accepted
---

# Top navigation shell with a Queue Drawer

The App Shell moved from a left sidebar plus a fixed right Queue column to a Top Nav, a page that fills the width between two insets, the floating Player Bar, and the Queue as a drawer that slides up on the right. The decision came out of a `?variant=` UI prototype flipped through on the Albums page; the "ledger-centered" variant won and the prototype files were removed once it was folded into `packages/ui` and the Web Client.

## One horizontal inset for nav, page and Player Bar

The Web Client defines `--shell-inset` on `:root` as `clamp(1.5rem, (100vw - var(--shell-max-content)) / 2, 17rem)`, so content grows to `--shell-max-content` (1600px) and only wider screens get side gutters. The Top Nav content, every page region (`PAGE_CONTENT_PADDING_CLASS`) and the Player Bar dock (`SHELL_INSET_CLASS`) pad with that one value, so the brand, the page title, the bar and the drawer's right edge all sit on the same lines at any viewport width. The old 1476px content cap is gone: on wide screens the inset grows instead of leaving a gap in front of the queue. Page regions carry a `page-content-column` marker class so tests can still prove a header and its content share a column.

## The Queue is a drawer driven by the existing `collapsed` preference

`QueueDrawer` parks between the Top Nav and the Player Bar (a margin below the nav, a gap above the bar) and slides below the viewport when closed. Its open state is the Queue panel's `layout.collapsed` flag from the shared layout preferences, so the Player Bar's queue button, the drawer's close button and the Desktop Client all drive one persisted value; no new preference was added. While the drawer is open the `<main>` element, at every viewport width, takes extra right padding equal to the drawer width plus its gap, so page content gives way; the nav and the Player Bar keep their boxes. The drawer stays mounted and `inert` while closed so it can animate and never traps focus.

From `lg` (1024px) the drawer fills whatever the fixed 250px collection cards leave over, so its left edge always sits one gap from the last card. Below `lg` the drawer is a fixed 386px (275px below `md`) and the collection grid stretches two columns into the rest, instead of falling to one fixed column beside a wide drawer.

## Page headers lost their description line and divider

With the nav divider spanning only the content column, page headers no longer render a description or a bottom border; `PageHeader` dropped the `description` prop. Radio Discover, which had its own header markup, uses the same inset padding.

## Left over on purpose

`sidebarPosition` and the left widget panel remain in the preference schema for compatibility with stored preferences and older servers. The Top Nav has no sidebar, so `sidebarPosition` only decides which `collapsed` key the Queue uses, and left-panel widgets are not rendered by the shell.
