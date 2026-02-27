# 管理员任务中心：LLM 调度观测与调用排障（#erscd）

## 状态

- Status: 已完成
- Created: 2026-02-27
- Last: 2026-02-27

## 背景 / 问题陈述

当前系统已存在进程内全局 LLM 调度器（串行发放 + 固定节流），但管理员页面缺少针对该调度器与调用链路的可观测能力，排查慢、定位成本高。

## 目标 / 非目标

### Goals

- 在 `/admin/jobs` 提供 LLM 调度状态与调用列表（含筛选）。
- 提供调用详情（完整 prompt/response/error）与父任务跳转能力。
- 详情支持多轮输入/输出消息展示，并展示输入/输出/缓存 token 指标。
- 为调用级观测新增后端日志落库与 7 天自动清理。

### Non-goals

- 不实现调用重放/重跑动作。
- 不做多实例聚合（按单后端实例设计）。
- 不做历史数据回填。

## 范围（Scope）

### In scope

- DB migration：新增 `llm_calls`。
- 后端：`src/ai.rs` 调度观测与调用日志、管理员 LLM API。
- 后端：`src/jobs.rs` LLM 调用上下文透传。
- 前端：`web/src/admin/JobManagement.tsx` 增加 “LLM 调度” 标签页与筛选/详情。
- 前端：`web/src/api.ts` 新增 LLM 调度 API 类型与请求。
- 自动化测试：Rust API 测试 + Playwright 管理页用例 + Storybook mock。

### Out of scope

- 非管理员用户可见性扩展。
- 现有实时任务/定时任务页面重构。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） |
| --- | --- | --- | --- | --- | --- | --- |
| `GET /api/admin/jobs/llm/status` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-admin |
| `GET /api/admin/jobs/llm/calls` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-admin |
| `GET /api/admin/jobs/llm/calls/{call_id}` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-admin |
| `llm_calls` | DB schema | internal | New | `./contracts/db.md` | backend | backend |

### 契约文档（按 Kind 拆分）

- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/db.md](./contracts/db.md)

## 验收标准（Acceptance Criteria）

- Given 管理员访问 `/admin/jobs`
  When 切换到 “LLM 调度”
  Then 可见调度状态卡、调用列表与筛选区。

- Given 调用列表筛选 `status + source + requested_by + time`
  When 发起查询
  Then 返回结果与总数、分页一致。

- Given 点击某条调用详情
  When 抽屉打开
  Then 可见完整 prompt/response/error 与等待/首字/耗时/重试次数。

- Given 调用记录存在 `parent_task_id`
  When 点击 “查看父任务”
  Then 打开现有任务详情并定位对应任务。

- Given 日志保留策略
  When 超过 7 天
  Then 旧记录自动清理，近 7 天可查询。

## 实现前置条件（Definition of Ready / Preconditions）

- [x] 单实例部署约束已确认。
- [x] 数据策略已确认（完整文本明文、7 天保留）。
- [x] 页面位置已确认（`/admin/jobs` 新标签）。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Rust tests：管理员鉴权、筛选、分页、详情、状态聚合。
- Web e2e：管理页可切换到 LLM 调度并展示列表/详情。
- Storybook：补充 LLM 调度标签 mock 数据覆盖。

### Quality checks

- `cargo test`
- `cd web && bun run build`
- `cd web && bun run e2e -- admin-jobs.spec.ts`

## 文档更新（Docs to Update）

- `docs/specs/README.md`：新增规格索引并在实现后更新状态。
- 本规格 `contracts/*`：维护接口和数据结构。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 数据层（`llm_calls` + 索引）与保留清理任务。
- [x] M2: 调度观测与调用日志埋点落地（含上下文透传）。
- [x] M3: 管理员 LLM API（status/list/detail）。
- [x] M4: 前端 LLM 调度标签页 + 筛选 + 详情 + 父任务跳转。
- [x] M5: 自动化测试与文档同步完成。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：明文保存完整 prompt/response 带来敏感信息泄漏风险（已接受）。
- 风险：长期高并发下 SQLite 写入量增长，需依赖 TTL 维持体积可控。
- 假设：当前运行拓扑为单后端实例。

## 变更记录（Change log）

- 2026-02-27: 创建规格并冻结实现边界。
- 2026-02-27: 完成后端/前端实现，新增迁移与管理员 LLM 观测页面，并通过 `cargo test` + `web build` + `admin-jobs e2e` 验证。
- 2026-02-27: 增补多轮消息 JSON 展示与 token 指标（含 cached tokens），扩展 llm_calls schema 与 admin API/UI。
- 2026-02-27: 增补首字等待时间（first token wait）落库与详情/列表展示，用于排查模型首包延迟。
- 2026-02-27: 新增 `llm_call_events` 与 `llm.call` SSE 事件，支持后台页面对 LLM 调用列表/状态/详情的实时刷新。
