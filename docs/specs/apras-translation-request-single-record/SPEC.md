# 翻译请求单条记录制重建（#apras）

## 状态

- Status: 部分完成（3/4）
- Created: 2026-03-09
- Last: 2026-03-09

## 背景 / 问题陈述

当前翻译调度已把真正的聚合职责放进 scheduler / batch 层，但 request 层仍残留旧的“一个 request 可以挂多个 item”的结构，导致语义与展示都出现偏差：

- `translation_requests` 仍需要通过 `translation_request_items` / `translation_work_watchers` 才能拿到真实结果，模型冗余且维护成本高。
- 管理端需求队列只能展示 `completed_item_count / item_count`，会把“单个翻译请求”误显示成 `2/3` 之类的伪进度。
- `POST /api/translate/requests` 仍让生产者决定 request-stage batching，与“请求层单条、调度层聚合”的目标相冲突。
- Feed 自动翻译为了配合旧接口保留 request-stage batch stream，前后端都被迫兼容过时结构。

本次需要用破坏式重建把 request 层彻底收敛为“1 request = 1 logical translation item”，并删除所有仅为旧聚合模型存在的中间表与公开契约。

## 目标 / 非目标

### Goals

- 重建翻译请求模型为单条记录制，删除 `translation_request_items` / `translation_work_watchers`。
- 让 `translation_requests` 直接内联请求输入、执行绑定与结果字段，成为请求层唯一事实来源。
- 改造 `POST /api/translate/requests` 为双形态：单条 `{ mode, item }`；批量 async `{ mode: 'async', items }`。
- 将单请求公开响应、stream 事件、管理端请求详情全部改为 singular `result` 结构。
- 保留 scheduler/batch/work-item 聚合能力，但 request 侧不再出现任何进度型字段。
- 让 feed 自动翻译切换到并发受控的单 item wait 提交，管理端需求队列按“一行一请求”展示。

### Non-goals

- 不保留旧翻译请求/批次历史数据；迁移允许直接清空相关调度域表。
- 不修改 `ai_translations` 缓存表与缓存命中语义。
- 不改变 worker 槽位、`request_origin` 分流和 batch / LLM 追链目标。
- 不为仓库外未知消费者保留兼容层。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） |
| --- | --- | --- | --- | --- | --- | --- |
| `POST /api/translate/requests` | HTTP API | external | Breaking | `./contracts/http-apis.md` | backend | web-feed / web-sidebar |
| `GET /api/translate/requests/{request_id}` | HTTP API | external | Breaking | `./contracts/http-apis.md` | backend | web producers |
| `GET /api/translate/requests/{request_id}/stream` | HTTP API | external | Breaking | `./contracts/http-apis.md` | backend | web producers |
| `/api/admin/jobs/translations/requests*` | HTTP API | external | Breaking | `./contracts/http-apis.md` | backend | web-admin |
| `translation_requests` / `translation_work_items` / `translation_batches` / `translation_batch_items` | DB schema | internal | Breaking | `./contracts/db.md` | backend | backend |

### 契约文档（按 Kind 拆分）

- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/db.md](./contracts/db.md)

## 验收标准（Acceptance Criteria）

- Given 调用方提交单个翻译项
  When 服务端创建请求
  Then `translation_requests` 直接写入该项的输入字段、绑定字段与结果字段，不再写 `translation_request_items` 或 watcher 表。

- Given 调用方以 `mode=async` 提交多个翻译项
  When 服务端接收请求
  Then 服务端按输入顺序创建多条独立 `translation_requests`，并返回一组按输入顺序排列的 request 摘要。

- Given 调用方以 `mode=wait` 或 `mode=stream` 提交单个翻译项
  When 请求完成
  Then `GET` 与 stream 事件都只返回单个 `result`，不存在 `items[]`、`batch_ids[]`、`completed_item_count/item_count`。

- Given 多条 request 命中同一 work item
  When batch 执行完成或失败
  Then 所有关联 request 都通过 `translation_requests.work_item_id` 被正确 fan-out 更新，`translation_batches.request_count` 统计真实 request 数。

- Given 管理员查看需求队列或请求详情
  When 页面载入或打开抽屉
  Then 队列一行只对应一条 request，页面不再展示 request 进度列，请求详情只展示单个结果对象。

- Given 迁移应用到现有数据库
  When 新版本启动
  Then 旧的翻译调度历史被清空，运行时不再依赖 `translation_request_items` / `translation_work_watchers`。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Rust tests：单请求提交、批量 async 拆单、request fan-out、batch `request_count` 统计、单请求 stream / detail 返回。
- Web tests：feed 自动翻译单 item wait 路径、release detail 翻译、admin 需求队列与请求详情。
- Playwright：管理员翻译调度页回归、请求详情/批次详情跳转、需求队列不再显示进度列。

### Quality checks

- [x] `cargo test`
- [x] `cd web && bun run build`
- [x] `cd web && bun run e2e -- e2e/admin-jobs.spec.ts e2e/release-detail.spec.ts`
- [x] `cd web && bun run storybook:build`

## 变更记录（Change log）

- 2026-03-09: 新增 follow-up spec，明确 request 层单条化、破坏式迁移与公开 API breaking changes；替代旧 spec 中 request-stage aggregation 的描述。
- 2026-03-09: 实现与验证同步完成：后端迁移/调度逻辑切到单 request 直连 work item，前端与 fixtures 改为 singular result，并补跑 Rust、web build、Playwright 与 Storybook build。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: follow-up spec 与 breaking contracts 冻结。
- [x] M2: 迁移与后端 request/work-item/batch 逻辑重建，移除 request-item/watcher 依赖。
- [x] M3: 前端 API、feed 自动翻译与 admin 请求视图切换到单条 request 语义。
- [ ] M4: 测试、快车道 PR、checks 与 review-loop 收敛。

## 风险与开放问题

- 这是显式 breaking change，需要前后端与 fixtures/stories 一次性同步完成，否则构建会整体失配。
- 数据层采用破坏式重建，任何现存翻译调度历史都会在迁移后丢失。
- 若仓库外仍有未发现调用方依赖旧接口，本次不会提供兼容层。
