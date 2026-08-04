# DB / runtime contracts

## New table: `admin_runtime_settings`

- singleton row keyed by `singleton_key = 'default'`
- columns:
  - `llm_max_concurrency INTEGER NOT NULL`
  - `translation_general_worker_concurrency INTEGER NOT NULL`
  - `translation_dedicated_worker_concurrency INTEGER NOT NULL`
  - `created_at TEXT NOT NULL`
  - `updated_at TEXT NOT NULL`

## Modified table: `translation_batches`

- add `worker_id TEXT NOT NULL`
- add `worker_kind TEXT NOT NULL CHECK (worker_kind IN ('general', 'user_dedicated'))`
- existing `worker_slot` remains the rendered slot order for runtime status and history views

## Seed semantics

- On startup, if `admin_runtime_settings` has no row, the application writes one seed row using the effective boot values:
  - `AI_MAX_CONCURRENCY`
  - translation general worker default
  - translation dedicated worker default
- After the seed row exists, admin updates replace these values and later restarts read from the stored row first.

## Runtime semantics

- LLM runtime uses a mutable permit ceiling instead of a fixed semaphore size.
- Reducing the LLM ceiling never aborts active calls; it only blocks future slot acquisition until occupancy drops below the target.
- Translation workers are identified by stable ids:
  - `translation-worker-general-<n>`
  - `translation-worker-user-dedicated-<n>`
- Runtime display order is always:
  - all `general` workers first
  - all `user_dedicated` workers after them
- Translation shrink behavior is drain-first:
  - removed workers disappear immediately from the desired profile set
  - already-running workers finish their current batch
  - once the current batch ends, those workers exit and are removed from runtime status
