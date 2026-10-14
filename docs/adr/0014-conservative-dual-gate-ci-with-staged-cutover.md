# Pull requests use conservative dual CI gates with a staged cutover

> Superseded by [ADR 0023](0023-bun-viteplus-single-ci-workflow.md).

Every pull request reports stable Fast Gate and Integration Gate results, with a repository-owned classifier choosing a deliberately conservative integration set and global or unknown changes selecting all applicable tests. Fast and integration work starts in parallel, superseded pull-request runs are cancelled, and conditional jobs report through stable aggregators so required checks never disappear. The new gates run in shadow mode for ten representative pull requests before an atomic ruleset cutover, followed by a 30-run review before narrower classification or a new CI engine is considered.

On 2026-09-08, the repository owner explicitly chose to end duplicate legacy CI and require `PR / Fast Gate` plus `PR / Integration Gate` after the controlled failure/cancellation checks and clean-head verification in PR #138. The legacy `e2e`/`lint-test` workflow is removed; the outstanding representative post-optimization p95 evidence is accepted as a follow-up risk rather than a cutover blocker. The 30-run post-cutover review remains applicable.
