# 实现状态（移除公开文档中的 Storybook 导览页）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-19
- Last: 2026-04-19
- Summary: 已交付；fast-track follow-up to public-docs-rewrite; docs-site now links directly to Storybook without guide page
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs-site/rspress.config.ts`
- `docs-site/docs/index.md`
- `docs-site/docs/quick-start.md`
- `docs-site/docs/storybook.mdx`
- `web/README.md`
- `web/.storybook/preview.ts`
- `.github/scripts/assemble-pages-site.sh`
- `.github/workflows/docs-pages.yml`
- `docs/specs/README.md`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 删除 Storybook 导览页并移除 docs-site 中的所有入口
- [x] M2: 清理仓库文档中的相关死链并通过构建验证

## 验证摘要

- docs-site build：通过
- Storybook build：通过
- assembled Pages smoke check：通过
- 组装结果确认：`storybook-guide.html` 仅保留旧链接兼容重定向，不再作为导航中的导览页
- 视觉证据：公开文档首页导航与侧边栏只保留直接 `Storybook` 入口
