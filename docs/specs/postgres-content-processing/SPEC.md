# PostgreSQL 全局内容处理运行时

> This file is the durable topic requirements contract. Current implementation facts belong in `IMPLEMENTATION.md`; lifecycle and change references belong in `HISTORY.md`.

## Context and Scope

- Context: 全局翻译与润色流水线（`content_processing`）目前只有 SQLite 写入实现。PostgreSQL 运行时切换后，读路径已经从 `content_current_result_projections` 取内容，但没有任何生产者继续写入工作项、尝试事件和结果投影，导致切换后的内容面停更。
- In scope: PostgreSQL 原生的全局内容 admission、claim/lease、provider 调用、输出契约、结果投影发布、失败恢复、调度接线与自动生产入口；覆盖 Release 翻译、Release 润色/阅读、公告与通知的翻译/润色。
- Out of scope: 修改既有 prompt 与输出字段契约；日报生成；重新设计管理端读模型；SQLite 历史数据回填。

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

### REQ-PGCP-003

- Each attempt MUST persist `attempt_queued`/`attempt_started`/`attempt_completed` facts, a provider-admission fact before every provider request, and an attributable `llm_calls` audit row; only outputs passing the pipeline output contract may be published.
- Inputs: 已认领工作项、尝试级配置与路由快照、provider 响应。
- Outputs: 尝试事件、`content_attempt_llm_calls`、`content_attempt_provider_admissions`、`content_result_projections` 与 `content_current_result_projections`。

### REQ-PGCP-004

- Recoverable failures MUST follow the existing classification and backoff policy; `configuration` and exhausted items MUST NOT auto-retry; provider breaker state MUST defer queued work instead of calling the provider.
- Inputs: 失败分类、尝试次数、`llm_model_health` 与持久化运行配置。
- Outputs: `failed`/`blocked_config`/`deferred_provider` 状态与到期恢复。

### REQ-PGCP-005

- The runtime MUST cover Release detail translation, Release polishing/reading, announcement translation/polishing, and notification translation/polishing with the pipeline/variant names the read overlay already expects.
- Inputs: 现有源快照与 `pipeline`/`variant` 取值。
- Outputs: 与 `postgres_content_overlay` 兼容的投影，使既有读路径无需新分支。

### REQ-PGCP-006

- Exactly one writer MUST own global content projections after cutover; the runtime MUST NOT double-write SQLite and PostgreSQL, and legacy `translation.generate` auto-enqueue MUST be retired only after the covered pipelines publish equivalents.
- Inputs: `content_processing_control` 模式与部署迁移。
- Outputs: 单一写入者、无重复投影、可审计的回滚边界。

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

## Verification

### VER-PGCP-001

- Method: 在隔离 PostgreSQL 18 上启动运行时，创建一个新 Release 并观察工作项、尝试事件、LLM 调用与投影。
- covers: `REQ-PGCP-001`, `REQ-PGCP-003`
- Pass condition: 无 SQLite 依赖，工作项完成并发布与源哈希一致的投影。

### VER-PGCP-002

- Method: 多用户并发请求同一 Release 的同一 pipeline，另一个用户请求不同 pipeline。
- covers: `REQ-PGCP-002`
- Pass condition: 同一身份只有一个工作项与一次 provider 调用，请求者关联完整，缓存命中不调用 provider。

### VER-PGCP-003

- Method: 构造可恢复失败、`configuration` 失败、provider 熔断与重复重试。
- covers: `REQ-PGCP-004`
- Pass condition: 退避与次数上限正确，`blocked_config` 不进入终态失败，熔断期间不调用 provider。

### VER-PGCP-004

- Method: 对 Release 翻译、Release 润色/阅读、公告与通知各执行一条真实链路并读取既有读接口。
- covers: `REQ-PGCP-005`
- Pass condition: 四类读路径返回新投影，无需新增读分支。

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
- covers: `REQ-PGCP-006`
- Pass condition: 同一身份只有 PostgreSQL 写入者发布投影，legacy `translation.generate` 自动入队在覆盖后停止且没有双写。

## Related ADRs

- [ADR 0007: 全局翻译与润色工作模型](../../adr/0007-global-translation-and-polish-work-model.md)
- [ADR 0012: 模型无关的全局内容工作身份](../../adr/0012-model-independent-content-work.md)
- [ADR 0024: PostgreSQL 原生全局内容处理运行时](../../adr/0024-postgres-content-processing-runtime.md)

## Visual Evidence

- None

## References

- `./IMPLEMENTATION.md`
- `./HISTORY.md`
