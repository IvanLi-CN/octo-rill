# 演进记录（Release 失败 Telegram 告警接入）

## 生命周期

- Lifecycle: active
- Last: 2026-05-11

## 变更记录

- 2026-04-11: 为 `octo-rill` 接入共享 Telegram 发布失败告警与 repo-local smoke test。
- 2026-05-11: Release workflow 为 `workflow_dispatch` 补发路径增加内联失败告警 job，普通 `push@main` 失败继续走独立 `workflow_run` notifier，避免重复告警。
