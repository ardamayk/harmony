
## Architecture

```text
packages/contracts/openapi.yaml   ← source of truth
        ↓ generate
packages/api-client               ← TypeScript client
server/                           ← Go API + static embed
web/                              ← React SPA (dev proxy → Go)
packages/ui                       ← shared layout/widget shell
packages/docs                     ← MDX product docs → /docs
server/migrations                 ← goose migrations; also the sqlc schema
```

Run everything through `mise run <task>` (`mise tasks` lists them). TypeScript tooling is bun + Vite+ (`vp check`, `vp test`); see ADR 0023.

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
