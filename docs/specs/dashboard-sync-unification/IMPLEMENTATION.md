# 实现状态（Dashboard 同步入口收敛与顺序固定）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-03-27
- Last: 2026-09-23
- Summary: 同步详情默认收起并由用户主动恢复；按钮背景以 SSE 确认阶段为锚点展示受限、连续、平滑的预测进度，并覆盖刷新成功、失败、断线和 reduced-motion
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
- [x] M6: 同步开始时详情默认收起，主动交互才恢复 tooltip，并补齐默认收起回归。
- [x] M7: 将确认阶段与按钮预测填充解耦，实现单调、不越界、可平滑校准的视觉进度；覆盖断线、刷新完成、失败和 reduced-motion。
- [x] M8: 更新 Storybook 与视觉证据，覆盖桌面端和移动端的预测填充、阶段校准、成功与失败终态。
