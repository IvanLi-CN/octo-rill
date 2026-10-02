# 演进记录（GitHub 登录态持久化与稳定 session cookie）

## 生命周期

- Lifecycle: active
- Created: 2026-04-21
- Last: 2026-10-02

## 历史摘要

- 2026-04-21: 建立该主题规格并冻结基础范围。
- 2026-04-21: 已交付；fast-track / 30d sliding session + stable cookie-name config / PR #110
- 2026-10-02: PR3.9.2 candidate 将 session load 与 session write 分离到 reader/writer pool；session critical write 共享单调 foreground deadline，activity-only refresh 在 writer pressure 下保持原业务响应并禁止虚假续期 cookie。前一候选已通过 fresh HTTP acceptance；本轮补充 stale session field merge，最终候选仍待重新验收、review、CI 与 merge。

## 交付记录

- PR: #110 `fix: persist GitHub session cookies`
