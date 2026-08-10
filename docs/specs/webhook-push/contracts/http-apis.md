# HTTP APIs

## User APIs

- `GET /api/me/webhook-push`：读取配置、前置条件、汇总、仓库状态与调度时间。
- `PATCH /api/me/webhook-push`：请求体 `{ "enabled": boolean }`；开启时校验前置条件并排队全量注册。
- `POST /api/me/webhook-push/register`：人工全量注册，包含权限暂停仓库。
- `POST /api/me/webhook-push/check`：人工全量只读检查。
- `DELETE /api/me/webhook-push/hooks`：关闭状态下异步批量删除。
- `POST /api/me/webhook-push/repos/{repo_id}/register`：人工逐仓注册并在成功后解除暂停。
- `POST /api/me/webhook-push/repos/{repo_id}/check`：人工逐仓只读检查。

会排队后台任务的写操作返回 `{ "task_id": string, "reused": boolean }`，前端通过现有 task API/SSE 跟踪。关闭 Webhook 推送时不排队任务，返回 `task_id: null`。

## Admin APIs

- `GET /api/admin/jobs/webhook-push/runtime-config`
- `PATCH /api/admin/jobs/webhook-push/runtime-config`，请求体 `{ "audit_interval_days": integer }`，范围 `1..=30`。

## Receiver

- `POST /api/webhooks/github/releases`
- 必需 headers：`X-GitHub-Delivery`、`X-GitHub-Event`、`X-Hub-Signature-256`；`ping` 不要求已存在 release payload。
- 成功响应 `{ "accepted": true, "queued": boolean, "reason": string }`。
