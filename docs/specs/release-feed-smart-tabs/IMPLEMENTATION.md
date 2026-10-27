# 实现状态（Release Feed 三 Tabs 与润色版本变化卡片）

## 当前状态

- Lifecycle: active
- Implementation: 三态阅读交互与结构化 Release 阅读结果已接入；权限隔离和集成验证待完成。
- Created: 2026-04-07
- Last: 2026-06-29
- Summary: 已交付；PR #53; page-level lane selector, segmented selector polish, visual evidence refreshed, translation empty-content retries capped at 8 attempts in-call, native title removed from card lane tooltip triggers, Dashboard page-level lane selector styling refreshed with Storybook evidence, upstream chat 403 release translation/polish failures are recoverable through foreground generation plus `retry.recent_failures`, release-smart canonical feed lookup now anchors on the target release before repo-local previous-tag resolution to avoid global visible-release sorting under translation load, Dashboard feed now auto-rescues retryable smart failures once per page session with a neutral waiting state, and the shared lane contract is reused by announcement reading in `announcement-discussion-reading`
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 内容规则实现缺口

本主题的历史交付包含遗留 body-first / diff fallback；当前 global 路径已接入 Release 专属 prompt、语义版本基线和结构化 `reading`。具体内容与剩余集成验证见 [统一阅读内容实现状态](../release-reading-content/IMPLEMENTATION.md)。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: Spec 与 API/数据契约冻结。
- [x] M2: 后端 `release_smart` 调度、prompt、diff fallback 与 `/api/feed` smart lane 完成。
- [x] M3: 前端三 tabs、呼吸态、折叠卡片与 on-demand 生成完成。
- [x] M4: Storybook / Playwright / visual evidence / PR merge-ready 收敛完成。
- [x] M5: 上游聊天通道 403 失败在 release 翻译 / 润色链路中进入前台与后台自动恢复路径。
- [x] M6: release-smart canonical feed lookup 改为 target-first + repo-local previous-tag 回查，避免翻译链路在可见 release 全集上做窗口排序。
