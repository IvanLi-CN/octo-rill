# Monitoring Dashboard Validation

## Local checks

- The pre-repair candidate passed `cargo test --locked --all-features` with 1050 passed, 0 failed, and 2 ignored after the SQLite lifecycle instrumentation change.
- The repair candidate passes the focused metrics suite (19 tests), the webhook outcome classification test, the GitHub outcome classification test, and `cargo check --locked --all-features` locally; a fresh full suite is delegated to CI because the first Agent VM cold build exceeded the 25-minute CI envelope before producing a test result.
- `cargo fmt --all -- --check`, Rust source quality checks, observability asset validation, CLI fixture tests, and the topic contract checker are required release gates.
- The docs site production build completed with `DOCS_BASE=/octo-rill/ bun run build`.

## Isolated VM checks

- Metrics disabled leaves port `58091` closed; enabling metrics with the configured token exposes OpenMetrics on the independent listener.
- Wrong Bearer credentials return `401`; the correct token returns OpenMetrics without querying the business database during scrape.
- On the visual-evidence candidate synced into the isolated VM, Prometheus 3.15.0 reports the application target `up=1` with an empty error; Grafana 13.2.3 reports healthy and provisions the five stable dashboard UIDs (`octorill-overview`, `octorill-http-sse`, `octorill-sqlite`, `octorill-queues`, `octorill-upstream`). Later repair commits changed only metric semantics and recording/query expressions; the stored screenshots remain layout/provisioning evidence from that runtime candidate, while the exact final application rebuild remains a CI gate.
- The Grafana datasource proxy and direct Prometheus backend both return the fixed CLI health query; default rules are not loaded by the sample Compose file.
- The visual-evidence candidate smoke returned `401` for an incorrect token and OpenMetrics for the correct token; `octorill_enabled=1`, `octorill_sampler_stale=0`, and the payload was 206078 bytes. The Grafana datasource proxy returned CLI schema 1 with `status=ok` and finite samples for all six health queries.
- An earlier 100-scrape enabled sample measured p50 4.64ms, p95 35.23ms, p99 61.95ms, maximum 61.95ms, and maximum payload 78051 bytes; the latest smoke confirms the same listener and sampler contract after the visual-evidence candidate rebuild.
- The repair-candidate Agent VM check parsed `docker compose config` and all 11 Prometheus rules with `promtool`; the application full-suite container was stopped after a 180-second hard timeout without a test result. The final candidate's required CI Backend Tests and Build (Release) checks later passed on the exact PR head.

## Runtime A/B measurements

The final candidate image was built from `e03ee8a9` in the session-owned Agent VM (`2 vCPU`, `4 GiB`, Linux/Docker) and exercised with a fresh temporary SQLite database and encryption key per run. Each run used eight `/api/health` foreground clients and eight SQLite writer workers against an isolated probe table, with a 60-second warmup and 180-second observation window. Process CPU and RSS came from the container process `/proc` data; all foreground requests completed successfully.

| Pair | Metrics | HTTP/s | CPU s | CPU s/HTTP | p95 s | p99 s | RSS peak | SQLite commits | SQLite busy |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | off | 1257.32 | 61.00 | 0.000270 | 0.01262 | 0.02662 | 43.9 MiB | 22685 | 10030 |
| 1 | on | 1484.86 | 61.50 | 0.000230 | 0.01047 | 0.02075 | 45.6 MiB | 24400 | 10420 |
| 2 | off | 1055.29 | 61.46 | 0.000324 | 0.01602 | 0.04523 | 46.2 MiB | 19962 | 9377 |
| 2 | on | 1001.39 | 60.14 | 0.000334 | 0.01849 | 0.04784 | 44.8 MiB | 19884 | 9572 |
| 3 | off | 1392.28 | 60.42 | 0.000241 | 0.01111 | 0.02071 | 43.5 MiB | 24042 | 10369 |
| 3 | on | 1468.91 | 62.38 | 0.000236 | 0.01047 | 0.01930 | 43.8 MiB | 24480 | 10432 |

The three-pair medians are `1257.32 -> 1468.91 HTTP/s` (`+16.8%`), `0.000270 -> 0.000236 CPU seconds/HTTP` (`-12.5%`), `0.01262 -> 0.01047s` p95 (`-17.0%`), `0.02662 -> 0.02075s` p99 (`-22.0%`), and `43.9 -> 44.8 MiB` peak RSS (`+0.86 MiB`). Pair 2's individual throughput change is a `5.1%` decrease; it is retained as an outlier in the raw table and does not change the plan's explicitly median-based A-PERF gate. The median results satisfy the throughput, CPU, latency, and RSS budgets.

Prometheus recorded 39 enabled-window `scrape_duration_seconds` samples across the three on runs; their p95 was `37.8ms` and maximum `54.9ms`, below the `250ms` scrape budget. This closes A-PERF for the documented synthetic mixed-load scenario; it does not claim production traffic representativeness.

## Evidence status

- The 101 instance was inspected read-only; production target enablement, network changes, credentials, and online connectivity remain deferred.
- Real Grafana screenshots were captured from the final candidate in the session-owned Agent VM after Grafana provisioning completed; the owner confirmed the five current-only images. The canonical image set and capture scope are recorded in the [SPEC Visual Evidence section](./SPEC.md#visual-evidence), with no representative or mock images used.
