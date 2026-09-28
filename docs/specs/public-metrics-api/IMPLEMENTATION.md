# 公开指标 API 实施覆盖

## 当前状态

- Lifecycle: active
- Implementation: committed locally; visual evidence is owner-confirmed and persisted; PR publication is next.
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

Focused Rust tests cover the public field set, metrics, freshness order, partial warm-up, 12-point ordering, stale cache fallback, CORS, ETag, GET-only behavior and rate limiting.

- `cargo fmt --all -- --check`
- `bash ./scripts/check-rust-source-quality.sh` on `codex-testbox` (pass)
- `cargo check --locked` on `codex-testbox`
- `cargo test --locked public_metrics -- --nocapture` on `codex-testbox` (`8 passed`)
- `cd docs-site && bun run build`
- Ego Browser: navigated from the docs home to the public metrics page and captured `1440x900` and `393x852` candidates; neither viewport has page-level horizontal overflow.
- `git diff --check`

## Remaining Gaps

- Remote CI and review convergence have not started.
