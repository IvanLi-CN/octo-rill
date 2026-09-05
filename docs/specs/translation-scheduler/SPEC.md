# 统一翻译调度器与独立管理界面改造

## 背景 / 问题陈述

当前翻译链路仍由生产者直接决定批量方式与返回模式：

- Feed 自动翻译、Release Detail 翻译、Notification 翻译各自直接命中不同 API 与任务模型。
- `translate.release*` / `translate.notification` 仍被建模为 `job_tasks`，导致“执行状态”与“业务结果”语义纠缠。
- 生产者无法只表达“我需要这份翻译，并且最多等多久”，而必须自行决定是否单条、批量、同步或流式。
- 管理端缺少“翻译需求 -> 调度批次 -> LLM 调用 -> 结果扇出”的独立观测入口。

需要把翻译从“调用方临时拼 batch”改成“统一需求调度”，让生产者只提交需求，由调度器按时间窗口与内容长度窗口统一组批执行。

## Context and Scope

本主题拥有统一翻译调度器、可恢复失败的尝试审计，以及任务中心内按采集记录查询内容处理状态的管理界面。它覆盖 Release、公告和日报的记录读取与审计展示，不把日报生成纳入翻译调度器。

## Requirements

- REQ-SCHEDULER: 调度器必须把翻译或润色请求、work item、批次和结果扇出保持为独立领域模型，生产者只提交需求而不拥有组批策略；Release 润色只能由调度器执行和写入终态缓存。
- REQ-ATTEMPT-AUDIT: 每次初始执行、自动恢复、手动重试及其状态转换必须在同一事务追加元数据专用的尝试事件；已到期并重新入队时，当前尝试状态必须呈现为 `queued` 而非保留过期的重试安排；事件不得保存源文本、prompt、原始模型响应或原始上游错误。每个尝试必须记录处理阶段、稳定错误代码、安全摘要、重试处置以及精确的模型调用归因链接。
- REQ-COLLECTION-RECORDS: 管理端必须按 Release、公告、日报分组、分页并以最近 24 小时为默认时间范围展示采集记录；筛选和排序使用各类型来源时间，历史空发现时间仍保留并显示为“未知”；行数据必须包含该类型的基础区分信息、发现时间和翻译或润色任务摘要，并支持在计数和分页前按记录级总尝试次数筛选。
- REQ-RECORD-DETAIL: 桌面端必须在抽屉展示记录详情与尝试历史，移动端必须导航至详情路由；详情必须暴露处理阶段、模型调用与输出契约的独立结果、错误分类、重试信息和可用下钻链接。诊断载荷过期时必须明确呈现为过期，不得显示为未关联模型调用。

## Verification

- VER-RUST-SCHEDULER: 覆盖: REQ-SCHEDULER, REQ-ATTEMPT-AUDIT。通过 Rust 单元与集成测试验证队列、单一 Release 润色所有者、批次、自动恢复、手动重试、精确调用归因和追加式事件写入。
- VER-ADMIN-API: 覆盖: REQ-ATTEMPT-AUDIT, REQ-COLLECTION-RECORDS, REQ-RECORD-DETAIL。通过管理员 API 测试验证时间筛选、分页、任务摘要、尝试历史、安全错误字段与诊断载荷过期语义。
- VER-WEB-ADMIN: 覆盖: REQ-COLLECTION-RECORDS, REQ-RECORD-DETAIL。通过 Web 构建、Playwright 回归与 Storybook canvas 视觉证据验证分组列表、桌面抽屉、移动详情路由、模型调用与输出契约的双状态，以及尝试列表直接可见的模型、错误和重入队状态。

## 目标 / 非目标

### Goals

- 新建统一翻译调度器，接收单条或多条翻译需求并按 `max_wait_ms` + token 窗口统一组批。
- 翻译工作从 `job_tasks` 域中剥离，形成独立的 requests / work items / batches 领域模型；request 单条结果契约由后续 `translation-request-single-record` 继续收口。
- 提供统一生产者 API，支持 `async` / `wait` / `stream` 三种交付方式。
- 支持同一批次内混合 `release_summary`、`release_detail`、`notification`，并与后续单-request 合同兼容。
- 在 `/admin/jobs` 新增“翻译调度”标签页，提供调度状态、请求视图、批次视图与 LLM 调用追链。
- 管理员可按发布记录查询追加式尝试事件，审计首次执行、自动恢复、手动重试及其关联批次和 LLM 调用。
- 管理员可在“内容处理”中按 Release、公告和日报查看采集记录；列表按时间筛选并分页，详情提供对应翻译或润色的尝试历史与下钻链接。
- Release 润色的所有生产者只提交调度请求，调度器是唯一可执行模型调用或写入终态润色缓存的所有者。
- 停止为新的翻译工作创建 `translate.release*` / `translate.notification` 类 `job_tasks`。

### Non-goals

- 不把日报生成等非翻译类 LLM 任务并入本调度器。
- 不实现多实例分布式抢占与跨实例协调。
- 不做历史翻译任务数据回填。
- 不实现人工重跑 / 重放动作。

## Related ADRs

- [ADR 0001: LLM Recovery Boundary](../../adr/0001-llm-recovery-boundary.md)
- [ADR 0003: Collection Record Time and Attempt Filtering](../../adr/0003-collection-record-time-and-attempt-filtering.md)
- [ADR 0004: AI Diagnostics Evidence Boundary](../../adr/0004-ai-diagnostics-evidence-boundary.md)

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） |
| --- | --- | --- | --- | --- | --- | --- |
| `POST /api/translate/requests` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-feed / web-sidebar / future producers |
| `GET /api/translate/requests/{request_id}` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web producers |
| `GET /api/translate/requests/{request_id}/stream` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web producers |
| `/api/admin/jobs/translations/*` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-admin |
| `/api/admin/jobs/ai-records/{record_kind}/*` | HTTP API | external | New | `./contracts/http-apis.md` | backend | web-admin |
| `translation_requests` / `translation_work_items` / `translation_batches` / `translation_batch_items` / `translation_attempt_events` | DB schema | internal | New | `./contracts/db.md` | backend | backend |
| `llm_calls` delivery metadata and translation parent link | DB schema | internal | Modify | `./contracts/db.md` | backend | backend / web-admin |
| `translation_attempt_llm_calls` | DB schema | internal | New | `./contracts/db.md` | backend | backend / web-admin |

### 契约文档（按 Kind 拆分）

- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/db.md](./contracts/db.md)

## 验收标准（Acceptance Criteria）

- Given 生产者提交单条或多条翻译需求
  When 请求进入统一调度器
  Then 服务端创建独立 `translation_request`，并将每个 item 归并到去重 `work_item` 或缓存结果。

- Given 多个请求命中同一 `scope_user_id + kind + variant + entity_id + target_lang + source_hash`
  When 调度器尚未完成翻译
  Then 多个请求共享同一 `work_item`，批次完成后向所有关联 request 扇出相同结果。

- Given 队列累计 token 达到阈值或最早 `deadline_at` 到达
  When 调度器扫描工作项
  Then 创建 `translation_batch` 并将符合条件的 work items 一次封批执行。

- Given 请求使用 `wait`
  When `item.max_wait_ms` 预算耗尽且所属 work item 仍未终态
  Then 请求方收到当前 request 的单结果快照，`result.status` 允许保持 `queued` 或 `running`，而不是继续同步阻塞到批次终态。

- Given 翻译或润色 work item 遇到可恢复的空内容、限流或瞬态上游错误
  When 错误被判定为结构化可恢复失败
  Then 调度器必须先持久化真实 `error` 状态、失败分类、尝试历史与下一次恢复时间；到期恢复器再将 request 与 work item 重新置为 `queued`。

- Given 先前失败的 work item 已到达自动恢复时间
  When 恢复器将它重新入队
  Then 该失败尝试的重试安排仍保留为历史事件，而管理员读取的当前尝试状态显示 `queued` 与自动恢复触发方式。

- Given 一个发布记录对应的 work item 被首次执行、手动重试或自动恢复
  When 管理员按 `entity_id` 查询尝试审计
  Then 返回按时间追加的入队、开始、完成与重试安排事件；每条事件包含尝试序号、触发方式、处理阶段、结果或失败分类、是否可重试、下次重试时间和精确的 request、batch、LLM 调用归因。

- Given 尝试审计长期保留
  When 管理员读取某条发布记录的历史
  Then 审计表只保存元数据和安全错误摘要，不保存翻译源文本、prompt 或模型原始响应；审计功能启用前的历史记录不做回填。

- Given 管理员打开“内容处理”
  When 选择 Release、公告或日报并保留默认时间范围
  Then 分页列表按 Release 来源时间 `COALESCE(published_at, created_at, updated_at)`、公告聚合 `occurred_at` 或日报 `created_at` 显示最近 24 小时内的该类采集记录；即使 `detected_at` 为空也不得遗漏，并将其显示为“未知”；每行展示该类型需要区分记录的基础信息、发现时间，以及翻译或润色的重试次数、开始、上次尝试和完成时间。

- Given 管理员设置总尝试次数范围
  When 范围为 `0..10` 的闭区间或省略上限
  Then 服务端按适用处理链路的最大总尝试次数过滤记录；无任务或调用的记录为 `0` 次，范围在总数、排序和分页前生效，越界或上限小于下限返回 `400`。

- Given 管理员打开一条采集记录详情
  When 在桌面端选择列表行或在移动端进入详情路由
  Then 展示该记录对应的尝试历史、模型调用结果、输出契约结果、错误分类和可用的下钻链接；桌面端使用抽屉，移动端不复用桌面抽屉。

- Given 模型返回内容但 Release 润色输出未通过 JSON 或 schema 校验
  When 内容处理尝试完成
  Then 模型调用记录 provider 成功，尝试记录输出契约失败与稳定错误代码；二者不得被合并为泛化的“翻译失败”。

- Given 批次包含多个 work item 或润色结果复用了已有证据
  When 管理员查看某个处理尝试
  Then 只显示实际参与该尝试的模型调用及其阶段和关系，不得把同批其他条目的调用推断为该尝试的调用。

- Given 关联的 LLM 调用已超过诊断载荷保留期
  When 管理员查看处理尝试
  Then 保留调用归因和处理阶段，并明确标记诊断内容已过期。

- Given 历史记录没有结构化失败分类
  When 恢复器扫描到该记录
  Then 不得自动补跑，只允许受控人工重试。

- Given 批次执行器完成但存在失败或缺失 item
  When 管理端计算业务结果
  Then 结果必须为 `partial`，不能因为执行器返回成功而覆盖业务失败。

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

- Rust tests：请求去重、缓存命中、deadline flush、token flush、批次扇出、批次终态返回、自动/手动重试审计、管理员 API 聚合。
- Web tests：Feed 自动翻译、Release Detail 翻译、管理员“翻译调度”标签页与发布记录重试审计。
- Playwright：producer `wait`/`stream` 行为与管理员视图回归，包括内容处理记录的重入队状态、模型、错误和下钻。

### Quality checks

- `cargo test`
- `cd web && bun run build`
- `cd web && bun run e2e -- release-detail.spec.ts admin-jobs.spec.ts`
