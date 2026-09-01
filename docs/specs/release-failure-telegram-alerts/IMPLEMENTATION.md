# 实现状态（Release 失败 Telegram 告警接入）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Last: 2026-09-01
- Summary: 已交付；three release-failure call sites now use the pinned Oidrune OIDC notifier with caller-owned summaries
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 实现说明

- `.github/workflows/notify-release-failure.yml` migrates both `notify_failure` and `smoke_test` calls to `IvanLi-CN/oidrune/.github/workflows/notify.yml@e48822f99c6402a753ed86557ea029754cbab20b`.
- `.github/workflows/release.yml` keeps the inline `notify-on-failure` job for manual Release failures and migrates its direct call to the same pinned Oidrune workflow.
- Each caller job grants `id-token: write` and supplies `outcome` plus a complete summary containing project, status/result, target SHA, run URL, and the applicable failure, smoke, or release title.
- The original workflow filters, failure predicates, Release context, project-specific behavior, and manual smoke entry remain unchanged; the legacy Telegram secret wiring and gateway overrides are removed.
