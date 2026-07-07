# 演进记录（“我的发布”开关与自有仓库 Release 可见性扩展）

## 生命周期

- Lifecycle: active
- Created: 2026-04-20
- Last: 2026-04-20

## 历史摘要

- 2026-04-20: 建立该主题规格并冻结基础范围。
- 2026-04-20: 部分完成（3/4）；PR #101 open; local implementation + validation + owner-facing evidence landed
- 2026-07-07: owner repo baseline 增加真实 `is_private` 持久化，`user_release_visible_repos` owned 分支不再写死 public，为私有 owner repo 的 OctoRill 内公开发布提供权限事实来源。
