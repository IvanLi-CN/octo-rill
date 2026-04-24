# 实现状态（LinuxDO 绑定与用户设置页改造）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-18
- Last: 2026-04-22
- Summary: 已交付；PR #94; fast-track / unified settings page + LinuxDO binding + inline PAT fallback + PAT visual-mask guard
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 新增本规格索引行，并在进度推进时更新状态/备注
- `docs/product.md`: 如设置入口或 LinuxDO 绑定成为用户可见能力，需要同步补充口径

## 计划资产（Plan assets）

- Directory: `docs/specs/y9ngx-linuxdo-user-settings/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- Visual evidence source: maintain `## Visual Evidence` in this spec when owner-facing or PR-facing screenshots are needed.

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新增 LinuxDO 绑定 migration、配置解析、OAuth callback 与 `/api/me/linuxdo` 接口
- [x] M2: 前台 `/settings` 页面落地，并迁移 GitHub PAT / 日报设置入口
- [x] M3: Dashboard 账号菜单与 PAT fallback UX 完成切换
- [x] M4: Storybook、Playwright、视觉证据与文档同步完成
