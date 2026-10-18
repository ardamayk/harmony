# Library Search timing

The existing `TestLibrarySearchQuality` checks 142 labeled queries and prints one
summary: `SEARCH count=… total=… p50=… p95=…`.

```bash
go -C server test ./internal/modules/librarysearch -run '^TestLibrarySearchQuality$' -count=1 -v
```

Each sample measures an in-process HTTP search call plus response decoding.
Database setup and correctness assertions are excluded; the first search is
included. Percentiles use nearest rank. These are small-fixture timings, not
network/browser latency or a production capacity guarantee. No speed threshold
fails the test; correctness failures still do.

The dedicated 50,000-Track seeder, browser benchmark and raw JSON reporting were
removed on 2026-09-12 at the user's request. The normal Playwright suite retains
`web/e2e/library-search.spec.ts` for real import, search, playback, Album navigation
and deletion freshness. Detailed load testing can return if an observed slowdown
requires it.
