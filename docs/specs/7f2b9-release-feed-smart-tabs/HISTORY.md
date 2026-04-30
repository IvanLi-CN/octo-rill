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
