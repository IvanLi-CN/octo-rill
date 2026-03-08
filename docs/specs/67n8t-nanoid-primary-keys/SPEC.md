# 全库主键 NanoID 化与公开标识收口（#67n8t）

## 状态

- Status: 部分完成（3/4）
- Created: 2026-03-07
- Last: 2026-03-08

## 背景 / 问题陈述

- 当前仓库混用本地顺序整数主键与 UUID-like 文本主键，前端与非管理员接口仍可观察到部分本地顺序标识。
- 用户要求统一切换为更短、更适合前端展示的 NanoID，并明确允许重建 SQLite 数据库，不保留历史数据兼容。
- 若继续保留顺序本地主键，会泄露系统规模下界，也会让任务链路的字符串 ID 风格继续分裂。

## 目标 / 非目标

### Goals

- 将实体/关系表的本地主键统一为 16 位小写安全集 NanoID。
- 保持 GitHub 外部资源键（`release_id`、`thread_id`、`repo_id`、`github_user_id`）的现有语义。
- 统一后端 ID 生成与校验规则，并同步更新前后端类型、测试、文档与迁移。
- 通过破坏性迁移保证空库初始化时直接得到新 schema。

### Non-goals

- 不做旧 SQLite 数据兼容迁移、双写或在线切换。
- 不把天然业务键主键（调度槽位、调度状态键）强行替换为 NanoID。
- 不修改非管理员公开路由以外部资源键进行查询的现有产品语义。

## 范围（Scope）

### In scope

- `users` 及所有直接/间接引用本地用户主键的表与查询。
- 当前实体/关系表中使用 `INTEGER PRIMARY KEY AUTOINCREMENT` 或 UUID-like `TEXT PRIMARY KEY` 的主键与外键。
- `web/src`、Rust handler、测试 seed、serde/type definitions 中对本地主键的类型假设。
- 与本次主键切换直接相关的 README/spec/contracts 更新。

### Out of scope

- GitHub 外部资源键本身的值域、来源与路由格式。
- 会话存储实现以外的第三方表结构。
- 任何依赖旧库在线迁移的发布方案。

## 需求（Requirements）

### MUST

- 所有实体/关系表本地主键统一为 `TEXT PRIMARY KEY`，值满足固定 16 位 NanoID 格式。
- 所有引用本地主键的外键列同步改为文本，并保留既有唯一约束与级联行为。
- 后端集中生成 NanoID，前端只消费不生成。
- `/api/me` 以及所有管理员接口中暴露的本地主键全部改为字符串。
- 非管理员接口继续以 `release_id` / `thread_id` 等外部键寻址，不回退到本地主键。
- 空库执行全部迁移后，应用可正常启动并通过现有关键测试。

### SHOULD

- 任务、翻译、LLM 调用链路中所有本地字符串 ID 收敛到同一 NanoID 风格，不再混用 UUID 前缀格式。
- 提供统一的 ID 校验辅助，避免手写字符串校验分散在 handler 中。
- 文档明确标注本次为重建库切换，提醒不兼容旧 SQLite 数据文件。

### COULD

- 为关键公开/管理响应增加更清晰的类型别名，减少前后端继续把本地主键误用为数字的机会。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 用户登录/会话恢复后，`/api/me` 返回的本地用户标识为 NanoID 字符串；管理员前端用该字符串继续完成“当前用户识别”“编辑用户”“查看资料”。
- 后端创建任务、LLM 调用、翻译请求/工作项/批次时，全部生成同格式 NanoID，本地存储与 API 返回保持一致。
- 公开 release/feed/notification 查询仍使用 GitHub 外部键；本地主键变化不影响这些入口的 URL 与交互。
- 空数据库启动时，迁移直接建立 NanoID schema，后续同步、翻译、管理员页面与前端构建正常工作。

### Edge cases / errors

- 任何接收本地主键的管理员接口在遇到非法格式 ID 时，应返回明确客户端错误而非静默查询失败。
- 旧数据库文件不在兼容范围内；若使用旧库启动，允许通过重建流程解决，而不是隐式尝试修复。
- 非管理员接口不得因本地主键格式变化而改变 `release_id` / `thread_id` 的入参校验。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Application schema | db | internal | Modify | ./contracts/db.md | backend | backend/tests | 破坏性重建本地主键与外键 |
| REST responses using local ids | http | external | Modify | ./contracts/http-apis.md | backend/web | web/admin | `user.id` 等本地主键改为字符串 |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/db.md](./contracts/db.md)

## 验收标准（Acceptance Criteria）

- Given 空 SQLite 数据库
  When 应用执行全部迁移并启动
  Then 所有实体/关系表本地主键均为 16 位 NanoID 文本，且外键完整可用。
- Given 已登录用户访问 `/api/me`
  When 前端读取当前用户信息
  Then `user.id` 为字符串 NanoID，前端与管理员页面不再假设其为数字。
- Given 管理员调用任务、翻译、用户管理相关接口
  When 创建或读取本地实体
  Then 返回/接收的本地主键全部为统一 NanoID 字符串。
- Given 普通用户访问 release detail/feed/notifications/translation 请求入口
  When 使用 `release_id` 或 `thread_id`
  Then 现有路由与产品语义保持不变。
- Given 开发者使用旧 SQLite 数据文件
  When 尝试切换到该版本
  Then 文档能明确说明需要重建库，且实现不承诺旧数据兼容。

## 实现前置条件（Definition of Ready / Preconditions）

- 目标/非目标、范围、公开路由保留策略已冻结。
- NanoID 格式已冻结为 16 位小写安全集。
- 数据库策略已冻结为允许重建库，不保留旧数据。
- 天然业务键主键保留原样，不纳入本次统一替换。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 补齐 NanoID 生成/校验与关键 handler 类型变更测试。
- Integration tests: 覆盖空库迁移启动、认证/管理员接口、翻译请求链路。
- E2E tests (if applicable): 覆盖前端登录后读取 `me`、管理员用户/任务页面关键流程。

### UI / Storybook (if applicable)

- Stories to add/update: 与管理员用户/任务页 ID 类型变化直接相关的 stories。
- Visual regression baseline changes (if any): 仅在字符串 ID 导致展示变化时更新。

### Quality checks

- Lint / typecheck / formatting: `cargo test`、前端 `bun`/`npm` 既有检查、相关 e2e 或 build。

## 文档更新（Docs to Update）

- `README.md`: 说明本地 ID 已统一为 NanoID，以及旧 SQLite 文件需重建。
- `docs/specs/README.md`: 新增并更新本规格状态。

## 计划资产（Plan assets）

- Directory: `docs/specs/67n8t-nanoid-primary-keys/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- PR visual evidence source: maintain `## Visual Evidence (PR)` in this spec when PR screenshots are needed.
- If an asset must be used in impl (runtime/test/official docs), list it in `资产晋升（Asset promotion）` and promote it to a stable project path during implementation.

## Visual Evidence (PR)

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 统一后端 NanoID 生成/校验与所有本地主键类型定义
- [x] M2: 追加破坏性迁移并完成 schema/查询/外键切换
- [x] M3: 更新前端类型、管理员页面与关键测试/构建
- [ ] M4: 完成快车道交付（验证、PR、review-loop、spec 同步）

## 方案概述（Approach, high-level）

- 采用单次破坏性迁移重建应用表，避免对历史顺序主键做映射迁移。
- 以统一 NanoID helper 收口所有本地 ID 生成与格式校验，减少多处拼接与风格分裂。
- 保持外部资源键与公开路由语义不变，只替换本地实体标识与内部关系。
- 以前后端类型同步与测试补齐兜底，避免遗漏数字主键假设。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：改动跨 schema、后端、前端与测试，容易漏掉隐藏的 `i64` 假设。
- 风险：破坏性迁移会让旧 SQLite 数据文件失效，必须通过文档与验证明确该约束。
- 需要决策的问题：None
- 假设（需主人确认）：None

## 变更记录（Change log）

- 2026-03-08: PR #29 review-loop 补齐 NanoID 后的最新事件窗口与翻译请求项顺序修复，避免随机主键导致结果顺序漂移。
- 2026-03-07: 完成本地实现与验证，后端/前端本地主键已切换为 16 位 NanoID，README 补充重建库说明。
- 2026-03-07: 创建规格并冻结 NanoID、天然键保留、公开路由保留外部键、允许重建库的实现口径。

## 参考（References）

- `docs/specs/README.md`
- `migrations/*.sql`
- `src/api.rs`
- `src/translations.rs`
