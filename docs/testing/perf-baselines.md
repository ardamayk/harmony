# Performance baselines

Benchmark medians recorded at each release by the `release-check` skill, so the next release compares against real numbers. Each entry names the version, commit, date, machine and library size; numbers from different machines are not compared.

The budgets live in `docs/design-rules.md` ("Performance budgets"). `perf:server-load` and `perf:snapshot` have no budgets yet; once three releases are recorded here, set budgets from them.

<!-- New entries go below, newest first. -->

## Rust Music Server 0.1.0 (pre-release) — bbfef4a, 2026-10-17

First baseline after the switch from Go to Rust. Machine: AMD Ryzen 7 7800X3D (8 cores / 16 threads), 30 GB RAM, Linux 7.2 (CachyOS). Library: 163 Tracks, 10 Albums, 19 Artists. `bin/server` release build; three `perf:server-load` runs, medians shown.

`perf:server-load` (400 requests per endpoint, 8 concurrent)

| | value |
|---|---|
| startup (warm) | 77 ms (first cold run 242 ms) |
| idle memory / CPU | 14 MB / 0.0% |
| under load: peak memory / CPU | 33 MB / 531% of one core |
| stream throughput (64.3 MB Track) | 256 MB/s, peak 33 MB |

| endpoint | p50 | p95 |
|---|---|---|
| albums list | 1.3 ms | 1.6 ms |
| album detail | 1.1 ms | 1.4 ms |
| tracks list (100) | 3.2 ms | 3.9 ms |
| artists list | 0.3 ms | 0.4 ms |
| queue | 2.8 ms | 3.2 ms |
| search, covers, waveform, playlists, radio, health | ≤ 0.2 ms | ≤ 1.2 ms |

No failed requests.

`perf:snapshot` (30 s per phase, Music Server only): idle 0.0% CPU, 14 MB; streaming one Track at 200 KB/s avg 0.1% CPU (peak 1.0%), 16 MB.

`web:lighthouse`: all budgets pass. LCP 1521–1962 ms (`/library/albums` slowest), TBT ≤ 5 ms, CLS ≤ 0.009, TTFB 0 ms.
