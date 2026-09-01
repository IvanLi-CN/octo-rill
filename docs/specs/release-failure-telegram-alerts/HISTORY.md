# 演进记录（Release 失败 Telegram 告警接入）

## 生命周期

- Lifecycle: active
- Last: 2026-05-11

## 变更记录

- 2026-04-11: 为 `octo-rill` 接入共享 Telegram 发布失败告警与 repo-local smoke test。
- 2026-05-11: Release workflow 为 `workflow_dispatch` 补发路径增加内联失败告警 job，普通 `push@main` 失败继续走独立 `workflow_run` notifier，避免重复告警。
- 2026-09-01: 三处失败通知调用迁移到 Oidrune `v0.1.14` 的完整提交 `e48822f99c6402a753ed86557ea029754cbab20b`；调用方改用 OIDC 权限和显式完整 summary，移除旧 Telegram secret wiring 与网关覆盖参数，保留原有过滤、判定和 smoke 路径。
