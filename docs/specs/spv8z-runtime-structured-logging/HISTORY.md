# 演进记录（运行时结构化日志合同与容器日志收口）

## 生命周期

- Lifecycle: active
- Created: 2026-06-23
- Last: 2026-06-23

## 变更记录

- 2026-06-23: 创建规格并锁定 `JSON Only`、`Metadata Only`、`Container-first`、`Slow+Error Only`、`Accept+Echo X-Request-Id` 与三类默认慢阈值。
- 2026-06-23: 保留现有 `tracing + tracing-subscriber + tower-http` 作为正式日志框架，不切换到 `log4rs`、`flexi_logger` 或平台绑定方案。
- 2026-06-23: 开始把 HTTP 入口、AI / sync 上游调用、SQLite 写热点统一到单行 JSON stdout 与最小必要排障字段合同。
