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
- social activity snapshot 与 feed activity event 持久化已接入 writer coordinator；读取 current-member / 去重快照后再写入 `social_activity_events` 的事务现在也通过 `BEGIN IMMEDIATE` 串行进入 SQLite writer。
- translation request/batch claim/finalize/recovery/heartbeat 已接入 writer coordinator。
- translation batch 启动写段已补齐到 writer coordinator：`translation_batches` 的 `queued -> running` 与 `translation_work_items` 的 `running` 标记在单个短事务内串行提交，AI 调用继续留在 permit 外。
- LLM call insert/event/running/requeue/finalize/heartbeat/recovery 已接入 writer coordinator。
- LLM call retention cleanup 改为 best-effort writer lane；writer permit 不可得或 SQLite busy 时跳过本轮清理，并留下结构化 `sqlite.write` downgrade 日志，而不是把后台保留任务放大成周期性 warning spam 或主流程失败。
- runtime owner register/heartbeat 已接入 writer coordinator；`touch_user_last_active_at` 使用非阻塞 best-effort writer 尝试，拿不到 permit 时跳过，SQLite busy/locked 时记录 warning 并继续用户请求。
- feed reaction refresh 的 counts 持久化改为 best-effort writer lane；writer permit 不可得或 SQLite busy 时跳过持久化，但保留 live payload 返回与结构化 warning。
- subscription sync 的 `sync_subscription_events` 插入已接入 writer coordinator；`repo_release_watchers` / `sync_subscription_events` 历史裁剪改为 best-effort writer lane，在 writer permit 不可得或 SQLite busy 时跳过本轮清理并留下结构化 downgrade 日志。
- subscription sync history prune 的两个 retention 相位现在彼此独立：`repo_release_watchers` 裁剪即使因为 writer pressure / SQLite busy 降级跳过，也不会短路后续 `sync_subscription_events` 裁剪，避免一个 best-effort 相位把另一相位一起饿死。
- `starred_repos` 的增量 upsert、通知 inbox upsert / open-url repair，以及 `public_repo_release_usage` 元数据刷新也已收回 writer coordinator；其中 notification open-url repair 的 GitHub thread lookup 保持在 writer permit 外，只把最终批量更新放进短事务，避免后台修复路径长时间占住 SQLite writer。
- notification open-url repair 在短事务里会先按 `thread_id` 重读当前行，再决定是否回写修复结果；这样既保留 lookup 在 permit 外的短锁收益，也不会让较早的 repair lookup 覆盖并发 notification sync 刚写入的更新标题、`updated_at`、`unread` 或目标 URL。
- jobs scheduler 的 `daily_brief_hour_slots.last_dispatch_at`、`scheduled_task_dispatch_state` 写入，以及 brief history/content refresh 失败标记也已收回 writer coordinator，避免 20s/45s 周期调度写和失败补偿写继续绕过协调层挤占 task claim / heartbeat。
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
- `src/translations.rs` 新增 batch 启动写段在 writer 压力下串行化回归，以及结果聚合在 writer 背压下直接复用 pending 快照的回归。
- `src/sync.rs` 新增 social activity snapshot 与 feed activity event 在 competing writer 下等待并成功提交的并发回归。
- `src/api.rs` 新增 feed reaction refresh 在 SQLite writer 压力下跳过持久化但继续返回 live item 的回归。
- `src/jobs.rs` 新增后台 writer 压力下 `enqueue_task` 等待 coordinator 而不是绕过写入背压的回归测试。
- `src/jobs.rs` 新增 daily slot dispatch、scheduled dispatch state 与 brief failure mark 在 competing writer 下等待成功提交的并发回归测试。
- `src/sync.rs` 新增 subscription event 写入在 competing writer 下等待成功提交，以及 subscription history prune 在 writer permit 不可得或 SQLite busy 时降级跳过的回归测试。
- `src/sync.rs` 新增 `starred_repos` 增量 upsert 与通知 upsert 在 competing writer 下等待成功提交的并发回归测试。
- `src/ai.rs` 新增 LLM retention cleanup 在 writer permit 不可得或 SQLite busy 时降级跳过的回归测试。

## References

- `./SPEC.md`
- `./HISTORY.md`
