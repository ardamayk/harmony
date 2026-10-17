# Design rules

> **Reference only.** Copy of `ui-design-new` skill RULES.md plus `Project:` lines: this app's parts, tokens, numbers, tools, exceptions. Rule lines verbatim from skill; project lines only where app has own value. States limits, never code to copy. Agents read, never edit; changes come from maintainer.

Applies to `web/src`, `packages/ui/src`. Pattern no rule covers: raise with maintainer before code. **Enforced** = checked by `web/src/design-rules.test.ts`; red check means code breaks rule, fix code.

## Product principles

- **One look.** Every part (nav, page, panels, overlays, frame): same tokens, radii, states, motion. No part invents own color, corner, effect.
  - Project: Parts: Top Nav, Player Bar, page content, Queue Drawer, desktop window frame.
- **Premium by restraint.** Few colors, few sizes, consistent spacing, good type, content in front.
  - Project: Content in front = album covers.
- **Setting lives where it acts.** Natural home on surface it changes: control there. No home or app-wide: settings page. Never both.
- **Pages compose shared parts.** Own component only when shared one cannot be customized; same tokens. Second page needs it: move to shared.
- **Widths.** Project names supported range and main target. No layouts for widths outside it.
  - Project: 640px up, main target 768px up, Desktop window 900px min. Phones: separate client, no phone layouts here.
- **Every shell alike.** UI shipped in more than one shell (browser, Electron, webview): same behavior where both have feature; change done only when checked in each. Shell-only features never change others.
  - Project: Web Client (browser), Desktop Client (Electron, frameless transparent window; transparency, compositing, drag regions, native window behavior differ).

## Hierarchy and emphasis

- Every screen has reading order: first, then second. Size, weight, color, position set it. Everything emphasized = nothing emphasized.
- One primary action per area: only filled accent control there; rest quiet.
  - Project: Model: Player Bar's filled play button among plain icons.
- Few colors, many tones. Accent valuable because rare. States: opacity steps of same color, never new color.

## Color proportion and contrast

- **60-30-10 guide, not law.** ~60% dominant neutral, ~30% secondary surfaces, ~10% accent. In-between tones: mix two tokens or opacity step; never fourth color.
  - Project: 60: `background`. 30: `card`, `sidebar`, `player`, `popover`, Queue Drawer. 10: `primary`, `player-control-primary`, `shell-active-foreground`, `shell-brand`.
- Accent at rest marks only: area's primary action, current item, selected option, brand mark, progress. Hover, focus, pressed may use briefly. Resting decoration never.
  - Project: Current item = track playing now. Progress = playback progress.
- Contrast is emphasis tool. Highest contrast where eye reaches first; secondary text steps down. Raise one thing, never color many.
  - Project: Text steps: `heading`, `foreground`, `caption`.
- Contrast never below WCAG AA, every theme and mode: 4.5:1 text (also on filled controls), 3:1 large text, icons, control against surface.
  - Project: Preset fixes: `web/src/themes/contrast.css`, derived from preset's own palette. Surface darker than theme mode (Vintage Harbor: Top Nav, Player Bar, Queue Drawer dark in light mode) carries `data-shell-surface`, re-scopes text tokens there.

## Palette order

Build palette in this order; each step uses previous.

1. Accent hue (H), neutral base: pure gray or tinted.
   - Project: Accent per preset (`--primary`, `--player-control-primary`); cover accent overrides in Player Bar. Neutrals tinted to preset base hue (Vintage Harbor h 252).
2. Family ladders: L steps of neutral and accent families.
3. Surface ladder (60/30): page, alt surface, card, dark band, footer. Light-dark rhythm set here; grayscale test shows it.
   - Project: Vintage Harbor dark, OKLCH L: page / Top Nav 23.5, card / Player Bar / Queue Drawer 27.5, raised control 31.5, active 39.5.
4. Text hierarchy: each surface gets primary, secondary, faint text. 4.5:1, 7:1 decide here. Dark surfaces own text set.
   - Project: Tokens `heading`, `text`, `caption`. Vintage Harbor dark L 93 / 80 / 70, hue 80, C 0.02; caption 4.5:1 up to raised controls. Other presets: caption brighter than text, open.
5. Lines, controls: decorative line no threshold; input border, icon 3:1.
6. Accent placement (10): button fill, text on it 4.5:1, dark-surface variant, accent surface.
   - Project: One accent per surface: seek fill = play button colour.
7. States: hover, focus 3:1, error, disabled.
8. Verify: contrast every pair, grayscale test, warm/cool test, color-blind simulation, dark mode.
   - Project: Contrast: `web/src/themes/themes.test.ts`. Grayscale, warm/cool, color-blind: by hand in browser, every page.

Ratios: decision tool in steps 4–7, check in step 8.

## Spacing and alignment

- Spacing from one scale, never one-off. Tailwind: steps `1, 2, 3, 4, 6, 8, 12, 16, 24` (4px base); `0.5` only optical nudge. Arbitrary spacing only for layout variable.
  - Project: Enforced. Layout variables: `--shell-inset`, `--queue-drawer-clearance`, `--player-dock-clearance`.
- Proximity groups: related close, unrelated far. Space over lines and boxes.
- Everything on one shared line: page edges, headers, footers align to same inset. New surface aligns to it, never adds own padding.
  - Project: Line = `--shell-inset` (Top Nav, page, Player Bar).
- Space is design element. Busy layout: remove before adding dividers.

## Typography

- Sizes from short scale. Tailwind: `text-xs` 12, `text-sm` 14, `text-base` 16, `text-xl` 20, `text-2xl` 24, `text-3xl` 30px. Headings share steps. No arbitrary size.
  - Project: Enforced.
- Weights: normal, medium, semibold. No bold, no light.
  - Project: Enforced.
- Hierarchy by size, weight, opacity before color: primary full, secondary ~70%, caption ~50%.
  - Project: Tokens `heading`, `foreground`, `caption` (same steps).
- Reading text 50–75 characters per line; cap width (`max-w-prose`).

## Consistency and states

- Same thing, same look everywhere: delete button, empty state, card: one style across pages. Tokens and shared parts exist for this.
- Every interactive element has all states: default, hover, focus, pressed/active, disabled, loading, error. Focus and disabled most forgotten. New control = all states.
- Design edge cases: long names, empty list, one item, many items, slow connection. Truncate with ellipsis; full text in `title` or tooltip.

## Accessibility

- Contrast measured, not guessed: 4.5:1 text, 3:1 large text and icons. Muted and caption text on dark surfaces breaks most; check against real background.
- Color never only signal: state also shows by icon, text, shape.
  - Project: Example: pin icon changes shape, not only color.
- Hit area 32px min, 44px preferred. Control may look small; click area never shrinks.
- Element matches job: `<button>` acts, `<a>` or router link navigates, headings in order, lists for lists. Clickable `<div>` is bug.
- No visible text, clickable or hover-acts (icon button, icon, swatch, avatar): tooltip required, short function, verb ("Pin queue", not "Pin icon"). `aria-label` same words.

## Keyboard and focus

- Keyboard does everything mouse does. Tab order = reading order. Nothing hover-only.
- Dialogs, drawers, full-screen views: focus moves in, stays in, Escape closes, focus returns to opener. Build on accessible primitive library; hand-built one matches it.
  - Project: Primitives: `radix-ui` Dialog, Popover, menus.
- Menus and lists: arrow keys move, Enter or Space acts.
- Shortcuts discoverable (help overlay or tooltips); never fire while focus in text field.
  - Project: Shortcut list: `ShortcutHelpOverlay`.

## Icons

- One icon set, one stroke width. Second set or hand-drawn SVG beside it breaks look.
  - Project: `lucide-react`, default stroke.
- Icon size follows text beside it: 16px buttons and menus, 14px beside small metadata, 20px large controls. Larger only in empty states.
  - Project: `size-4` Top Nav controls; `size-3.5` with `text-xs`; `size-5` Player Bar transport.
- Icon earns place by faster recognition. Label that reads alone gets no icon.
- Icons where they carry meaning, not everywhere. Row of labels with icon on each = decoration; one icon on the one that needs it.
- Emoji judged per app: fine where product's tone allows (casual, social, playful); never in tools, data, settings, errors. Default no. Emoji where not needed makes app cheap.

## Words in the UI

- Sentence case everywhere. Product's domain words; same word for same thing on every page.
  - Project: Domain words: `CONTEXT.md` (Track, Album, Queue Drawer, Managed Import).
- Buttons say what happens, with verb: "Delete playlist". "OK", "Submit" say nothing.
- Error says what went wrong and what to do next, plain words. No "Oops", no blame, no raw code as only text.
  - Project: Example: "Couldn't reach the Music Server. Check that it is running, then retry."
- Empty state says why empty, offers one action that fills it.

## Feedback: loading, empty and error

- Loading content: skeletons of final size (protects CLS). Spinner only for short action on control, after ~300ms.
- Every data area: loading, empty, error with retry, from shared parts.
  - Project: Shared: `Empty`, `CollectionGrid` states.
- Background results and undoable actions report through one toast system. Toast never holds only copy of something user must act on.
  - Project: `toast()` in `packages/ui/src/layout/Toaster.tsx`.
- Destructive action undoable when possible (toast with Undo). Otherwise dialog names exactly what is lost; confirm button repeats verb ("Delete 12 items").
- Control that started work shows busy, cannot trigger twice.

## Forms and settings

- Every field has visible label. Placeholder is example, never label.
- Validate on blur or submit, never per keystroke. Message under its field.
- Setting that applies at once needs no Save. Form with Save keeps it disabled until something changed.

## Toggles and switches

- Switch = on/off that applies at once. Checkbox = choice collected for submit. Button = action. Never switch for action, never checkbox for live setting.
- Switch states readable without color: thumb position plus label; icon or text for on/off where meaning not obvious.
- Switch motion explains change: thumb slides to new side, ~150ms; track color follows. No bounce, no glow.
- Label names what is on, not question: "Dark mode", not "Enable dark mode?". Label click toggles.
- Switch that triggers slow work shows busy on itself and blocks second flip until done.

## Numbers and time

- Numbers that change in place (timers, counters): `tabular-nums`, no jitter.
- Durations `m:ss`, `h:mm:ss` from one hour. Large counts use locale grouping.
- In lists and tables numbers align right, fixed-width column.

## Layering

- Five steps, back to front: local (inside one component), shell (sticky headers, drawers, shell controls), floating (menus, popovers, dialogs, toasts), full screen, above full screen. Each step one `z-index`. New layer takes step, never new number.
  - Project: Local `z-10` to `z-30`: inside one component.
  - Project: Shell `z-40`: sticky page headers, Queue Drawer, shell controls.
  - Project: Floating `z-50`: Player Bar, menus, popovers, dialogs, toasts.
  - Project: Full screen `z-[60]`: Now Playing, Lyrics.
  - Project: Above `z-[70]`: `ShortcutHelpOverlay`.
- Popover inside component portals to body, never raises own `z-index`.

## Depth and elevation

- Each layer one surface, one shadow, back to front: page none; card border, shadow only when lifted; large media `shadow-xl`; menu, popover, tooltip `shadow-lg`; shell surfaces own shadow token; dialog, drawer `shadow-xl`.
  - Project: Page `bg-background`, none.
  - Project: Card `bg-card` `border-border`, none; card hover shadow when lifted.
  - Project: Large artwork: cover, `shadow-xl`.
  - Project: Menu, popover, tooltip `bg-popover` `border-border`, `shadow-lg`.
  - Project: Shell (Player Bar, Queue Drawer) `bg-player` `border-player-border`, `--player-shadow`.
  - Project: Dialog, side drawer `bg-popover` or `bg-background`, `shadow-xl`.
- Dark themes: depth from slightly lighter surface, never heavier shadow.

## Performance budgets

- Every platform sets four budgets: startup, main content visible, input latency, layout shift. Measured on production build, in CI, against baseline.
- Web and webview: LCP ≤ 2.5s, INP ≤ 200ms (lab: TBT ≤ 200ms stands in), CLS ≤ 0.1, TTFB ≤ 800ms.
- Native and mobile: startup time, frame time, input latency targets. Project names tool and numbers in design.md.
- Measurement profile matches platform: check which platforms project targets, measure each (desktop, mobile, webview). Never measure one, claim all.
  - Project: Platforms: Web Client on desktop browsers, Desktop Client (Electron). Lighthouse desktop profile only. Mobile with this UI ever: add `--form-factor=mobile` run, same budgets.
- Measure after changes that affect loading: dependency, font, large component, media, page layout. Heavy interaction change (search, large list): also check by hand in profiler.
  - Project: `mise run web:lighthouse`: builds server with embedded Web Client, runs Lighthouse on main pages, fails over budget.
- Server and background work: CPU, RAM, disk idle and under load; p90, p95, p99 latency; throughput (rps). Set thresholds, or use suitable known ones. Compare before and after.
  - Project: `mise run perf:snapshot` (after `mise run build`, Linux): CPU and memory of server idle and streaming, plus running Desktop Client (PSS per process). No thresholds yet; compare before and after.
- Every app can measure itself: own script or task for its budgets, repeatable, in repo. Project adds per-app commands to design.md.
- Media (images, SVG, video) loads at shown size (about twice CSS size, from server's size steps), never original except full-screen. Compressed, modern format (WebP, AVIF, or other) where supported. Memory follows pixels, not file format.
  - Project: Cover size steps: 64, 128, 256, 512, 1024. Enforced.
- CLS by reserving space: fixed aspect or size for media, skeletons of final size.

## Tailwind tokens

- Design values are CSS variables exposed as theme tokens (Tailwind: `@theme`). Theme presets set values; components never pick values.
  - Project: `@theme inline` in `web/src/styles.css`: colors `--color-*`, radii `--radius-*`. Presets in `web/src/themes/`.
- Use token class (`bg-surface`, `border-primary/35`), never arbitrary `bg-[var(--x)]` when token exists.
  - Project: Enforced. Examples: `bg-player-pill`, `text-shell-active-foreground`, `border-player-border`.
- New color = new token in theme, then its class. Never literal in component.
  - Project: Variable in `web/src/themes/layers.css` (preset overrides where differ), registered `--color-<name>` in `@theme inline`.
- Arbitrary `[var(--x)]` only for non-token values: per-element runtime variable, layout geometry, third-party variable.
  - Project: Runtime: `--queue-group-color`. Layout: `--shell-inset`, `--queue-drawer-clearance`. Third-party: Radix.

## Theme and mode

- Every UI ships light and dark mode. Both complete: every token has value in each; contrast rules hold in each.
  - Project: Light and dark, every preset.
- Default follows system (`prefers-color-scheme`). User override persists across sessions.
- Mode switch lives in settings; app-wide, so not in page content. Switch or icon button; shows current mode by icon and label, never color alone.
  - Project: Settings page, Appearance section; light, dark, system.
- Mode change: instant or ≤ 200ms fade on tokens. No full-page animation, no flash of wrong mode on load.
- Media and content keep own colors; only UI tokens change.

## Corner radius

- Three steps plus full and none. Small: controls to ~32px tall, small thumbnails, inline code. Medium: cards, large media, menus, popovers, tooltips, notices, nav pills, icon buttons, window corners. Large: shell surfaces, dialogs, page-level panels. Full: circles, capsules, slider thumbs, count badges, hairlines.
  - Project: Small `--radius-sm` 6px `rounded-sm`. Medium `--radius-md` 10px `rounded-md`. Large `--radius-lg` 16px `rounded-lg`. Enforced.
  - Project: Large parts: Player Bar, Queue Drawer, dialogs, search card, page header panels.
- Side and corner variants same three sizes (`rounded-r-sm`, `rounded-t-lg`).
- Never: bare `rounded`, `rounded-xs`, `rounded-xl` and up, arbitrary `rounded-[…]`, raw length in CSS `border-radius`. CSS: `var(--radius-*)`, `0`, `9999px`.
- Pick step by what element is, not its size in one screenshot. Unsure: smaller.
- Element flush inside rounded parent takes no radius; parent clips with `overflow-hidden`. Nested element with own edge: step no larger than parent's.

## Color

- Colors from tokens. No hex, `rgb()`, `white`, `black` literals in new code; existing ones move to tokens when component touched.
- Exceptions: shadow color `rgb(0 0 0 / alpha)` inside shadow; content-derived colors (next line).
- Color derived from content (cover, avatar, image): clamped for contrast, scoped to its surface, cached per source, falls back to theme. Its computed colors are only allowed literals.
  - Project: Cover accent, setting `playback.accentFromCover`.
  - Project: Accent = cover's dominant saturated hue; near-grey, very dark, very light pixels ignored.
  - Project: Saturation and lightness clamped per theme mode; text on accent dark on light, light on dark.
  - Project: Scoped to Player Bar and Now Playing. Overrides theme accent tokens inside bar only; controls read tokens, never compute colors. Now Playing keeps primary control neutral.
  - Project: Cached per album; previous accent stays while new cover loads. Monochrome or failed cover: theme colors.
  - Project: Accent's computed colors: only color literals this feature may produce.
- Translucent tint = opacity step of token, never new color.

## Interaction states

- Hover and keyboard focus look same. Focus style never removed without visible replacement.
- Current page, open section, pressed toggle share one active look across app.
  - Project: Lit look: 12% `heading` tint background, `heading` text, inset top highlight, soft drop shadow.
- Cards that open something share one hover and focus look (small lift, tinted border, soft shadow, media scale). Titles never underlined.
  - Project: Card look: 4px lift, `shell-active-foreground` border 35% (60% focus), soft dark shadow, artwork scale 1.04.

## Motion

- Motion explains where thing comes from, where it goes. Decoration-only motion removed.
  - Project: Example: Queue Drawer slides up from Player Bar.
- Durations: ~150ms small feedback, 200–300ms ease-out most transitions, 500ms only large media.
- Keyframe animations behind `prefers-reduced-motion: no-preference`.
- Width animation that re-flows grid item by item: snap instead.

## Layout

- Horizontal edges follow one inset variable; content caps at one max width. Breakpoints: Tailwind defaults unless project says otherwise; responsive classes mobile-first within supported range.
  - Project: Edges `var(--shell-inset)`. Content max `--shell-max-content` 1600px.
  - Project: Collection cards 250px, fixed from `lg` up; below `lg` with Queue Drawer open, grid stretches two columns (ADR 0021).
  - Project: Breakpoints Tailwind defaults: `sm` 640px narrowest, `md` 768px main target, `lg` 1024px fixed card layout.

## Avoiding AI slop

Marks of generated, unconsidered UI. Each: replace with restrained version.

- Gradients, glows, blur everywhere: flat token surfaces, one accent.
- Purple-blue gradients, glowing borders: project palette, plain borders.
- Cards with different radii and shadows: one card style.
- Decorative icons, emoji, "✨": content and clear labels.
- Everything centered, same weight: reading order, one focal point.
