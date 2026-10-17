# Design rules

Every visual change in `web/src` and `packages/ui/src` (Tailwind classes and CSS) follows these rules. They cover the whole app as one design: the Top Nav, the page content, the Queue Drawer, the Player Bar, every page, and the Desktop Client's window frame. No part gets its own look; a part that needs something new gets a rule here first, then the code. Rules marked **enforced** are checked by `web/src/design-rules.test.ts`; a failing check means the change breaks the rule, not that the check needs loosening.

## Product principles

- **One look across the app.** The Top Nav, Player Bar, page content, Queue Drawer and desktop window frame use the same tokens, radii, states and motion. A part never invents its own colors, corners or effects.
- **Modern and premium, not AI slop.** Premium comes from restraint: few colors, few sizes, consistent spacing, good typography, and the content (album covers) in front. See "Avoiding AI slop" below for what not to do.
- **Customization lives in place, otherwise in Settings.** When a setting has a natural home on the surface it changes, it sits there as a control (the Queue pin on the Queue header, album filters in the Top Nav, shuffle and repeat on the Player Bar). Settings that have no such home, or that change the whole app, go to the Settings page. Do not scatter the same setting in both.
- **Pages compose shared components.** Pages build from the shared pieces (`PageShell`/`PageHeader`, `CollectionGrid` and its states, `AlbumArt`, `TopNavActions`, `web/src/components/ui/*` and the `@repo/ui` layout parts). A page writes its own component only when it has to customize one, and builds it from the same tokens and classes; when a second page needs it, it moves to the shared components.
- **Widths.** The Web Client targets 640px and up and is designed mainly for 768px and up; the Desktop Client window is at least 900px. Phones get a separate mobile client app, so this UI gets no phone-only layouts.
- **Web and Desktop work alike.** The Web Client and the Desktop Client (Electron) run the same UI and behave the same wherever both have the feature. A fix or change in one is checked in the other before it counts as done: the Desktop Client renders in Electron's Chromium inside a frameless, transparent window, so transparency, compositing, drag regions (`data-window-drag-region`) and native window behavior can differ from a browser tab. Desktop-only features (window frame, mini player, output devices) stay behind `isDesktopClient()` and never change the Web Client.
- **Web Client and Music Server are separate.** The Web Client is its own app that talks to the Music Server only through the generated API client (`packages/api-client`). Nothing in the UI assumes it is served by, or runs next to, the server.

## Hierarchy and emphasis

- Every screen has a reading order: what the eye lands on first, then second. Size, weight, color and position set it. If everything is emphasized, nothing is.
- One primary action per area. It is the only filled, accent-colored control there; the rest stay quiet. The Player Bar's filled play button among plain icons is the model.
- Few colors, many tones. The accent is valuable because it is rare. Show states with opacity steps of the same color, not with new colors.

## Color proportion and contrast

- **60-30-10 as a guide, not a law.** About 60% of a screen is the dominant neutral (`background`), about 30% the secondary surfaces (`card`, `sidebar`, `player`, `popover`, the Queue Drawer), and about 10% the accent (`primary`, `player-control-primary`, `shell-active-foreground`, `shell-brand`). In-between tones are welcome: a mix of two tokens (`color-mix`) or an opacity step (`bg-primary/10`) sits between the bands and keeps the palette whole without adding a fourth color.
- **The accent at rest marks only what matters**: the area's primary action, the item playing now, a selected option, the brand mark and playback progress. Hover, focus and pressed states may use it briefly; resting decoration may not.
- **Contrast is the emphasis tool.** The highest contrast goes to what the eye should reach first (headings, the primary action, the current item); secondary text steps down (`foreground`, then `caption`). Emphasis comes from raising one thing's contrast, not from coloring many things.
- **Contrast never drops below WCAG AA**, in every preset and both modes: 4.5:1 for text (including text and glyphs on a filled control), 3:1 for large text, icons and a control against its surface. Fixes that a preset needs live in `web/src/themes/contrast.css`, derived from the preset's own palette where possible. A surface whose background differs from the theme mode (Vintage Harbor keeps the Top Nav, Player Bar and Queue Drawer dark in light mode) carries `data-shell-surface` and re-scopes its text tokens there.

## Spacing and alignment

- Spacing comes from a scale, never from a one-off value: Tailwind steps `1`, `2`, `3`, `4`, `6`, `8`, `12`, `16`, `24` (4, 8, 12, 16, 24, 32, 48, 64, 96px) for padding, margin and gaps. `0.5` (2px) is only for optical nudges. Arbitrary spacing is only a layout variable (`px-[var(--shell-inset)]`). **Enforced.**
- Proximity groups things: related items sit close, unrelated ones far apart. Prefer space over lines and boxes to show a group.
- Everything sits on a shared invisible line. `--shell-inset` is that line for the Top Nav, the page and the Player Bar; new surfaces align to it rather than adding their own padding.
- Space is a design element. Premium comes from generous space, not ornament; when a layout feels busy, remove things before adding dividers.

## Typography

- Sizes come from a short scale: `text-xs` 12px, `text-sm` 14px, `text-base` 16px, `text-xl` 20px, `text-2xl` 24px, `text-3xl` 30px. Headings share these steps instead of each getting its own size; no arbitrary sizes like `text-[11px]`. **Enforced.**
- Weights: `font-normal`, `font-medium`, `font-semibold`. No bold or light. **Enforced.**
- Build hierarchy with size, weight and opacity before color: primary text at full strength, secondary around 70%, captions around 50% (or the `heading` → `foreground` → `caption` tokens, which follow the same steps).
- Text meant for reading runs about 50–75 characters per line; cap its width (`max-w-prose` or similar).

## Consistency and states

- The same thing looks the same everywhere: a delete button, an empty state or a card has one style across all pages. That is why tokens and shared components exist.
- Every interactive element has all its states designed: default, hover, focus, pressed or active, disabled, loading and error. Focus and disabled are the ones most often forgotten; a change that adds a control adds all of them.
- Design for edge cases: long names, an empty list, a single item, many items and a slow connection. Truncate with an ellipsis and keep the full text in a `title` or tooltip, so a cut title like "Snow On The Beach (fea…" can still be read.

## Accessibility

- Contrast is measured: at least 4.5:1 for normal text and 3:1 for large text and icons (WCAG AA). Grey on grey in the dark themes is where this breaks most often; check muted and caption text against its actual background.
- Never rely on color alone. A state also shows through an icon, text or shape (the pin icon changes shape, not only color).
- Hit areas are at least 32px, preferably 44px, even when the visible icon is smaller. A control that must look small keeps its size and extends its hit area with an invisible pseudo-element (`relative after:absolute after:-inset-1 after:content-['']`).
- Use the element that matches the job: `<button>` for actions, `<a>`/router `Link` for navigation, real headings in order, lists for lists. A clickable `<div>` is a bug.
- An icon-only control has an `aria-label` that names the action ("Pin queue", not "Pin icon") and a tooltip with the same words.

## Keyboard and focus

- Everything a mouse can do, the keyboard can do. Tab order follows the reading order; nothing is reachable only by hover.
- Dialogs, drawers and full-screen views move focus inside when they open, keep it there, close on `Escape`, and give focus back to the control that opened them. The `radix-ui` Dialog, Popover and menu primitives do this; build overlays on them, and a hand-built one matches them.
- Menus and lists move with the arrow keys; `Enter` or `Space` acts.
- A new keyboard shortcut is listed in the shortcut help overlay (`ShortcutHelpOverlay`) and never fires while the focus is in a text field.

## Icons

- One icon set: `lucide-react`, at its default stroke width. Icons from another set, or a hand-drawn SVG beside Lucide ones, break the look.
- Icon sizes follow the text they sit with: `size-4` (16px) in buttons, menus and Top Nav controls; `size-3.5` (14px) inline with `text-xs` metadata; `size-5` (20px) for the Player Bar's transport and other large controls. Larger icons only in empty states.
- An icon earns its place by helping someone recognize an action faster. A label that reads fine alone gets no icon.

## Words in the UI

- Sentence case everywhere: "Add to queue", not "Add To Queue". Use the domain words from `CONTEXT.md` (Track, Album, Queue Drawer, Managed Import), the same word for the same thing on every page.
- Buttons say what happens, with a verb: "Delete playlist", "Scan library". "OK" and "Submit" say nothing.
- An error says what went wrong and what to do next, in plain words: "Couldn't reach the Music Server. Check that it is running, then retry." No "Oops", no blame, no raw error codes as the only text.
- Empty states say why the area is empty and offer the one action that fills it.

## Feedback: loading, empty and error

- Loading content shows skeletons of the final size (they also protect CLS). A spinner is for a short action on a control, and only after about 300ms, so fast answers do not flash.
- Every data area designs its three non-happy states with the shared pieces: loading, empty (`Empty`, `CollectionGrid`'s states) and error with a retry.
- Background results and undoable actions report through the shared `toast()` (`packages/ui/src/layout/Toaster.tsx`); a toast never carries the only copy of something the user must act on.
- Destructive actions are undoable when they can be (a toast with Undo); when they cannot, a dialog names exactly what will be lost, and its confirm button repeats the verb ("Delete 12 tracks").
- A control that started work shows it is busy and cannot be triggered twice.

## Forms and settings

- Every field has a visible `Label`; a placeholder is an example, never the label.
- Validate on blur or submit, not on every key press, and show the message under the field it belongs to.
- A setting that applies immediately (a `Switch`, a `Select`) needs no Save button; a form that needs one keeps it disabled until something changed.

## Numbers and time

- Durations, times, counts and other numbers that change in place use `tabular-nums`, so digits do not jitter while a Track plays.
- Durations read `m:ss` (`h:mm:ss` from an hour). Large counts use the locale's grouping ("12,480 tracks").
- In lists and tables, numbers align right and their column has a fixed width.

## Layering

Stacking order uses a few steps, from back to front:

| Step | Class | Use for |
|---|---|---|
| Local | `z-10` to `z-30` | Stacking inside one component (a cover strip, a sticky row) |
| Shell | `z-40` | Sticky page headers, Queue Drawer, shell controls |
| Floating | `z-50` | Player Bar, menus, popovers, selects, dialogs, toasts |
| Full screen | `z-[60]` | Now Playing view, Lyrics view |
| Above full screen | `z-[70]` | The shortcut help overlay |

A new layer takes one of these steps instead of a new number. A popover inside a component portals to the body (Radix does this) rather than raising its own `z-index`.

## Depth and elevation

Each layer has one consistent surface and shadow, from back to front:

| Layer | Surface | Shadow |
|---|---|---|
| Page | `bg-background` | none |
| Card | `bg-card` with `border-border` | none at rest; the card hover shadow when lifted |
| Large artwork | the cover itself | `shadow-xl` |
| Menu, popover, tooltip | `bg-popover` with `border-border` | `shadow-lg` |
| Shell surface (Player Bar, Queue Drawer) | `bg-player` with `border-player-border` | the `--player-shadow` shadows they already use |
| Dialog, side drawer | `bg-popover` or `bg-background` | `shadow-xl` |

In dark themes depth comes mostly from a slightly lighter surface rather than a heavier shadow; the lit pill on the active section works this way.

## Performance budgets

Every page meets these budgets, measured on a production build:

| Metric | Budget | What it means |
|---|---|---|
| LCP, Largest Contentful Paint | ≤ 2.5s | The main content (usually the first covers) is visible |
| INP, Interaction to Next Paint | ≤ 200ms | Clicks and key presses answer quickly |
| CLS, Cumulative Layout Shift | ≤ 0.1 | Nothing jumps while the page loads |
| TTFB, Time to First Byte | ≤ 800ms | The server starts answering quickly |

- `mise run web:lighthouse` builds the Music Server with the embedded Web Client, starts it and runs Lighthouse against the main pages; it fails when a budget is exceeded. Run it after changes that can affect loading: new dependencies, fonts, large components, images or page layout.
- The check uses Lighthouse's **desktop** profile only, because this UI targets the Web Client on desktop browsers and the Desktop Client. If the project ever targets mobile with this UI, add a run with the mobile profile (`--form-factor=mobile` with mobile throttling) and hold it to the same budgets; the separate mobile client app gets its own performance checks.
- Lighthouse measures a page load without a user, so it cannot measure INP directly. It reports Total Blocking Time (TBT) instead, the lab stand-in for INP, and the check holds TBT to the same 200ms. A change to a heavy interaction (search, queue, large lists) is also checked by hand in the browser's performance panel.
- `mise run perf:snapshot` (after `mise run build`, Linux) prints CPU and memory of the built Music Server, idle and while one client streams a Track at playback speed, plus a Desktop Client that is already running (memory as PSS, broken down by process). It sets no thresholds yet; compare its numbers before and after changes that can affect resource use, and add budgets here once typical values are known.
- Artwork loads at the size it is shown. Album covers are requested with `getAlbumCoverUrl(albumId, size)`, where `size` is the step (64, 128, 256, 512, 1024) at or above the displayed size times two; the server scales and caches them. An original-size cover is only for a full-screen view. **Enforced** (a call without a size fails the design check). Memory follows pixels, not file format, so a smaller format (WebP, AVIF) cuts transfer but not the webview's memory.
- Layout stability (CLS) comes from reserving space: images and artwork have a fixed aspect ratio or size, and loading states use skeletons of the final size.

## Tailwind tokens

- Design values are CSS variables exposed to Tailwind as theme tokens in `@theme inline` in `web/src/styles.css`: colors as `--color-*`, corner radii as `--radius-*`. Theme presets in `web/src/themes/` set the variables' values; components never pick values themselves.
- Use the token class: `bg-player-pill`, `text-shell-active-foreground`, `border-player-border`, with an opacity modifier where needed (`border-shell-active-foreground/35`). **Enforced:** `bg-[var(--x)]` and the like fail when `--color-x` exists.
- A new color is a new variable in `web/src/themes/layers.css` (plus preset overrides where they differ) registered as `--color-<name>` in `@theme inline`; then use its class.
- An arbitrary `[var(--…)]` stays only for values that are not tokens: a variable set per element at runtime (`--queue-group-color`), layout geometry (`--shell-inset`, `--queue-drawer-clearance`) and third-party variables (Radix).

## Player Bar accent from the cover

The Player Bar takes its accent color from the playing album's cover. The `playback.accentFromCover` preference (Settings) turns it on or off.

1. `useCoverAccent` (`packages/ui/src/playback/use-cover-accent.ts`) loads the cover with CORS so the canvas stays readable.
2. `extractAccent` (`packages/ui/src/lib/cover-accent.ts`) draws it onto a 32×32 canvas and groups the pixels into 30° hue buckets. It skips pixels that are nearly grey (saturation under 20%), very dark or very light. The bucket with the most saturated weight wins, and its weighted average becomes the accent.
3. The accent is clamped so it stays readable on the bar: saturation 35–85%, and lightness 55–70% in dark themes or 30–45% in light ones. Text on the accent turns dark on light accents and light on dark ones.
4. The result is set as CSS variables on the Player Bar's root element only: `--player-accent`, `--player-control-primary` (each with a `-foreground`), `--player-live-progress` and `--player-control-shadow`. Inside the bar they override the theme's defaults; outside it nothing changes. The Now Playing view receives the same variables but keeps its primary control neutral (its always-dark palette sits under `[data-now-playing-surface]` in `web/src/styles.css`).
5. Each accent is cached per album, so tracks from the same album never flicker. While a new cover loads, the previous accent stays. A monochrome cover or a failed load falls back to the theme's colors.

Rules for it: controls inside the bar read these variables through token classes (`bg-player-control-primary`, `text-player-control-primary-foreground`) and never compute colors of their own. The accent stays scoped to the Player Bar and the Now Playing view and does not tint the rest of the app. The computed foreground colors in `cover-accent.ts` are the only color literals this feature may produce.

## Corner radius: three steps (enforced)

| Step | Token | Class | Use for |
|---|---|---|---|
| Small | `--radius-sm` 6px | `rounded-sm` | Controls up to ~32px tall (small buttons, inputs, chips, menu items), artwork thumbnails up to 64px, inline code |
| Medium | `--radius-md` 10px | `rounded-md` | Collection cards and large artwork, menus, popovers and tooltips, notices and banners, Top Nav pills and icon buttons, the desktop window's corners |
| Large | `--radius-lg` 16px | `rounded-lg` | Shell surfaces: Player Bar, Queue Drawer, dialogs and the search card, page-level header panels |

- Also allowed: `rounded-full` (circles, capsules, slider thumbs, count badges, hairline bars) and `rounded-none`.
- Side and corner variants use the same three sizes (`rounded-r-sm`, `rounded-t-lg`).
- Never: bare `rounded`, `rounded-xs`, `rounded-xl`, `rounded-2xl` and up, arbitrary `rounded-[…]`, or a raw length in a CSS `border-radius`. In CSS, use `var(--radius-sm|md|lg)`, `0` or `9999px`.
- Pick the step by what the element is, not by its size in one screenshot. When unsure between two steps, take the smaller one.
- An element flush inside a rounded parent takes no radius of its own; the parent clips it with `overflow-hidden`. A nested element with its own visible edge uses a step no larger than its parent's.

## Color

- Colors come from theme tokens (see Tailwind tokens). New code does not add hex, `rgb()` or `white`/`black` color literals; existing ones move to tokens when their component is touched.
- The exceptions are shadow color, `rgb(0 0 0 / alpha)` inside `shadow-[…]`, and the cover accent's computed colors.
- Translucent tints mix a token: `color-mix(in srgb, var(--heading) 12%, transparent)`.

## Interaction states

- Hover and keyboard focus look the same: `focus-visible:` repeats the hover background, and `focus-visible:outline-none` replaces the browser ring. Never remove a focus style without that replacement.
- The open section, the current page and a pressed control get the lit pill: `color-mix(--heading 12%)` background, `text-heading`, an inset top highlight and a soft drop shadow. Reuse `TOP_NAV_ICON_BUTTON_CLASS` or the Top Nav link classes rather than restyling.
- Collection and station cards on hover and focus: lift `-translate-y-1`, border `shell-active-foreground` at 35% (60% on focus), a soft dark drop shadow, artwork scaled to `1.04`. Titles are not underlined. Every card takes this from `CARD_INTERACTION_CLASS` (`web/src/components/collection-grid-layout.tsx`), which also keeps `transform-gpu backface-hidden` so the desktop webview does not flicker card edges.

## Motion

- Motion explains something: where a thing comes from and where it goes, like the Queue Drawer sliding up from the Player Bar. Motion that is only decoration is removed.
- Durations: `duration-150` for small feedback, `duration-200` to `duration-300 ease-out` for most transitions, `duration-500 ease-out` only for artwork zoom.
- Respect reduced motion: keyframe animations sit behind `@media (prefers-reduced-motion: no-preference)`.
- Do not animate a width that makes a collection grid re-flow card by card; snap it instead.

## Layout

- Horizontal edges follow `var(--shell-inset)`; content grows to `--shell-max-content` (1600px).
- Collection cards are 250px and fixed from `lg` up; below `lg` with the Queue Drawer open, the grid stretches two columns (see ADR 0021).
- Breakpoints are Tailwind's defaults: `sm` 640px is the narrowest supported width, `md` 768px the main design target, `lg` 1024px where the fixed card layout starts.

## Avoiding AI slop

These are the common traps; none of them belong in this app:

- Gradients, glows or blur added to everything.
- Purple-blue gradients and glowing borders with no purpose.
- Cards drawn with different radii and shadows from each other.
- Decoration instead of content: meaningless icons, emoji, "✨" highlights.
- Everything centered and the same weight, so there is no hierarchy.
