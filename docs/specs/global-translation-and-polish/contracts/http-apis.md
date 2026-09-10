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
- Admin AI-record list and detail GETs are side-effect free. Each pipeline lane reports `global_work`, `result_projection` and `legacy_evidence` separately.
- A lane without a global work or result but with retained cache evidence reports `legacy_cached`; a lane with irreconcilable retained evidence reports `legacy_conflict`. Neither is serialized as `ready`, `failed`, `unstarted` or an attempt count.
- User-visible and administrator-facing labels are exactly “翻译” and “润色”.

## Retry Authorization

Any caller authorized to read the canonical resource may request retry of a terminal global failure. The service applies the same cooldown, lifecycle and access checks regardless of which authorized caller made the original request.
