# 实现状态（Dashboard 同步入口收敛与顺序固定）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-03-27
- Last: 2026-08-09
- Summary: 已交付；同步进度气泡支持 outside-click / Escape 临时关闭，并通过 Tooltip 原语的 hover、pointer move、聚焦或点击同步按钮恢复当前详情；重复点击不再发出替代详情的 toast
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/README.md`
- `docs/specs/dashboard-sync-unification/SPEC.md`
- `docs/specs/dashboard-sync-unification/IMPLEMENTATION.md`
- `docs/specs/dashboard-sync-unification/HISTORY.md`

## 计划资产（Plan assets）

- Directory: `docs/specs/dashboard-sync-unification/assets/`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新建 spec 并写入 `docs/specs/README.md`。
- [x] M2: 完成 Dashboard 同步入口与文案收敛。
- [x] M3: 完成 Storybook、视觉证据、快车道 PR 与 review-loop 收敛。
- [x] M4: 同步进度气泡支持点击空白处或 `Escape` 关闭，并补齐 Storybook play 回归。
- [x] M5: 收起后的同步进度气泡支持 hover、聚焦或点击恢复，重复点击不再显示替代详情的 toast，并补齐 Dashboard E2E 回归。
