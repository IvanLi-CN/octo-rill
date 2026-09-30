# SQLite 单写入调度层实现状态

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
- social activity snapshot 与 feed activity event 持久化已接入 writer coordinator；social snapshot 先在 permit 外读取 current-member、history、stale association 与 stale repo/member 候选，再按固定 64 行 chunk 分阶段执行 `BEGIN IMMEDIATE`，chunk 之间释放 permit。current-member/history materialization、baseline、stale cleanup 与 association source 清理保持幂等和可中断恢复，并记录候选读取、writer wait、query elapsed、chunk elapsed 与 chunk count。
- translation request/batch claim/finalize/recovery/heartbeat 已接入 writer coordinator。
- translation batch 启动写段已补齐到 writer coordinator：`translation_batches` 的 `queued -> running` 与 `translation_work_items` 的 `running` 标记在单个短事务内串行提交，AI 调用继续留在 permit 外。
- LLM call insert/event/running/requeue/finalize/heartbeat/recovery 已接入 writer coordinator。
- LLM running/requeue/finalize 更新会比较当前 runtime owner 与状态；失去 lease 的旧 worker 不得覆盖新的 recovery 结果。LLM queued/running/requeue/recovery/finalize 都会在同一 `BEGIN IMMEDIATE` 事务中提交状态更新和对应事件，避免 lifecycle 状态与审计事件分叉。
- LLM call retention cleanup 改为 best-effort writer lane；writer permit 不可得或 SQLite busy 时跳过本轮清理，并留下结构化 `sqlite.write` downgrade 日志，而不是把后台保留任务放大成周期性 warning spam 或主流程失败；完成态额外记录 cutoff、删除行数与 elapsed_ms，便于区分“无事可做”“writer pressure 跳过”“真实慢删除”。
- runtime owner register/heartbeat 已接入 writer coordinator；`touch_user_last_active_at` 使用非阻塞 best-effort writer 尝试，拿不到 permit 时跳过，SQLite busy/locked 时记录 warning 并继续用户请求。
- API key 创建/撤销、daily brief profile 更新、LinuxDO/GitHub connection unlink 与 passkey 删除使用 foreground lane；每个 AppState 使用独立的 FIFO 后台 worker，按 key 合并 `last_used_at` touch 后排队到 background lane，并在优雅关机时有界 drain，避免跨库队头阻塞、饥饿与 task 积压；SQL 只允许时间戳单调前进；stale runtime owner prune 使用 best-effort lane，runtime owner unregister 使用 background lane，避免这些应用内写路径绕过共享 coordinator。
- feed reaction refresh 的 counts 持久化改为 best-effort writer lane；writer permit 不可得或 SQLite busy 时跳过持久化，但保留 live payload 返回与结构化 warning。
- repo refresh governance rebuild 已拆成 cleanup、snapshot upsert chunks、terminal member reconciliation chunks、legacy member backfill chunks、snapshot completion 与 cycle reconciliation 阶段；snapshot/member chunk size 固定为 500，并输出 chunk count 与最大 writer chunk elapsed telemetry。
- subscription sync 的 `sync_subscription_events` 插入已接入 writer coordinator；`repo_release_watchers` / `sync_subscription_events` 历史裁剪改为 best-effort writer lane，在 writer permit 不可得或 SQLite busy 时跳过本轮清理并留下结构化 downgrade 日志。
- 对超大 retention 表的运行态补丁继续落在同一 contract 内：watcher / event 裁剪批次扩大，并追加完成耗时、batch 配置与降级原因埋点，避免“小批次永远追不上存量”时把 writer contract 正确性问题和 backlog 问题混在一起。
- subscription sync history prune 的两个 retention 相位现在彼此独立：`repo_release_watchers` 裁剪即使因为 writer pressure / SQLite busy 降级跳过，也不会短路后续 `sync_subscription_events` 裁剪，避免一个 best-effort 相位把另一相位一起饿死。
- `starred_repos` 的增量 upsert、通知 inbox upsert / open-url repair，以及 `public_repo_release_usage` 元数据刷新也已收回 writer coordinator；其中 notification open-url repair 的 GitHub thread lookup 保持在 writer permit 外，只把最终批量更新放进短事务，避免后台修复路径长时间占住 SQLite writer。
- notification open-url repair 在短事务里会先按 `thread_id` 重读当前行，再决定是否回写修复结果；这样既保留 lookup 在 permit 外的短锁收益，也不会让较早的 repair lookup 覆盖并发 notification sync 刚写入的更新标题、`updated_at`、`unread` 或目标 URL。若 refresh 拿到的 thread metadata 时间戳更新，则 repair 仍会覆盖旧的标题/类型/reason/目标 URL，避免“当前行非空但已过时”把修复永久卡住。
- jobs scheduler 的 `daily_brief_hour_slots.last_dispatch_at`、`scheduled_task_dispatch_state` 写入，以及 brief history/content refresh 失败标记也已收回 writer coordinator，避免 20s/45s 周期调度写和失败补偿写继续绕过协调层挤占 task claim / heartbeat。
- admin runtime settings 的 seed/backfill/update、LLM recovery flags 与 model health、translation runtime settings 均通过共享 `AppState.sqlite_writer` 写入；生产启动和 runtime heartbeat 不再调用 raw pool writer。
- admin LLM/translation runtime PATCH 由共享 runtime-settings lock 串行化；在 live scheduler 应用失败时，LLM、recovery flags 与 translation worker settings 通过同一个 coordinator transaction 恢复此前持久化快照，再 reconcile 内存 scheduler。scheduler 同步会在修改内存前读取新的 model health，并在 translation slot 持久化失败时恢复 LLM routing/concurrency/health。
- repo release work item 的成功、失败、deadline recovery 与 stale recovery 会在一个协调器事务中同时提交 work item 终态、pending watcher 状态和 governance attempt；执行 worker 的 deadline failure 还比较 runtime owner 与 started-at 快照，防止旧 worker 终止新 claim。
- translation worker runtime slot reconciliation 与 worker removal 的 production internal helpers 现在要求非可选 `&SqliteWriteCoordinator`，删除了 `Option<&SqliteWriteCoordinator>` 的 raw `BEGIN IMMEDIATE` fallback；仅 test-only convenience wrappers 创建局部 coordinator。
- reaction PAT state、dashboard daily rollup、scheduled slot patch 与 public release usage metadata refresh 均进入明确的 foreground/background writer lane，补齐 API 与高频 sync metadata 的遗漏写路径。
- `tools/rust-source-check` 为 `src/admin_runtime.rs`、`src/ai.rs`、`src/api.rs`、`src/jobs.rs`、`src/sync.rs` 与 `src/translations.rs` 增加 AST guard：生产 direct pool write、pool accessor、未协调的 pool transaction、`SqliteConnection` 别名与 raw `BEGIN IMMEDIATE` 失败，`cfg(test)`、bootstrap/read-only 范围保持显式边界；已由 coordinator 提供的通用 `Executor` helper 与 Transaction helper 保持可复用，subscription prune facade 只有在 marker、直接 callback 转发与 coordinator 调用同时通过 AST 验证时才作为结构化安全边界处理。
- 网络、GitHub API、AI 调用与长耗时处理仍留在 writer permit 外；permit 只包住 SQLite 写入段。

## Validation

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --locked --all-features`
- `bash scripts/check-rust-source-quality.sh`（含应用/源检查器 fmt、全 feature Clippy/check、checker 单测和全仓 guard scan）

## Remaining Gaps

- 待完成 PR CI / review 收敛与 merge cleanup。

## Related Changes

- `docs/solutions/backend/sqlite-wal-write-transactions.md` 更新为 writer coordinator + `BEGIN IMMEDIATE` 的复用方案。
- `src/sqlite_write.rs` 新增 WAL + 多连接 pool 并发写入与 foreground 优先级回归测试。
- `src/translations.rs` 新增 batch 启动写段在 writer 压力下串行化回归，以及结果聚合在 writer 背压下直接复用 pending 快照的回归。
- `src/sync.rs` 新增 social activity snapshot 与 feed activity event 在 competing writer 下等待并成功提交的并发回归。
- `src/api.rs` 新增 feed reaction refresh 在 SQLite writer 压力下跳过持久化但继续返回 live item 的回归。
- `src/api.rs` 新增 reaction PAT check result 在 foreground writer 压力下等待 coordinator 后提交的回归，并为 best-effort PAT state persistence 失败保留结构化 warning。
- `src/sync.rs` 新增 governance rebuild 对超过 500 个候选 repo 的 chunk stats 回归，并保留 active member reconciliation 语义回归。
- `src/sync.rs` 新增 social snapshot 在第一个 writer chunk 后释放 permit 的回归，以及 397 个 owned repo association、100000 条 search document 共存时 dashboard updates、session save、task enqueue 与 snapshot chunk 指标的生产形验证。
- `src/jobs.rs` 新增后台 writer 压力下 `enqueue_task` 等待 coordinator 而不是绕过写入背压的回归测试。
- `src/jobs.rs` 新增 daily slot dispatch、scheduled dispatch state 与 brief failure mark 在 competing writer 下等待成功提交的并发回归测试。
- `src/sync.rs` 新增 subscription event 写入在 competing writer 下等待成功提交，以及 subscription history prune 在 writer permit 不可得或 SQLite busy 时降级跳过的回归测试。
- `src/sync.rs` 新增 `starred_repos` 增量 upsert 与通知 upsert 在 competing writer 下等待成功提交的并发回归测试。
- `src/ai.rs` 新增 LLM retention cleanup 在 writer permit 不可得或 SQLite busy 时降级跳过的回归测试。
- `tools/rust-source-check/src/main.rs` 新增 coordinator bypass AST guard 及 direct write、pool accessor、`SqliteConnection` 参数/获取别名、未协调 transaction、coordinator closure、test-only、read-only marker、wrapper 与 `cfg(not(test))` 回归测试。
- `src/ai.rs` 与 `src/sync.rs` 的 stale recovery update 对 selected owner/heartbeat 做 CAS；`src/translations.rs` 的 runtime resize/remove 在 slot 持久化失败时恢复内存状态，避免恢复竞争或部分提交造成状态漂移。
- `src/ai.rs` 的 LLM owner CAS 与 recovery/finalize event 事务、`src/sync.rs` 的 repo-release terminal transition 事务，以及 admin runtime PATCH 的 serialized atomic persisted snapshot rollback，补齐“数据库状态、内存状态、watcher/governance 与事件必须一致”的失败路径。

## References

- `./SPEC.md`
- `./HISTORY.md`
