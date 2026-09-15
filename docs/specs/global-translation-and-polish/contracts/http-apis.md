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

## Persistent Migration Operations

Valid content-processing submissions remain on the existing admission contract during DDL, DML and historical backfill. A historical `legacy` or `rollback_freeze` control value is repaired to `global` inside the same `BEGIN IMMEDIATE` transaction that creates or associates the authoritative `content_work_items` row. No migration-specific `503`, freeze route or cutover route exists.

Administrators can inspect and control the online operator through the existing admin boundary:

- `GET /admin/jobs/migrations` and `GET /admin/jobs/migrations/{migration_id}` return the immutable run and operation definition checksums, ordered status, cursor, progress, owner heartbeat and redacted error summary.
- `POST /admin/jobs/migrations/{migration_id}/pause` requests a pause at the next committed batch boundary.
- `POST /admin/jobs/migrations/{migration_id}/resume` clears the pause request and resumes from the durable cursor.

The operator uses a named persistent lease after runtime-owner registration. Each historical backfill transaction processes at most 100 rows and yields to foreground SQLite writers before acquiring its permit.

## Read Semantics

- Public and authenticated resource reads return only the current validated result projection when the caller is authorized for the canonical resource. A prior projection may remain present while its newer work is `queued` or `running`.
- Admin AI-record list and detail GETs are side-effect free. Each pipeline lane reports `global_work`, `result_projection` and `legacy_evidence` separately.
- A lane without a global work or result but with retained cache evidence reports `legacy_cached`; a lane with irreconcilable retained evidence reports `legacy_conflict`. Neither is serialized as `ready`, `failed`, `unstarted` or an attempt count.
- User-visible and administrator-facing labels are exactly “翻译” and “润色”.

## Retry Authorization

Any caller authorized to read the canonical resource may request retry of a terminal global failure. The service applies the same cooldown, lifecycle and access checks regardless of which authorized caller made the original request.
