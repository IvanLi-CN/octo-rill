# DB contracts

## New tables

- `translation_requests`
  - producer-facing request envelope
- `translation_request_items`
  - per-request item status and returned fields
- `translation_work_items`
  - deduplicated scheduler work items keyed by scope + kind + variant + entity + source hash
- `translation_work_watchers`
  - request-item to work-item fan-out mapping
- `translation_batches`
  - actual scheduled batches with trigger reason and token estimate
- `translation_batch_items`
  - batch membership and terminal per-item status/error

## Modified tables

- `translation_batches`
  - add `runtime_owner_id TEXT NULL`
  - add `lease_heartbeat_at TEXT NULL`
  - `running` batch rows are owned by the current process and refreshed every 10s
  - stale or owner-mismatched `running` rows are failed as `runtime_lease_expired`

- `llm_calls`
  - add `parent_translation_batch_id`
  - add `runtime_owner_id TEXT NULL`
  - add `lease_heartbeat_at TEXT NULL`
  - add translation-specific linkage indexes for admin tracing

## Runtime recovery semantics

- Boot-time recovery runs before workers/schedulers start.
- Periodic runtime sweep reclaims orphaned `running` work after 90s without heartbeat.
- Translation request/work-item rows do not own leases directly; they are failed by batch-level recovery.
