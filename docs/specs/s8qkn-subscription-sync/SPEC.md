# 全局 Repo Release 复用与访问触发增量同步（#s8qkn）

## 状态

- Status: Implemented

## 背景 / 问题陈述

旧实现把 Release 同步和存储都绑定在用户维度：

- `sync.releases` 直接按用户当前 `starred_repos` 向 GitHub 抓取并写入用户私有 `releases`
- `sync.subscriptions` 虽然聚合了 repo 抓取，但仍会 fan-out 回写到每个用户的 `releases`
- 新用户或长时间未访问的用户进入站点后，看不到“先展示已有缓存、再补齐最新数据”的 staged refresh
- Dashboard 的 `Sync all` 由多个 sibling tasks 组成，顺序不可控，无法保证 `starred` 先于 `releases`

这一轮把 Release 读写模型、任务编排和访问 bootstrap 一次性收敛到共享仓库模型。

## 目标 / 非目标

### Goals

- 引入共享 `repo_releases` 缓存，所有用户从 `starred_repos + repo_releases` 读取 Release。
- 引入全局 `repo_release_work_items` / `repo_release_watchers`，把访问触发、手动同步、定时订阅同步统一汇聚到 repo 级共享队列。
- 新增 `sync.access_refresh`：
  - 覆盖 `Star + Release + social + Inbox`
  - 首访或超过 1 小时未访问时自动触发
  - `star_refreshed` 后立即让前端刷新可见缓存
  - Release work 完成后再刷新一次，并在 social / Inbox 阶段结束后收口
  - social / Inbox 失败保持 best-effort，不把整轮访问刷新降级成硬失败
- `sync.subscriptions` 改成：
  - 刷新用户 Star
  - 聚合 repo demand
  - 挂到共享 repo release queue
  - 等待关联 outcome 并输出 `Release + social + Inbox` 摘要
- `GET /api/me` 返回 `access_sync` 元信息，前端可直接附着到用户自己的 task SSE。

### Non-goals

- 不改定时任务的半小时 bucket 调度频率。
- 不引入跨实例分布式锁或 leader election。
- 不引入 GitHub webhook / upstream push 模式。

## 范围（Scope）

### In scope

- DB migration:
  - `repo_releases`
  - `repo_release_work_items`
  - `repo_release_watchers`
  - 从旧 `releases` 去重回填到 `repo_releases`
- 后端运行时：
  - repo 级共享 release worker
  - runtime lease heartbeat / recovery
  - `sync.access_refresh`
  - `/api/me.access_sync`
  - `/api/tasks/{task_id}/events`
- 读模型切换：
  - `/api/releases`
  - `/api/releases/{id}/detail`
  - feed
  - brief
  - release translation
  - release reaction
- Dashboard staged refresh：
  - 读取 `me.access_sync`
  - 订阅 task SSE
  - 在 `star_refreshed` / `task.completed` 两次刷新
  - 空态改成“自动同步中 / 无缓存内容”两态
- Storybook：
  - 新增 `AccessSyncEmptyState` 稳定场景

### Out of scope

- 删除历史 `releases` 表或一次性清理所有遗留测试数据。
- 新增专门的 repo release 管理后台页面。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） |
| --- | --- | --- | --- | --- | --- | --- |
| `GET /api/me` | HTTP API | external | Modify | `./contracts/http-apis.md` | backend | web |
| `GET /api/tasks/{task_id}/events` | HTTP API / SSE | external | New | `./contracts/http-apis.md` | backend | web |
| `POST /api/sync/all` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web |
| `POST /api/sync/releases` | HTTP API | external | Modify | `./contracts/http-apis.md` | backend | web |
| `sync.access_refresh` | Job task | internal | New | `./contracts/http-apis.md` | backend | api / web |
| `sync.subscriptions` | Job task | internal | Modify | `./contracts/http-apis.md` | backend | scheduler / admin |
| `repo_releases` / `repo_release_work_items` / `repo_release_watchers` | DB schema | internal | New | `./contracts/db.md` | backend | backend |

## 功能与行为规格（Functional / Behavior Spec）

### 访问触发刷新

- `GET /api/me` 必须先读取用户旧 `last_active_at`，再决定是否 stale，最后才 touch 新的 `last_active_at`。
- 触发条件：
  - `last_active_at IS NULL`
  - 或者距当前时间超过 1 小时
- 若当前用户已有 `queued|running` 的 `sync.access_refresh`，则直接复用该 task，`reason=reused_inflight`。
- 否则新建 `sync.access_refresh`，`reason=first_visit|inactive_over_1h`。

### `sync.access_refresh`

- 阶段固定为：
  1. 刷新当前用户 Star 快照
  2. 触发 `task.progress(stage=star_refreshed)`
  3. 将当前可见 repo 挂到共享 repo release queue
  4. 触发 `task.progress(stage=release_attached)`
  5. 等待关联 work item 满足“已有新鲜缓存或本轮完成”
  6. 触发 `task.progress(stage=release_summary)`
  7. 触发 `task.progress(stage=social_summary)`
  8. 触发 `task.progress(stage=notifications_summary)`
  9. `task.completed`
- 访问自动刷新会继续补齐 `social + Inbox`，但 `star_refreshed` 仍然是前端第一次刷新缓存的关键节点。
- social 或 Inbox 若失败，访问刷新任务仍返回成功，并把对应错误附带到阶段事件 / 结果 JSON 中。

### 共享 repo release queue

- `repo_release_work_items` 一条 repo 只保留一条共享 work item。
- claim 顺序固定为：
  - `priority DESC`
  - `has_new_repo_watchers DESC`
  - `deadline_at ASC`
  - `created_at ASC`
- 访问触发 demand 使用 `priority=interactive`，定时订阅同步使用 `priority=system`。
- 若系统排队 work item 被访问 demand 命中，则允许升级优先级，但不抢占已 running work item。
- 单个 repo 抓取时按“当前仍 star 该 repo 的用户”挑选候选 token，排序规则：
  - `last_active_at DESC`
  - `user_id ASC`
- 成功抓取后写入共享 `repo_releases`，并把等待中的 watcher 标记为 `succeeded`。
- 失败时把等待中的 watcher 标记为 `failed`。

### `sync.subscriptions`

- Star 阶段仍然按用户活跃度刷新 `starred_repos`，失败用户不会参与 repo 聚合。
- Release 阶段不再 inline 抓 GitHub Release，也不再 fan-out 写用户私有 `releases`。
- Release 阶段改成：
  - 聚合 repo demand
  - 挂到共享 repo release queue
  - 等待共享 queue 结果
  - 在任务结果里输出 repo 级摘要
- Release 结束后继续按 Star 成功用户 fan-out：
  - `social_summary`：调用 `sync_social_activity_best_effort`，聚合 `repo_stars / followers / events`
  - `notifications_summary`：调用 `sync_notifications`，聚合新增通知数
- social / Inbox 任一用户失败时，本轮 `sync.subscriptions` 仍继续执行并返回完成态，但必须把失败写入 `sync_subscription_events` / run log / admin diagnostics。

### 读模型与可见性

- Release 可见性统一为“当前用户 stars 了该 repo”。
- 下列读路径统一改成 `starred_repos + repo_releases`：
  - `/api/releases`
  - `/api/releases/{id}/detail`
  - `/api/feed`
  - daily brief
  - release batch translation / detail translation
  - release reaction toggle / reaction count persistence
- reaction counts 保存在共享 `repo_releases`。
- viewer-specific reaction 状态继续按当前用户 live 查询，不做共享存储。

### Dashboard staged refresh

- 首屏仍先加载旧 feed / brief / inbox。
- 若 `me.access_sync.task_id` 存在，则立即连用户侧 SSE。
- 收到 `star_refreshed` 时执行第一次 `refreshAll()`，以显示服务端已知的共享缓存。
- 收到 `task.completed(status=succeeded)` 时执行第二次 `refreshAll()`。
- 当 access sync 进行中且 feed 为空时，空态显示“正在同步你的 Star / Release”。
- 当 access sync 不在进行中且 feed 为空时，空态显示“还没有缓存内容”。

## 验收标准（Acceptance Criteria）

- Given 用户首次访问或超过 1 小时未访问
  When `GET /api/me` 返回
  Then 响应包含 `access_sync.task_id`，并且同一用户不会重复入队多个 `sync.access_refresh`。

- Given 用户已经有旧缓存 Release
  When `sync.access_refresh` 发出 `star_refreshed`
  Then Dashboard 第一次刷新可以看到与当前 `starred_repos` 匹配的共享 `repo_releases`。

- Given 某个 repo 已经被系统同步或另一位用户访问 demand 排队
  When 当前用户访问再次命中同 repo
  Then 不会产生重复 GitHub 抓取，队列只升级 demand。

- Given shared `repo_releases` 已启用
  When feed / release detail / brief / translation / reaction 读取 Release
  Then 只依赖“当前用户 star 可见 + 共享 repo release 缓存”，不依赖用户私有 `releases`。

- Given `sync.subscriptions` 被 scheduler 在半小时槽触发
  When 本轮 Star / Release 摘要已经完成
  Then 同一 task 还会继续发出 `social_summary` 与 `notifications_summary`，并在 `result_json` 中包含四段聚合摘要。

- Given 某个用户的 social 或 Inbox 拉取失败
  When `sync.subscriptions` 结束
  Then 整轮任务仍可完成，但 Admin Jobs 详情页会展示 partial outcome，并能从 `recent_events` 看到对应失败线索。

## Visual Evidence

source_type=storybook_canvas  
target_program=mock-only  
capture_scope=element  
sensitive_exclusion=N/A  
submission_gate=pending-owner-approval  
story_id_or_title=Pages/Dashboard/AccessSyncEmptyState  
state=auto-sync-empty-state  
evidence_note=验证访问触发同步期间，Dashboard 空态不再提示手动 Sync all，而是展示 staged refresh 文案

![Access sync empty state](./assets/access-sync-empty-state.png)

source_type=storybook_canvas  
target_program=mock-only  
capture_scope=browser-viewport  
sensitive_exclusion=N/A  
submission_gate=approved  
story_id_or_title=Admin/Task Type Detail/SyncSubscriptions  
state=scheduler-social-and-inbox-summary  
evidence_note=验证 Admin Jobs 的 sync.subscriptions 详情页已展示 Star、Release、Social、Inbox 四阶段摘要与最近关键事件。

PR: include
![Admin sync subscriptions detail](./assets/admin-sync-subscriptions-detail.png)
