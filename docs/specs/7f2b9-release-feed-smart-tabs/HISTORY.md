# 演进记录（Release Feed 三 Tabs 与润色版本变化卡片）

## 生命周期

- Lifecycle: active
- Created: 2026-04-07
- Last: 2026-04-30

## 变更记录

- 2026-04-08：补齐页面级默认 lane selector、单卡 icon-only selector，以及翻译 / 润色加载时继续显示原文的交互口径与视觉证据。
- 2026-04-08：补齐页面级切换复用单卡按需生成逻辑，并收紧 segmented selector 的选中 / 加载态层级。
- 2026-04-30：移除单卡 icon-only selector 触发器上的浏览器原生 `title`，确保 `原文 / 翻译 / 润色` 只显示产品内 tooltip，并补充 Storybook 回归断言。
- 2026-04-30：刷新 Dashboard 页面级阅读模式切换器的产品化样式合同，补充暗色桌面与窄平板 Storybook 视觉证据，确保控件与管理员入口同高、同基线、低阴影。
- 2026-06-01：将 release 翻译 / 润色链路中的上游聊天通道 403 归为可恢复终态，由前台按需生成和后台 `retry.recent_failures` 重新排队，底层 AI 单次调用仍快速失败。
- 2026-06-25：release-smart canonical feed lookup 改为先按目标 `release_id` 收口，再按 repo 内排序键回查 `previous_tag_name`，并补充 query-plan 回归，避免翻译 canonicalization 在 `user_release_visible_repos` 全集上做窗口排序。
- 2026-06-29：Dashboard feed 对 retryable `smart.error` 新增当前页面会话内一次性自动补救；自动补救进行中改为居中的中性等待面，失败后再恢复原有错误块与手动重试按钮，并补充 Storybook / Playwright 证据。
