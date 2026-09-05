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
- `first_token_wait_ms INTEGER NULL`
- `duration_ms INTEGER NULL`
- `input_tokens INTEGER NULL`
- `output_tokens INTEGER NULL`
- `cached_input_tokens INTEGER NULL`
- `total_tokens INTEGER NULL`
- `input_messages_json TEXT NULL` (JSON array of input conversation messages)
- `output_messages_json TEXT NULL` (JSON array of output conversation messages)
- `prompt_text TEXT NOT NULL`
- `response_text TEXT NULL`
- `error_text TEXT NULL`
- `finish_reason TEXT NULL`
- `provider_request_id TEXT NULL`
- `provider_http_status INTEGER NULL`
- `created_at TEXT NOT NULL`
- `started_at TEXT NULL`
- `finished_at TEXT NULL`
- `updated_at TEXT NOT NULL`
- `failure_class TEXT NULL` (`empty_content|transient|rate_limited|configuration`)
- `final_model TEXT NULL`
- `fallback_count INTEGER NOT NULL DEFAULT 0`
- `retry_scheduled_at TEXT NULL`
- `recovery_attempt_count INTEGER NOT NULL DEFAULT 0`

Indexes:

- `idx_llm_calls_status_created_at` on `(status, created_at DESC)`
- `idx_llm_calls_requested_by_created_at` on `(requested_by, created_at DESC)`
- `idx_llm_calls_source_created_at` on `(source, created_at DESC)`
- `idx_llm_calls_parent_task_id` on `(parent_task_id)`
- `idx_llm_calls_created_at` on `(created_at DESC)`

Retention:

- Background cleanup removes `llm_calls` rows older than 7 days by `created_at`. Durable content-processing attribution retains the call identity, stage, relation role, and an `expired` availability state without retaining the payload.

## `llm_call_events` (new)

Append-only event stream for SSE fan-out of LLM call state transitions.

Key columns:

- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `call_id TEXT NOT NULL` (FK to `llm_calls.id`, `ON DELETE CASCADE`)
- `event_type TEXT NOT NULL` (e.g. `llm.queued|llm.running|llm.attempt_failed|llm.route_switched|llm.succeeded|llm.failed`)
- `status TEXT NOT NULL` (`queued|running|succeeded|failed`)
- `source TEXT NOT NULL`
- `requested_by INTEGER NULL`
- `parent_task_id TEXT NULL`
- `payload_json TEXT NULL` (small event metadata snapshot)
- Structured recovery columns (`failure_class`, `model`, `attempt`, `retry_after_ms`, `from_model`, `to_model`, `fallback_count`) are nullable for compatibility; payload remains the audit envelope.
- `created_at TEXT NOT NULL`

Indexes:

- `idx_llm_call_events_call_id_id` on `(call_id, id)`

Event payloads contain only safe failure classes, route metadata, retry timing, and attempt counters. Upstream response bodies and user content are excluded.
