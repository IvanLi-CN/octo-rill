# SQLite 单写入调度层实现状态（#5prh7）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Implementation: 已实现，本地验证通过
- Lifecycle: active
- Catalog note: fast-track / SQLite writer coordinator

## Coverage / rollout summary

- 新增 `src/sqlite_write.rs`，提供 `SqliteWriteCoordinator`、单 writer permit、foreground/background/best-effort priority、`BEGIN IMMEDIATE` 事务入口、busy/locked 分类、bounded retry 与 tracing telemetry。
- `AppState` 持有共享 coordinator；生产启动与测试 state 初始化均注入同一运行时组件。
- `job_tasks` enqueue/event/cancel/claim/finalize/heartbeat 已接入 writer coordinator；enqueue/event/cancel 使用 foreground lane。
- session create/save/delete 使用 foreground lane 与短 busy retry；过期 session 清理使用 best-effort lane。
- repo release attach/claim/finalize/watchers/heartbeat/fail/upsert/sync-state 已接入 writer coordinator。
- translation request/batch claim/finalize/recovery/heartbeat 已接入 writer coordinator。
- LLM call insert/event/running/requeue/finalize/heartbeat/recovery 已接入 writer coordinator。
- runtime owner register/heartbeat 已接入 writer coordinator；`touch_user_last_active_at` 使用非阻塞 best-effort writer 尝试，拿不到 permit 时跳过，SQLite busy/locked 时记录 warning 并继续用户请求。
- 网络、GitHub API、AI 调用与长耗时处理仍留在 writer permit 外；permit 只包住 SQLite 写入段。

## Validation

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --locked --all-features`

## Remaining Gaps

- 待完成 PR CI / review 收敛与 merge cleanup。

## Related Changes

- `docs/solutions/backend/sqlite-wal-write-transactions.md` 更新为 writer coordinator + `BEGIN IMMEDIATE` 的复用方案。
- `src/sqlite_write.rs` 新增 WAL + 多连接 pool 并发写入与 foreground 优先级回归测试。
- `src/jobs.rs` 新增后台 writer 压力下 `enqueue_task` 等待 coordinator 而不是绕过写入背压的回归测试。

## References

- `./SPEC.md`
- `./HISTORY.md`
