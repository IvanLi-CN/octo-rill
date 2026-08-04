# 实现状态（仓库级 Worktree Bootstrap）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-03-06
- Last: 2026-03-07
- Summary: 已交付；local implementation completed; repo-local hook installer + worktree smoke CI
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `README.md`: 新增 repo-local hook 安装与 `.env.local` worktree bootstrap 说明。
- `docs/specs/README.md`: 新增并同步该规格状态。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 增加 repo-local hook 安装入口与 `post-checkout` wiring。
- [x] M2: 落地 worktree 同步脚本与资源清单。
- [x] M3: 增加 smoke test 与 CI matrix 校验，并同步文档与规格状态。
