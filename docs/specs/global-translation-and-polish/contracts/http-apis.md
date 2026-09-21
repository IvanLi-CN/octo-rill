# HTTP 与管理读取合同

## Request Submission

Existing translation and polishing endpoints remain externally available but become adapters for global work submission. Their successful request shapes retain the existing `async`, `wait` and `stream` meanings and include:

```json
{
  "request_id": "request_xxx",
  "work_item_id": "global_work_xxx",
  "status": "queued | running | ready | failed | not_applicable | deferred_provider | blocked_config | cancelled | superseded",
  "poll_url": "/api/translate/requests/request_xxx"
}
```

The response does not expose another requester's identity, association, private source snapshot or diagnostics.

Legacy `sync` submission is implemented as bounded `wait`: it submits or associates with global work, never invokes the provider inline, and returns the same pending snapshot with `poll_url` when its wait budget expires.

## Active Retry Response

If an authorized retry finds its global work already `queued` or `running`, the API first records the requester association and then returns `409 Conflict`:

```json
{
  "code": "content_processing_active",
  "request_id": "request_xxx",
  "work_item_id": "global_work_xxx",
  "status": "queued | running",
  "last_attempt_status": "queued | running | failed",
  "poll_url": "/api/translate/requests/request_xxx"
}
```

The client treats this as synchronization with the active work, immediately renders the returned state and polls `poll_url`. It must not create an optimistic retry attempt.

When provider health has opened the persistent circuit, an authorized retry records the association but receives the current `deferred_provider` state and a polling URL. Only the scheduler's controlled probe may initiate another provider call.

## Transition Response

During `rollback_freeze`, every content-processing submission returns `503 Service Unavailable` with the same `request_id`, `work_item_id` when known, current mode and a polling URL. It neither falls back to old direct writes nor creates a partially switched work item.

The operational cutover endpoint `POST /api/admin/jobs/content-processing/cutover` accepts `{ "switch_token": "..." }`. It performs the single `rollback_freeze -> global` transaction and returns `409 content_processing_cutover_not_ready` for any other current mode.

The operational freeze endpoint `POST /api/admin/jobs/content-processing/freeze` accepts the same payload. It performs the serialized `legacy -> rollback_freeze` transition only after legacy batches have reached a terminal state, and returns `409 content_processing_freeze_not_ready` otherwise. This is the compatibility release's controlled handoff into the PR-2 cutover window.

## Read Semantics

- Public and authenticated resource reads return only the current validated result projection when the caller is authorized for the canonical resource. A prior projection may remain present while its newer work is `queued` or `running`.
- Model-route or configuration changes alone do not invalidate a published projection or create work for an unchanged source. A projection is a cache hit only when its `published_source_hash` matches the current source hash.
- Admin AI-record list and detail GETs are side-effect free. Each pipeline lane reports `global_work`, `result_projection` and `legacy_evidence` separately.
- A lane without a global work or result but with retained cache evidence reports `legacy_cached`; a lane with irreconcilable retained evidence reports `legacy_conflict`. Neither is serialized as `ready`, `failed`, `unstarted` or an attempt count.
- User-visible and administrator-facing labels are exactly “翻译” and “润色”.

## Admin Collection Activity

`GET /api/admin/jobs/ai-records/{kind}/activity` is an administrator-only,
side-effect-free read for `release`, `announcement`, `notification` or `brief`.
It accepts no list filters or pagination parameters. Its fixed window starts at
the UTC hour containing the server's current time minus eleven hours and ends
at the next UTC hour boundary; both endpoints are serialized as UTC RFC 3339
timestamps, with the end exclusive. The response always contains twelve
hourly buckets in newest-first order, including the current partial hour.

The response shape is:

```json
{
  "kind": "release",
  "bucket_minutes": 60,
  "bucket_count": 12,
  "window_started_at": "2026-09-20T00:00:00Z",
  "window_ended_at": "2026-09-20T12:00:00Z",
  "summary": {
    "content_count": 2,
    "completed_count": 1,
    "processing_count": 0,
    "exception_count": 1,
    "neutral_count": 0
  },
  "buckets": [
    {
      "started_at": "2026-09-20T11:00:00Z",
      "ended_at": "2026-09-20T12:00:00Z",
      "cells": [
        {
          "id": "383114065",
          "title": "Bun v1.4.2",
          "repository": "oven-sh/bun",
          "source_time": "2026-09-20T11:55:00Z",
          "translation_status": "succeeded",
          "polish_status": "failed",
          "composite_status": "exception"
        }
      ]
    }
  ]
}
```

Each canonical source record appears exactly once in its source-time bucket.
`translation_status` is `null` for briefs; `repository` is `null` for briefs.
Lane statuses use the existing `display_status` vocabulary. Composite status
is `completed`, `processing`, `exception` or `neutral`, and follows the
precedence defined by `REQ-GTP-ADMIN-ACTIVITY`. The summary counts the same
records as the cells and includes neutral records even though the UI presents
their count in the legend.

The read uses an activity-specific keyed singleflight and a five-second budget.
List reads and activity reads do not reject one another due to application
contention. A safety timeout preserves the endpoint's 503 error code,
`Retry-After: 1`, and complete-failure behavior; the endpoint never returns a
partial set of cells.

## Retry Authorization

Any caller authorized to read the canonical resource may request retry of a terminal global failure. The service applies the same cooldown, lifecycle and access checks regardless of which authorized caller made the original request. `blocked_config` is not a terminal failure and is not manually retried; a relevant valid configuration update resumes it through the scheduler.

## Identity Upgrade Operations

The admin-only `GET /api/admin/jobs/content-processing/identity-upgrade` returns the migration-free identity upgrade generation, status, phase, last safe error code, timestamps, per-phase processed/total/completed counters, blocked-identity count and pending search-index refresh flag. It does not expose work payloads, credentials or model-call diagnostics.

The admin-only `POST` endpoint accepts one of:

```json
{ "action": "pause" }
```

```json
{ "action": "resume" }
```

Pause takes effect between committed SQLite batches. Resume continues from the persisted phase cursor; failed upgrades can also resume after an operator has addressed the cause. Actions outside the applicable state return `409 content_identity_upgrade_state_conflict`. The endpoint does not alter `content_processing_control` or enable/disable global content processing.
