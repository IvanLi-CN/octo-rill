# Release Feed 正文卡片与同步后后台翻译（#3k9fd）

## 背景

- 现有 release feed 只返回摘录式 `excerpt`，正文阅读被拆到独立 detail 链路。
- 当前同步完成后不会自动预热新 release 的译文，列表首屏需要等懒加载自动翻译。
- 主列表应直接承担 release 正文阅读职责；对超长正文必须避免触发一次无法承载的翻译请求。

## 目标

- feed 卡片直接展示 release 正文，并按统一字符上限裁切。
- feed 自动翻译改为翻译正文卡片内容，而不是摘要摘录。
- `sync.releases`、`sync.all` 的 release 阶段与 `sync.subscriptions` 在写入新增 release 后，自动追加异步后台翻译任务。
- 超长正文直接落终态失败，不做分块翻译。

## 关键约束

- `RELEASE_FEED_BODY_MAX_CHARS = 3000` 是单一真相源：
  - feed 卡片原文正文按此上限裁切显示；
  - 后台判定“正文超长不可单次翻译”也按此上限执行。
- 列表正文使用 Markdown 原文结构，不再构造 `excerpt` 作为主展示内容。
- 后台自动翻译只覆盖本轮同步新增的 release，不回扫历史全量数据。
- 现有 `release_detail` API 和 UI 保留兼容，不作为主列表正文唯一来源。

## 实现要求

- Feed API:
  - release item 返回 `body` 与 `body_truncated`。
  - `ai_translations(entity_type='release')` 的 source hash 改为基于正文卡片内容。
- Feed UI:
  - 卡片正文读 `body`，中文视图读翻译后的正文字段。
  - 当 `body_truncated=true` 时显示“列表正文已截断显示”的提示。
  - 当自动翻译终态为不可自动重试时，不再提供“翻译”按钮重试入口。
- 翻译:
  - `release_summary` / feed 变体切到正文输入（`body_markdown`）。
  - `release_summary.feed_body -> release_detail` fallback 在进入 Markdown 结构校验前，先规范化外层 fenced Markdown、包装空白与尾部换行；规范化后仍不保结构才记为失败。
  - 超长正文直接写入 `error` 终态，并带明确错误文案。
- 同步:
  - 手动同步与订阅同步完成后，基于同步前后 release id 差集收集新增 release。
  - 为每个相关用户异步追加 `translate.release.batch` 后台任务，不等待其完成。

## 验收

- `/api/feed` 返回的 release 项不再依赖 `excerpt` 作为正文主字段。
- Dashboard release 卡片默认展示正文；中/原文切换作用于同一张卡片正文区域。
- sync 后新增 release 会自动触发后台翻译任务；再次打开列表时优先命中已预热译文。
- 正文超过 3000 字符的 release 不进入分块翻译；后台记录明确 `error` 终态。
- `release_summary.feed_body` 因 Markdown 结构不一致失败时，后台保留分类后的短提示与原始错误原因，不再只显示 `translation failed`。
