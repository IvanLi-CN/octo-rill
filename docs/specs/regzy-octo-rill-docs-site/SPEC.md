# OctoRill 文档站点与 Storybook 文档快车道实施（#regzy）

## 状态
- 当前状态：已完成
- Spec 目录：`docs/specs/regzy-octo-rill-docs-site/`
- 负责人：Codex

## 背景 / 问题陈述
- 当前仓库只有产品 README、`docs/product.md` 与 `web` 内部 Storybook，缺少一个可公开访问、可持续维护的文档站点。
- 现有 GitHub Actions 会构建 Storybook，但不会把 Storybook 与面向用户的文档内容装配成统一站点。
- 对外介绍、安装启动、配置说明与页面信息架构分散在多个文件里，不适合作为公开文档入口。

## 目标 / 非目标
### Goals
- 新建独立 `docs-site/` Rspress 文档工程，作为 OctoRill 的公开文档入口。
- 首版提供中文单语最小可用文档：首页、快速开始、配置参考、产品说明、Storybook 入口。
- 保持 `web` Storybook 独立构建，并装配到同一 GitHub Pages 站点下的 `/storybook/`。
- 为核心 Storybook 页面/故事补齐 docs 描述，让 docs 视图可读，不只是 Canvas。

### Non-goals
- 不做英文镜像或 i18n 文档。
- 不在首版补齐完整 API、运维、FAQ、术语表体系。
- 不修改运行时 HTTP API、数据库 schema、认证模型或页面业务逻辑。

## 范围（Scope）
### In scope
- `docs-site/` 工程脚手架、Rspress 配置与中文导航/侧边栏。
- `docs-site/docs/index.md`、`quick-start.md`、`config.md`、`product.md`、`storybook.mdx`、`404.md`。
- `.github/scripts/assemble-pages-site.sh` 与 `.github/workflows/docs-pages.yml`。
- `web/.storybook/*` 与核心 stories 的 docs 元数据增强。
- 根 README 与 `web/README.md` 的文档站/Storybook 说明同步。

### Out of scope
- 自定义域名配置。
- 外部搜索服务接入。
- 公开暴露 `docs/specs/**` 内容。

## 功能与行为规格（Functional / Behavior Spec）
### 文档站
- `docs-site` 使用 Rspress，`root` 固定为 `docs-site/docs`。
- `DOCS_BASE` 必须支持 GitHub Pages 子路径部署；空值与 `/` 都规范化为 `/`。
- 顶部导航至少包含：文档首页、快速开始、配置参考、产品说明、Storybook、GitHub。
- 侧边栏仅展示首版最小页面集合，不引入空白占位页面。

### Storybook 装配
- GitHub Pages 产物根目录放置 Rspress 构建结果。
- Storybook 静态产物装配到 `storybook/` 子目录。
- 根目录额外生成 `storybook.html` 跳转页，转到相对路径 `./storybook/`。
- 文档首页与 Storybook docs 页都要提供 Storybook 跳转入口。

### Storybook 文档体验
- 核心组：Landing、Dashboard、Admin Panel、Admin Jobs、Task Type Detail、UI Primitives、App Meta Footer。
- 每个核心组都要有可读的 docs 描述（组件用途、适用页面或关键交互）。
- Storybook 分组命名固定为 `Pages/Landing`、`Pages/Dashboard`、`Admin/Admin Panel`、`Admin/Admin Jobs`、`Admin/Task Type Detail`、`Layout/App Meta Footer`、`UI/Primitives`，并便于从公开文档站跳转定位。

## 接口契约（Interfaces & Contracts）
### 新增构建接口
- `docs-site/package.json`
  - `dev`: `rspress dev --host 127.0.0.1 --port ${DOCS_PORT:-50885}`
  - `build`: `rspress build`
  - `preview`: `rspress preview --host 127.0.0.1 --port ${DOCS_PORT:-50885}`
- 环境变量：`DOCS_BASE`
- GitHub Pages 路径：`/storybook/`、`/storybook.html`

### 不变接口
- 运行时 API：不存在变更。
- 数据库契约：不存在变更。

## 验收标准（Acceptance Criteria）
1. Given 在 `docs-site` 内执行 `bun run build`，When 设置 `DOCS_BASE=/octo-rill/`，Then 文档站能成功构建，且生成的站内链接与静态资源路径都指向 `/octo-rill/` 子路径。
2. Given 在 `web` 内执行 `bun run storybook:build`，When 打开生成产物，Then 核心故事组均可在 docs 视图看到说明文字。
3. Given 执行装配脚本，When 传入 docs 与 storybook 两个构建目录，Then 输出目录同时包含 `index.html`、`storybook/index.html` 与 `storybook.html`，且 `storybook.html` 提供 Storybook 总入口与核心故事深链。
4. Given GitHub Actions 触发 `Docs Pages` 工作流，When 构建成功，Then 能上传 assembled pages artifact，并在非 PR 场景部署 GitHub Pages。
5. Given 开发者阅读根 README 与 `web/README.md`，When 按说明启动文档站与 Storybook，Then 端口、命令与实际脚本一致。

## 非功能性验收 / 质量门槛（Quality Gates）
- `cd docs-site && DOCS_BASE=/octo-rill/ bun run build`
- `cd web && bun run lint`
- `cd web && bun run storybook:build`
- `bash ./.github/scripts/assemble-pages-site.sh <docs_build> <storybook_build> <output_dir>` smoke check

## 文档更新（Docs to Update）
- `README.md`
- `web/README.md`
- `docs/specs/README.md`

## 实现里程碑（Milestones / Delivery checklist）
- [x] M1: 新 spec 与 docs-site 工程落地
- [x] M2: 中文最小文档内容与 Storybook 跳转页完成
- [x] M3: Storybook docs 元数据与 README 说明同步
- [x] M4: Pages workflow、装配脚本与验证通过
- [x] M5: PR、标签、checks 与 review-loop 收敛完成

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）
- 假设：首版公开文档以中文为唯一语言，主人未要求英文镜像。
- 假设：GitHub Pages 使用仓库默认 Pages 配置，无需额外自定义域名。
- 风险：`CI Pipeline` 需要同步补上 docs-site build 与 assembled-site smoke check，否则 PR 阶段无法提前暴露公开站点路径问题。
- 风险：Storybook 静态资源在子目录下可能需要额外 smoke check，避免相对路径失效。

## 变更记录（Change log）
- 2026-03-09: 重建为 `regzy` 规格并按快车道继续推进，PR #31 已创建，checks 与 review-loop 收敛完成。
