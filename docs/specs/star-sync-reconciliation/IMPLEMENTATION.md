# 星标同步分片对账与独立调度 Implementation

## Status

- Lifecycle: active
- Delivery mode: local implementation
- Current state: runtime settings、持久化 coordinator、admin surface 与回归覆盖已实现

## Scope coverage

- [x] Star runtime settings and migration
- [x] Connection-level membership and epoch persistence
- [x] Delta and one-page reconciliation tasks
- [x] Independent scheduler and recovery
- [x] Aggregate Star association recomputation
- [x] Remove Star phase from `sync.subscriptions`
- [x] Admin runtime config and settings UI
- [x] Backend / API / scheduler regression coverage
- [x] Storybook interaction coverage
- [x] Controlled visual evidence confirmed and stored in `./assets/`

## Validation target

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- targeted `cargo test` modules for Star sync, jobs, admin runtime, and API
- `cd web && bun run lint`
- `cd web && bun run build`
- `cd web && bun run storybook:build`

## References

- `./SPEC.md`
- `./HISTORY.md`
- [ADR 0006](../../adr/0006-decouple-star-sync-schedules.md)
