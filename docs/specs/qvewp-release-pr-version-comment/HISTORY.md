# 演进记录（Release 成功后回写 PR 版本评论）

## 生命周期

- Lifecycle: active
- Created: 2026-04-04
- Last: 2026-04-28

## 历史摘要

- 2026-04-04: 建立该主题规格并冻结基础范围。
- 2026-04-04: 已交付；PR #46, PR #47; release run #43 comments PRs and rerun updates in place
- 2026-04-28: Release backfill 的 GitHub API 读取请求增加有界重试，缓解 GitHub 5xx / 429 / 网络瞬断导致的发布计划失败。
- 2026-06-23: 补齐 `http.client.RemoteDisconnected` 回归覆盖；Release backfill 现在会把 GitHub 远端直接断开连接视为 GET 瞬时失败并进入既有退避重试。
