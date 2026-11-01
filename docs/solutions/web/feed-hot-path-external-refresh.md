---
title: Feed hot path external refresh split
module: web
problem_type: performance
component: dashboard-feed
tags:
  - api-hot-path
  - async-refresh
  - external-dependency
status: active
related_specs:
  - docs/specs/feed-hot-path-reaction-refresh/SPEC.md
---

# Feed hot path external refresh split

## Context

首屏 API 如果要在用户可感知时间内开始返回，热路径必须只依赖本地、可控、低抖动资源。第三方 API 即使平均很快，也会因为 TLS、限流、网络抖动或权限错误把首屏 skeleton 拖住。

## Symptoms

- 页面壳层和侧栏已经出现，主 feed 仍停留在 skeleton。
- 接口逻辑先查本地数据库，再同步请求 GitHub GraphQL 或其它外部服务。
- 外部请求失败或慢速时，用户无法先看到缓存数据。

## Root cause

首屏“读取 feed”与“刷新外部最新状态”耦合在同一个 HTTP 请求中，导致 TTFB 由最慢外部依赖决定，而不是由本地缓存决定。

## Resolution

- 把 feed/list 类首屏接口收敛为本地缓存热路径。
- 外部最新状态改为独立 refresh API、后台任务或 SSE 后续刷新。
- 首屏响应保持兼容 shape，允许部分字段先返回缓存/默认值。
- 异步 refresh 成功后合并进当前 UI，并按需持久化回缓存。
- 异步 refresh 失败只影响对应增强字段，不阻断主内容展示。
- 前端 batch 大小必须匹配后端解析上限；分页累积列表不能把所有已渲染 ID 无界塞进单个 refresh 请求，也不能对已刷新或已在 refresh 中的同一 ID 重复发起请求。
- 在迁移完成前必须给遗留 compare 调用加应用级截止时间和尝试次数上限；超时、503、认证失败和文件数量超限都要进入可观测的终止路径。

## Guardrails / Reuse notes

- 热路径里不要同步调用 GitHub、OpenAI、对象存储、第三方图像探测或其它不可控网络。
- 如果必须保留最新性，优先使用“缓存先返回 + refresh after render + task/SSE completion refresh”。
- 对 optimistic UI 或用户正在提交的局部状态，异步 refresh 合并前必须检查 pending guard，避免旧状态覆盖新操作；缓存 feed 重新替换或合并时也要保留已知 live viewer 状态，避免默认值短暂回滚 UI。
- 如果缓存默认值可能改变用户操作语义，应优先让 mutation API 做实时校验或采用显式 desired-state 设计兜底；后台 refresh 不应通过额外 UI 阻断首屏或正常操作。
- 给热路径和 refresh path 分别记录耗时，防止外部依赖重新混入首屏接口。
- GitHub compare 指标使用低基数 `provider=github, operation=compare`，结果至少区分 `success`、`timeout`、`service_unavailable`、`authentication_failed`、`too_many_files`、`fallback` 和 `enrichment`；Feed 路由耗时继续由 HTTP route 指标记录。
- 当前临时护栏基线为单次 compare 总截止时间 3 秒、最多 8 次请求、最多 64 个文件；这是保护性假设，不是已确认的产品 SLO。迁移到后台 evidence collector 后，应以采集队列和实际 GitHub 分位数重新校准。
- Feed 只允许精确错误码 `release_reading_evidence_unavailable` 降级为原始 Release/邻近动态继续返回；数据库、权限、解析、认证和其他 GitHub/API 错误必须继续终止请求并保留错误观测。

## References

- `docs/specs/feed-hot-path-reaction-refresh/SPEC.md`
- `src/api.rs`
- `web/src/pages/Dashboard.tsx`
