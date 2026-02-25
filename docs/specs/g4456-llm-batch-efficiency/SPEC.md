# LLM 批处理效率改造（#g4456）

## 状态

- Status: 部分完成（3/4）
- Created: 2026-02-25
- Last: 2026-02-25

## 背景 / 问题陈述

- 现状：Feed 自动翻译和部分后端流程主要按“单条输入 -> 单次 LLM 请求”执行，调用次数高。
- 问题：在连续浏览场景，单位时间请求量大，吞吐受限，且在大模型/长文本时更容易触发上下文超限。
- 目标：把多条数据在一次 LLM 请求中批量处理，并在不改变业务语义的前提下降低调用次数、提升处理效率。

## 目标 / 非目标

### Goals

- 固定模型上限信源与同步策略（内置，不开放配置）。
- 引入 token 预算驱动的批量装箱，覆盖 release/release detail/notification/brief 相关 LLM 路径。
- 保持现有单条 API 对外兼容，新增 batch API 供前端聚合调用。

### Non-goals

- 不新增管理后台或运行时配置页面。
- 不引入更多 token 预算环境变量。
- 不改变翻译结果语义与 UI 交互语义（仅改变调用组织方式）。

## 范围（Scope）

### In scope

- 后端：`src/ai.rs`、`src/api.rs`、`src/server.rs`、`src/config.rs`
- 前端：`web/src/feed/useAutoTranslate.tsx`、`web/src/feed/types.ts`
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
- 新增 batch API：
  - `POST /api/translate/releases/batch`
  - `POST /api/translate/release/detail/batch`
  - `POST /api/translate/notifications/batch`
- 单条 API 保持兼容并委托 batch 核心执行。

### SHOULD

- batch 支持部分成功返回，避免整批失败导致全量重试。
- batch 解析失败时自动回退到单条翻译。

### COULD

- 增加 batch 指标日志（batch_size / saved_calls / fallback_source）。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- Feed 自动翻译：可见 release 条目聚合为 batch 请求，后端按 token 预算分组后调用 LLM。
- Release detail 翻译：详情 Markdown chunk 批量翻译，失败 chunk 再单条重试。
- Notification 翻译：支持线程批量翻译，缓存命中时直接返回。
- Brief 生成：多个 repo 的 release 摘要可在同一批次请求中处理。

### Edge cases / errors

- 外部目录刷新失败：不影响主流程，使用已缓存目录/内置映射/兜底值。
- batch 返回格式不合法：批次降级到单条翻译。
- 单条资源不存在：batch item 返回 `status=error` + `error` 信息；单条接口保持 404 语义。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `/api/translate/releases/batch` | HTTP | internal | New | ./contracts/http-apis.md | backend | web feed auto-translate | release 批量翻译 |
| `/api/translate/release/detail/batch` | HTTP | internal | New | ./contracts/http-apis.md | backend | web detail panel | release detail 批量翻译 |
| `/api/translate/notifications/batch` | HTTP | internal | New | ./contracts/http-apis.md | backend | future inbox batching | notification 批量翻译 |
| `/api/translate/release` | HTTP | internal | Modify | ./contracts/http-apis.md | backend | existing web/manual retry | 内部改为委托 batch |
| `/api/translate/release/detail` | HTTP | internal | Modify | ./contracts/http-apis.md | backend | existing web detail | 内部改为委托 batch core |
| `/api/translate/notification` | HTTP | internal | Modify | ./contracts/http-apis.md | backend | existing inbox usage | 内部改为委托 batch |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/http-apis.md](./contracts/http-apis.md)

## 验收标准（Acceptance Criteria）

- Given Feed 同时出现 >= 8 条待翻译 release
  When 自动翻译触发
  Then 前端调用 batch 接口而非逐条接口，后端可返回逐项结果。

- Given 模型目录刷新失败
  When 计算 token 预算
  Then 系统仍可使用缓存/内置映射/兜底值继续翻译，不中断主流程。

- Given 单条接口被调用
  When 请求有效
  Then 结果语义与历史一致（ready/disabled，404 语义不变）。

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
- Integration tests: 新 batch API 与旧单条 API 的兼容行为。
- E2E tests: Dashboard 自动翻译流程不回归。

### Quality checks

- `cargo test`
- `cargo fmt --check`
- `cd web && bun run build`

## 文档更新（Docs to Update）

- `README.md`: 增加 `AI_MODEL_CONTEXT_LIMIT` 与模型目录行为说明。
- `docs/product.md`: 补充自动翻译批处理策略。
- `.env.example`: 增加 `AI_MODEL_CONTEXT_LIMIT`。

## 计划资产（Plan assets）

- Directory: `docs/specs/g4456-llm-batch-efficiency/assets/`
- In-plan references: None

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 建立 specs-first 文档基线与接口契约。
- [x] M2: 后端模型上限解析器 + 固定信源同步机制。
- [x] M3: 后端 batch API 与单条委托改造（release/release detail/notification/brief）。
- [ ] M4: 前端自动翻译切换到 batch API + 回归验证。

## 方案概述（Approach, high-level）

- 在 `ai.rs` 建立统一 token 预算与批量装箱能力，作为 LLM 路径共享底座。
- 在 `api.rs` 实现 batch endpoint，单条 endpoint 委托 batch 内核，保持对外兼容。
- 在 `web` 端通过短窗口聚合调用 batch 接口，减少逐条请求。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：不同模型返回 JSON 稳定性差，需确保 batch 解析失败可安全回退。
- 开放问题：notification batch 当前未在 UI 主流程消费（接口先行）。
- 假设：现有 OpenAI-compatible 网关遵守 chat/completions 协议并支持既定 `max_tokens`。

## 变更记录（Change log）

- 2026-02-25: 创建规格并冻结“固定信源 + 固定同步 + 固定兜底 + 单环境变量”约束。
- 2026-02-25: 完成后端批处理核心与前端自动翻译批调接入；前端构建受缺失依赖阻塞，待补依赖后复验。

## 参考（References）

- OpenRouter models catalog: https://openrouter.ai/api/v1/models
- LiteLLM model map: https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json
