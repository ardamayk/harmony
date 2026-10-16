
## Architecture

```text
packages/contracts/openapi.yaml   ← source of truth
        ↓ generate
packages/api-client               ← TypeScript client
server/                           ← Go API + static embed (being replaced by server-rs/)
server-rs/                        ← Rust Music Server, ported module by module (ADR 0025)
web/                              ← React SPA (dev proxy → Go)
packages/ui                       ← shared layout/widget shell
packages/docs                     ← MDX product docs → /docs
server/migrations                 ← goose migrations; also the sqlc schema
desktop/                          ← Electron shell (main + preload, `vp pack`) around web/
desktop/core                      ← Rust core sidecar: mpv playback, media proxy, Managed Import
packages/desktop-bridge           ← renderer ↔ shell contract (window.earthlyDesktop, command allowlist)
```

Run everything through `mise run <task>` (`mise tasks` lists them). TypeScript tooling is bun + Vite+; run Vite+ on the Bun runtime as `bun --bun vp …` (`bun --bun vp check`, `bun --bun vp test`), never bare `vp`, which runs on Node; see ADR 0023.

## UI design rules

Before changing Tailwind classes or CSS in `web/src` or `packages/ui/src`, read `docs/design-rules.md` and stay within it. The Top Nav, page content, Queue Drawer, Player Bar and desktop window frame follow one design; no part styles itself on its own. In short: theme tokens through Tailwind classes (`bg-player-pill`, not `bg-[var(--player-pill)]`), three corner radii (`rounded-sm` 6px, `rounded-md` 10px, `rounded-lg` 16px, plus `rounded-full`/`rounded-none`), the spacing and type scales, one primary action per area, complete interaction states, and the shared hover/focus/lit states and motion durations. Pages compose shared components; settings live in place on their surface, otherwise in Settings. The Web and Desktop Clients behave alike: a UI fix is checked in both. Pages stay within the performance budgets (LCP ≤ 2.5s, INP ≤ 200ms, CLS ≤ 0.1, TTFB ≤ 800ms); `mise run web:lighthouse` checks them on a production build. `web/src/design-rules.test.ts` enforces the radius and token rules; fix the code, not the check. A new visual pattern gets a rule in that file first.

## Music Server modules

`server/internal/modules/<module>/` is one Go package per module, layered:

```text
handler.go     HTTP: parse request → call service → respond
service.go     domain rules; no SQL, no net/http
repository.go  the module's only database access; maps rows to domain types
queries.sql    hand-written SQL (Managed Import splits it into queries/*.sql)
db/            sqlc output — regenerate with `mise run generate`, never edit
module.go      wiring and route registration
```

- A schema change is a new goose migration in `server/migrations`; sqlc reads the migrations as its schema. Add the query to `queries.sql`, then `mise run generate`.
- Multi-statement writes run in one repository transaction (`q.WithTx(tx)`); the database begins transactions IMMEDIATE.
- Modules reach each other only through exported service interfaces (for example `library.TrackReader`), never another module's `db/` or repository.
- sqlc on SQLite leaves `sqlc.arg()` untranslated inside `ORDER BY`; bind such parameters through a `WITH params AS (...)` CTE instead.

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues using the `gh` CLI; external PRs are not a triage surface for now. See `docs/agents/issue-tracker.md`.

### Triage labels

Uses the default five-label triage vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout: repo-root `CONTEXT.md` plus `docs/adr/` for ADRs. See `docs/agents/domain.md`.
