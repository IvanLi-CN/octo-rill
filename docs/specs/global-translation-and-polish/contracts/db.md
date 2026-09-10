# 数据库合同

## Global Identity

`content_work_items` has one unique identity:

```text
(canonical_resource_type, canonical_resource_id, pipeline, variant,
 target_lang, source_hash, protocol_version, model_profile)
```

`pipeline` is `translation` or `polishing`. `canonical_resource_type` is `release`, `announcement` or `notification`. `source_snapshot_json` and `configuration_fingerprint` are immutable after creation; the configuration fingerprint is intentionally not part of the uniqueness key.

## New Tables

| Table | Ownership | Required facts |
| --- | --- | --- |
| `content_processing_control` | cutover controller | singleton mode, switch token, update time |
| `content_work_items` | scheduler | global identity, source snapshot/hash, frozen configuration, lifecycle, priority, recovery and source-supersession facts |
| `content_batches` | scheduler | durable general-worker batch, partition, trigger, lease and aggregate result facts |
| `content_batch_items` | scheduler | ordered batch membership, request count, token estimate and item result facts |
| `content_result_projections` | scheduler | latest validated payload, published source hash, active work item and publication facts |
| `content_request_links` | API adapter | requester/system producer, authorization snapshot, request source, delivery mode, global work item and returned response fact |
| `content_attempt_events` | scheduler | append-only attempt number, trigger, state transition, safe error, retry disposition and timing |
| `content_attempt_llm_calls` | scheduler | exact attempt-to-call relation, call identifier and safe metrics |
| `content_legacy_observations` | migration/read model | old table, old primary key, canonical resource facts where known, classification and immutable observation basis |

## State and Publication Invariants

- `content_work_items.status` is one of `queued`, `running`, `ready`, `failed`, `not_applicable`, `deferred_provider`, `blocked_config`, `cancelled` or `superseded`.
- `queued -> running` is scheduler claim only. `running -> ready` requires a valid output; `running -> failed` records a terminal attempt; `running -> blocked_config` and `running -> deferred_provider` preserve a non-provider execution block; `running -> cancelled` or `running -> superseded` requires source cancellation or replacement. `not_applicable` is terminal without a provider call.
- A structured recoverable `failed` attempt can transition through a recorded recovery event back to `queued`; a manual retry may take a terminal `failed` item to `queued` only after its cooldown. A scheduler-controlled provider probe may take `deferred_provider` to `queued`. No other actor may do so.
- A published result references a `ready` work item whose output contract passed. No failed, cancelled or legacy item may become a result projection.
- When a matching current projection lacks a retained current work item, the scheduler creates exactly one `ready` work item with `cache_hit=true` and zero attempts before linking the request.
- Source changes create a new work item. The result table retains the prior successful projection and points `active_work_item_id` at the newer work until successful replacement.
- Each state transition and attempt event is written transactionally. The provider call may be delivered at least once through an idempotency key; result publication is exactly once in the database.
- The global unique index and write transaction serialize retry acquisition. An active item cannot gain a second concurrent manual attempt.

## Legacy Boundary

- No migration modifies `translation_work_items`, `translation_requests`, their attempts or `ai_translations`.
- `content_legacy_observations` can identify legacy evidence but cannot be used as a foreign-key source for a global result or work state.
- `legacy_cached` means a displayable old cache lacks matching work evidence. `legacy_conflict` means old evidence cannot be safely reconciled. Neither classification creates an attempt count or current status.

## Polishing Migration Mapping

The global tables are shared by both pipelines. A global polishing item uses `pipeline='polishing'`; it has the same global identity, source snapshot, frozen configuration, result projection, requester association and attempt audit as translation.

| Retained source | Legacy polishing discriminator | Migration treatment |
| --- | --- | --- |
| `ai_translations` | `entity_type='release_smart'` or `entity_type='announcement_smart'` | Preserve the row unchanged and create only a provenance observation when needed. Do not copy its title, summary, source hash, status or user ID into a global result. |
| `translation_work_items` | `kind='release_smart'` or `kind='announcement_smart'` | Preserve the row unchanged as historical work evidence. Do not promote it to `content_work_items`, including when it is `ready`. |
| Legacy attempt events and call links | Belong to a retained smart work-item ID | Preserve as historical audit evidence; do not generate global attempts, call links or retry counters. |
| New coverage or a new authorized request | `pipeline='polishing'` | Create or associate the global work item and let the scheduler write the global result and attempts. |

This is a schema migration for polishing as well as translation: it adds the global shared tables and routes future polishing writes to them. It is deliberately not a rewrite of the old polishing cache or work tables.

## Control-Mode Invariants

- `legacy` permits only the compatibility version's legacy writers.
- `rollback_freeze` permits no content-processing writer and returns the transition response to new requests.
- `global` permits only global writers. Compatibility-version legacy writers must fail closed in this mode.
