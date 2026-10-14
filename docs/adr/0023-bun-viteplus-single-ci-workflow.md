---
status: accepted
---

# bun and Vite+ replace pnpm, Turbo, Biome, and Husky; one CI workflow

The TypeScript workspace uses bun workspaces for dependencies and Vite+ (`vp`) for formatting (Oxfmt), linting (Oxlint, type-aware), type checks, tests (Vitest), builds, staged-file checks, Git hooks, and workspace task running. Mise still pins tools and composes the cross-language public tasks (`check`, `test`, `build`, `ci`), but it no longer sources a storage layer: tools write to their normal locations (`node_modules`, `dist`, `target`, the Go cache).

The worktree-aware external storage wrappers, the pull-request change classifier, the dual Fast/Integration gates, clean-room verification, and the tests that asserted workflow YAML text were removed. A single `ci.yml` runs web, server, desktop, and E2E jobs on every pull request and the slow integration job (HLS, pinned mpv, import parity, release build) on `main`, weekly, or on demand. For a single-developer, self-hosted project, that infrastructure cost more to maintain than the build minutes and disk it saved.

Supersedes [ADR 0012](0012-mise-public-tasks-and-turbo-workspace-graph.md) and [ADR 0014](0014-conservative-dual-gate-ci-with-staged-cutover.md).
