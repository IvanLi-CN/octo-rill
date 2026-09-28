---
title: 公开指标 API
description: 读取 OctoRill 仓库刷新治理指标。
---

# 公开指标 API

此只读接口提供 OctoRill 的仓库刷新治理统计。它不要求登录，也不接受 Cookie 或 API Key，可供外部网页读取。

## 请求

```http
GET /api/public/metrics/v1/octo-rill
```

请向 OctoRill 后端部署域名发出请求。接口只支持 `GET`，不支持 `HEAD`。浏览器调用时，页面 origin 必须在后端的 `OCTORILL_PUBLIC_METRICS_CORS_ORIGINS` allowlist 中。

## 响应

```json
{
  "deduplicatedRepositories": {
    "value": 126,
    "trend": [121, 124, 126]
  },
  "pressure": {
    "value": 0.8,
    "trend": [1.1, 0.9, 0.8]
  },
  "freshness": [0, 1, 2, 3, 4]
}
```

只有以上三个顶层字段会公开：

- `deduplicatedRepositories.value` 是治理快照中的去重仓库数；`trend` 是该统计最近可用的小时趋势。
- `pressure.value` 是当前仓库刷新压力，定义为 `sum(max(0, min(urgency_score, 4) - 1)) / repo_refresh_system_budget_per_window`；其 `trend` 是同口径小时趋势。
- `freshness` 按治理优先级排列，与仓库明细不带仓库名称或标识。数值 `0` 表示最近成功刷新不超过 4 小时，`1` 不超过 12 小时，`2` 不超过 24 小时，`3` 超过 24 小时，`4` 表示没有成功刷新记录。

## 趋势数据

服务第一次成功聚合时会立即返回当前统计，并记录第一个真实样本。服务每小时保留一个样本，最多返回最近 12 个 UTC 小时样本，按时间从旧到新排列；同一小时再次采样会更新为该小时最近的真实统计。

新部署或历史不足时，`trend` 会包含当前真实统计点及已有历史点，通常只有 1 到 12 个。服务不会补零、插值或伪造历史。趋势点会随服务持续运行逐步累积。

## 缓存与条件请求

响应带有 `ETag` 和 `Cache-Control`。后续请求可发送 `If-None-Match: <etag>`；数据未变化时返回 `304 Not Modified`。服务按需刷新并每 5 分钟后台刷新；刷新失败时继续返回最后一次成功数据。服务启动初期若尚无成功聚合结果，暂时返回 `503` 和 `Retry-After`。

每个客户端 IP 每分钟最多请求 120 次；超限返回 `429` 和 `Retry-After`。

## CORS 配置

默认允许 `https://ivanli.cc` 与本地 `http://127.0.0.1:12620`。自托管时可配置：

```dotenv
OCTORILL_PUBLIC_METRICS_CORS_ORIGINS=https://app.example.com,http://127.0.0.1:12620
```

这里只填写 origin，不带路径；接口不启用 credential CORS。该 allowlist 独立于 OctoRill 现有登录接口。
