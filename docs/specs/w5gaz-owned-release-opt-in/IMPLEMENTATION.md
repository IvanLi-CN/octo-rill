# 实现状态（“我的发布”开关与自有仓库 Release 可见性扩展）

## 当前状态

- Lifecycle: active
- Implementation: 部分完成（3/4）
- Created: 2026-04-20
- Last: 2026-04-20
- Summary: 部分完成（3/4）；PR #101 open; local implementation + validation + owner-facing evidence landed
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `docs/specs/README.md`
- `docs/specs/w5gaz-owned-release-opt-in/SPEC.md`

## 计划资产（Plan assets）

- Directory: `docs/specs/w5gaz-owned-release-opt-in/assets/`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新增 spec、README 索引、`include_own_releases` migration 与 profile 契约扩展。
- [x] M2: Release 可见性统一改到 `user_release_visible_repos`，并接通同步 / 详情 / 翻译 / 润色 / 日报。
- [x] M3: Settings / Dashboard Storybook、Playwright 与 owner-facing 视觉证据完成。
- [ ] M4: 提交、推送、PR、review-loop 收敛到 merge-ready。
