# SQLite 单写入调度层实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- PR #340 executable candidate `97843ed6` applies one shared Social failure classifier to subscription, access-refresh, and `sync.all` paths; retryable completed source degradation now remains `social_source_degraded` warning/recoverable in access-refresh progress/log payloads, while non-retryable upstream errors remain `social_sync_failed` error/non-recoverable, with a production-path regression.
- PR #340 executable candidate `b0c272cf644cba486aa83cba77e6bbd13bfbd945` carries retryability through Social source-error aggregation so terminal credentials/forbidden/inaccessible-repo errors remain `social_sync_failed` error/non-recoverable; it also makes dropped `LeaseHeartbeat` handles abort their active task and adds a focused lifecycle regression.
- PR #340 executable candidate `d1f4ae30d649c8b34831df808559b6ca3373f968` preserves the completed `sync.all` Social source-degradation contract: the task remains failed with partial data, while `task.completed` carries `social_source_degraded` warning/recoverable retry metadata; fatal upstream exceptions remain `social_sync_failed` error/non-recoverable.
- PR #340 executable candidate `dc1056f800a46cdd7faa16db69346770c91f3a72` makes owned task finalization and rescheduling atomically clear any webhook operation lease bound to the task, so a release write denied by SQLite backpressure cannot leave a terminal or queued task blocking the same user; regressions cover both transitions.
- PR #340 executable candidate `e103af0c9dee6f13b168b0a89c141777f7307089` separates completed-but-partially-degraded Social source errors from fatal Social failures: the former retain `social_source_degraded` warning/recoverable compatibility events, while the latter retain `social_sync_failed` error/non-recoverable classification.
- PR #340 executable candidate `ae4e522bc4eeada802f2d9c951b9c6fc23830da7` adds a production-path webhook operation-lease contention regression for owner-CAS retry payload advancement and budget exhaustion, while retaining the candidate's heartbeat cleanup and access-refresh Social run-log ordering fixes.
- PR #340 executable candidate `ce27dba716ab85971a014596c2050b73c8e887f9` adds a direct regression proving recoverable Social run-log fallback leaves `critical_events` unchanged at zero; genuine `severity=error` upstream events retain their critical count.
- 2026-10-23 candidate: background admission denial now uses the existing coordinator deadline and 25/50/100ms capped backoff without increasing the one-waiter queue; Social SQLite exhaustion is classified as recoverable warning metadata while preserving failed-user accounting.
- PR #340 executable candidate `08429680b8f8570a94f9774911ae41ae5042db7e` bounds webhook user-operation lease admission to a 30-second wait budget, then returns through the existing owner-CAS reschedule/backoff path instead of holding a task lease indefinitely.
- PR #340 executable candidate `5ced09070ddc126c8c706fa58b0420476ea5ee91` extends stable Social source-error de-duplication through typed persistence failures, so Admin Jobs and compatibility failure payloads do not repeat diagnostics collected by multiple fetch phases.
- Subscription key events write the run log before the event table; retryable SQLite append pressure is retained in that durable log fallback and does not abort a Social worker or the remaining subscription phases.
- Social write failures retain typed upstream/source diagnostics when SQLite backpressure is layered on top; completed-but-partially-degraded retryable source results use warning/recoverable compatibility metadata, while non-retryable upstream exceptions remain non-recoverable.
- Access-refresh regression coverage verifies the Social source-error result is retained while the Notifications phase continues.
- Access-refresh progress/log payloads carry the shared typed Social failure classification; SQLite writer pressure and retryable completed source degradation are warning/recoverable with retry metadata, while non-retryable upstream source errors remain error/non-recoverable. Legacy `sync.all` retains completed Notifications data but finalizes a Social business failure as a failed task with the full `error_kind`/`severity`/`recoverable`/`retryable`/`retry_after` classification in `task.completed`.
- Social persistence errors now carry the partial snapshot through the typed error context; `sync.subscriptions`, access-refresh, and `sync.all` retain completed counters and failed/partial repos instead of replacing them with an empty result. `sync.all` finalization commits the failed task row and `task.completed` event atomically on the foreground lane, and stops the heartbeat before propagating any finalization error.
- Feed activity event persistence uses the same typed committed-event context across short batches; if a later batch reaches writer pressure, Social failure summaries include events already committed by earlier batches instead of reporting zero.
- Social snapshot failures retain the number of event rows committed by earlier 64-row chunks, including when a later chunk exhausts SQLite backpressure; the compatibility result therefore reflects persisted events instead of under-reporting them as zero.
- Social snapshot result assembly and typed persistence-failure extraction de-duplicate source errors that arrive from multiple fetch phases, so shared diagnostics remain visible once in compatibility results and Admin Jobs payloads.
- `sync.subscriptions` now carries failed Social users as explicit failed outcomes with their partial counters, so progress and final summaries count persisted `repo_stars`/`followers`/`events` without counting the user as succeeded.
- Background admission and `BEGIN IMMEDIATE` busy retries cap every sleep at the original absolute deadline, so a retry cannot extend heartbeat, worker, or finalization latency beyond its caller budget.
- Active job requeue/deferral paths for cancellation, webhook retry, and SQLite writer backpressure now use the claimed runtime owner as a CAS predicate; a late worker cannot requeue a task that lease recovery has already assigned to another runtime.
- Owned task finalization and rescheduling now commit the task transition together with `DELETE FROM webhook_push_user_operation_leases WHERE task_id = ?`; this closes the release-failure path where a terminal or queued webhook task could otherwise retain an operation lease until expiry.
- The access-refresh dependency seam is private test wiring grouped in a dependency struct; the production path keeps the real Social/Notifications calls without a lint suppression.
- The file-backed scheduler/session pressure regression keeps its empty-queue worker join bounded with a 30-second test budget for the Agent VM acceptance; production admission and writer deadlines remain unchanged.
- Implementation: PR3.9 writer-pool 路由、事务清理与 translation deadline 修复已实现；PR3.9.1 补齐异步取消连接恢复与后台 claim 退避；PR3.9.2 session writer isolation and pressure semantics implemented；PR3.9.3 补齐共享 background admission/cadence、HTTP request-to-writer correlation 与 file-backed scheduler/session regression；当前直接修复候选还收敛 scheduler atomic dispatch、search projection outbox lease safety、FTS duplicate-write removal、公告多用户 lane rebuild 与 resource-key query plan。
- Lifecycle: active
- Catalog note: fast-track / SQLite writer coordinator

## Current performance follow-up

- 本修复候选将调度 active-task 检查改为命中 `idx_job_tasks_status_created_at` 的短路 `EXISTS ... LIMIT 1`，避免在 writer 事务内统计历史 `job_tasks`；subscription、recent-failures、star reconciliation 与 daily brief 调度的查询错误路径会在 writer 释放前显式等待 rollback，保留任务、事件和 dispatch marker 的原子提交语义。回滚成功后连接可继续复用，回滚失败才进入现有不确定连接驱逐兜底。
- 本轮修复候选收敛调度原子性与搜索投影解耦：任务、事件和 dispatch marker 共用同一 writer 事务，搜索核心行先提交、FTS 通过 durable outbox 异步重试；Dashboard signature 查询合同保持不变。
- worker 处理并发不等同于 SQLite writer 并发；网络、GitHub 和 LLM 工作继续在 writer permit 外执行，只有 claim 的短写入段受 admission 约束。
- 搜索 projection 的 `failed` 状态继续保持显式恢复语义；只有 writer 压力稳定并完成线上验收后，才执行既有管理员 resume 操作。
- 当前跟进候选为后台准入拒绝增加 250 ms 共享冷却与 `background_admission_coalesced` 计数，并将通知 upsert 与 feed activity event 写入拆成固定 10 行的幂等短事务；这只降低无效拒绝日志和单次写入持锁时间，尚未代表已发布或完成 101 线上验收。
- 当前 content recovery hotfix candidate `74c34a00` 将 supersession reconciliation 拆成 reader 分页与有界 writer apply：候选每批最多 100 项，单个资源历史每页最多 256 行，页间释放 reader 连接并让出执行权；共享 RAII in-flight guard 合并 recovery 与 runtime-reload 的重叠扫描，游标只在 apply 成功后推进。
- migrations `0092_content_work_supersession_lookup_index.sql`、`0093_search_projection_outbox.sql`、`0094_search_projection_resource_key_index.sql`、`0095_search_projection_announcement_key_repair.sql`、`0096_search_projection_outbox_poll_index.sql`、`0097_search_projection_failure_recovery.sql`、`0098_search_projection_scheduler_recovery.sql` 和 `0099_search_projection_scheduler_index_guard.sql` 均由 SQLx 在 listener bind 前事务性应用，旧程序不支持直接回滚，故发布故障采用 forward repair 或独立 backup restore。

## Coverage / rollout summary

- 新增 `src/sqlite_write.rs`，提供 `SqliteWriteCoordinator`、单 writer permit、foreground/background/best-effort priority、`BEGIN IMMEDIATE` 事务入口、busy/locked 分类、bounded retry 与 tracing telemetry。
- PR3.9 为生产文件型 SQLite 配置独立单连接 writer pool，读池保留原容量；foreground/background 共用单调 deadline，best-effort 立即尝试且不排队，取得 permit 后最多使用 2500 ms 启动预算；SQLite busy timeout 为 100 ms，普通 callback 最多 4 次尝试，`BEGIN IMMEDIATE` 在同一总 deadline 内按 25/50/100 ms 退避，避免连接驱逐后的短暂文件锁提前终止恢复。
- 普通 coordinator callback 的 SQL 查询显式使用专用 writer pool；`BEGIN IMMEDIATE` 入口也会选择专用池，未配置独立池的内存数据库继续使用传入池。
- deadline 限制 writer queue、write-pool acquisition、事务启动、callback 启动和重试启动；callback 一旦启动就持有 permit 直到真实结果返回，并记录 deadline overrun。Webhook receiver 将同一个绝对阶段 deadline 传入 delivery claim、release enqueue、queued-state update 和恢复写入的 coordinator，因此队列、连接池与 `BEGIN IMMEDIATE` 到期后不会再启动新事务；queued-state 与恢复写入均通过 deadline-aware `BEGIN IMMEDIATE` transaction 执行，保留 COMMIT 派发前的 deadline 检查。短 enqueue 阶段跳过独立的批量 deadline 清扫，避免将多行清理纳入 750 ms admission budget。已启动阶段仍等待实际结果，并把 coordinator deadline 映射为 retryable 响应。SQLite progress handler 只约束事务语句与 COMMIT 派发前的阶段；COMMIT 派发前过期会在 150 ms cleanup budget 内尝试回滚，COMMIT 派发后关闭 progress handler 并等待 SQLite 的实际结果，late completion telemetry 记录完成状态与超期毫秒。退避无法在剩余 deadline 内开始时记录为 deadline，而非 retry。已知 busy 在预算外返回时按 deadline 结束且不再重试；其他提交错误保留实际错误。前台 deadline 映射为 retryable 503，后台 job 持久化延迟重试，未启动的 best-effort 写入跳过。
- 异步取消事务现在使用独立的 progress-handler interrupt 标志：正常 commit/rollback 会移除 handler，事务 future 被取消时会中断活动 SQLite 语句。专用 writer pool 的 `after_release` hook 会在 transaction depth 非零时立即返回错误并让 SQLx hard-close/evict 不确定连接；普通读池/内存 fallback 在同一个 150 ms budget 内完成 queued rollback，失败仍 hard-close/evict；depth 已清零时移除旧 handler并确认可复用。回归覆盖 101 次取消后写入、内存 fallback 取消后的 schema 保留、强制清理失败驱逐和真实长 SQL 清理超时后的连接重建。
- content processing recovery/claim、已领取任务的数据库执行、job task claim 与 repo release claim loop 共用 `worker_backoff`，连续失败的基础等待为 1/2/4/8/16/30 秒并加入不提前的抖动；content claim 成功后立即复位 claim 退避，执行阶段的数据库失败使用独立退避，其他 worker 在成功 DB 尝试后复位；每个 worker 只有一笔同类 claim 在途，日志记录 lane、错误类别、失败计数和 retry wait。
- deadline telemetry 区分 writer queue、write-pool acquisition、`BEGIN IMMEDIATE` 与 transaction 阶段，并记录 `writer_wait_ms`、`pool_wait_ms`、`begin_ms`、`transaction_ms` 和 `deadline_ms`。
- PR3.9.3 为 background writer 增加共享有界等待预算（最多一个 waiting background，foreground waiter 出现后拒绝新的 background admission），并暴露 waiting/admitted/rejected/coalesced runtime counters；content processing recovery 通过 coordinator 的 1 秒 per-lane cadence admission 合并重复 worker pass，admission denial 进入已有 worker backoff。
- 本轮 parent 的 writer child `#328`（`fd2bb6935516cc51f239b8edafdcd57b6933a888`）保持 worker 业务并发，只限制每个 claim lane 的数据库领取准入；dashboard child `#327`（`d36d34b01a8fae78b706d5a3903e50874795dd14`）将 `/api/dashboard/updates` 改为有界 signature 查询。
- repair child `#329`（`54e00b23d86f273450cd9555f855b4a1266bcec4`）释放 content claim 准备阶段的 mode 读锁，并在短 claim transaction 内重新检查持久化 mode；这避免后台 claim 等待 runtime settings 时阻塞 foreground `submit_item`，同时保留 rollback freeze 语义。
- HTTP access middleware 现在为正常成功请求也写入 `http.access`，并在 task-local request context 中保留 request ID、method、route；SQLite transaction-end、session write 与 background admission telemetry 复用这些字段完成 request-to-writer correlation。
- 内容提交的模型档案选择仅读取已刷新的 scheduler routing；模型目录刷新不会在持有 SQLite writer transaction 时发生。
- `AppState` 持有共享 coordinator；生产启动与测试 state 初始化均注入同一运行时组件。
- `job_tasks` enqueue/event/cancel/claim/finalize/heartbeat 已接入 writer coordinator；enqueue/event/cancel/finalize 使用 foreground lane，claim/heartbeat 继续使用 background admission retry。
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
- scheduler 的 active-task coalescing 会把 queued/running 行（包括 heartbeat 已过期但尚未被 recovery worker 以 lease CAS 接管的行）继续视为 active；只有 recovery 完成终态转换后才允许重新 dispatch，避免 scheduler 与 recovery 竞态造成重复外部工作。
- admin runtime settings 的 seed/backfill/update、LLM recovery flags 与 model health、translation runtime settings 均通过共享 `AppState.sqlite_writer` 写入；生产启动和 runtime heartbeat 不再调用 raw pool writer。
- admin LLM/translation runtime PATCH 由共享 runtime-settings lock 串行化；在 live scheduler 应用失败时，LLM、recovery flags 与 translation worker settings 通过同一个 coordinator transaction 恢复此前持久化快照，再 reconcile 内存 scheduler。scheduler 同步会在修改内存前读取新的 model health，并在 translation slot 持久化失败时恢复 LLM routing/concurrency/health。
- repo release work item 的成功、失败、deadline recovery 与 stale recovery 会在一个协调器事务中同时提交 work item 终态、pending watcher 状态和 governance attempt；执行 worker 的 deadline failure 还比较 runtime owner 与 started-at 快照，防止旧 worker 终止新 claim。
- translation worker runtime slot reconciliation 与 worker removal 的 production internal helpers 现在要求非可选 `&SqliteWriteCoordinator`，删除了 `Option<&SqliteWriteCoordinator>` 的 raw `BEGIN IMMEDIATE` fallback；仅 test-only convenience wrappers 创建局部 coordinator。
- reaction PAT state、dashboard daily rollup、scheduled slot patch 与 public release usage metadata refresh 均进入明确的 foreground/background writer lane，补齐 API 与高频 sync metadata 的遗漏写路径。
- `tools/rust-source-check` 为 `src/admin_runtime.rs`、`src/ai.rs`、`src/api.rs`、`src/jobs.rs`、`src/sync.rs` 与 `src/translations.rs` 增加 AST guard：生产 direct pool write、pool accessor、未协调的 pool transaction、`SqliteConnection` 别名与 raw `BEGIN IMMEDIATE` 失败，`cfg(test)`、bootstrap/read-only 范围保持显式边界；已由 coordinator 提供的通用 `Executor` helper 与 Transaction helper 保持可复用，subscription prune facade 只有在 marker、直接 callback 转发与 coordinator 调用同时通过 AST 验证时才作为结构化安全边界处理。
- 网络、GitHub API、AI 调用与长耗时处理仍留在 writer permit 外；permit 只包住 SQLite 写入段。
- `src/session_store.rs` 增加协调式 session layer：读路径调用 reader-backed store，写路径在 writer transaction 内直接执行 `tower_sessions` schema 的 MessagePack SQL；每个请求在 task-local scope 内保存与 session ID 绑定的 request baseline，用 baseline diff 区分 activity-only 与 mixed critical changes，避免 writer pressure 下误发刷新 cookie。取消了跨请求共享的有限 session snapshot cache/history；existing-row save 若没有同请求 baseline，或 request baseline 存在但当前行已消失，会返回 retryable session conflict，避免 stale caller 覆盖未知字段或复活已删除 session。并发字段按 baseline diff 合并，expiry 只单调前进。
- `src/session_store.rs` 回归覆盖 activity-only refresh 保留原响应、critical retryable failure 的 `503`/`Retry-After`、expiry-only refresh 分类，以及真实文件型 WAL reader/writer contention。
- `src/sqlite_write.rs` 新增 background admission bounded-queue 回归；`src/content_processing.rs` 新增跨 worker recovery coalescing 与文件型 WAL 下 scheduler pressure/session foreground write 生产形回归，覆盖 waiter cleanup、foreground success、recovery amplification 与后续 writer reuse。
- `src/server.rs` 更新正常成功请求的 access-log 回归，锁定 request ID、route、status、latency/slow 字段和敏感 query/cookie 不泄漏。

## Validation

- `cargo fmt --all --check`
- `cargo build --release --locked`（远端 CI 的 Build (Release) required check 通过；本机 macOS target 的 stale dylib linker artifact 使本地 release build 不可作为证据）
- `cargo test --locked --all-targets --all-features -- --test-threads=1`（当前候选串行套件 1133 passed, 0 failed, 3 ignored；PR #340 required check 以最新 current head 为准）
- `cargo check --offline --all-targets --all-features`
- `cargo clippy --offline --all-targets --all-features -- -D warnings`
- `bash scripts/check-rust-source-quality.sh`（PR #340 current-head required check 通过）
- 当前调度查询/回滚候选的 `jobs::tests::` 67 项全部通过；新增回归锁定 status-first query plan，以及显式 scheduler rollback 后 writer connection 可复用。`cargo check --offline --all-targets --all-features`、`cargo clippy --offline --all-targets --all-features -- -D warnings` 与 source-quality gate 均在候选 head 通过。
- PR #340 executable candidate `6431ee1fff24f3eef977cd0cb7a1f24f41259954` retains the unchanged scheduler/session code path from implementation-evidence commit `f0e7f3467975b0db225443b0db7c54da7b94c3f9`. The file-backed WAL acceptance ran directly in Agent VM (Rust 1.91, 8 vCPU, 12 GiB) once with `1 passed, 0 failed` and a 15.57 s test body; an immediate same-VM repeat returned `503` during foreground session writes after 17.16 s. The observation remains explicit; the executable candidate's GitHub Backend Tests passed, four parallel runs of the exact local binary passed, and the Tier 3 test-platform review accepted the single repeat as VM scheduling variance while retaining it as a limitation. Both direct VM runs covered foreground session writes, background claim coalescing, busy telemetry, and writer cleanup. The later executable candidate `5ced09070ddc126c8c706fa58b0420476ea5ee91` adds typed persistence-failure source-error de-duplication without changing this scheduler/session code path.
- file-backed WAL acceptance：实现候选 `014b166cb237242caac012f0aafdad391337e64f` 在 Agent VM（Rust 1.91、8 vCPU、12 GiB）连续两次通过 `content_processing::tests::file_backed_scheduler_pressure_preserves_foreground_session_writes`（均为 1 passed, 0 failed；测试本体分别耗时 2.31 s 与 4.59 s）；foreground session writes、background claim coalescing、busy telemetry 和 writer cleanup 均有断言。这是历史实现候选的继承证据，不代表后续修复提交的 exact head 已通过 A6。
- 真实 `tower_sessions` statement-interruption probe（existing focused regression：retryable `503` with `Retry-After: 1`, interrupted session unchanged, connection eviction observed, trigger removed, then `/api/me` `200` after recovery）
- 此前候选覆盖 0097/0098/0099 终态投影失败隔离与延迟恢复、内容资格转换、announcement lane 分批、scheduler active lookup 防坏 JSON、failed-outbox recovery 轮询索引和 outbox lease 竞争；本地门禁、定向 search/content-processing 回归与完整串行套件均以最终 head 的本地日志和 CI 为准。
- 当前修复候选的生产形状并发回归：397 个 owned repos、至少 100,000 条 search documents 下，dashboard updates、session save、task enqueue 在后台 social snapshot 压力下均成功，`busy_or_500=0`；完整候选 head 的数值证据以 PR #340 当前 CI 与本地 acceptance 输出为准。
- 当前代码候选的回归覆盖 supersession 分页/中间页最高 source revision、共享 in-flight coalescing、candidate/history keyset query plan、migration index upgrade、content-processing、sqlite-writer、scheduler dispatch、realtime search outbox、content-first deferred projection、outbox polling query plan 和 realtime backlog 的 FTS fallback guard；这些证据尚不替代合并后的 101 线上观察。
- PR #340 的 required checks、正式 review 和生产形态验收必须绑定最终 current head；任何文档或审查元数据变更后都要重新读取最新 head 的 required checks。

## Remaining Gaps

- 当前候选尚未合入 `main`，因此没有新的发布版本或线上 rollout 事实；发布版本和 101 线上观察必须绑定合并后的 image/merge SHA。
- `0092`、`0093`、`0094`、`0095`、`0096`、`0097`、`0098` 和 `0099` 应绑定发布候选的 image/merge SHA 观察；默认 INFO 不显示 supersession page counters，线上验收需临时启用 `octo_rill::content_processing=debug`、`octo_rill::search_index=debug` 与 `octo_rill::sqlite_write=debug`，并记录 scheduler coalesced/committed/rolled-back、outbox backlog/attempt/lag、terminal projection failures/recovery、announcement fanout cursor、writer wait/deadline、HTTP p95/p99 与 CPU 五分钟中位数。
- file-backed WAL acceptance 的 PR #340 executable candidate `6431ee1fff24f3eef977cd0cb7a1f24f41259954` 已有一次直接通过，重复运行出现一次 foreground session `503`；该候选的 GitHub Backend Tests 已通过，且 Tier 3 test-platform review 已接受该单次重复为 Agent VM 调度方差，同时保留限制记录。后续 executable candidate `5ced09070ddc126c8c706fa58b0420476ea5ee91` 只补充 typed persistence-failure source-error 去重，不改变该 scheduler/session 代码路径。此前 Agent VM 的 readiness 超时尝试仅作为环境诊断记录保留。HTTP 经验验收与合并后的 101 线上 30 分钟观察仍是发布后门禁，在后者完成前不能宣称线上 rollout 已验证。

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
