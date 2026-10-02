# SQLite 单写入调度层实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Implementation: PR3.9 writer-pool 路由、事务清理与 translation deadline 修复已实现；PR3.9.1 补齐异步取消连接恢复与后台 claim 退避；PR3.9.2 session writer isolation and pressure semantics implemented; code candidate `12c8ea90` passed fresh WAL HTTP acceptance, statement-interruption recovery, quality gates and persistence checks; fresh review, CI and merge remain
- Lifecycle: active
- Catalog note: fast-track / SQLite writer coordinator

## Coverage / rollout summary

- 新增 `src/sqlite_write.rs`，提供 `SqliteWriteCoordinator`、单 writer permit、foreground/background/best-effort priority、`BEGIN IMMEDIATE` 事务入口、busy/locked 分类、bounded retry 与 tracing telemetry。
- PR3.9 为生产文件型 SQLite 配置独立单连接 writer pool，读池保留原容量；foreground/background 共用单调 deadline，best-effort 立即尝试且不排队，取得 permit 后最多使用 2500 ms 启动预算；SQLite busy timeout 为 100 ms，普通 callback 最多 4 次尝试，`BEGIN IMMEDIATE` 在同一总 deadline 内按 25/50/100 ms 退避，避免连接驱逐后的短暂文件锁提前终止恢复。
- 普通 coordinator callback 的 SQL 查询显式使用专用 writer pool；`BEGIN IMMEDIATE` 入口也会选择专用池，未配置独立池的内存数据库继续使用传入池。
- deadline 限制 writer queue、write-pool acquisition、事务启动、callback 启动和重试启动；callback 一旦启动就持有 permit 直到真实结果返回，并记录 deadline overrun。Webhook receiver 将同一个绝对阶段 deadline 传入 delivery claim、release enqueue、queued-state update 和恢复写入的 coordinator，因此队列、连接池与 `BEGIN IMMEDIATE` 到期后不会再启动新事务；queued-state 与恢复写入均通过 deadline-aware `BEGIN IMMEDIATE` transaction 执行，保留 COMMIT 派发前的 deadline 检查。短 enqueue 阶段跳过独立的批量 deadline 清扫，避免将多行清理纳入 750 ms admission budget。已启动阶段仍等待实际结果，并把 coordinator deadline 映射为 retryable 响应。SQLite progress handler 只约束事务语句与 COMMIT 派发前的阶段；COMMIT 派发前过期会在 150 ms cleanup budget 内尝试回滚，COMMIT 派发后关闭 progress handler 并等待 SQLite 的实际结果，late completion telemetry 记录完成状态与超期毫秒。退避无法在剩余 deadline 内开始时记录为 deadline，而非 retry。已知 busy 在预算外返回时按 deadline 结束且不再重试；其他提交错误保留实际错误。前台 deadline 映射为 retryable 503，后台 job 持久化延迟重试，未启动的 best-effort 写入跳过。
- 异步取消事务现在使用独立的 progress-handler interrupt 标志：正常 commit/rollback 会移除 handler，事务 future 被取消时会中断活动 SQLite 语句。专用 writer pool 的 `after_release` hook 会在 transaction depth 非零时立即返回错误并让 SQLx hard-close/evict 不确定连接；普通读池/内存 fallback 在同一个 150 ms budget 内完成 queued rollback，失败仍 hard-close/evict；depth 已清零时移除旧 handler并确认可复用。回归覆盖 101 次取消后写入、内存 fallback 取消后的 schema 保留、强制清理失败驱逐和真实长 SQL 清理超时后的连接重建。
- content processing recovery/claim、已领取任务的数据库执行、job task claim 与 repo release claim loop 共用 `worker_backoff`，连续失败的基础等待为 1/2/4/8/16/30 秒并加入不提前的抖动；content claim 成功后立即复位 claim 退避，执行阶段的数据库失败使用独立退避，其他 worker 在成功 DB 尝试后复位；每个 worker 只有一笔同类 claim 在途，日志记录 lane、错误类别、失败计数和 retry wait。
- deadline telemetry 区分 writer queue、write-pool acquisition、`BEGIN IMMEDIATE` 与 transaction 阶段，并记录 `writer_wait_ms`、`pool_wait_ms`、`begin_ms`、`transaction_ms` 和 `deadline_ms`。
- 内容提交的模型档案选择仅读取已刷新的 scheduler routing；模型目录刷新不会在持有 SQLite writer transaction 时发生。
- `AppState` 持有共享 coordinator；生产启动与测试 state 初始化均注入同一运行时组件。
- `job_tasks` enqueue/event/cancel/claim/finalize/heartbeat 已接入 writer coordinator；enqueue/event/cancel 使用 foreground lane。
- session load 使用 reader pool；create/save/delete 使用独立 writer pool 上的 direct SQL、foreground lane 与统一单调 deadline/短 busy retry；过期 session 清理使用 best-effort lane。Session middleware 对 activity-only refresh pressure 保留原业务响应并省略新 cookie，对 critical/mixed save failure 返回可识别的 retryable 503。
- repo release attach/claim/finalize/watchers/heartbeat/fail/upsert/sync-state 已接入 writer coordinator。
- social activity snapshot 与 feed activity event 持久化已接入 writer coordinator；social snapshot 先在 permit 外读取 current-member、history、stale association 与 stale repo/member 候选，再按固定 64 行 chunk 分阶段执行 `BEGIN IMMEDIATE`，chunk 之间释放 permit。current-member/history materialization、baseline、stale cleanup 与 association source 清理保持幂等和可中断恢复，并记录候选读取、writer wait、query elapsed、chunk elapsed 与 chunk count。
- translation request/batch claim/finalize/recovery/heartbeat 已接入 writer coordinator。
- translation batch 启动写段已补齐到 writer coordinator：`translation_batches` 的 `queued -> running` 与 `translation_work_items` 的 `running` 标记在单个短事务内串行提交，AI 调用继续留在 permit 外。
- translation batch finalize 遇到 deadline 时，事务会持久化重排请求与 work item，清除旧 runtime lease 并将 batch 放回 queued；现有 90 秒 reclaim 窗口避免同一轮立即重试。
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
- `src/session_store.rs` 增加协调式 session layer：读路径调用 reader-backed store，写路径在 writer transaction 内直接执行 `tower_sessions` schema 的 MessagePack SQL；每个请求在 task-local scope 内保存与 session ID 绑定的 request baseline，用 baseline diff 区分 activity-only 与 mixed critical changes，避免 writer pressure 下误发刷新 cookie。取消了跨请求共享的有限 session snapshot cache/history；existing-row save 若没有同请求 baseline，或 request baseline 存在但当前行已消失，会返回 retryable session conflict，避免 stale caller 覆盖未知字段或复活已删除 session。并发字段按 baseline diff 合并，expiry 只单调前进。
- `src/session_store.rs` 回归覆盖 activity-only refresh 保留原响应、critical retryable failure 的 `503`/`Retry-After`、expiry-only refresh 分类，以及真实文件型 WAL reader/writer contention。

## Validation

- `cargo fmt --all --check`
- `cargo build --release --locked`
- `cargo test --offline --all-targets --all-features`（code candidate `12c8ea90`: 1007 passed, 0 failed, 2 ignored）
- `cargo check --offline --all-targets --all-features`
- `cargo clippy --offline --all-targets --all-features -- -D warnings`
- `bash scripts/check-rust-source-quality.sh`（code candidate `12c8ea90`: source quality scan passed）
- `tmp/sqlite_acceptance.py` file-backed WAL HTTP acceptance（code candidate `12c8ea90`: 21,600 records；baseline/competition/recovery/sustained requests all succeeded；external lock 下 60 个 foreground task enqueue/cancel 按合同返回 `503` + `Retry-After: 1`，无 unexpected failure；task persistence/idempotence、background recovery、WAL integrity 与 post-run counts 通过）
- 真实 `tower_sessions` statement-interruption probe（code candidate `12c8ea90`: retryable `503` with `Retry-After: 1`, interrupted session unchanged, connection eviction observed, trigger removed, then `/api/me` `200` after recovery）

## Remaining Gaps

- 待完成当前候选的 fresh review、PR CI 收敛及 merge 后 target CI 验证。

## Related Changes

- `docs/solutions/backend/sqlite-wal-write-transactions.md` 更新为 writer coordinator + `BEGIN IMMEDIATE` 的复用方案。
- `src/sqlite_write.rs` 覆盖 WAL + 多连接并发写入、foreground 优先级、独立 writer pool callback、内存库 fallback、callback deadline overrun 与 permit 保持、调用方绝对 deadline 限制 writer 队列、late `BEGIN IMMEDIATE` busy 分类、COMMIT 跨过 deadline 后的真实成功/非 busy 错误、提交前过期回滚与 writer 连接复用、busy 重试耗尽，以及队列、连接池、重试和提交阶段的 busy/deadline telemetry 分类；`src/webhook_push.rs` 覆盖已启动 receiver 阶段在 deadline 后等待实际结果。
- `src/sqlite_write.rs` 另外覆盖活动长 SQL 被取消后的 101 轮连续写入、清理失败驱逐、清理超时驱逐与重建；`src/worker_backoff.rs` 覆盖 1/2/4/8/16/30 秒基础等待、抖动不提前和成功复位。
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
