# HTTP APIs

## User APIs

- `GET /api/me/webhook-push`：读取目标状态、当前操作、当前用户最近完成检查时间、前置条件、汇总、Owner 分组、仓库观察状态和最新终态失败。
- `PATCH /api/me/webhook-push`：请求体 `{ "desired_state": "enabled" | "paused" | "deleted" }`；启用时只读取已持久化的 PAT、callback 和 secret 前置条件，所有 GitHub 访问与 Hook 动作异步排队。
- `POST /api/me/webhook-push/reconcile`：请求体可选 `{ "repo_id": number }`；由用户手动发起检查并修复全量或单仓，PAT 的实时有效性由后台 worker 检查。启用目标在“我的发布”关闭时拒绝；暂停或删除目标仍允许用户修复遗留 Hook。用户已有未终态操作时返回 `409 webhook_push_operation_in_progress`。
- `PATCH /api/me/profile`：只更新日报时区和 `include_own_releases`；关闭“我的发布”时会将启用目标转为暂停并排队对齐。Webhook 目标状态只能通过专用 PATCH 路由变更，避免绕过启用前置校验。

会排队后台任务的写操作返回 `{ "task_id": string, "status": string, "operation": string }`，前端通过现有 task API/SSE 跟踪。目标状态写入成功后，即使远端任务失败也保留该目标。

`GET /api/me/webhook-push` 的 `summary.pending` 是当前仓库观察中需要用户关注或仍在对齐的数量；`summary.missing` 同时包含真实缺失和启用目标尚未生成本地 Hook 记录的 `waiting_registration` 仓库。`archived`、`pat_scope_excluded` 与 `out_of_scope` 保留为可见处置状态但不计入 pending。`last_operation_failure` 只表示未能完成的基础设施任务；存在更晚的 queued/running 或 succeeded manage 任务时返回 `null`，单仓处置结果不产生它。

仓库观察状态包含持久/派生状态 `waiting_registration`、`registered`、`missing`、`permission_paused`、`archived`、`pat_scope_excluded`、`out_of_scope`、`conflict` 和 `error`，以及操作覆盖态 `registering`、`processing`、`delete_pending` 和关闭目标的 `not_configured`。全量对齐完成表示每个当时合格目标都有独立处置结果，即使某些行仍要求用户处理。GitHub 暂时错误在 `1/5/15` 分钟重试耗尽后保留仓库级 `error` 并确认已完成 generation；若 SQLite 等基础设施无法持久化处置，任务进入失败终态并抑制该 generation 的自动重投，等待显式 reconcile 恢复。

定时 audit 任务只负责枚举用户并投递带 `scheduled: true` 的 `webhook.push.manage` 任务；它不执行 GitHub 请求，也不持有用户级远端操作 lease。

audit 在部分用户派发失败时继续处理其余用户，并将同一 audit 任务按 `retry_count` 以 `1/5/15` 分钟退避重新排队；只有重试耗尽才进入终态失败，成功派发的用户不会重复创建未终态任务。

新自有仓库不经由 HTTP 触发注册。成功提交基线会推进持久化的 Webhook 对齐需求；dispatcher 在提交后投递或复用用户的 manage 任务。需求 generation 在完整对齐后确认，运行中出现的新 generation 会产生一个后续对齐任务。

## Admin APIs

- `GET /api/admin/jobs/webhook-push/runtime-config`
- `PATCH /api/admin/jobs/webhook-push/runtime-config`，请求体 `{ "audit_interval_days": integer }`，范围 `1..=30`。

## Receiver

- `POST /api/webhooks/github/releases`
- 必需 headers：`X-GitHub-Delivery`、`X-GitHub-Event`、`X-GitHub-Hook-ID`、`X-Hub-Signature-256`；`ping` 不要求已存在 release payload。
- 成功响应 `{ "accepted": true, "queued": boolean, "reason": string }`。
