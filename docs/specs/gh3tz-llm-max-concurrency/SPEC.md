# 可配置 LLM 并行调度（取消固定限流）（#gh3tz）

## 状态
- 当前状态：已完成
- Spec 目录：`docs/specs/gh3tz-llm-max-concurrency/`
- 负责人：Codex

## 背景 / 问题陈述
- 当前进程内 LLM 调度器采用“串行发放 + 固定 1 秒节流”，没有一个可配置的最大并行数入口。
- 任务 worker 已支持并发处理，但多个任务命中 LLM 时仍会被固定节流串行化，吞吐受限。
- 管理员页面展示的是节流槽位视角，无法直接反映“允许多少并行、当前占用多少并行”的运行状态。

## 目标 / 非目标
### Goals
- 新增环境变量 `AI_MAX_CONCURRENCY`，以正整数控制单进程内 LLM 最大并行数，未配置时默认 `1`。
- 移除固定 1Hz 发放节流，改为仅按 permit / semaphore 控制最大同时在途请求数。
- 保留 `scheduler_wait_ms`、`waiting_calls`、`in_flight_calls` 观测能力，但语义改为“等待 permit 的排队耗时 / 数量”。
- 管理员 `/admin/jobs` 中的 LLM 调度页切换到并发语义：保留 24h 聚合摘要，移除独立并发状态卡片，并确保调用列表按状态分组（进行中 / 排队中 / 终止态）与创建时间倒序展示。
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
- 若 `AI_MAX_CONCURRENCY` 为 `0`、负数或非整数，启动直接失败，错误信息必须明确指向该变量。
- 即使未配置 `AI_API_KEY`，配置解析也保持可预测：`AI_MAX_CONCURRENCY` 仍采用默认值 `1` 或在非法值时给出明确错误。

### 调度 runtime
- `chat_completion` 在每次尝试真正发起上游请求前必须先获取 permit；permit 获取前的排队时间累计到 `scheduler_wait_ms`。
- 任意时刻单进程内在途 LLM 请求数不得超过 `AI_MAX_CONCURRENCY`。
- permit 在每次 attempt 完成后立即释放；重试 attempt 需要重新排队获取 permit。
- 若某次 attempt 因 `429` / `5xx` 等 retryable 错误进入退避等待，调用状态必须在退避窗口回写为 `queued`，避免管理页把已释放 permit 的调用误显示为 `running`。
- LLM 调用列表的“进行中 / 排队中 / 终止态”排序必须保持可索引实现，避免管理页热路径退化为整表扫描。
- 取消固定 1 秒发放间隔后，不再维护“下一次槽位时间”概念。

### 管理员观测
- `GET /api/admin/jobs/llm/status` 改为返回并发语义字段：`max_concurrency`、`available_slots`、`waiting_calls`、`in_flight_calls`，并继续保留 24h 聚合指标。
- `/admin/jobs` 的 LLM 调度页不再展示固定节流文案，也不再保留独立的并发状态卡片；并发字段仍由状态接口提供给管理端与测试契约。
- 前端 mock / 测试 fixture / e2e 断言必须与新接口字段保持一致。

## 接口契约（Interfaces & Contracts）
- `docs/specs/gh3tz-llm-max-concurrency/contracts/http-apis.md`

## 验收标准（Acceptance Criteria）
1. Given 未设置 `AI_MAX_CONCURRENCY`，When 应用启动并发起 LLM 请求，Then 单进程最大并行数按 `1` 生效。
2. Given `AI_MAX_CONCURRENCY=3`，When 4 个 LLM 调用同时进入调度，Then 最多只有 3 个请求同时在途，剩余调用仅因 permit 不足而等待。
3. Given `AI_MAX_CONCURRENCY=0` 或 `AI_MAX_CONCURRENCY=abc`，When 应用启动，Then 启动失败且错误信息明确包含 `AI_MAX_CONCURRENCY`。
4. Given 管理员打开 `/admin/jobs` 的 `LLM 调度` 标签，When 状态接口返回并发数据，Then 页面仅保留 24h 聚合摘要，不再出现 `request_interval_ms` / `next_slot_in_ms` 节流语义，也不再展示独立的并发状态卡片。
5. Given LLM 调用列表同时包含进行中、排队中与终止态记录，When 管理员查看 `/admin/jobs` 的 `LLM 调度` 标签，Then 列表按“进行中 -> 排队中 -> 终止态”排序，且每组内按 `created_at` 倒序排列。
6. Given LLM 调用某次 attempt 因 retryable 错误进入退避等待，When 管理员查看 `/admin/jobs` 的 `LLM 调度` 标签，Then 该调用在退避窗口显示为 `queued`，且不计入实时 `in_flight_calls`。
7. Given LLM 调用因 permit 等待后成功或失败，When 管理员查看调用详情，Then `scheduler_wait_ms` 仍记录排队耗时。

## 非功能性验收 / 质量门槛（Quality Gates）
- `cargo test`
- `cd web && bun run build`
- `cd web && bun run e2e -- admin-jobs.spec.ts`

## 文档更新（Docs to Update）
- `README.md`
- `.env.example`
- `docs-site/docs/config.md`
- `docs-site/docs/quick-start.md`
- `docs/specs/README.md`

## 实现里程碑（Milestones / Delivery checklist）
- [x] M1: `AI_MAX_CONCURRENCY` 配置解析与 runtime semaphore 落地
- [x] M2: 管理员 LLM 状态 API / UI / mock / e2e 切换到并发语义，并同步新的列表排序/摘要展示口径
- [x] M3: README / `.env.example` / docs-site 配置说明同步
- [x] M4: PR、checks、review-loop 与 spec-sync 收敛完成

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）
- 假设：主人明确接受“仅保留 24h 聚合摘要与调用列表，不在页面展示独立并发状态卡片”。
- 假设：默认值维持 `1`，以避免旧部署在未改配置时突然放大量。
- 风险：取消固定节流后，上游供应商的真实速率限制仍可能暴露；当前仅保留单次请求失败后的 `Retry-After` / 指数退避重试，不提供独立 QPS 保护。
- 风险：管理员页面与 Storybook / e2e fixture 若未完全同步，会出现类型或断言漂移。

## 变更记录（Change log）
- 2026-03-09: 新建规格，冻结 `AI_MAX_CONCURRENCY`、无固定限流、管理员并发状态改造范围与验收标准。
- 2026-03-09: 完成后端 permit scheduler、管理员并发状态 API、README / docs-site / e2e 同步，并通过 `cargo test`、`bun run build`、`bun run e2e -- admin-jobs.spec.ts` 本地验证。
- 2026-03-10: 根据主人确认的最新 UI 口径，LLM 调度页移除独立并发状态卡片，仅保留 24h 聚合摘要，并把调用列表排序固定为“进行中 -> 排队中 -> 终止态 / 组内按创建时间倒序”。
- 2026-03-10: 根据 review-loop 修正 retryable LLM 调用在退避窗口的状态回写，确保 permit 已释放后调用重新显示为 `queued`，且不影响实时任务列表原有的按创建时间倒序排序。
- 2026-03-10: 根据 review-loop 为 LLM 调用列表补齐专用排序索引，并为 `Retry-After` 增加最小退避下限，避免热路径整表排序与 0ms 重试抖动。
- 2026-03-09: 根据 review-loop 补上 `AI_MAX_CONCURRENCY` 空字符串回退默认值 `1` 的配置兼容，并将 blank-value 容忍范围限制在该变量本身。
- 2026-03-09: 根据 review-loop 恢复 retryable LLM 请求的 `Retry-After` / 指数退避，避免取消固定节流后出现重试风暴。
- 2026-03-09: PR #34 已创建并更新到 `fd53622`，GitHub checks 全绿，review-loop 清零，spec-sync 收敛完成。
