# 实现状态（修复 Release 自动发版断链并补齐漏发版本）

## 当前状态

- Lifecycle: active
- Implementation: 已实现
- Created: 2026-04-11
- Last: 2026-04-11
- Summary: 已实现；release runs on push@main with backfill/repair planning, and candidate scanning now covers squash/direct PR commits
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 实现里程碑

- [x] M1: 新建 release reliability spec，并冻结补发顺序与修复 PR label 约束。
- [x] M2: Release workflow 改为 `push@main` + `workflow_dispatch`，并加入 audit/backfill 规划与串行补发。
- [x] M3: `release-intent.sh` / `compute-version.sh` / 新 audit helper 完成幂等与 backfill 支撑。
- [x] M4: CI 自测覆盖 release automation contract，并通过真实 merge + backfill 验证。
- [x] M5: Backfill candidate 扫描覆盖 first-parent 主干提交，避免 squash/direct PR commit 被漏发队列和 repair 队列忽略。
