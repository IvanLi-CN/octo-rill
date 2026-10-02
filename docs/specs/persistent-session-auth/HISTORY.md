# 演进记录（GitHub 登录态持久化与稳定 session cookie）

## 生命周期

- Lifecycle: active
- Created: 2026-04-21
- Last: 2026-10-02

## 历史摘要

- 2026-04-21: 建立该主题规格并冻结基础范围。
- 2026-04-21: 已交付；fast-track / 30d sliding session + stable cookie-name config / PR #110
- 2026-10-02: PR3.9.2 candidate 将 session load 与 session write 分离到 reader/writer pool；session critical write 共享单调 foreground deadline，activity-only refresh 在 writer pressure 下保持原业务响应并禁止虚假续期 cookie。候选 `54e504ba` 补齐 stale session field merge、共享快照历史、无基线字段保留与 expiry 单调合并，并通过 fresh HTTP acceptance、真实语句中断恢复与质量门禁；review、CI 与 merge 仍待完成。

## 交付记录

- PR: #110 `fix: persist GitHub session cookies`
