# LLM 批处理效率改造（#g4456）

## 状态

- Status: 已完成
- Created: 2026-02-25
- Last: 2026-03-30

## 背景 / 问题陈述

- 现状：Feed 自动翻译和部分后端流程主要按“单条输入 -> 单次 LLM 请求”执行，调用次数高。
- 问题：在连续浏览场景，单位时间请求量大，吞吐受限，且在大模型/长文本时更容易触发上下文超限。
- 目标：把多条数据在一次 LLM 请求中批量处理，并在不改变业务语义的前提下降低调用次数、提升处理效率。

## 目标 / 非目标

### Goals

- 固定模型上限信源与同步策略（内置，不开放配置）。
- 引入 token 预算驱动的批量装箱，覆盖 release/release detail/notification/brief 相关 LLM 路径。
- 把 release feed 自动翻译切换为“可见卡片 + 最后一个可见卡片之后 10 条”的结果聚合模型。
- 为 release feed 新增后端 ensure/resolve 结果接口，由后端负责结果表状态、work item 去重与排队复用。

### Non-goals

- 不新增管理后台或运行时配置页面。
- 不引入更多 token 预算环境变量。
- 不改翻译调度器 worker 分流规则或 feed 分页行为。

## 范围（Scope）

### In scope

- 后端：`src/ai.rs`、`src/api.rs`、`src/server.rs`、`src/config.rs`、`src/translations.rs`
- 前端：`web/src/api.ts`、`web/src/feed/useAutoTranslate.tsx`、`web/src/feed/types.ts`
- 文档：`README.md`、`docs/product.md`、`.env.example`

### Out of scope

- UI 大改（样式/布局/交互文案重写）
- 新增第三方任务队列、消息总线

## 需求（Requirements）

### MUST

- 模型上限信源固定为：OpenRouter + LiteLLM，代码写死。
- 模型目录同步固定为：启动时懒刷新 + 每 24h 刷新。
- 未识别模型兜底固定为：`32768`。
- 仅保留一个手动覆盖变量：`AI_MODEL_CONTEXT_LIMIT`。
- feed 自动翻译必须调用 `POST /api/translate/results`，由前端上报可见 release 与最后一个可见 release 之后的连续 10 条；当窗口条目数超过接口单次 `60` 条上限时，前端必须按连续顺序拆成多次调用。
- `ai_translations` 必须升级为带状态的结果真相源；同一用户、同一实体、同一语言在任意时刻只保留 1 条当前结果记录，并显式表达 `queued/running/ready/disabled/missing/error`。
- 结果聚合接口必须先读取结果表；只有结果表未命中当前 source hash，或结果表声明 pending 但找不到活跃 work item 时，后端才允许进入 work item 队列层。
- 后端必须保证同一用户、同一 release、同一 source hash、同一请求模式的重复 resolve / 重复同源请求会复用同一条 `translation_requests` 记录，不会新增重复 `translation_work_items`，也不会因为轮询继续制造额外 request 行。
- 当较新的 source hash 进入排队时，后端不得为了挂起新任务而清空已经 `ready` 的旧译文；现有可展示译文必须保留到新任务真正进入终态为止。
- feed / release detail 的普通读取接口必须透传当前 source hash 上的 `disabled/missing/error` 终态；其中 `missing/error` 需要显式标记为“禁止自动重排队”，避免页面刷新后再次自动建队列。
- 前端必须按真实 DOM 视口几何持续重算 demand window，不再在前端执行 token 装箱或优先级拆批。
- feed 自动翻译的用户侧等待预算必须保持短窗口；默认 `max_wait_ms=500`，避免页面首次打开后长时间停留在 `queued`。
- 后端调度器继续以 work item token 预算与现有 worker 分流规则决定实际批次，但单批输入预算必须被 `1800 tokens` 硬上限截断，不得随模型上下文上限膨胀到数千 token。
- 同一条 release 处于 `queued/running/ready/disabled/error` 任一状态时，前端不得重复制造新的自动翻译记录。
- 自动结果轮询不得把 `error` 条目重新加入队列。
- 手动“翻译”必须复用同一套结果聚合接口；仅当调用方显式声明重试意图时，后端才允许把旧 `error` request/work item 重置并重新入队。

### SHOULD

- 结果聚合接口支持部分成功返回，避免整批失败导致全量重试。
- 前端对 pending 条目执行批量状态轮询，避免退回逐条 request_id 查询。

### COULD

- 增加 batch 指标日志（batch_size / saved_calls / fallback_source）。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- Feed 自动翻译：前端在首载、滚动、resize、feed 追加与卡片高度变化后重算真实视口；把当前可见 release 与最后一个可见卡片之后连续 10 条按原顺序交给结果聚合接口。若候选数超过单次 `60` 条限制，则前端继续按顺序拆成多次 resolve 调用。接口先读取结果表：`ready/disabled/missing/error` 直接返回；`queued/running` 会继续核对活跃 work item；只有结果缺失或 pending 漂移时才在后端 ensure 队列任务。
- Feed 自动翻译：`release_summary/feed_card` 的 source hash 以服务端当前 release 数据为准；若旧页面带来的旧 source blocks 晚到，后端必须先 canonicalize 到当前 release，再决定复用/建队列，旧 source 不得覆盖当前结果行。
- Feed 自动翻译：当前可见窗口的自动任务默认只给后端约 `500ms` 聚合时间；若窗口内请求足够多，调度器优先按 `1800 tokens` 左右切成多个小批，以便更多 worker 并行启动，而不是等待大批次攒满再跑。
- Release detail 翻译：详情 Markdown chunk 批量翻译，失败 chunk 再单条重试。
- Notification 翻译：支持线程批量翻译，缓存命中时直接返回。
- Brief 生成：多个 repo 的 release 摘要可在同一批次请求中处理。

### Edge cases / errors

- 外部目录刷新失败：不影响主流程，使用已缓存目录/内置映射/兜底值。
- batch 返回格式不合法：批次降级到单条翻译。
- 同一 demand window 在滚动抖动期间被多次 resolve：后端复用已存在结果行/work item，不重复创建记录。
- 不同 source hash 的 resolve 交错到达：较晚到达的旧 source hash 不得把结果表回退到更旧的 pending/ready 状态，也不得抢占已经绑定中的活跃 work item。
- 已失败条目在自动轮询中再次被 resolve：后端保持 `error` 终态，不自动重排队。
- 已失败条目被用户手动重试：后端复用原 request/work item，并优先复用最近一次失败 request 的快照把它重置为 queued；较早的历史 request 快照不得被批量回写。
- 旧 source hash 的 work item 晚到完成：不得覆盖结果表里已经切换到更新 source hash 的状态或译文。
- 单条资源不存在：batch item 返回 `status=error` + `error` 信息；单条接口保持 404 语义。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `/api/translate/results` | HTTP | internal | New | ./contracts/http-apis.md | backend | web feed auto-translate | release feed 的结果聚合 + ensure 入口 |
| `/api/translate/requests` | HTTP | internal | Existing | ./contracts/http-apis.md | backend | web detail panel | 详情侧栏仍使用单条/批量异步提交入口 |
| `/api/translate/requests/{request_id}` | HTTP | internal | Existing | ./contracts/http-apis.md | backend | web detail panel | 单条 request 查询与状态回收 |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/http-apis.md](./contracts/http-apis.md)

## 验收标准（Acceptance Criteria）

- Given 首批 feed 中存在多个可见 release 且其后仍有待翻译条目
  When 自动翻译触发
  Then 前端必须调用 `POST /api/translate/results`，并按顺序覆盖所有可见 release 与最后一个可见卡片之后的连续 10 条 release；若总数超过 60 条，则允许拆成多次连续调用。

- Given 同一条 release 在短时间内被前端多次 resolve
  When source hash 未变化
  Then 后端不得新增重复 `translation_work_items`，且自动 resolve 不得继续制造新的 `translation_requests`。

- Given 某条 release 的译文尚未 ready
  When 结果聚合接口被重复轮询
  Then 接口必须持续返回 `queued/running` 并复用原结果行/work item，直到任务进入终态。

- Given 某条 release 已经存在一条可展示的 `ready` 译文
  When 新 source hash 被结果聚合接口补建 work item
  Then 旧的 `ready` 译文必须继续保留，直到新 work item 产出终态结果；中途不得因为 pending 标记让 UI 回退成空白。

- Given 用户打开 feed 首屏并触发自动翻译
  When 当前可见窗口只产生少量 release 任务
  Then 首批 work item 应在约 `500ms` 聚合窗口后开始 claim，不得继续等待 `4s` 级别的 deadline。

- Given 可见窗口与后续 10 条合计形成大量待翻译 release
  When 调度器装箱 batch
  Then 单个 translation batch 的输入预算必须被 `1800 tokens` 截断，避免只生成 1-2 个超大 batch 而闲置其余 worker。

- Given 某条 release 已进入 `error`
  When 自动可见窗口继续调用结果聚合接口
  Then 后端必须继续返回 `error`，不得仅因轮询再次把该条 release 加回队列。

- Given 模型目录刷新失败
  When 计算 token 预算
  Then 系统仍可使用缓存/内置映射/兜底值继续翻译，不中断主流程。

- Given 某条 release 已有自动 request 在途
  When 用户手动点击“翻译”
  Then 前端直接复用已有 request，不重复创建新的自动翻译任务。

- Given release detail 存在多个 chunk
  When 调用翻译
  Then 后端优先执行 chunk 批量翻译，失败 chunk 自动回退单条重试。

## 实现前置条件（Definition of Ready / Preconditions）

- [x] 目标/范围/边界已冻结
- [x] 新接口清单已明确
- [x] 唯一环境变量约束已确认（仅 `AI_MODEL_CONTEXT_LIMIT`）

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 批量解析、装箱、fallback、缓存命中路径。
- Integration tests: 新 results resolve API 与旧单条 request API 的兼容行为。
- E2E tests: Dashboard 自动翻译流程不回归。

### Quality checks

- `cargo test`
- `cargo fmt --check`
- `cd web && bun run build`
- `cd web && bun run storybook:build`
- `cd web && bun run e2e -- release-detail.spec.ts`

## 文档更新（Docs to Update）

- `README.md`: 增加 `AI_MODEL_CONTEXT_LIMIT` 与模型目录行为说明。
- `docs/product.md`: 补充 visible-window 自动翻译策略与结果聚合接口语义。
- `.env.example`: 增加 `AI_MODEL_CONTEXT_LIMIT`。

## 计划资产（Plan assets）

- Directory: None
- In-plan references: None

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 建立 specs-first 文档基线与接口契约。
- [x] M2: 后端模型上限解析器 + 固定信源同步机制。
- [x] M3: 后端 batch API 与单条委托改造（release/release detail/notification/brief）。
- [x] M4: 前端自动翻译切换到 visible-window 结果聚合接口 + Storybook / E2E 回归验证。

## 方案概述（Approach, high-level）

- 在 `ai.rs` 建立统一 token 预算与批量装箱能力，作为 LLM 路径共享底座。
- 在 `translations.rs` 中新增结果聚合接口，由后端复用现有 request/work item/cache 模型并负责幂等 ensure。
- 在 `web` 端以真实视口为基础维护 visible-window，持续请求译文结果而不是直接创建 request。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：不同模型返回 JSON 稳定性差，需确保 batch 解析失败可安全回退。
- 开放问题：notification batch 当前未在 UI 主流程消费（接口先行）。
- 假设：现有 OpenAI-compatible 网关遵守 chat/completions 协议并支持既定 `max_tokens`。

## Visual Evidence

- 本任务不提交静态截图资产。
- 可通过 Storybook `Pages/Dashboard/VisibleWindowQueue` 与 `Pages/Dashboard/VisibleWindowSettling` 复现可见窗口自动翻译状态。

## 变更记录（Change log）

- 2026-02-25: 创建规格并冻结“固定信源 + 固定同步 + 固定兜底 + 单环境变量”约束。
- 2026-02-25: 完成后端批处理核心与前端自动翻译批调接入；前端构建受缺失依赖阻塞，待补依赖后复验。
- 2026-03-30: 完成 release feed visible-window 结果聚合接口、request/work item 双层去重、Storybook 场景与 Playwright 回归验证。

## 参考（References）

- OpenRouter models catalog: https://openrouter.ai/api/v1/models
- LiteLLM model map: https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json
