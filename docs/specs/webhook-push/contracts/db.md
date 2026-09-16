# Database Contract

## `users`

- `webhook_push_enabled INTEGER NOT NULL DEFAULT 0`
- `webhook_push_desired_state TEXT NOT NULL DEFAULT 'deleted' CHECK (webhook_push_desired_state IN ('enabled', 'paused', 'deleted'))`
- `webhook_push_last_completed_check_at TEXT`
- `webhook_push_secret_ciphertext BLOB`
- `webhook_push_secret_nonce BLOB`
- `webhook_push_callback_key TEXT UNIQUE`
- secret 两列必须同时为空或同时有值。

## `webhook_push_repos`

- 主键：`(user_id, repo_id)`
- 身份：`owner_login`, `repo_name`, `repo_full_name`
- hook：`hook_id`, `callback_url`, `status`
- 错误：`error_kind`, `error_message`, `permission_paused`
- 时间：`last_checked_at`, `last_registered_at`, `updated_at`
- `status` persists only values accepted by the deployed `0066` CHECK constraint: `unknown|missing|registered|conflict|permission_paused|error|delete_pending`.
- The API derives the public `archived` observation when `error_kind = 'archived'`; this preserves archived visibility without rewriting an already-deployed SQLite constraint.

Rows are observations of a repository Hook. A missing row for an enabled target is derived as `waiting_registration`, not `unknown` or an error observation; the HTTP `summary.missing` count includes this waiting state so an enabled repository is visible as pending registration.

The API derives `pat_scope_excluded` for a known-private owned baseline outside the current validated classic PAT scope, and `out_of_scope` for an observed repository no longer owned by the current PAT owner. Neither requires a remote Hook mutation.

## `webhook_push_reconcile_demands`

- 主键：`user_id`
- `requested_generation INTEGER NOT NULL`
- `completed_generation INTEGER NOT NULL DEFAULT 0`
- `requested_at TEXT NOT NULL`
- `updated_at TEXT NOT NULL`

The owned-baseline transaction increments `requested_generation` only when it first persists an eligible repository for an enabled Webhook target. A dispatcher may enqueue or reuse one manage task for the user after commit; it must not advance `completed_generation` until that task has independently disposed every target for its captured generation. A newer requested generation remains durable while a task is running and requires one follow-up reconciliation. If the task exhausts its infrastructure retry budget without a durable disposition, the failed task suppresses automatic redispatch for that generation; an explicit reconcile can recover it without losing the demand.

## `job_tasks`

- `available_at TEXT`: nullable UTC time used by durable delayed retries; queued tasks are claimable only when this is null or due.
- Webhook manage and audit payloads store `retry_count`; manage payloads additionally store `scheduled` and, when dispatched from a demand, its captured reconciliation generation. SQLite busy exhaustion and partial audit dispatch failures reschedule the same task using `available_at` before a terminal failure.
- Webhook worker local writes use `SqliteWriteCoordinator`; the writer permit never spans GitHub requests.

## `webhook_push_deliveries`

- 主键：`delivery_id`
- `hook_id`, `repo_id`, `event`, `action`, `received_at`, `queued_task_id`
- 保留最近 30 天；清理由后台任务 best-effort 执行。

## `admin_runtime_settings`

- `webhook_push_audit_interval_days INTEGER NOT NULL DEFAULT 7 CHECK (webhook_push_audit_interval_days BETWEEN 1 AND 30)`
- `webhook_push_audit_last_started_at TEXT`
