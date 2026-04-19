# OctoRill Web UI

`web/` 包含 React + Vite 前端，以及用于页面状态验证的 Storybook。这里不解释产品定位；产品概览见 `docs-site/docs/product.md`，内部语义见 `docs/product.md`。

## 这份文档回答什么

- 如何启动前端和本地预览
- 如何构建与校验 Storybook
- 做 UI 改动时，最少要补哪些前端验证

## 安装与常用命令

### 开发服务器

```bash
bun install
bun run dev
```

打开 `http://127.0.0.1:55174`。

### 生产预览

```bash
bun run build
bun run preview
```

预览地址：`http://127.0.0.1:55175`。

### Storybook 开发模式

```bash
bun run storybook
```

打开 `http://127.0.0.1:55176`。

如果 docs-site 也在本地运行，公开文档里的 Storybook 入口会自动跳回这个 dev server；你也可以手动设置 `VITE_DOCS_SITE_ORIGIN`。

### Storybook 静态构建

```bash
bun run storybook:build
```

输出目录：`web/storybook-static/`。GitHub Pages 发布时，它会被组装到 `/storybook/`。

### 其他前端检查

```bash
bun run lint
bun run e2e
bun run test:embedded-version
bun run verify:mobile-header-drag
```

按改动范围选择运行；不要把所有命令都当成每次必跑的默认套餐。

## Storybook 在这个仓库里的作用

Storybook 不是展示橱窗，它是 UI 改动的稳定验证面。当前公开覆盖主要分成四类：

- `Pages/*`：Landing、Dashboard 等页面级状态
- `Admin/*`：管理端任务中心、用户管理与任务详情
- `Layout/*`：布局与元信息回退状态
- `UI/*`：基础组件和原语

当你改动可见界面时，优先让 Storybook 成为“可重复验证”的入口，而不是只依赖一次性的手工页面截图。

## 做 UI 改动时的最低要求

- 变更影响现有故事时，更新对应 stories，而不是只改运行页。
- 新增明显页面状态时，补一个可以稳定复现该状态的 story。
- 文档页和故事说明保持简短，直接说明“这个状态验证什么”。
- 如果公开文档或 stories 里已有深链入口，别把链接改断。

## 和 docs-site 的关系

- docs-site 负责解释如何启动、如何配置、产品在解决什么问题。
- Storybook 负责展示页面状态、控件组合和视觉边界。

如果你在改公开文档入口、Storybook 跳转或静态组装路径，记得同时验证：

```bash
(cd docs-site && bun run build)
(cd web && bun run storybook:build)
bash ./.github/scripts/assemble-pages-site.sh docs-site/doc_build web/storybook-static .tmp/pages-site
```
