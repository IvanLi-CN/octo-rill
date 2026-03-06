# 全用户 Star / Release 半小时定时同步闭环（#s8qkn）

## 状态

- Status: 待实现
- Created: 2026-03-06
- Last: 2026-03-06

## 背景 / 问题陈述

当前项目仅支持用户手动触发 Star / Release 同步，缺少面向全体启用用户的周期性订阅同步能力，也缺少对应的运行日志、关键事件审计与后台可观测入口。

## 目标 / 非目标

### Goals

- 新增系统级定时任务 `sync.subscriptions`，每 30 分钟自动执行一次全用户订阅同步。
- 同步流程固定为两阶段：先按用户活跃度刷新 Star 快照，再按聚合仓库抓取 Release 并 fan-out 回写到关联用户。
- 两阶段均采用 5 个 worker 并发；恢复性错误最多重试 3 次，不可恢复错误落库记录。
- 每次调度都生成任务运行记录与 NDJSON 日志文件，并在管理端详情页可查看摘要与下载日志。
- 新增 `sync_subscription_events` 领域事件表，支持按 task / user / repo 维度检索关键事件。
- 管理端 Scheduled Runs 支持同时查看 `brief.daily_slot` 与 `sync.subscriptions`。

### Non-goals

- 不重构用户侧 `/api/starred`、`/api/releases`、feed、brief、翻译、reaction` 的读模型。
- 不引入跨实例分布式锁；当前按单 SQLite 进程模型做调度去重。
- 不在本轮实现真实程序截图或新的可视化报表页。

## 范围（Scope）

### In scope

- DB migration：`job_tasks.log_file_path`、半小时调度去重状态表、`sync_subscription_events`。
- 后端运行时：全用户订阅同步协调器、5x5 worker pool、公平排序、错误分类与重试。
- 日志与可观测：NDJSON 日志文件、关键事件落库、管理端任务详情诊断、日志下载接口。
- 管理端前端：Scheduled Runs 列表扩容到新任务类型，新增 `sync.subscriptions` 专属详情展示与 Storybook story。
- 自动化测试：排序/去重/诊断/日志/管理端任务详情覆盖。

### Out of scope

- 跨节点 leader election / Redis 分布式调度。
- Release 数据模型去重重构或用户视图改造成仓库共享表。
- 事件表归档与冷热分层。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） |
| --- | --- | --- | --- | --- | --- | --- |
| `sync.subscriptions` | Job task | internal | New | `./contracts/http-apis.md` | backend | scheduler / admin |
| `GET /api/admin/jobs/realtime/{task_id}` | HTTP API | external | Modify | `./contracts/http-apis.md` | backend | web-admin |
| `GET /api/admin/jobs/realtime/{task_id}/log` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-admin |
| `GET /api/admin/jobs/realtime` | HTTP API | external | Modify | `./contracts/http-apis.md` | backend | web-admin |
| `job_tasks` / `scheduled_task_dispatch_state` / `sync_subscription_events` | DB schema | internal | Modify/New | `./contracts/db.md` | backend | backend |

### 契约文档（按 Kind 拆分）

- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/db.md](./contracts/db.md)

## 功能与行为规格（Functional / Behavior Spec）

### 调度与去重

- 半小时调度键使用 UTC `YYYY-MM-DDTHH:MM`（分钟仅 `00` 或 `30`）。
- 同一 `schedule_key` 只允许 dispatch 一次。
- 若上一轮 `sync.subscriptions` 仍处于 `queued|running`，当前触发不并发执行，而是生成一条 `status=succeeded + result.skipped=true + skip_reason=previous_run_active` 的运行记录，并生成日志文件。

### Star 阶段

- 仅处理 `users.is_disabled = 0` 的用户。
- 用户排序：`last_active_at DESC, user_id ASC`；`NULL last_active_at` 排最后。
- 采用 5 个 worker 并发刷新 Star 快照。
- 单个用户只有在完整获取成功后才事务性替换 `starred_repos`；失败时保留旧快照。
- Star 阶段失败的用户不参与本轮 repo 聚合。

### Repo / Release 阶段

- repo 聚合输入仅来自本轮 Star 阶段成功用户。
- 聚合结果为 `(repo_id, repo_full_name, is_private, related_users[])`，其中 `related_users` 必须同时满足“本轮 Star 成功 + 当前仍对该 repo 加星”。
- repo 队列排序：`related_users.len DESC, repo_full_name ASC`。
- 候选凭据排序：`related_user.last_active_at DESC, user_id ASC`。
- 每个 repo 只向 GitHub 拉取一次 Release；成功后 fan-out upsert 到全部关联用户的现有 `releases` 记录。
- repo 级恢复性错误：对当前候选凭据最多重试 3 次（带退避），重试耗尽后再切换到下一位关联用户。
- repo 级不可恢复错误：记录关键事件后直接切换下一位候选用户；全部候选均失败则该 repo 记为失败。

### 错误与事件

- 恢复性错误：transport/connect/timeout/429/5xx/secondary rate limit。
- 不可恢复错误：缺 token、401、scope 不足、repo 明确 404/451；`401/403` 若明显是凭据/权限问题，记用户事件并继续尝试下一位候选用户。
- 关键事件写入 `sync_subscription_events`；高层进度继续写 `job_task_events`。
- 每次任务都生成 NDJSON 日志文件；日志包含阶段切换、worker 结果、重试、skip、最终汇总。

## 验收标准（Acceptance Criteria）

- Given UTC 到达新的整点或半点且当前没有活动中的 `sync.subscriptions`
  When 调度器运行
  Then 只生成一条对应 `schedule_key` 的运行记录与日志文件，并按 Star -> Release 顺序执行。

- Given 上一轮 `sync.subscriptions` 仍处于 `queued|running`
  When 下一次半小时调度触发
  Then 新增一条 `skipped=true` 的运行记录与日志文件，不与上一轮并发执行。

- Given 多个用户 `last_active_at` 不同
  When Star 阶段分派
  Then 实际分派顺序遵循 `last_active_at DESC, user_id ASC`，并发不超过 5。

- Given 多个聚合 repo 关联用户数不同
  When Release 阶段分派
  Then repo 队列遵循 `related_users DESC, repo_full_name ASC`，候选凭据顺序遵循关联用户活跃度倒序。

- Given 某 repo 抓取成功且存在 N 个关联用户
  When fan-out 回写
  Then Release 只从 GitHub 拉取一次，但会 upsert 到全部 N 个用户的 `releases`。

- Given 网络超时 / 429 / 5xx / secondary rate limit
  When 请求失败
  Then 每个候选凭据最多重试 3 次，并记录 recoverable 关键事件。

- Given 管理员查看 `sync.subscriptions` 任务详情
  When 页面加载完成
  Then 可看到两阶段摘要、skip/失败原因、最近关键事件与日志下载入口。

## 实现里程碑（Milestones / Delivery checklist）

- [ ] M1: Spec / contract / migration 冻结。
- [ ] M2: 半小时调度、任务日志、领域事件表与全用户同步运行时落地。
- [ ] M3: 管理端任务详情、Scheduled Runs 与 Storybook 完成。
- [ ] M4: Rust + Web 验证完成，spec 同步收口。

## 文档更新

- 更新 `docs/specs/README.md` index。
- 补充本规格 `contracts/db.md` 与 `contracts/http-apis.md`。

## 风险与开放问题

- SQLite 单机调度去重不覆盖多实例并发；后续若引入多实例需补 leader / distributed lock。
- `sync_subscription_events` 长期增长后可能需要 retention / archive 策略。
- 本轮 fan-out 沿用现有 `releases` 用户维度存储，后续若做共享仓库模型需另开规格。

## 假设

- 默认使用本地文件系统存储任务日志，路径位于应用可写目录下。
- 当前部署为单进程 SQLite，半小时调度去重无需跨实例协调。

## 变更记录（Change log）

- 2026-03-06: 新建规格，冻结半小时订阅同步、事件表、日志下载与管理端可观测范围。
