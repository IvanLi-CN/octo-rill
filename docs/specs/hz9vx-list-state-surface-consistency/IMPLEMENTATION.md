# 实现状态（列表状态一致性收敛）

## 当前状态

- Lifecycle: active
- Implementation: 已实现，待 PR 合并
- Created: 2026-06-30
- Last: 2026-06-30
- Summary: fast-track follow-up；统一后台列表五态 contract，首批覆盖 public releases / users / repos / jobs / inbox，含 Storybook 五态入口与视觉证据
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/README.md`：新增本规格索引与实现状态。
- 当前 spec companion docs 已同步：契约、实现状态、视觉证据与演进记录齐备。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 落地共享 list state hook 与渲染 primitives，冻结稳定 selector。
- [x] M2: 接入 `/admin/public-releases`、`/admin/users`、`/admin/repos`、`/admin/jobs` 主列表段、Dashboard `InboxList`。
- [x] M3: 补齐 Storybook 五态场景、视觉证据与 targeted validation。

## 验证结论（Validation Summary）

- `bun run build`：通过
- `bun run storybook:build`：通过
- Storybook 视觉证据：已落盘到 `SPEC.md` 的 `## Visual Evidence`
- Overlay refresh follow-up：刷新提示已改为不占文档流的覆盖层，并完成更新后的 `refreshing` 证据回拍

## 剩余事项（Remaining）

- 无实现缺口；剩余流程动作为提交分支、创建/收敛 PR 并完成合并。
