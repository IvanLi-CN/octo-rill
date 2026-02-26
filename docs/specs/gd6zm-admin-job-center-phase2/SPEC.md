# 管理员任务中心（二期）+ 用户管理字段补齐（#gd6zm）

## 状态

- Status: 已完成
- Created: 2026-02-25
- Last: 2026-02-26

## 背景 / 问题陈述

管理员面板一期仅覆盖用户管理，缺少任务可观测与调度治理能力；同时用户表新增日报相关字段后，用户管理界面缺少对应可见性。

## 目标 / 非目标

### Goals

- 新增独立管理员路由 `/admin/jobs`，提供任务总览与任务管理。
- 引入通用异步任务引擎（任务主表 + 事件表 + 运行时 worker）。
- 落地 24 个 UTC 小时槽定时日报任务（命中槽位后按用户串行生成，失败不中断）。
- 扩展触发类接口支持 `return_mode=sync|task_id|sse`。
- 在用户管理中补齐新增字段可视化：`last_active_at` 列表展示、`daily_brief_utc_time` 详情抽屉展示（只读）。
- 任务详情按 `task_type` 提供专属详情页，并在 Storybook 为各类任务提供独立示例。

### Non-goals

- 不做任务类型可视化创建器。
- 不做小时槽数量动态扩缩（固定 24）。
- 不在用户管理页提供新增字段编辑能力。

## 范围（Scope）

### In scope

- DB migration：`users.daily_brief_utc_time`、`users.last_active_at`、`job_tasks`、`job_task_events`、`daily_brief_hour_slots`。
- 后端任务运行时：队列消费、任务状态流转、重试/取消、SSE 事件流。
- 调度器改造：每小时轮询，命中 `hour_utc` 槽位后入队日报任务。
- 管理员 API：任务总览、实时任务列表/详情、重试、取消、定时槽位查询与启停。
- 前端 `/admin/jobs` 页面与用户管理字段展示补齐；任务详情区按 `task_type` 渲染专属信息卡片与说明。
- Storybook 覆盖任务详情页：为每类任务提供单独故事条目，便于视觉回归与需求对齐。
- 自动化测试补充（Rust + Playwright）。

### Out of scope

- 用户时区个性化调度策略。
- 任务模板管理、任务参数 DSL。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） |
| --- | --- | --- | --- | --- | --- | --- |
| `GET /api/admin/users/{user_id}/profile` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-admin |
| `GET /api/admin/jobs/overview` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-admin |
| `GET /api/admin/jobs/realtime` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-admin |
| `GET /api/admin/jobs/realtime/{task_id}` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-admin |
| `POST /api/admin/jobs/realtime/{task_id}/retry` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-admin |
| `POST /api/admin/jobs/realtime/{task_id}/cancel` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-admin |
| `GET /api/admin/jobs/scheduled` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-admin |
| `PATCH /api/admin/jobs/scheduled/{hour_utc}` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-admin |
| `POST /api/sync/*` + `POST /api/briefs/generate` + `POST /api/translate/*` | HTTP API | external | Modify | `./contracts/http-apis.md` | backend | web |
| `users` / `job_tasks` / `job_task_events` / `daily_brief_hour_slots` | DB schema | internal | Modify/New | `./contracts/db.md` | backend | backend |

### 契约文档（按 Kind 拆分）

- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/db.md](./contracts/db.md)

## 验收标准（Acceptance Criteria）

- Given 管理员访问 `/admin/jobs`
  When 页面加载完成
  Then 可见“任务总览 + tabs（实时异步任务/定时任务）”。

- Given 定时任务页
  When 查询槽位
  Then 固定返回 24 个 UTC 小时槽，并支持启停。

- Given 每小时调度轮询
  When 命中当前 UTC 小时槽
  Then 入队对应日报任务，并按 `last_active_at DESC, user_id ASC` 串行生成日报。

- Given 串行执行中某用户生成失败
  When 继续执行
  Then 任务记录失败用户并继续后续用户，不中断整槽任务。

- Given 触发类接口请求 `return_mode=task_id`
  When 请求成功
  Then 立即返回 `task_id`。

- Given 触发类接口请求 `return_mode=sse`
  When 建立连接
  Then 首条及后续事件都包含 `task_id`。

- Given 用户管理列表
  When 查看用户行
  Then 显示 `last_active_at`（浏览器时区 `HH:mm`）。

- Given 用户详情抽屉
  When 打开用户详情
  Then 显示 `daily_brief_utc_time` 的本地时区 `HH:mm` 与 UTC 原值（只读）。

- Given 管理员在任务中心点击任意任务“详情”
  When 任务详情抽屉打开
  Then 根据该任务 `task_type` 展示对应专属详情页（包含业务字段与语义化说明），不使用单一通用模板替代。

- Given Storybook 打开 `Admin/TaskTypeDetailPage`
  When 查看故事列表
  Then 各任务类型均有独立 story，覆盖任务详情页的专属展示分支。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: DB migration 与任务引擎基础设施（worker + scheduler + events）。
- [x] M2: 管理员任务 API 与触发接口 `return_mode` 扩展。
- [x] M3: 前端 `/admin/jobs` 页面与用户管理字段补齐。
- [x] M4: Rust / Web 测试与验证通过。
- [x] M5: 任务类型专属详情页与 Storybook 分类型示例补齐。

## 变更记录（Change log）

- 2026-02-25: 新建规格并落地实现，完成本地验证。
- 2026-02-26: 新增“任务类型专属详情页”要求并完成实现；同步 Storybook 与契约文档。
