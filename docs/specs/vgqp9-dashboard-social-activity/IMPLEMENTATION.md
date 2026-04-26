# 实现状态（Dashboard 社交活动记录扩展（含头像））

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-10
- Last: 2026-04-26
- Summary: 已交付；全部 tab 已扩展公告与 Fork 混排，专属 tabs 保持发布 / 加星 / 关注 / 日报 / 收件箱口径
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/vgqp9-dashboard-social-activity/SPEC.md`
- `docs/specs/s8qkn-subscription-sync/SPEC.md`

## 计划资产（Plan assets）

- Directory: `docs/specs/vgqp9-dashboard-social-activity/assets/`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 冻结 spec、tab 口径、baseline 规则与 feed mixed contract。
- [x] M2: 落地后端 social activity schema、同步链路与 mixed feed API。
- [x] M3: 落地 Dashboard 新 tabs、社交卡片、头像 fallback 与日组行为。
- [x] M4: 完成 Storybook、视觉证据、回归测试与 merge-ready 收口。
- [x] M5: 公告与 Fork 作为 ambient feed 事件进入 `全部` tab，不新增独立 tab。
