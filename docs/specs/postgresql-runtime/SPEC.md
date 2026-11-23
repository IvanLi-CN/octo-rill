# PostgreSQL 运行时持久化

> 本文件是该主题的持久需求契约。当前实现事实在 `IMPLEMENTATION.md`；生命周期与变更引用在 `HISTORY.md`。

## Context and Scope

- Context: OctoRill 的运行时持久化从 SQLite 单写入调度层切换为 PostgreSQL 18，并保留 SQLite 快照作为一次性迁移输入与回滚产物。
- In scope: 运行时权威存储、schema 所有权、任务与调度语义、单仓 Release 刷新语义、全局内容处理读投影、切换与回滚契约。
- Out of scope: 旧 SQLite 模块的删除进度、生产凭据与 OAuth 端到端验收、发布流水线本身。

## Terms and Interfaces

- `runtime authority`: 唯一接受运行时写入的持久化存储。
- `import`: `octo-rill-migrate` 把 SQLite 快照写入新建 PostgreSQL 库的操作，含 `preflight`/`import`/`verify`/`rebuild`。
- `release revision repair`: `octo-rill-migrate repair-release-revisions` 从 GitHub 重读缓存 Release，并以强制覆盖写回内容与 `updated_at`；用于修复被非内容写入推进的源修订号。
- `work item` / `task`: 分别为 legacy 刷新队列行与 PostgreSQL `job_tasks` 行；运行时只认后者。
- Interface: `octo-rill-migrate` 子命令、`DATABASE_URL`、`OCTORILL_PG_WORKER_ENABLED`、`OCTORILL_PG_SCHEDULER_ENABLED`、`admin_runtime_settings.repo_release_worker_concurrency`。

## Requirements

### REQ-PGR-001

- 系统 MUST 只把 PostgreSQL 当作运行时权威存储；`DATABASE_URL` 指向 SQLite 时启动失败关闭。
- Inputs: 部署配置。
- Outputs: 进程启动结果与明确错误。

### REQ-PGR-002

- 导入 MUST 只写新建库，按表校验行数与确定性摘要；源侧删除或同表内容变化 MUST 失败关闭并提示导入新库；中断重入使用同一目标库补齐。
- Inputs: SQLite 快照路径、目标库、manifest。
- Outputs: 导入/校验退出码与 manifest。

### REQ-PGR-003

- PostgreSQL schema MUST 由 `migrations_pg/` 独立维护；已发布迁移不可修改；导入库与空库 MUST 启动到同一可运行 schema，SQLite 类型差异通过 forward migration 或读路径归一。
- Inputs: 已发布迁移集合。
- Outputs: 两库一致的列类型与可启动服务。

### REQ-PGR-004

- 任务执行 MUST 使用 durable 认领、租约心跳、事务重试与按 payload 作用域单飞；七个调度通道 MUST 幂等入队。
- Inputs: `job_tasks` 队列与通道触发时刻。
- Outputs: `queued → running → succeeded/failed` 流转与调度游标。

### REQ-PGR-005

- 单仓 Release 刷新 MUST 共用一套判定：候选用户轮换仅在凭据/权限类失败时发生，公共仓库可在凭据耗尽后匿名读取，并发上限取自 `repo_release_worker_concurrency`；账号级任务 MUST NOT 因单仓不可读而整体失败。
- 每个分页请求 MUST 至多发送两次（初次 + 一次重试），仅在 transport `network_error`、`timeout`、HTTP 5xx 与成功状态下的非 JSON 响应体可重试；退避 MUST 为 300 ms。`repo_inaccessible`、`credentials_forbidden`、401/403/404/451、限流与 `deferred_budget` MUST NOT 重试。重试 MUST 重发同一页，MUST NOT 重复计页或追加部分解码结果。
- 账号级 `sync.releases` 的 `result_json` MUST 包含 `failed_count`、`deferred_count`、`failure_ratio`（0–1，两位小数）、`failure_reasons` 与 `degraded`；部分仓库失败时任务保持 `succeeded` 并标记 `degraded=true`，所有实际尝试均失败时任务 MUST 记为 `failed`。每个任务 MUST 只输出一条聚合告警，MUST NOT 逐仓库告警。
- Inputs: 仓库、请求用户、连接凭据、usage 登记。
- Outputs: Release 快照、`via` 记录（轮换到的账号身份只进服务端日志）、usage 状态、失败分类与账号级聚合摘要。

### REQ-PGR-006

- 全局内容处理读投影 MUST 使用与写入一致的身份重建；MUST NOT 为 legacy 不存在的宽松形状增加读取分支。
- Inputs: `content_result_projections`、`content_work_identities`、`html_url`。
- Outputs: 与写入一致的内容结果读取。

### REQ-PGR-007

- 切换 MUST 冻结写入、取快照、导入新库、定向校验、冒烟后判定；回滚只对冻结前数据成立，切换后写入在回滚时丢弃；回滚产物 MUST 在冻结时刻复验索引与校验和。
- Inputs: 冻结时刻、快照、目标库。
- Outputs: 成功判定或回滚动作及其公告内容。

### REQ-PGR-008

- 未认证 Release 读取 MUST 共用一份进程级预算，初始余量低于 GitHub 的 60 次/小时 IP 限额，并按 `X-RateLimit-Remaining`/`X-RateLimit-Reset` 校准；每个真实匿名请求（含 304、重定向跳转）MUST 先扣减，预算为 0 时 MUST NOT 发请求。
- 预算耗尽 MUST 返回独立延后结果：MUST NOT 记为失败、MUST NOT 写重试退避、MUST NOT 改写 `public_repo_release_usage` 状态；单仓任务与账号级任务都只延后该仓库并留待下一周期。候选轮换判定 MUST NOT 受预算影响。
- Inputs: 匿名响应头、请求时刻、进程内共享预算。
- Outputs: 允许或延后决定、`deferred_budget` 结果、预算余量与延后次数指标。

### REQ-PGR-009

- 账号级与单仓 Release 刷新 MUST 只为本次真正插入 `public.repo_releases` 的 Release 排队 `translation.generate` 工作；未配置 AI provider 时 MUST NOT 写入任何翻译行或任务。
- 自动排队 MUST 复用请求/工作项/任务三段式，并按 `(scope_user_id, kind, variant, entity_id, target_lang, source_hash)` 去重：结果已为 `ready` 的工作项 MUST NOT 再排队；同一工作项已有 `queued`/`running` 翻译任务时 MUST NOT 重复排队；无在途任务的终态工作项按授权重试语义重新入队。
- 单次排队调用 MUST 最多为 25 个 Release 创建任务；其余可翻译候选 MUST 持久化为 `queued` 工作项，由后续周期排空，MUST NOT 因上限丢弃。排空同样每次最多 25 个，MUST 跳过已有 `queued`/`running` 任务的工作项，MUST NOT 重排 `failed` 或已完成的终态工作项。
- 单个 Release 入队失败、积压写入失败或排空失败 MUST 只记录告警，MUST NOT 中断其余 Release 或刷新任务。两个执行器的 `result_json` MUST 报告 `translations_enqueued`（含同一次刷新排空的任务）。
- 可恢复失败 MUST 由后续周期自动恢复：只有最后一次 `attempt_completed` 事件标记 `retry_eligible`、`next_retry_at` 已到期、且没有在途任务的工作项才会重排；退避按已尝试次数增长（第 1 次失败后 10 分钟，第 2 次后 1 小时），总尝试次数 MUST 不超过 3。不可恢复失败（`configuration`）、`missing` 与到达上限的项 MUST NOT 自动重试。
- 每轮排空 MUST 先处理到期的自动重试，再用剩余额度处理积压，单轮总数不超过 25；重排 MUST 复用授权重试的重置语义。
- Inputs: 刷新插入的 Release ID、请求用户、AI provider 配置。
- Outputs: `translation_work_items`/`translation_requests`/`job_tasks` 行与 `translations_enqueued` 计数。

## Verification

### VER-PGR-001

- Method: 以 SQLite URL 启动二进制。
- covers: `REQ-PGR-001`
- Pass condition: 进程以明确错误退出，不进入服务监听。

### VER-PGR-002

- Method: `scripts/pg_seed_import_test.py` 与切换演练（源侧删除、同表内容变化、中断重入）。
- covers: `REQ-PGR-002`
- Pass condition: 正常导入通过；删除与内容变化失败关闭；重入补齐成功。

### VER-PGR-003

- Method: `scripts/pg_seed_fresh_schema_test.py`、导入库与空库启动、列类型审计。
- covers: `REQ-PGR-003`
- Pass condition: 两库均可启动，抽查列类型一致，`_sqlx_migrations` 不出现在导入库。

### VER-PGR-004

- Method: `scripts/pg_seed_worker_test.py`、`scripts/pg_seed_scheduler_test.py`。
- covers: `REQ-PGR-004`
- Pass condition: 认领/终结/租约恢复通过；重复触发不产生重复 queued 行。

### VER-PGR-005

- Method: `scripts/pg_seed_repo_release_fallback_test.py` 与 `cargo test --locked --bin octo-rill postgres_repo_release`。
- covers: `REQ-PGR-005`
- Pass condition: 候选轮换、公共 fallback、失败分类、usage 状态、账号级韧性、在途并发上界、瞬时失败单次重试与页面不重复计数、`degraded`／全失败判定全部通过。

### VER-PGR-006

- Method: `scripts/pg_content_parity.py` 对生产副本抽样。
- covers: `REQ-PGR-006`
- Pass condition: 抽样投影全部 `ready`，无因身份不一致导致的缺失。

### VER-PGR-007

- Method: `仓库外运维记录` 演练记录与生产副本读写探测。
- covers: `REQ-PGR-007`
- Pass condition: 可逆写全部还原，读探测除环境限制项外通过，回滚契约与公告文本已确认。

### VER-PGR-008

- Method: `cargo test --locked --bin octo-rill postgres_repo_release` 与 `scripts/pg_seed_repo_release_anon_budget_test.py`。
- covers: `REQ-PGR-008`
- Pass condition: 多页匿名读取按页扣减、响应头校准剩余额度、预算耗尽时延后且 usage 状态与退避不变、零预算不再发请求全部通过。

### VER-PGR-009

- Method: `cargo test --locked --bin octo-rill postgres_translate`、`TEST_PG_TRANSLATE_URL=… cargo test --locked --bin octo-rill postgres_backlog -- --ignored`、`cargo test --locked --bin octo-rill postgres_tasks` 与 `cargo test --locked --bin octo-rill postgres_repo_release`。
- covers: `REQ-PGR-009`
- Pass condition: 批次上限与去重、超限候选写入积压、积压跨周期有界排空且幂等、可恢复失败按退避与次数上限自动重试、未到期与不可恢复失败不重试、空内容跳过、`ready` 与在途任务不重复排队、无 AI provider 时零写入、两执行器 `translations_enqueued` 计数全部通过。

## Related ADRs

- [ADR 0019: Validated SQLx Migrations Apply Before Startup](../../adr/0019-sqlx-startup-migration-compatibility.md)
- [ADR 0023: PostgreSQL 作为运行时持久化权威](../../adr/0023-postgresql-runtime-authority.md)

## Visual Evidence

- None

## References

- `./IMPLEMENTATION.md`
- `./HISTORY.md`
