# 演进记录（Release Feed 正文卡片与同步后后台翻译）

## 生命周期

- Lifecycle: superseded
- Created: 2026-04-03
- Last: 2026-04-13
- Superseded by: [../release-translation-input-budget-runtime/SPEC.md](../release-translation-input-budget-runtime/SPEC.md)

## 历史摘要

- 2026-04-03: 建立该主题规格并冻结基础范围。
- 2026-04-13: 已由 #y2yf8 接替；historical body-limit contract superseded by #y2yf8 LLM-input-budget chunk translation
- 2026-06-29: Dashboard feed 对 retryable `translated.error` 新增当前页面会话内一次性自动补救；自动补救进行中改为居中的中性等待面，失败后再恢复原有错误块与手动重试按钮，并补充 Storybook / Playwright 证据。
- 2026-07-07: 扩展 Dashboard feed translated lane retryable 错误分类，覆盖响应体解码、AI/GitHub 发送请求失败、缺失 AI content、DNS/TLS/proxy/连接中断与 upstream 500 等瞬时失败；配置、模型、GitHub compare 语义、Markdown 结构和正文过大错误仍保留终态。
