# 实现状态（修复 Release 自动发版断链并补齐漏发版本）

## 当前状态

- Lifecycle: active
- Implementation: 待实现
- Created: 2026-04-11
- Last: 2026-04-11
- Summary: 待实现；release trigger cut over to push@main + backfill queue planned
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 实现里程碑

- [ ] M1: 新建 release reliability spec，并冻结补发顺序与修复 PR label 约束。
- [ ] M2: Release workflow 改为 `push@main` + `workflow_dispatch`，并加入 audit/backfill 规划与串行补发。
- [ ] M3: `release-intent.sh` / `compute-version.sh` / 新 audit helper 完成幂等与 backfill 支撑。
- [ ] M4: CI 自测覆盖 release automation contract，并通过真实 merge + backfill 验证。
