# 统一翻译调度器与独立管理界面改造（#35r55）

## 状态

- Status: 已实现（待合并收口）
- Created: 2026-03-07
- Last: 2026-03-25

## 背景 / 问题陈述

当前翻译链路仍由生产者直接决定批量方式与返回模式：

- Feed 自动翻译、Release Detail 翻译、Notification 翻译各自直接命中不同 API 与任务模型。
- `translate.release*` / `translate.notification` 仍被建模为 `job_tasks`，导致“执行状态”与“业务结果”语义纠缠。
- 生产者无法只表达“我需要这份翻译，并且最多等多久”，而必须自行决定是否单条、批量、同步或流式。
- 管理端缺少“翻译需求 -> 调度批次 -> LLM 调用 -> 结果扇出”的独立观测入口。

需要把翻译从“调用方临时拼 batch”改成“统一需求调度”，让生产者只提交需求，由调度器按时间窗口与内容长度窗口统一组批执行。

## 目标 / 非目标

### Goals

- 新建统一翻译调度器，接收单条或多条翻译需求并按 `max_wait_ms` + token 窗口统一组批。
- 翻译工作从 `job_tasks` 域中剥离，形成独立的 requests / work items / batches / watchers 领域模型。
- 提供统一生产者 API，支持 `async` / `wait` / `stream` 三种交付方式。
- 支持同一批次内混合 `release_summary`、`release_detail`、`notification`，并只在批次终态时向请求方返回整组结果。
- 在 `/admin/jobs` 新增“翻译调度”标签页，提供调度状态、请求视图、批次视图与 LLM 调用追链。
- 停止为新的翻译工作创建 `translate.release*` / `translate.notification` 类 `job_tasks`。

### Non-goals

- 不把日报生成等非翻译类 LLM 任务并入本调度器。
- 不实现多实例分布式抢占与跨实例协调。
- 不做历史翻译任务数据回填。
- 不实现人工重跑 / 重放动作。

## 当前实现说明

- 统一翻译调度器已落地：请求、去重 work item、watcher、batch 与 `llm_calls.parent_translation_batch_id` 已入库。
- 新生产者入口已切到 `POST/GET /api/translate/requests*`，旧翻译接口统一返回 `410 Gone`，避免新旧双轨并存。
- Feed 自动翻译改为 `stream` 请求，Release Detail 改为 `wait` 请求，管理员在 `/admin/jobs` 可查看“翻译调度”标签页与批次/LLM 追链。
- 当前批次执行层仍按 `kind + entity_id + scope_user_id` 复用既有翻译核心函数；`source_blocks` / `target_slots` 已作为统一协议、去重哈希与管理端展示输入。
- `translation_batches` 与关联 `llm_calls` 现在持有运行期 lease；服务启动前先回收孤儿 `running` 记录，运行中按固定心跳/过期阈值做 sweep，避免请求、work item、batch、LLM 调用卡死在 `running`。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） |
| --- | --- | --- | --- | --- | --- | --- |
| `POST /api/translate/requests` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-feed / web-sidebar / future producers |
| `GET /api/translate/requests/{request_id}` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web producers |
| `GET /api/translate/requests/{request_id}/stream` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web producers |
| `/api/admin/jobs/translations/*` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-admin |
| `translation_requests` / `translation_request_items` / `translation_work_items` / `translation_work_watchers` / `translation_batches` / `translation_batch_items` | DB schema | internal | New | `./contracts/db.md` | backend | backend |
| `llm_calls` translation parent link | DB schema | internal | Modify | `./contracts/db.md` | backend | backend / web-admin |

### 契约文档（按 Kind 拆分）

- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/db.md](./contracts/db.md)

## 验收标准（Acceptance Criteria）

- Given 生产者提交单条或多条翻译需求
  When 请求进入统一调度器
  Then 服务端创建独立 `translation_request`，并将每个 item 归并到去重 `work_item` 或缓存结果。

- Given 多个请求命中同一 `scope_user_id + kind + variant + entity_id + target_lang + source_hash`
  When 调度器尚未完成翻译
  Then 多个请求共享同一 `work_item`，批次完成后向所有 watcher 扇出相同结果。

- Given 队列累计 token 达到阈值或最早 `deadline_at` 到达
  When 调度器扫描工作项
  Then 创建 `translation_batch` 并将符合条件的 work items 一次封批执行。

- Given 请求使用 `wait` 或 `stream`
  When 所属批次尚未完成
  Then 请求方不能提前拿到单项结果；只有批次终态后才能收到本请求对应的整组结果。

- Given 一个批次同时包含 `release_summary`、`release_detail`、`notification`
  When 管理员查看批次详情
  Then 页面显示批次触发原因、item kinds、token 预算、LLM 调用与每项结果。

- Given 新翻译工作由统一调度器处理
  When 管理员查看旧的实时任务列表
  Then 不会再出现新的 `translate.release*` / `translate.notification` 任务记录；历史记录仍可只读查看。

- Given 服务重启前存在孤儿 `translation_batches.running`、`translation_work_items.running`、`translation_requests.running` 或关联 `llm_calls.running`
  When 新进程完成启动前 recovery pass
  Then 它们统一收口到终态 `failed`，并使用 `runtime_lease_expired` 作为错误原因，而不是继续停留在 `running`。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Rust tests：请求去重、缓存命中、deadline flush、token flush、批次扇出、批次终态返回、管理员 API 聚合。
- Web tests：Feed 自动翻译、Release Detail 翻译、管理员“翻译调度”标签页。
- Playwright：producer `wait`/`stream` 行为与管理员视图回归。

### Quality checks

- [x] `cargo test`
- [x] `cd web && bun run build`
- [x] `cd web && bun run e2e -- release-detail.spec.ts admin-jobs.spec.ts`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: DB schema + translation scheduler runtime + unified producer API.
- [x] M2: Feed / Release Detail / Notification producers migrated to unified API.
- [x] M3: Admin translations tab + request/batch detail + LLM linkage.
- [ ] M4: Validation, review convergence, and PR-ready docs sync.
