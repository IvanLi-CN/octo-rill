# HTTP APIs

## User APIs

- `GET /api/me/webhook-push`：读取目标状态、当前操作、当前用户最近完成检查时间、前置条件、汇总、Owner 分组、仓库观察状态和最新终态失败。
- `PATCH /api/me/webhook-push`：请求体 `{ "desired_state": "enabled" | "paused" | "deleted" }`；启用时只读取已持久化的 PAT、callback 和 secret 前置条件，所有 GitHub 访问与 Hook 动作异步排队。
- `POST /api/me/webhook-push/reconcile`：请求体可选 `{ "repo_id": number }`；由用户手动发起检查并修复全量或单仓，PAT 的实时有效性由后台 worker 检查。启用目标在“我的发布”关闭时拒绝；暂停或删除目标仍允许用户修复遗留 Hook。用户已有未终态操作时返回 `409 webhook_push_operation_in_progress`。
- `PATCH /api/me/profile`：只更新日报时区和 `include_own_releases`；关闭“我的发布”时会将启用目标转为暂停并排队对齐。Webhook 目标状态只能通过专用 PATCH 路由变更，避免绕过启用前置校验。

会排队后台任务的写操作返回 `{ "task_id": string, "status": string, "operation": string }`，前端通过现有 task API/SSE 跟踪。目标状态写入成功后，即使远端任务失败也保留该目标。

`GET /api/me/webhook-push` 的 `summary.pending` 是当前仓库观察中需要用户关注或仍在对齐的数量。`last_operation_failure` 为最新终态失败任务的 `{ task_id, operation, error_message, failed_at, retry_count }`；存在更晚的 queued/running 或 succeeded manage 任务时返回 `null`。

定时 audit 任务只负责枚举用户并投递带 `scheduled: true` 的 `webhook.push.manage` 任务；它不执行 GitHub 请求，也不持有用户级远端操作 lease。

## Admin APIs

- `GET /api/admin/jobs/webhook-push/runtime-config`
- `PATCH /api/admin/jobs/webhook-push/runtime-config`，请求体 `{ "audit_interval_days": integer }`，范围 `1..=30`。

## Receiver

- `POST /api/webhooks/github/releases`
- 必需 headers：`X-GitHub-Delivery`、`X-GitHub-Event`、`X-GitHub-Hook-ID`、`X-Hub-Signature-256`；`ping` 不要求已存在 release payload。
- 成功响应 `{ "accepted": true, "queued": boolean, "reason": string }`。
