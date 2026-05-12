# 实现状态（Release 失败 Telegram 告警接入）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Last: 2026-05-11
- Summary: 已交付；manual Release dispatches now have an inline failure notifier, while push Release failures stay on the standalone workflow_run notifier
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 实现说明

- `.github/workflows/release.yml` contains an inline `notify-on-failure` job that calls the shared Telegram notifier when a manual `workflow_dispatch` Release run fails.
- `.github/workflows/notify-release-failure.yml` remains responsible for `push@main` Release failures and manual smoke testing, avoiding duplicate production alerts.
