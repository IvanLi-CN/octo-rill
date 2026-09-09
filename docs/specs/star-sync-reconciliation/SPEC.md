# 星标同步分片对账与独立调度

> 当前有效规范以本文为准；实现覆盖见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## Related ADRs

- [ADR 0006: Decouple Star Synchronization from Subscription Governance](../../adr/0006-decouple-star-sync-schedules.md)

## 背景 / 问题陈述

当前的定时 Star 同步读取最新窗口并只做 upsert。取消星标的仓库若不再位于该窗口，既不会重新出现，也不会被删除，导致“全部”列表长期保留陈旧仓库。将全量 Star 一次性翻完能够修正数据，却会把所有页面请求集中到一个任务并与订阅/Release 治理共用同一节拍。

GitHub GraphQL 的 `StarredRepositoryConnection` 可返回 `totalCount`、`isOverLimit` 和 cursor `pageInfo`。cursor 只能顺序向前推进，不能安全按页码跳转，因此全量扫描必须切成连续的单页 slice，而不是并行猜测页范围。

## 目标 / 非目标

### Goals

- Star delta 与 Release subscription/governance 彻底解耦，分别配置频率。
- 以稳定、持久的单页 slice 完成每个 GitHub connection 的完整 Star 对账，均匀化 GitHub 与 SQLite 压力。
- 只有已成功完成 epoch 才删除缺失 membership，最终修正取消星标。
- 维持 `starred_repos` / `user_repo_associations` 的现有读模型契约，避免所有消费者感知 connection 级细节。
- 在管理员界面展示独立设置与对账进度/最近完成事实。

### Non-goals

- 不把 cursor 分片并行化，也不依赖 `totalCount` 推断完成。
- 不为每个用户或每个 GitHub connection 增加独立管理员配置。
- 不修改 Release governance 的预算、窗口、worker 并发或其 `sync_auto_fetch_interval_minutes` 语义。
- 不承诺 GitHub upstream 的原子快照；下一次 epoch 负责收敛扫期间的上游变动。

## 范围（Scope）

### In scope

- DB migration：Star connection membership、epoch/slice 状态、scheduler dispatch state 和两项 runtime setting。
- 后端 Star Sync Coordinator：delta、full epoch start/advance/finalize、lease recovery 和 aggregate association recompute。
- 独立 job task：`sync.starred.delta` 与 `sync.starred.reconcile`。
- scheduler：按独立频率 enqueue due connection、间隔时仅执行一个 full page slice。
- `GET/PATCH /api/admin/jobs/sync/runtime-config` 的 Star setting / progress 扩展。
- Admin Jobs 同步设置面提供独立的 Release 与 Star 配置区；任务详情不再把 Star 表现为 `sync.subscriptions` 的阶段。
- 后端及 API / scheduler / migration 回归测试；Admin Jobs Storybook 场景。

### Out of scope

- 线上立即执行 SQL 修复、部署或回填所有已有 Star。
- 修改 GitHub OAuth scope 或 token 存储方式。
- 将 Star 的完整扫描改为同步 HTTP 请求。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口 | 类型 | 变更 | 契约 | Owner | Consumers |
| --- | --- | --- | --- | --- | --- |
| `sync.starred.delta` | Job task | New | [HTTP / task](./contracts/http-apis.md) | backend | scheduler, access refresh |
| `sync.starred.reconcile` | Job task | New | [HTTP / task](./contracts/http-apis.md) | backend | scheduler, admin |
| `starred_repo_connection_memberships` / `star_sync_epochs` | DB schema | New | [DB](./contracts/db.md) | backend | coordinator |
| `GET/PATCH /api/admin/jobs/sync/runtime-config` | HTTP API | Modify | [HTTP](./contracts/http-apis.md) | backend | admin web |

## 功能与行为规格（Functional / Behavior Spec）

### 独立调度

- `star_sync_delta_interval_minutes` 默认 `30`，范围 `1-120` 分钟；它仅决定 due connection 的 delta enqueue cadence。
- `star_sync_full_sweep_interval_minutes` 默认 `1440`，范围 `60-10080` 分钟；它是期望完成整个 connection 全量 epoch 的时间预算，不是每页立即执行的间隔。
- Star scheduler 使用自己的 dispatch keys、in-flight dedupe 与失败记录，不读取或写入 `sync_auto_fetch_effective_at`，也不改变 Release governance cycle。
- 每个 tick 可为 due connection enqueue 一项 delta work，并为 active/due full epoch enqueue 至多一个 reconcile slice。一个 connection 同时至多持有一个 running Star job。

### Delta

- Delta 请求 `first=50` newest Star edges，并记录本次可见 edge 的 `starredAt` / repository identity。
- Delta 将看到的 repo 写入该 connection membership，并更新 aggregate Star association；它不得删除任何 membership。
- 新 connection、无可用 full baseline 或 delta 请求发现不支持完整计数时，仍可写入最新可见 edge，但必须把完整性事实标为未知，等待 full epoch。

### Full reconciliation epoch

- 每个 epoch 固定 `(user_id, github_connection_id, started_at, cursor, total_count_at_start)`；`total_count_at_start` 来自第一页 connection 的 `totalCount`，仅供显示和 slice 节拍计算。
- slice 请求固定 `first=100`，从 epoch cursor 顺序开始。完成一个 page 后立即持久化 cursor、processed page/item count 与 lease heartbeat；网络请求不在 SQLite writer permit 内。
- 下一页请求的平均间隔为 `full_sweep_interval / max(ceil(total_count_at_start / 100), 1)`，并向上取整到 scheduler tick 粒度。实际执行可因 worker 不可用或 backoff 更晚，不能提前并发补跑。
- completion 只以 `pageInfo.hasNextPage=false` 证明。若 GitHub 返回 `isOverLimit=true`、cursor 不推进、页请求失败或 lease 丢失，epoch 记录失败/不完整，保留 membership，不得 prune。
- full page observation 以 `last_seen_epoch_id` 及时间记录到 connection membership。终页成功时，仅删除该 epoch 开始前已存在、且本 epoch 未见、且在 epoch 开始后未被 delta 更新的 membership。
- 删除或新增 connection membership 后，按 `(user_id, repo_id)` 重算 aggregate Star source；其他 GitHub connection 仍持有该 repo 时必须继续显示。

### 读模型与任务边界

- `starred_repos` 与 `user_repo_associations.source=GitHubStar` 继续表达用户级 aggregate membership；feed、Release、repo governance 和“全部”列表不查询 epoch 表。
- `sync.subscriptions` 不再 fetch Star、不再以 Star 成功用户集筛 release/social/inbox。它以任务启动时现有有效可见仓库集执行其 Release demand 和后续 best-effort 阶段。
- `sync.access_refresh` 的 Star 子阶段执行当前用户的 delta，并为前端保留 `star_refreshed` 只表示当前 delta 已完成；它不等待完整 epoch。

### 管理面

- Runtime config API 返回并可 PATCH 两项 Star interval，返回最近成功 delta、最近成功 full sweep、active epoch items/pages、totalCount 和 completion percentage（未取得总数时为 `null`）。
- 管理界面提供与“订阅同步”平级的“用户同步”计划任务页签，只展示 `sync.starred.delta` 与 `sync.starred.reconcile` 的执行记录、活跃 epoch 和最近完成事实。设置弹窗在此页签内，沿用订阅同步的对数滑块、当前值和预设刻度选择常用频率；“定时任务”总览不重复展示用户同步任务，Release 订阅设置继续是 `sync_auto_fetch_interval_minutes` 的唯一编辑入口。
- Star runtime 状态展示活跃 epoch 的页数、已处理项目数、预计总数和最近成功的 delta/full 时间；通用任务详情保留 task payload 与结果。

## 验收标准（Acceptance Criteria）

- Given 已有 Star watermark 的用户取消了一个很早的 Star
  When 一次 delta 完成
  Then 该 repo 可仍在 aggregate list 中，delta 不得误删其他旧 Star。

- Given 同一 connection 的 full epoch 成功走到 `hasNextPage=false`
  When 某旧 membership 本 epoch 未出现，且 epoch 开始后没有 delta observation
  Then membership 与 aggregate Star source 被删除，“全部”列表不再包含该 repo。

- Given full epoch 扫描过程中 delta 重新观察到同一 repo
  When epoch finalizes
  Then final prune 不得删除该较新的 delta observation。

- Given 两个 GitHub connection 属于同一用户且均 star 同一 repo
  When 一个 connection 的 epoch 删除它的 membership
  Then aggregate Star source 仍保留，直到最后一个 connection membership 消失。

- Given `totalCount=721`
  When full epoch starts
  Then admin progress 可显示预计 8 个 page/slice；completion 仍只在 `hasNextPage=false` 后成立。

- Given `isOverLimit=true`、cursor 不推进或任一 slice 失败
  When epoch ends
  Then epoch 为 failed/incomplete，且任何本地 membership 都未因该 epoch 被删除。

- Given 管理员修改 `sync_auto_fetch_interval_minutes`
  When Release scheduler 进入下一个对齐边界
  Then Release governance 采用新窗口；Star delta/full scheduler cadence 不改变。

- Given 管理员修改任一 Star setting
  When 后续 scheduler tick 运行
  Then 仅 Star task 的 due timing 变化，已开始 epoch 的 cursor 和 seen membership 不重置。

## 验收清单（Acceptance checklist）

- 取消星标在下一次成功 complete epoch 后可收敛删除。
- Star 与 Release 的运行时频率可独立保存与验证。
- 失败、超限和 cursor 异常不会触发删除。
- 用户级现有读模型不直接依赖 connection-level reconciliation tables。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: slice spacing、cursor state machine、terminal prune predicate、aggregate recompute。
- Integration tests: connection isolation、delta-versus-epoch race、failure/no-prune、runtime config validation and scheduler due keys。
- E2E/Storybook: Admin Jobs Star setting and active epoch progress states.

## Visual Evidence

已确认的 `ui_demo` 证据覆盖用户同步页、独立设置弹窗，以及运行中的全量对账详情；桌面截图为 1440px 宽度，移动截图为 393x852 CSS px。

- 设置弹窗：[desktop](./assets/user-sync-settings-desktop.png) · [mobile](./assets/user-sync-settings-mobile.png)
- 用户同步页：[desktop](./assets/user-sync-page-desktop.png) · [mobile](./assets/user-sync-page-mobile.png)
- 全量对账详情：[desktop](./assets/user-sync-detail-desktop.png) · [mobile](./assets/user-sync-detail-mobile.png)
