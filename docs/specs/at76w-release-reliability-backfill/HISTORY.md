# 演进记录（修复 Release 自动发版断链并补齐漏发版本）

## 生命周期

- Lifecycle: active
- Created: 2026-04-11
- Last: 2026-04-11

## 历史摘要

- 2026-04-11: 建立该主题规格并冻结基础范围。
- 2026-04-11: 待实现；release trigger cut over to push@main + backfill queue planned
- 2026-05-09: 发布补发扫描从 first-parent merge commit 扩展为 first-parent 主干提交，修复 squash/direct PR commit 无法进入 backfill/repair 队列的问题。
- 2026-05-09: 同一 PR 多个 first-parent 主干提交时只保留最后一个提交，避免 rebase merge PR 被重复发版。
