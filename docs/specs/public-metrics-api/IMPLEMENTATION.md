# 公开指标 API 实施覆盖

## 当前状态

- Lifecycle: active
- Implementation: review repairs are applied, local validation passes, and refreshed visual evidence is owner-confirmed and persisted.
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## Coverage Map

| Requirement | Intended implementation surface | Status |
| --- | --- | --- |
| REQ-PUBLIC-METRICS-001 | `src/public_metrics.rs` public serializer and router | implemented |
| REQ-PUBLIC-METRICS-002 | governance snapshot and runtime budget aggregation | implemented |
| REQ-PUBLIC-METRICS-003 | freshness-code calculation in priority order | implemented |
| REQ-PUBLIC-METRICS-004 | hourly aggregate migration and partial trend query | implemented |
| REQ-PUBLIC-METRICS-005 | process cache, singleflight refresh, timeout and stale fallback | implemented |
| REQ-PUBLIC-METRICS-006 | ETag, conditional response and cache headers | implemented |
| REQ-PUBLIC-METRICS-007 | separate public router and per-IP limit | implemented |
| REQ-PUBLIC-METRICS-008 | isolated CORS and environment configuration | implemented |

## Verification Coverage

Focused Rust tests cover the public field set, metrics, freshness order, partial warm-up, 12-point ordering, exact 24-hour retention, concurrent refresh coalescing, migration-backed sampling, cold-start retryable errors, stale cache fallback, CORS, ETag, GET-only behavior, rate limiting and authenticated-router isolation.

- `cargo fmt --all -- --check`
- `bash ./scripts/check-rust-source-quality.sh` on `codex-testbox` (pass)
- `cargo check --locked` on `codex-testbox`
- `cargo test --locked public_metrics -- --nocapture` (`13 passed`)
- `cd docs-site && bun run build`
- Ego Browser: owner-confirmed captures are persisted at `1440x900` and `393x852`; the locked base commit had no same-path image baseline.
- `git diff --check`

## Remaining Gaps

- Remote CI/review convergence has not started.
