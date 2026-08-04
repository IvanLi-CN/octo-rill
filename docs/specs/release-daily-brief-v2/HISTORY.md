# 演进记录（Release 日报内容格式 V2 与历史快照修复）

## 生命周期

- Lifecycle: active
- Created: 2026-04-16
- Last: 2026-07-19

## 变更记录

- 2026-04-16: 创建规格，冻结“移除概览/窗口、补社交摘要、支持历史内容刷新”的实现口径。
- 2026-04-19: 收紧 brief canonical Markdown 契约，新增结构校验 / deterministic fallback，并把 V2 正文层级漂移纳入历史刷新。
- 2026-05-10: 明确日报 release 要点默认简体中文倾向；AI 不可用或摘要不可解析时，deterministic fallback 改为中文提示式摘要，不直接复用原始 release notes bullet。
- 2026-07-19: 日报 release 要点复用 `release_smart` 的 valuable / compare fallback 语义，低信息 release 不再补伪摘要；历史 brief 可原位刷新去伪摘要。

## 变更记录（Change log）

- 2026-04-16: 创建规格，冻结“移除概览/窗口、补社交摘要、支持历史内容刷新”的实现口径。
- 2026-04-19: 收紧 brief canonical Markdown 契约，新增结构校验 / deterministic fallback，并把 V2 正文层级漂移纳入历史刷新。
- 2026-04-22: 同步 brief release 主链接 current truth；新生成内容默认输出 `/<owner>/<repo>/releases/tag/<tag>?from=briefs`，legacy query 链接继续兼容。
- 2026-07-19: 日报 release 要点改为复用 `release_smart` 的 valuable / compare fallback 事实链路；低信息 release 不再生成伪摘要，历史 brief refresh 继续原位修复。
