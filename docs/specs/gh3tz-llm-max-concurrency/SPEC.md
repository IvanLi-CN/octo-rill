# 可配置 LLM 并行调度（取消固定限流）（#gh3tz）

## 背景 / 问题陈述

- 当前进程内 LLM 调度器采用“串行发放 + 固定 1 秒节流”，没有一个可配置的最大并行数入口。
- 任务 worker 已支持并发处理，但多个任务命中 LLM 时仍会被固定节流串行化，吞吐受限。
- 管理员页面展示的是节流槽位视角，无法直接反映“允许多少并行、当前占用多少并行”的运行状态。

## 目标 / 非目标

### Goals
- 新增环境变量 `AI_MAX_CONCURRENCY`，以正整数控制单进程内 LLM 最大并行数，未配置时默认 `1`。
- 移除固定 1Hz 发放节流，改为仅按 permit / semaphore 控制最大同时在途请求数。
- 保留 `scheduler_wait_ms`、`waiting_calls`、`in_flight_calls` 观测能力，但语义改为“等待 permit 的排队耗时 / 数量”。
- 管理员 `/admin/jobs` 中的 LLM 调度页切换到并发语义：保留 24h 聚合摘要，移除独立并发状态卡片，并确保主 LLM 列表按状态分组（进行中 / 排队中 / 终止态）与创建时间倒序展示；任务详情里的关联 LLM 调用继续按创建时间倒序展示。
- 同步更新 README、`.env.example`、docs-site 配置文档与前端 mock / e2e 契约。

### Non-goals
- 不新增单独的 QPS / 节流环境变量。
- 不做多实例共享并发预算或分布式锁。
- 不修改 `llm_calls` 表结构，也不回填历史调度数据。

## 范围（Scope）

### In scope
- `src/config.rs` 的 `AI_MAX_CONCURRENCY` 解析、默认值与错误文案。
- `src/state.rs` / `src/server.rs` / `src/ai.rs` 的进程内 permit 调度 runtime。
- `src/api.rs` 的管理员 LLM 调度状态返回结构。
- `web/src/api.ts`、`web/src/admin/JobManagement.tsx`、Storybook mock、Playwright fixture 与断言。
- `README.md`、`.env.example`、`docs-site/docs/config.md`、`docs-site/docs/quick-start.md`。

### Out of scope
- 其它非 LLM 调度器（例如 translation worker board）的并发策略调整。
- 新增管理员写操作去在线调整并发上限。
- 新增独立 QPS / 节流配置，或做跨实例共享限流。

## 功能与行为规格（Functional / Behavior Spec）

### 配置与启动
- `AI_MAX_CONCURRENCY` 必须解析为正整数；空值视为未配置并回退到默认值 `1`。
- 若 `AI_MAX_CONCURRENCY` 为 `0`、负数、非整数或超过 Tokio semaphore 允许的最大 permits，启动直接失败，错误信息必须明确指向该变量。
- 即使未配置 `AI_API_KEY`，配置解析也保持可预测：`AI_MAX_CONCURRENCY` 仍采用默认值 `1` 或在非法值时给出明确错误。

### 调度 runtime
- `chat_completion` 在每次尝试真正发起上游请求前必须先获取 permit；permit 获取前的排队时间累计到 `scheduler_wait_ms`。
- 任意时刻单进程内在途 LLM 请求数不得超过 `AI_MAX_CONCURRENCY`。
- permit 在每次 attempt 完成后立即释放；重试 attempt 需要重新排队获取 permit。
- 若某次 attempt 因 `429` / `5xx` 等 retryable 错误进入退避等待，调用状态必须在退避窗口回写为 `queued`，避免管理页把已释放 permit 的调用误显示为 `running`。
- 已释放 permit 但数据库状态尚未落盘的瞬态必须通过短暂的观测状态覆盖来吸收，不能让管理页查询反向阻塞真实 LLM permit 释放。
- LLM 调用列表的“进行中 / 排队中 / 终止态”排序必须保持可索引实现，避免管理页热路径退化为整表扫描。
- 覆盖中的 `started_at` / `status` 瞬态必须参与管理页筛选与总数计算，不能因为数据库尚未落盘而把运行中的调用从时间过滤结果里漏掉。
- 取消固定 1 秒发放间隔后，不再维护“下一次槽位时间”概念。

### 管理员观测
- `GET /api/admin/jobs/llm/status` 改为返回并发语义字段：`max_concurrency`、`available_slots`、`waiting_calls`、`in_flight_calls`，并继续保留 24h 聚合指标。
- 管理员 LLM 状态 / 列表 / 详情接口在 retry / finalize 状态切换期间不得暴露“permit 已释放但调用仍显示为 `running`”的半完成快照；管理端要么看到切换前，要么看到切换后，但这种一致性不能通过阻塞 permit 释放来换取。
- `GET /api/admin/jobs/llm/calls` 默认按 `created_at DESC, id DESC` 返回；主 LLM 列表需要状态分组排序时必须显式声明排序模式，避免影响任务详情里的关联调用时序。
- `/admin/jobs` 的 LLM 调度页不再展示固定节流文案，也不再保留独立的并发状态卡片；并发字段仍由状态接口提供给管理端与测试契约。
- 前端 mock / 测试 fixture / e2e 断言必须与新接口字段保持一致。

## 接口契约（Interfaces & Contracts）

- `docs/specs/gh3tz-llm-max-concurrency/contracts/http-apis.md`

## 验收标准（Acceptance Criteria）

1. Given 未设置 `AI_MAX_CONCURRENCY`，When 应用启动并发起 LLM 请求，Then 单进程最大并行数按 `1` 生效。
2. Given `AI_MAX_CONCURRENCY=3`，When 4 个 LLM 调用同时进入调度，Then 最多只有 3 个请求同时在途，剩余调用仅因 permit 不足而等待。
3. Given `AI_MAX_CONCURRENCY=0`、`AI_MAX_CONCURRENCY=abc` 或 `AI_MAX_CONCURRENCY` 超过 Tokio semaphore permits 上限，When 应用启动，Then 启动失败且错误信息明确包含 `AI_MAX_CONCURRENCY`。
4. Given 管理员打开 `/admin/jobs` 的 `LLM 调度` 标签，When 状态接口返回并发数据，Then 页面仅保留 24h 聚合摘要，不再出现 `request_interval_ms` / `next_slot_in_ms` 节流语义，也不再展示独立的并发状态卡片。
5. Given LLM 调用列表同时包含进行中、排队中与终止态记录，When 管理员查看 `/admin/jobs` 的 `LLM 调度` 标签，Then 主 LLM 列表按“进行中 -> 排队中 -> 终止态”排序，且每组内按 `created_at` 倒序排列。
6. Given LLM 调用某次 attempt 因 retryable 错误进入退避等待，When 管理员查看 `/admin/jobs` 的 `LLM 调度` 标签，Then 该调用在退避窗口显示为 `queued`，且不计入实时 `in_flight_calls`。
7. Given LLM 调用因 permit 等待后成功或失败，When 管理员查看调用详情，Then `scheduler_wait_ms` 仍记录排队耗时。
8. Given 管理员从任务详情查看关联 LLM 调用，When 这些调用同时包含运行中与终止态记录，Then 关联调用列表仍按 `created_at DESC, id DESC` 展示，而不是被主 LLM 列表的状态分组排序改写。

## 非功能性验收 / 质量门槛（Quality Gates）

- `cargo test`
- `cd web && bun run build`
- `cd web && bun run e2e -- admin-jobs.spec.ts`

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 假设：主人明确接受“仅保留 24h 聚合摘要与调用列表，不在页面展示独立并发状态卡片”。
- 假设：默认值维持 `1`，以避免旧部署在未改配置时突然放大量。
- 风险：取消固定节流后，上游供应商的真实速率限制仍可能暴露；当前仅保留单次请求失败后的 `Retry-After` / 指数退避重试，不提供独立 QPS 保护。
- 风险：管理员页面与 Storybook / e2e fixture 若未完全同步，会出现类型或断言漂移。
