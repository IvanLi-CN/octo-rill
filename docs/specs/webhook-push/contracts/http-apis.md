# HTTP APIs

## User APIs

- `GET /api/me/webhook-push`：读取目标状态、当前操作、当前用户最近完成检查时间、前置条件、汇总、Owner 分组和仓库观察状态。
- `PATCH /api/me/webhook-push`：请求体 `{ "desired_state": "enabled" | "paused" | "deleted" }`；启用时只读取已持久化的 PAT、callback 和 secret 前置条件，所有 GitHub 访问与 Hook 动作异步排队。
- `POST /api/me/webhook-push/reconcile`：请求体可选 `{ "repo_id": number }`；由用户手动发起检查并修复全量或单仓，PAT 的实时有效性由后台 worker 检查。用户已有未终态操作时返回 `409 webhook_push_operation_in_progress`。
- `PATCH /api/me/profile`：除日报时区和 `include_own_releases` 外，可选接受 `webhook_push_desired_state`；当该字段改变时与 profile 更新一起持久化并排队同一用户的对齐任务，入队失败不会留下未对齐的目标状态。

会排队后台任务的写操作返回 `{ "task_id": string, "status": string, "operation": string }`，前端通过现有 task API/SSE 跟踪。目标状态写入成功后，即使远端任务失败也保留该目标。

## Admin APIs

- `GET /api/admin/jobs/webhook-push/runtime-config`
- `PATCH /api/admin/jobs/webhook-push/runtime-config`，请求体 `{ "audit_interval_days": integer }`，范围 `1..=30`。

## Receiver

- `POST /api/webhooks/github/releases`
- 必需 headers：`X-GitHub-Delivery`、`X-GitHub-Event`、`X-GitHub-Hook-ID`、`X-Hub-Signature-256`；`ping` 不要求已存在 release payload。
- 成功响应 `{ "accepted": true, "queued": boolean, "reason": string }`。
