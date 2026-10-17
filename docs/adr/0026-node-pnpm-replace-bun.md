---
status: accepted
---

# Node and pnpm replace bun

The TypeScript workspace runs on Node only. pnpm manages dependencies (`pnpm-workspace.yaml` holds the workspace packages, the `catalog:` versions, overrides and the packages allowed to run install scripts; `pnpm-lock.yaml` is the lockfile). Vite+ (`vp`) stays the toolchain for formatting (Oxfmt), linting (Oxlint, type-aware), type checks, tests (Vitest), builds, staged-file checks and Git hooks, and now runs on Node through its own shebang: bare `vp`, never `bun --bun vp`. Repository scripts in `scripts/*.ts` run with `node` (Node 24 strips TypeScript types natively). mise pins Node and pnpm.

Electron, Playwright and Vite+ already target Node, so one runtime now covers development, tests, CI, the Docker build and the Desktop Client.

Supersedes the bun part of [ADR 0023](0023-bun-viteplus-single-ci-workflow.md).
