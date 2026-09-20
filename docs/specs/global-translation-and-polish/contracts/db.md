# 数据库合同

## Work and Projection Identity

The model-independent work identity is:

```text
(canonical_resource_type, canonical_resource_id, pipeline, variant,
 target_lang, source_hash, protocol_version)
```

`pipeline` is `translation` or `polishing`. `canonical_resource_type` is `release`, `announcement` or `notification`. Model routes, model profiles and configuration fingerprints are not part of this identity. The source snapshot and source hash remain immutable for a work item. During the compatibility stage, the existing `content_work_items` unique key still includes `model_profile`; `content_work_identities` is the additive registry for the target identity, and `content_work_identity_members` maps retained work rows into it during the later cutover.

The model-independent result projection has the same complete identity, including `source_hash`:

```text
(canonical_resource_type, canonical_resource_id, pipeline, variant,
 target_lang, source_hash, protocol_version)
```

`content_current_result_projections.identity_id` references the unique registry row for that full identity. The existing `content_result_projections` table remains model-specific history during the compatibility stage. A projection is a cache hit only when its registered `source_hash` matches the requested work identity. Retained model-profile or work-level configuration-fingerprint fields are historical provenance only; they must not affect identity, current-result lookup or attempt configuration. Do not infer or fabricate per-attempt snapshots from old work-level fingerprints.

## Global Table Inventory

| Table | Ownership | Required facts |
| --- | --- | --- |
| `content_processing_control` | cutover controller | singleton mode, switch token, update time |
| `content_work_items` | scheduler | retained model-specific compatibility rows, immutable source snapshot/hash, lifecycle, priority and recovery facts; work-level configuration is historical provenance during identity upgrade |
| `content_work_identities` | scheduler | one registry row per full model-independent work identity, including `source_hash` |
| `content_work_identity_members` | scheduler | durable mapping from each retained model-specific work row to its model-independent identity |
| `content_batches` | scheduler | durable general-worker batch, partition, trigger, lease and aggregate result facts |
| `content_batch_items` | scheduler | ordered batch membership, request count, token estimate and item result facts |
| `content_result_projections` | scheduler | retained model-specific projection history and source payloads during identity upgrade |
| `content_current_result_projections` | scheduler | one validated current payload per registry identity, optional source-projection provenance and active work item |
| `content_request_links` | API adapter | requester/system producer, authorization snapshot, request source, delivery mode, global work item and returned response fact |
| `content_attempt_events` | scheduler | append-only attempt number, trigger, nullable safe attempt-start configuration/route snapshots and fingerprint, state transition, safe error, retry disposition and timing |
| `content_attempt_llm_calls` | scheduler | exact attempt-to-call relation, call identifier and safe metrics |
| `content_legacy_observations` | migration/read model | old table, old primary key, canonical resource facts where known, classification and immutable observation basis |
| `content_identity_upgrade_control` | migration/scheduler | singleton generation, backfill phase and cursor, pause/failure state and completion facts |

## State and Publication Invariants

The scheduler-owned admission/retry transaction may create or requeue a work item
after an API adapter has completed authorization. API adapters do not perform
provider calls or write attempt events, model-call links, or terminal result
projections; those writes remain worker-owned.

- `content_work_items.status` is one of `queued`, `running`, `ready`, `failed`, `not_applicable`, `deferred_provider`, `blocked_config`, `cancelled` or `superseded`.
- `queued -> running` is scheduler claim only. At the start of each attempt, the scheduler snapshots the then-current valid global route and relevant safe configuration. That snapshot is fixed for the attempt; a later attempt takes a new snapshot. Secret values are never stored, only safe configuration values and non-reversible fingerprints. Actual selected models are recorded on their exact call links.
- `running -> ready` requires a valid output; `running -> failed` records a terminal attempt; `running -> blocked_config` and `running -> deferred_provider` preserve a non-provider execution block; `running -> cancelled` or `running -> superseded` requires source cancellation or replacement. `not_applicable` is terminal without a provider call.
- `blocked_config -> queued` is scheduler-owned and event-driven: a relevant global model-configuration update or runtime configuration reload triggers revalidation, and only a currently valid configuration may requeue the work. Invalid configuration causes no provider call and no timer-based retry. The next attempt snapshots the new current configuration.
- A structured recoverable `failed` attempt can transition through a recorded recovery event back to `queued`; a manual retry may take a terminal `failed` item to `queued` only after its cooldown. A scheduler-controlled provider probe may take `deferred_provider` to `queued`. No other actor may do so.
- A published result references a `ready` work item whose output contract passed. No failed, cancelled or legacy item may become a result projection.
- Readers resolve the model-independent projection identity. A model-route change must not hide or invalidate a published projection; the projection is a cache hit for work only when `published_source_hash` matches that work's `source_hash`.
- When a matching current projection lacks a retained current work item, the scheduler creates exactly one `ready` work item with `cache_hit=true` and zero attempts before linking the request.
- Source changes create a new work item. The result table retains the prior successful projection and points `active_work_item_id` at the newer work until successful replacement.
- A model/configuration change alone neither creates work nor reruns a ready projection for an unchanged source. No explicit force-regenerate operation is part of this contract.
- Each state transition and attempt event is written transactionally. The provider call may be delivered at least once through an idempotency key; result publication is exactly once in the database.
- The unique identity registry and SQLite writer transaction serialize model-independent work admission. An active item cannot gain a second concurrent manual attempt.

## Identity Compatibility Schema

- The migration-bearing compatibility release adds the identity registry, member mapping, current-projection table, nullable attempt-snapshot columns and a pending upgrade-control row. It does not populate the registry, copy projections, rewrite work, or change the current model-specific runtime behavior.
- The registry unique key includes `source_hash` and excludes model/configuration fields. The current-projection row is unique by its registry identity, so different source versions cannot collapse into one result.
- The compatibility binary retains the existing model-specific tables and indexes. It can open the later cutover schema and is the supported rollback target; a binary without the compatibility migration is not supported after the migration is applied.
- The later identity cutover performs work-member reconciliation, projection backfill and blocked-configuration recovery as separately observable, pauseable, idempotent phases. DDL completion alone does not mark the upgrade complete.

## Model-Independent Identity Migration

- Reconcile existing model-profile-specific work and projection rows to the identity keys above without discarding attempt events, model-call links, requester associations or published payload evidence. Existing work rows and associations remain auditable; old work-level configuration fingerprints remain historical provenance. Reconciliation must not fabricate attempts, per-attempt snapshots or call history.
- For each full model-independent projection identity, including `source_hash`, with multiple valid old model-specific projections, the projection with the latest `published_at` becomes the single current projection; a stable projection ID breaks ties. Older candidates remain historical and are not eligible for current-result reads.
- Group existing `blocked_config` work by the new work identity. Automatically queue one recovery for an identity only when there is no valid current projection whose `published_source_hash` matches that identity's `source_hash`. If a matching projection exists, preserve and serve it without a new provider call.
- Recovery uses the normal scheduler, provider guard, priority and concurrency limits. It is not a bulk direct provider invocation. If current configuration is still invalid, the work remains `blocked_config` until a relevant configuration update or reload makes it valid.
- This reconciliation runs once as part of the model-independent identity upgrade. It does not regenerate ready results with unchanged source hashes.

## Legacy Boundary

- No migration modifies `translation_work_items`, `translation_requests`, their attempts or `ai_translations`.
- `content_legacy_observations` can identify legacy evidence but cannot be used as a foreign-key source for a global result or work state.
- `legacy_cached` means a displayable old cache lacks matching work evidence. `legacy_conflict` means old evidence cannot be safely reconciled. Neither classification creates an attempt count or current status.

## Polishing Migration Mapping

The global tables are shared by both pipelines. A global polishing item uses `pipeline='polishing'`; it has the same model-independent identity, source snapshot, result projection, requester association and attempt-level configuration audit as translation.

| Retained source | Legacy polishing discriminator | Migration treatment |
| --- | --- | --- |
| `ai_translations` | `entity_type='release_smart'` or `entity_type='announcement_smart'` | Preserve the row unchanged and create only a provenance observation when needed. Do not copy its title, summary, source hash, status or user ID into a global result. |
| `translation_work_items` | `kind='release_smart'` or `kind='announcement_smart'` | Preserve the row unchanged as historical work evidence. Do not promote it to `content_work_items`, including when it is `ready`. |
| Legacy attempt events and call links | Belong to a retained smart work-item ID | Preserve as historical audit evidence; do not generate global attempts, call links or retry counters. |
| New coverage or a new authorized request | `pipeline='polishing'` | Create or associate the global work item and let the scheduler write the global result and attempts. |

The identity upgrade applies equally to translation and polishing. It does not rewrite the old polishing cache or work tables.

## Control-Mode Invariants

- `legacy` permits only the compatibility version's legacy writers.
- `rollback_freeze` permits no content-processing writer and returns the transition response to new requests.
- `global` permits only global writers. Compatibility-version legacy writers must fail closed in this mode.
