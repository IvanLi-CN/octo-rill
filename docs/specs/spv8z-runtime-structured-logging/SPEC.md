# 运行时结构化日志合同与容器日志收口（#spv8z）

## 背景 / 问题陈述

当前 OctoRill 运行时已经使用 `tracing` 栈，但容器 stdout 仍然缺少明确的结构化合同、慢请求打点边界、request-id 贯通规则，以及“允许披露什么 / 禁止披露什么”的稳定约束。部署在 Docker 容器内时，维护者需要靠 `docker logs`、`jq` 与少量 grep 就能完成大部分一线排障，而不是依赖临时文本格式或额外平台绑定。

## 目标 / 非目标

### Goals

- 统一进程日志为 `JSON Only` 单行 stdout，正式日志框架继续采用现有 `tracing + tracing-subscriber + tower-http`。
- 固定 `Container-first` 运行时日志合同：容器 stdout 为主，现有 `OCTORILL_TASK_LOG_DIR` 文件任务日志保留为补充通道，不重写为主后端。
- 固定 `Metadata Only + error chain` 披露面：日志保留稳定排障元数据，不记录 body、token、cookie、email、login、完整 query string，不默认输出 Rust backtrace。
- 固定 HTTP access log 为 `Slow+Error Only`，同时落地 `Accept+Echo X-Request-Id` 契约。
- 为高价值慢路径与异常路径补统一字段：`event`、`operation`、`attempt`、`elapsed_ms`、`error_kind`、`error_chain`、`request_id`、`user_id`、`repo_id` 等。

### Non-goals

- 不切换到 `log4rs`、`flexi_logger`、平台专属 agent SDK 或集中式日志平台绑定。
- 不改写历史 task file logs 格式，也不把它们提升成统一主日志后端。
- 不把所有现有 `info!/warn!` 全量重构为新 schema；本轮只收口 HTTP 入口、AI / sync 上游调用热点、SQLite 写热点。
- 不记录 request / response body、完整 headers、OAuth / AI token、Cookie、email、login、完整 query string。

## 范围（Scope）

### In scope

- `Cargo.toml`：启用 `tracing-subscriber/json` 与 `tower-http/request-id` 所需 feature。
- `src/main.rs` / `src/observability.rs`：统一 JSON subscriber、慢阈值配置、request-id / access log helper、error chain helper。
- `src/server.rs`：统一 request-id + trace + slow/error access log middleware。
- `src/ai.rs` / `src/sync.rs`：补上游调用慢 / 错日志元数据，去掉容器日志中的 login 等敏感字段。
- `src/sqlite_write.rs` / `src/session_store.rs`：统一 SQLite 写争用、慢写、retry 字段命名与阈值判断。
- `docs-site/docs/config.md`、`docs/architecture.md`、`.env.example`：同步运行配置、排障入口与长期约束。

### Out of scope

- 多实例集中检索、日志聚合平台查询模型、vendor-specific 字段。
- 任务文件日志体系改造。
- 生产默认开启 backtrace。

## 运行时合同（Runtime Contract）

### 进程与容器日志

- 进程 stdout 必须输出单行 JSON。
- 应用内不提供 pretty/text 双格式切换；本地开发通过 `jq`、`lnav` 或包装脚本解决人类可读性。
- `RUST_LOG` 是唯一的日志级别入口。

### HTTP request-id 契约

- 若请求自带 `X-Request-Id`，服务端必须原样接受并回传到响应头。
- 若请求未带 `X-Request-Id`，服务端必须生成新值并回传到响应头。
- access log 与异常日志必须能通过同一 `request_id` 关联。

### HTTP access log

- 只记录慢请求与错误请求。
- 快速 `2xx` 默认不打 access log。
- 至少包含字段：`event=http.access`、`request_id`、`method`、`route`、`status`、`latency_ms`。
- `route` 记录路由模板或 path，不记录完整 query string。

### 慢阈值

- `OCTORILL_HTTP_SLOW_MS` 默认 `1000`。
- `OCTORILL_UPSTREAM_SLOW_MS` 默认 `2000`。
- `OCTORILL_SQLITE_WRITE_SLOW_MS` 默认 `250`。
- 三类阈值均允许通过 env 覆盖。

### 脱敏与禁止项

- 禁止记录 request / response body。
- 禁止记录 `Authorization`、Cookie、OAuth token、AI token。
- 禁止记录 email、login、完整 query string。
- 生产异常日志默认输出 `error_chain`，不默认输出 Rust backtrace。

### Task log 边界

- `OCTORILL_TASK_LOG_DIR` 继续用于后台任务专项文件日志。
- 它不是本轮统一主日志通道，不替代容器 stdout 合同。
- 容器 stdout 负责跨请求 / 跨模块的一线排障；task file log 负责任务级细节补充。

## 验收标准（Acceptance Criteria）

- Given 应用启动
  When 任何运行时日志输出到 stdout
  Then 每条日志都是单行 JSON，且可直接被 `jq` 解析。

- Given 请求未携带 `X-Request-Id`
  When 调用任意 HTTP 路由
  Then 响应头回传服务端生成的 `X-Request-Id`，对应日志可见同一 `request_id`。

- Given 请求已携带 `X-Request-Id`
  When 调用任意 HTTP 路由
  Then 响应头原样回显该值，且 access / error log 中使用同一值。

- Given 一个快速 `2xx` 请求
  When `latency_ms < OCTORILL_HTTP_SLOW_MS`
  Then 默认不输出 `http.access` 事件。

- Given 一个慢 `2xx/3xx` 请求或任意 `4xx/5xx`
  When 请求完成
  Then 输出 `http.access` 事件并携带标准字段。

- Given AI / sync / SQLite 热路径失败或变慢
  When 触发对应日志
  Then 至少带 `operation`、`attempt`、`elapsed_ms`、`error_kind`、`error_chain` 中的相关字段。

- Given 日志中出现用户或仓库相关标识
  When 维护者查看容器 stdout
  Then 只看到 `request_id`、`task_id`、`user_id`、`repo_id`、`connection_id` 等稳定内部标识，而不是 login / email / token / body。
