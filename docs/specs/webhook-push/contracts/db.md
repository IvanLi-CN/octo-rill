# Database Contract

## `users`

- `webhook_push_enabled INTEGER NOT NULL DEFAULT 0`
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
- `status`: `unknown|missing|registered|conflict|permission_paused|error|delete_pending`

## `webhook_push_deliveries`

- 主键：`delivery_id`
- `hook_id`, `repo_id`, `event`, `action`, `received_at`, `queued_task_id`
- 保留最近 30 天；清理由后台任务 best-effort 执行。

## `admin_runtime_settings`

- `webhook_push_audit_interval_days INTEGER NOT NULL DEFAULT 7 CHECK (webhook_push_audit_interval_days BETWEEN 1 AND 30)`
- `webhook_push_audit_last_started_at TEXT`
