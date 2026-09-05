# DB contracts

## New tables

- `translation_requests`
  - producer-facing request envelope
- `translation_work_items`
  - deduplicated scheduler work items keyed by scope + kind + variant + entity + source hash
- `translation_batches`
  - actual scheduled batches with trigger reason and token estimate
- `translation_batch_items`
  - batch membership and terminal per-item status/error
- `translation_attempt_events`
  - append-only, metadata-only attempt history keyed by `work_item_id` and queryable by `entity_id + kind + variant`
  - records `attempt_no`, `trigger`, `event_type`, processing stage, terminal result or safe failure classification, retry disposition, next retry time, and optional request/batch links
  - deliberately has no foreign keys so completed audit history remains readable if operational request, batch, or LLM records are later pruned
  - never stores source blocks, prompts, raw model responses, or raw upstream error text; it retains only normalized error code and summary
- `translation_attempt_llm_calls`
  - append-only attribution link keyed by `work_item_id + attempt_no + llm_call_id + stage + ordinal`
  - records the relation role and an availability snapshot without retaining prompt, response, or raw upstream errors
  - remains readable after `llm_calls` retention cleanup so the admin surface can distinguish expired diagnostic evidence from absent attribution
- `translation_work_items` recovery columns
  - `failure_class TEXT NULL`
  - `error_code TEXT NULL`
  - `processing_stage TEXT NULL`
  - `provider_status TEXT NULL` (`not_started | succeeded | failed`)
  - `output_contract_status TEXT NULL` (`not_run | passed | failed | recovered`)
  - `retry_disposition TEXT NOT NULL DEFAULT 'not_needed'` (`not_needed | scheduled | manual_only | in_attempt_recovered`)
  - `retry_count INTEGER NOT NULL DEFAULT 0`
  - `attempt_count INTEGER NOT NULL DEFAULT 0`
  - `next_attempt_trigger TEXT NOT NULL DEFAULT 'initial'`
  - `next_retry_at TEXT NULL`
  - `retry_expires_at TEXT NULL` (automatic recovery ends after 24 hours)
- `llm_recovery_flags`
  - persistent default-off canary switch and stable partition percentage

## Modified tables

- `translation_batches`
  - add `runtime_owner_id TEXT NULL`
  - add `lease_heartbeat_at TEXT NULL`
  - `running` batch rows are owned by the current process and refreshed every 10s
  - startup recovery fails `running` rows whose owner lease is missing or stale
  - periodic sweep only reclaims rows with missing heartbeat or heartbeat older than 90s

- `llm_calls`
  - add `parent_translation_batch_id`
  - records provider delivery metadata, including `finish_reason`, provider request identifier, and HTTP status when available
  - add `runtime_owner_id TEXT NULL`
  - add `lease_heartbeat_at TEXT NULL`
  - add translation-specific linkage indexes for admin tracing
  - diagnostic payload columns are retained for seven days; cleanup marks linked evidence `expired` before deleting payload rows

- `llm_diagnostic_access_audit`
  - stores only call id, administrator id, `reveal`/`copy` action, and timestamp
  - never stores prompt, response, or raw provider error text

- `runtime_owners`
  - runtime-level owner lease registry keyed by `runtime_owner_id`
  - startup recovery only reclaims foreign-owner rows when that owner lease is missing or heartbeat-stale
  - graceful shutdown removes the current owner row

## Runtime recovery semantics

- Boot-time recovery runs before workers/schedulers start and immediately reclaims rows whose runtime owner lease is missing or stale.
- Periodic runtime sweep reclaims orphaned `running` work after 90s without heartbeat.
- Translation request/work-item rows do not own leases directly; they are failed by batch-level recovery.
- Recovery scans run once per minute and only requeue structured, recoverable failures when the canary switch allows the stable partition. Historical unclassified failures remain manual-only.
- Each initial queue, execution start, terminal completion, retry schedule, automatic recovery, manual retry, and system requeue appends a `translation_attempt_events` row in the same write transaction as the state transition.
