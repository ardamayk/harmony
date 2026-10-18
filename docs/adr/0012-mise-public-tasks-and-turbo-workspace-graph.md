# Mise owns public tasks and Turbo owns the workspace graph

> Superseded by [ADR 0023](0023-bun-viteplus-single-ci-workflow.md).

Mise is the canonical developer and CI task interface and composes work across the TypeScript workspace, Music Server, and Desktop Client, while Turbo owns package relationships, filtering, caching, and parallelism inside the pnpm workspace. The Music Server remains a direct Go task and the Desktop Client participates through its existing workspace package; GitHub Actions owns hosted orchestration, and `act` is only an optional local workflow smoke test. This avoids competing task graphs and defers Just, Dagger, Earthly, and Moonrepo until measured follow-up research justifies migration.
