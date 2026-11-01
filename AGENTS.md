
## Architecture

```text
packages/contracts/openapi.yaml   ← source of truth
        ↓ generate
packages/api-client               ← TypeScript client
server/                           ← Rust Music Server (axum + sqlx) + embedded Web Client and docs
web/                              ← React SPA (dev proxy → Music Server)
packages/ui                       ← shared layout/widget shell
packages/docs                     ← MDX product docs → /docs
server/migrations                 ← goose-format SQL migrations; also the sqlx query schema
desktop/                          ← Electron shell (main + preload, `vp pack`) around web/
desktop/core                      ← Rust core sidecar: mpv playback, media proxy, Managed Import
packages/desktop-bridge           ← renderer ↔ shell contract (window.earthlyDesktop, command allowlist)
mobile/                           ← parked plan for a native iPhone client; phones use web/ as a PWA (ADR 0041)
```

Run everything through `mise run <task>` (`mise tasks` lists them). TypeScript tooling is Node + pnpm + Vite+ (`vp`: Oxlint, Oxfmt, Vitest); run it as `vp check`, `vp test` inside mise, which puts `node_modules/.bin` on `PATH`. Bun is not used; see ADR 0026.

## Music Server modules

`server/src/modules/<module>/` is one Rust module per domain, layered:

```text
mod.rs         wiring: submodules, what other modules may use, `routes`
handlers.rs    HTTP: parse request → call service → respond
service.rs     domain rules; no SQL, no HTTP types
repository.rs  the module's only database access; maps rows to domain types
```

- A schema change is a new migration in `server/migrations` (goose's `-- +goose Up` / `Down` format; the server's own migrator applies the Up sections). Write queries with `sqlx::query!`, which checks them against a schema built from the migrations; then `mise run generate` refreshes the offline query data in `server/.sqlx`, which CI checks.
- Multi-statement writes run in one transaction from `db::begin_write`, which begins IMMEDIATE.
- Modules reach each other only through public service APIs, never another module's repository.
- sqlx infers SQLite column nullability conservatively; override with `AS "name!"` / `AS "name?"` / `AS "name!: Type"` instead of unwrapping.
- Responses keep the JSON the Web and Desktop Clients expect: `gojson` holds the encoding rules (HTML-escaped JSON written to the database, whole floats without `.0`, RFC 3339 times) inherited from the original Go server (ADR 0025).

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues using the `gh` CLI; external PRs are not a triage surface for now. See `docs/agents/issue-tracker.md`.

### Triage labels

Uses the default five-label triage vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout: repo-root `CONTEXT.md` plus `docs/adr/` for ADRs. See `docs/agents/domain.md`.
