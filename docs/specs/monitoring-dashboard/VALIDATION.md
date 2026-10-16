# Monitoring Dashboard Validation

## Local checks

- `cargo test --locked --all-features`: 1050 passed, 0 failed, 2 ignored after the final SQLite lifecycle instrumentation change.
- `cargo fmt --all -- --check`, Rust source quality checks, observability asset validation, CLI fixture tests, and the topic contract checker are required release gates.
- The docs site production build completed with `DOCS_BASE=/octo-rill/ bun run build`.

## Isolated VM checks

- Metrics disabled leaves port `58091` closed; enabling metrics with the configured token exposes OpenMetrics on the independent listener.
- Wrong Bearer credentials return `401`; the correct token returns OpenMetrics without querying the business database during scrape.
- On the final candidate synced into the isolated VM, Prometheus 3.15.0 reports the application target `up=1` with an empty error; Grafana 13.2.3 reports healthy and provisions the five stable dashboard UIDs (`octorill-overview`, `octorill-http-sse`, `octorill-sqlite`, `octorill-queues`, `octorill-upstream`).
- The Grafana datasource proxy and direct Prometheus backend both return the fixed CLI health query; default rules are not loaded by the sample Compose file.
- The final candidate smoke returned `401` for an incorrect token and OpenMetrics for the correct token; `octorill_enabled=1`, `octorill_sampler_stale=0`, and the payload was 206078 bytes. The Grafana datasource proxy returned CLI schema 1 with `status=ok` and finite samples for all six health queries.
- An earlier 100-scrape enabled sample measured p50 4.64ms, p95 35.23ms, p99 61.95ms, maximum 61.95ms, and maximum payload 78051 bytes; the latest smoke confirms the same listener and sampler contract after the final candidate rebuild.

## Runtime A/B measurements

The same debug binary, SQLite database, encryption key, 32 concurrent clients, 60-second warmup, and 180-second observation window were used for each HTTP run. The process benchmark used `/api/health` and recorded process CPU and RSS from `/proc`; the required mixed SQLite competition load remains outstanding.

| Pair | Metrics | Throughput/s | CPU seconds | p95 seconds | p99 seconds | RSS delta | Failures |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | off | 156.72 | 281.86 | 0.27425 | 0.72469 | 2.97 MiB | 0 |
| 1 | on | 149.22 | 280.83 | 0.28369 | 0.73664 | 1.25 MiB | 0 |
| 2 | off | 389.44 | 246.01 | 0.09850 | 0.21137 | 0.25 MiB | 0 |
| 2 | on | 282.60 | 270.97 | 0.13441 | 0.30303 | 0.93 MiB | 0 |
| 3 | off | 162.16 | 287.97 | 0.26025 | 0.62137 | 1.70 MiB | 0 |
| 3 | on | 398.38 | 295.51 | 0.24582 | 0.56198 | 3.00 MiB | 0 |

The observed directions conflict across pairs, and pair 2 exceeds the five-percent throughput budget while pair 3 improves throughput with monitoring enabled. A-PERF therefore remains unresolved pending a repeat on a quieter, controlled VM allocation; acceptance thresholds are unchanged.

## Evidence status

- The 101 instance was inspected read-only; production target enablement, network changes, credentials, and online connectivity remain deferred.
- Real Grafana screenshots were captured from the final candidate in the session-owned Agent VM after Grafana provisioning completed; the owner confirmed the five current-only images. The canonical image set and capture scope are recorded in the [SPEC Visual Evidence section](./SPEC.md#visual-evidence), with no representative or mock images used.
