# 实现状态（Release Feed 三 Tabs 与润色版本变化卡片）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-07
- Last: 2026-06-29
- Summary: 已交付；PR #53; page-level lane selector, segmented selector polish, visual evidence refreshed, translation empty-content retries capped at 8 attempts in-call, native title removed from card lane tooltip triggers, Dashboard page-level lane selector styling refreshed with Storybook evidence, upstream chat 403 release translation/polish failures are recoverable through foreground generation plus `retry.recent_failures`, release-smart canonical feed lookup now anchors on the target release before repo-local previous-tag resolution to avoid global visible-release sorting under translation load, and Dashboard feed now auto-rescues retryable smart failures once per page session with a neutral waiting state
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: Spec 与 API/数据契约冻结。
- [x] M2: 后端 `release_smart` 调度、prompt、diff fallback 与 `/api/feed` smart lane 完成。
- [x] M3: 前端三 tabs、呼吸态、折叠卡片与 on-demand 生成完成。
- [x] M4: Storybook / Playwright / visual evidence / PR merge-ready 收敛完成。
- [x] M5: 上游聊天通道 403 失败在 release 翻译 / 润色链路中进入前台与后台自动恢复路径。
- [x] M6: release-smart canonical feed lookup 改为 target-first + repo-local previous-tag 回查，避免翻译链路在可见 release 全集上做窗口排序。
