# 公开文档体系重写与职责收口（#em5uh）

## 状态

- Status: 部分完成（2/3）
- Created: 2026-04-19
- Last: 2026-04-19

## 背景 / 问题陈述

- 现有公开文档在 README、docs-site、`docs/product.md`、`web/README.md` 之间重复描述同一批内容，阅读路径不清晰。
- docs-site 首页首屏信息密度低，存在“先看这 4 件事 / 适合谁看 / 当前首版范围”这类不直接推动任务的段落。
- 配置说明、产品说明与前端贡献说明边界模糊，导致文档像一组互相复述的摘要，而不是可执行入口。

## 目标 / 非目标

### Goals

- 让 `README.md`、docs-site、`docs/product.md`、`web/README.md` 各自只承担一种职责。
- 用更可扫读的结构重写公开文档，减少模板化表述和空泛过渡句。
- 用仓库真实配置和构建入口校正文档，消除端口、命令、环境变量与发布路径漂移。
- 在不扩大站点规模的前提下，重组公开 docs 的信息架构。

### Non-goals

- 不逐篇重写历史 `docs/specs/**` 或 contracts 文档。
- 不修改产品功能、接口、数据库或 CI / release 行为本身。
- 不把公开站点扩成完整内部设计门户。

## 范围（Scope）

### In scope

- 重写根目录 `README.md` 为仓库入口与开发者导览。
- 重写 `docs-site/docs/index.md`、`quick-start.md`、`config.md`、`product.md`、`storybook-guide.mdx`、`404.md`，并调整 `docs-site/rspress.config.ts` 的导航和侧栏。
- 保留 `docs-site/docs/storybook.mdx` 的 redirect 角色，只在需要时同步文案。
- 重写 `docs/product.md` 为内部产品参考。
- 重写 `web/README.md` 为前端与 Storybook 贡献文档。
- 用 docs-site build、Storybook build、assembled Pages smoke check 与浏览器预览验证结果。

### Out of scope

- 历史 `docs/specs/**` 的逐篇润色。
- 新增产品功能或运行时配置项。
- PR 合并与合并后 cleanup。

## 需求（Requirements）

### MUST

- 每个文档入口只能回答一类核心问题，避免跨文档重复复述。
- 公开 docs 采用更接近 Diátaxis 的分工：启动、配置、解释、预览分开。
- 所有命令、端口、环境变量与默认值必须和仓库真实实现一致。
- 文案必须可扫读，标题具体，指令使用主动语态。

### SHOULD

- 首页首屏先给定位与任务入口，再给补充说明。
- 配置页按核心运行时、OAuth、AI、预览相关变量分组。
- `docs/product.md` 与公开 `product.md` 的职责边界要能一眼看懂。

### COULD

- 在 README 中补充最少量的 release-label 流程提醒，帮助仓库协作者避免踩门禁。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 新读者进入 docs-site 首页后，能直接按任务跳到快速开始、配置、产品说明或 Storybook。
- 开发者从 README 进入仓库时，能在一个页面内知道仓库结构、最短启动路径和详细文档入口。
- 前端贡献者进入 `web/README.md` 后，能直接找到开发、预览、Storybook 和最小验证要求。
- 需要内部产品语义时，协作者从 `docs/product.md` 获取更完整的行为边界，而不是复述公开产品页。

### Edge cases / errors

- 如果本地只启动 docs-site，没有启动 Storybook，Storybook 导览仍应说明需要先启动对应 dev server。
- 如果未配置 AI，文档必须说明核心登录与浏览仍可运行，但翻译 / 日报能力会降级。
- 如果 OAuth callback 不匹配，快速开始和配置页都应明确这是确定性失败点。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

None

## 验收标准（Acceptance Criteria）

- Given 新读者打开 docs-site 首页
  When 查看首屏与首个滚动屏
  Then 能在不阅读长段背景说明的情况下找到启动、配置、产品说明和 Storybook 入口。

- Given 开发者阅读根目录 README
  When 需要本地启动项目
  Then 能在 README 里找到最短启动路径，并知道更详细内容分别在哪个文档里。

- Given 协作者同时查看公开 `product.md` 与内部 `docs/product.md`
  When 比较两者内容
  Then 能看出前者负责产品解释，后者负责更完整的内部语义与边界，而不是两份同义改写。

- Given 维护者运行 docs 验证链路
  When 执行 docs-site build、Storybook build 和 assembled Pages smoke check
  Then 三项都通过，且本地浏览器可以打开重写后的公开首页、快速开始、配置、产品和 Storybook 导览页面。

## 实现前置条件（Definition of Ready / Preconditions）

- 范围、非目标、验证要求和 PR-ready stop condition 已冻结。
- 文档事实源明确为 `.env.example`、`src/config.rs`、`package.json`、`web/package.json`、`docs-site/package.json`、现有 CI/workflow。
- 本任务仅允许修改 docs 与 README，不涉及运行时行为变更。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Docs build: `cd docs-site && bun run build`
- Storybook build: `cd web && bun run storybook:build`
- Assembled site smoke: `bash ./.github/scripts/assemble-pages-site.sh docs-site/doc_build web/storybook-static .tmp/pages-site`

### UI / Storybook (if applicable)

- 不新增 docs 页面专用 stories。
- 使用本地 docs-site 预览页进行 owner-facing 视觉验收。

### Quality checks

- 文案自审：标题、主动语态、链接文本、重复内容
- 浏览器核验：首页、快速开始、配置、产品、Storybook 导览可达

## 文档更新（Docs to Update）

- `README.md`：收口为仓库入口与最短启动路径
- `docs-site/docs/*`：按公开文档职责重写
- `docs/product.md`：重写为内部产品参考
- `web/README.md`：收口为前端与 Storybook 贡献说明
- `docs-site/rspress.config.ts`：调整导航与侧边栏

## Visual Evidence

本任务的 owner-facing docs 预览截图在对话中回传验收；未经额外允许，不把截图文件纳入仓库或 PR 正文。

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 重写 README 与 docs-site 公开文档，使每个入口只承担一种职责
- [x] M2: 重写内部产品参考与前端 README，消除与公开文档的重复描述
- [ ] M3: 通过 docs build、Storybook build、assembled smoke check，并完成浏览器验收与 PR-ready 收口

## 方案概述（Approach, high-level）

- 保持现有公开站点页数基本不变，但重写页面职责、标题顺序与首屏结构。
- 以仓库真实实现为事实源，先收敛文案边界，再做构建与浏览器验证。
- 视觉证据只在对话中展示；若后续需要把截图纳入 PR，再单独征得主人许可。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：docs-site 与内部产品参考若仍保留相近段落，容易回到双份维护。
- 风险：配置页遗漏 `src/config.rs` 中的真实变量，会继续制造文档漂移。
- 假设：本次继续保持中文单语公开文档，不引入英文版本。
- 假设：Storybook stories 本身不需要随本次 docs 改动新增或重命名。

## 参考（References）

- https://diataxis.fr/
- https://www.writethedocs.org/guide/writing/docs-principles/
- https://google.github.io/styleguide/docguide/best_practices.html
- https://developers.google.com/style/headings
- https://developers.google.com/style/tone
- https://developers.google.com/style/voice
- https://learn.microsoft.com/en-us/style-guide/scannable-content/headings
