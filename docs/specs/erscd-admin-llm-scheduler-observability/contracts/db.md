# DB contract

## `llm_calls` (new)

LLM scheduler/call observability table (per-call granularity).

Key columns:

- `id TEXT PRIMARY KEY`
- `status TEXT NOT NULL` (`queued|running|succeeded|failed`)
- `source TEXT NOT NULL`
- `model TEXT NOT NULL`
- `requested_by INTEGER NULL`
- `parent_task_id TEXT NULL`
- `parent_task_type TEXT NULL`
- `max_tokens INTEGER NOT NULL`
- `attempt_count INTEGER NOT NULL DEFAULT 0`
- `scheduler_wait_ms INTEGER NOT NULL DEFAULT 0`
- `duration_ms INTEGER NULL`
- `prompt_text TEXT NOT NULL`
- `response_text TEXT NULL`
- `error_text TEXT NULL`
- `created_at TEXT NOT NULL`
- `started_at TEXT NULL`
- `finished_at TEXT NULL`
- `updated_at TEXT NOT NULL`

Indexes:

- `idx_llm_calls_status_created_at` on `(status, created_at DESC)`
- `idx_llm_calls_requested_by_created_at` on `(requested_by, created_at DESC)`
- `idx_llm_calls_source_created_at` on `(source, created_at DESC)`
- `idx_llm_calls_parent_task_id` on `(parent_task_id)`
- `idx_llm_calls_created_at` on `(created_at DESC)`

Retention:

- Background cleanup removes rows older than 7 days by `created_at`.
