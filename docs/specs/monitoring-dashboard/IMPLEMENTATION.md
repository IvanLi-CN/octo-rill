# OctoRill 核心性能监控实施覆盖

## 当前状态

- Lifecycle: active
- Implementation: 应用 exporter、生命周期埋点、Grafana/Prometheus 资产和只读 CLI 已落地；隔离 VM 的 listener、Compose、Grafana 查询路径、HTTP 性能采集和真实看板视觉证据已完成，SQLite 竞争负载、CI 与 PR 收敛仍待完成。
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## Coverage Map

| Requirement | Implementation surface | Status |
| --- | --- | --- |
| REQ-MON-01～REQ-MON-08 | `src/metrics.rs` registry, OpenMetrics endpoint, bounded labels/encoding | implemented |
| REQ-MON-09～REQ-MON-11 | `src/observability.rs`, `src/jobs.rs` HTTP/SSE guards | implemented |
| REQ-MON-12～REQ-MON-13 | `src/sqlite_write.rs` lifecycle recording | implemented |
| REQ-MON-14～REQ-MON-15 | `src/metrics.rs` persistent queue sampler | implemented |
| REQ-MON-16～REQ-MON-17 | `src/sync.rs`, `src/webhook_push.rs`, `src/ai.rs` upstream outcome recording | implemented |
| REQ-MON-18～REQ-MON-20 | `src/metrics.rs` process snapshot and stale sampler | implemented |
| REQ-MON-21～REQ-MON-21 | `src/server.rs`, `src/metrics.rs` independent listener/auth | implemented |
| REQ-MON-22～REQ-MON-26 | `scripts/octorill-observe`, `ops/observability/query-catalog.json` | implemented |
| REQ-MON-27～REQ-MON-29 | `ops/observability/grafana`, `ops/observability/prometheus` | implemented |
| REQ-MON-30 | `docs-site/docs/monitoring.md`, internal runbook and `.env.example` | partial |

## Verification Coverage

- Local Rust compile and focused metric unit tests are required before commit.
- JSON catalog, dashboard files, rule files and CLI contract are checked by `scripts/validate_observability.py`.
- `python3 bin/spec_contract_check.py --path docs/specs/monitoring-dashboard/SPEC.md` validates stable requirement/verification references.
- Agent VM validation covers Compose provisioning, token boundary, Prometheus/Grafana query paths, sampler fault recovery and HTTP A/B runtime overhead; raw measurements are recorded in [VALIDATION.md](./VALIDATION.md).
- Real Grafana screenshots were captured from the session-owned Agent VM with `target_program=Grafana` and dashboard-only scope; the owner confirmed the five current-only images and they are persisted in the topic Spec's Visual Evidence section.

## Known Limits

- Metrics are process-local and not a history store; Prometheus retention and multi-instance aggregation belong to deployment.
- Resource metrics are unavailable on non-Linux platforms and report non-finite samples so dashboards and the CLI expose degraded/unavailable state rather than synthetic zero values.
- 101 target enablement and production connectivity are intentionally deferred to a separately authorized deployment change.
- The three A/B pairs covered the fixed HTTP load but not the required SQLite competition load; VM background activity also made throughput vary in both directions. The raw data does not satisfy the throughput gate, so A-PERF remains unresolved rather than being widened or treated as passed.
