# PostgreSQL 全局内容处理运行时

> This file is the durable topic requirements contract. Current implementation facts belong in `IMPLEMENTATION.md`; lifecycle and change references belong in `HISTORY.md`.

## Context and Scope

- Context: 全局翻译与润色流水线（`content_processing`）目前只有 SQLite 写入实现。PostgreSQL 运行时切换后，读路径已经从 `content_current_result_projections` 取内容，但没有任何生产者继续写入工作项、尝试事件和结果投影，导致切换后的内容面停更。
- In scope: PostgreSQL 原生的全局内容 admission、claim/lease、provider 调用、输出契约、结果投影发布、失败恢复、调度接线与自动生产入口；覆盖 Release 翻译、Release 润色/阅读、公告与通知的翻译/润色。Release 与公告读侧沿用 `postgres_content_overlay` 的投影；通知当前走翻译请求/读取链路而不是该 overlay，其读取适配属于本主题范围。
- Out of scope: 修改既有 prompt 与输出字段契约；日报生成；重新设计管理端读模型；SQLite 历史数据回填。
- 删除退役 SQLite 运行时代码、schema 收敛与共用核心抽取由 [`../sqlite-runtime-retirement/SPEC.md`](../sqlite-runtime-retirement/SPEC.md) 拥有；本主题只拥有功能等价与读侧真实状态。

## Terms and Interfaces

- `全局工作项`（work item）：按 `(canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version)` 唯一的持久工作身份。
- `结果投影`（projection）：通过输出契约校验后发布、供读路径消费的内容快照。
- `admission`：创建或复用全局工作项、登记请求者关联、判定配置与缓存命中的事务边界。
- `attempt`：一次 provider 调用及其 `attempt_*` 事件、provider admission 事实和 LLM 调用审计。
- Interface: PostgreSQL 任务执行器与调度 lane；`content_processing_control`；`content_current_result_projections`；`postgres_content_overlay` 读路径。

## Requirements

### REQ-PGCP-001

- The system MUST run the global content-processing producer and worker on the PostgreSQL runtime and MUST NOT require a SQLite writer for new work.
- Inputs: `DATABASE_URL` 指向 PostgreSQL 的启动配置与已配置 AI provider。
- Outputs: 启动后存在可认领、可执行、可发布投影的 PostgreSQL 内容处理通道。

### REQ-PGCP-002

- Admission MUST create or reuse one global work item per canonical identity, register requester links, honor cache hits without provider calls, and supersede older work for the same resource and pipeline.
- Inputs: 用户请求、自动生产者（Release 刷新、公告、通知）、冻结源快照与源修订。
- Outputs: `content_work_items`、`content_request_links`、`content_work_admission_events` 与稳定的请求状态响应。
- Inherits `REQ-GTP-AUTHORIZATION` and `REQ-GTP-RESULTS` unchanged from [`../global-translation-and-polish/SPEC.md`](../global-translation-and-polish/SPEC.md).

### REQ-PGCP-003

- Each attempt MUST persist `attempt_queued`/`attempt_started`/`attempt_completed` facts, a provider-admission fact before every provider request, and an attributable `llm_calls` audit row; only outputs passing the pipeline output contract may be published.
- Inputs: 已认领工作项、尝试级配置与路由快照、provider 响应。
- Outputs: 尝试事件、`content_attempt_llm_calls`、`content_attempt_provider_admissions`、`content_result_projections` 与 `content_current_result_projections`。
- The output contract and admission facts inherit `REQ-GTP-OUTPUT`, `REQ-GTP-CACHE-HIT` and `REQ-GTP-ADMISSION` unchanged; see [ADR 0014](../../adr/0014-global-content-output-contract.md) and [ADR 0015](../../adr/0015-content-work-admission-and-supersession.md).

### REQ-PGCP-004

- Recoverable failures MUST follow the existing classification and backoff policy; `configuration` and exhausted items MUST NOT auto-retry; provider breaker state MUST defer queued work instead of calling the provider.
- Inputs: 失败分类、尝试次数、`llm_model_health` 与持久化运行配置。
- Outputs: `failed`/`blocked_config`/`deferred_provider` 状态与到期恢复。
- Backoff and lifecycle inherit `REQ-GTP-LIFECYCLE`, `REQ-GTP-CONFIGURATION` and `REQ-GTP-PROVIDER-GUARD` unchanged.

### REQ-PGCP-005

- The runtime MUST cover Release detail translation, Release polishing/reading, announcement translation/polishing, and notification translation/polishing.
- Inputs: 现有源快照与 `pipeline`/`variant` 取值。
- Outputs: Release 与公告使用 `postgres_content_overlay` 已存在的 pipeline/variant 名称，读侧无需新分支；通知 MUST 通过其翻译请求/读取链路返回新结果，必要的读侧适配记录在实现内。

### REQ-PGCP-006

- Exactly one writer MUST own global content projections after cutover; the runtime MUST NOT double-write SQLite and PostgreSQL, and legacy `translation.generate` auto-enqueue MUST be retired only after the covered pipelines publish equivalents.
- Inputs: `content_processing_control` 模式与部署迁移。
- Outputs: 单一写入者、无重复投影、可审计的回滚边界。
- This requirement amends [`REQ-PGR-009`](../postgresql-runtime/SPEC.md) for the covered pipelines: after equivalence is proven, `translation.generate` auto-enqueue is retired instead of continuing beside the global runtime. Until then both paths remain, and no pipeline may double-publish a projection.

### REQ-PGCP-007

- Admin reads MUST reflect actual work, attempts, provider admissions and projections without synthesizing work items or cache-hit states; LLM 任务页与采集记录页 MUST show post-cutover activity.
- Inputs: 既有管理端读接口。
- Outputs: 与数据库事实一致的状态、来源与失败分类。

### REQ-PGCP-008

- Admission, attempt and publish MUST be idempotent under retries and concurrent workers; one failing item MUST NOT abort unrelated items in the same batch.
- Inputs: 重复调度、重试、并发认领与重复源修订。
- Outputs: 唯一约束与事务保证下的幂等写入、批次隔离。

### REQ-PGCP-009

- The runtime MUST expose stable logs and metrics for admission, claim, attempt outcome and projection publish so an owner can verify progress without database access.
- Inputs: 每次 admission/claim/attempt/publish。
- Outputs: 结构化事件与计数指标。

### REQ-PGCP-010

- 当翻译尚未产生全局工作或结果投影、而按用户隔离的 `translation_work_items` 已有终态行时，管理读 MUST 显示该行的真实状态与可显示输出，MUST NOT 合成为 `legacy_conflict`，MUST NOT 报告未知的尝试历史。
- 只有存在可追溯 observation 且证据确实无法调和时，MUST 继续使用 `legacy_cached` / `legacy_conflict`。
- 活动图的翻译 lane MUST 因此反映当前状态：已完成计入完成，仍在跑计入处理中，缺失才计入中性。
- 读取 MUST 保持纯只读，不补覆盖、不创建工作项、不写缓存。
- Inherits `REQ-GTP-ADMIN-READS` and `REQ-GTP-ADMIN-ACTIVITY` unchanged.
- Inputs: 源记录、`translation_work_items` 终态行、可选 observation。
- Outputs: 与事实一致的翻译状态、尝试次数与综合状态。

### REQ-PGCP-011

- 翻译完成后 MUST 发布结果投影；公开详情与已授权读取 MUST 按既有 audience/variant 规则命中翻译结果，MUST NOT 返回 `translated.status=missing`。
- 润色 MUST 发布对应受众变体（`smart_public` / `smart_authorized`）；公开读取 MUST NOT 依赖 legacy `smart` 变体。
- 翻译尚在旧入队路径期间，旧路径 MUST 只作为 producer；终态仍 MUST 由全局投影表达。
- Inputs: 已完成的翻译输出与 `source_hash`。
- Outputs: `content_result_projections`、`content_current_result_projections` 与可读的公开翻译。

## Verification

### VER-PGCP-001

- Method: 在隔离 PostgreSQL 18 上启动运行时，创建一个新 Release 并观察工作项、尝试事件、LLM 调用与投影。
- covers: `REQ-PGCP-001`, `REQ-PGCP-003`, `REQ-GTP-OUTPUT`
- Pass condition: 无 SQLite 依赖，工作项完成并发布与源哈希一致的投影。

### VER-PGCP-002

- Method: 多用户并发请求同一 Release 的同一 pipeline，另一个用户请求不同 pipeline。
- covers: `REQ-PGCP-002`, `REQ-GTP-RESULTS`, `REQ-GTP-AUTHORIZATION`, `REQ-GTP-CACHE-HIT`, `REQ-GTP-ADMISSION`
- Pass condition: 同一身份只有一个工作项与一次 provider 调用，请求者关联完整，缓存命中不调用 provider。

### VER-PGCP-003

- Method: 构造可恢复失败、`configuration` 失败、provider 熔断与重复重试。
- covers: `REQ-PGCP-004`, `REQ-GTP-LIFECYCLE`, `REQ-GTP-PROVIDER-GUARD`, `REQ-GTP-CONFIGURATION`
- Pass condition: 退避与次数上限正确，`blocked_config` 不进入终态失败，熔断期间不调用 provider。

### VER-PGCP-004

- Method: 对 Release 翻译、Release 润色/阅读、公告与通知各执行一条真实链路并读取既有读接口。
- covers: `REQ-PGCP-005`
- Pass condition: Release 与公告读路径返回新投影且无需新增分支；通知通过其翻译请求/读取链路返回新结果（允许本主题新增读侧适配）。

### VER-PGCP-005

- Method: 并发认领、重复 admission、重复源修订与批量内单条失败。
- covers: `REQ-PGCP-008`
- Pass condition: 无重复投影与重复请求，其他条目继续完成。

### VER-PGCP-006

- Method: 检查管理端 LLM 任务、采集记录与活动统计。
- covers: `REQ-PGCP-007`, `REQ-PGCP-009`
- Pass condition: 切换后产生新记录，来源与失败分类来自数据库事实，事件与指标可观察。

### VER-PGCP-007

- Method: 在切换窗口内同时观察 SQLite 写入者状态、PostgreSQL 投影发布与自动入队来源。
- covers: `REQ-PGCP-006`, `REQ-PGR-009`
- Pass condition: 同一身份只有 PostgreSQL 写入者发布投影，legacy `translation.generate` 自动入队在覆盖后停止且没有双写。

### VER-PGCP-008

- Method: 在只有按用户翻译工作行、没有全局工作与 observation 的 Release 上读取管理与活动接口。
- covers: `REQ-PGCP-010`, `REQ-GTP-ADMIN-READS`, `REQ-GTP-ADMIN-ACTIVITY`
- Pass condition: 管理端显示真实终态与尝试次数，不是 `legacy_conflict`；活动 cell 为完成；有可追溯冲突 observation 的旧行仍显示 `legacy_conflict`。

### VER-PGCP-009

- Method: 对刚完成翻译的 Release 读取公开详情与已授权详情。
- covers: `REQ-PGCP-011`, `REQ-GTP-RESULTS`
- Pass condition: `translated.status=ready` 且标题/摘要与投影一致；无全局投影时不再返回 `missing`；投影源哈希与身份一致。

## Related ADRs

- [ADR 0007: 全局翻译与润色工作模型](../../adr/0007-global-translation-and-polish-work-model.md)
- [ADR 0012: 模型无关的全局内容工作身份](../../adr/0012-model-independent-content-work.md)
- [ADR 0014: Global Content Output Contract Recovery](../../adr/0014-global-content-output-contract.md)
- [ADR 0015: Content Work Admission and Supersession](../../adr/0015-content-work-admission-and-supersession.md)
- [ADR 0024: PostgreSQL 原生全局内容处理运行时](../../adr/0024-postgres-content-processing-runtime.md)

## Visual Evidence

- None

## References

- `./IMPLEMENTATION.md`
- `./HISTORY.md`
- [`../global-translation-and-polish/SPEC.md`](../global-translation-and-polish/SPEC.md)
- [`../postgresql-runtime/SPEC.md`](../postgresql-runtime/SPEC.md)
- [`../sqlite-runtime-retirement/SPEC.md`](../sqlite-runtime-retirement/SPEC.md)
