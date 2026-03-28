# HTTP API contracts

## Modified: `GET /api/admin/jobs/llm/status`

Response keeps the existing scheduler summary fields and continues to expose:

- `max_concurrency: number`
- `available_slots: number`
- `waiting_calls: number`
- `in_flight_calls: number`

The returned `max_concurrency` is now the persisted runtime value after any admin override, not just the boot-time env default.

## New: `PATCH /api/admin/jobs/llm/runtime-config`

### Request

```json
{
  "max_concurrency": 5
}
```

### Response

Returns the refreshed `GET /api/admin/jobs/llm/status` payload after the new runtime ceiling is persisted and applied in-process.

### Notes

- `max_concurrency` must be a positive integer.
- Lowering the value below the current `in_flight_calls` does not cancel running calls; new calls wait until the live in-flight count falls under the target.

## Modified: `GET /api/admin/jobs/translations/status`

Response adds live per-kind worker counts:

- `general_worker_concurrency: number`
- `dedicated_worker_concurrency: number`

Existing fields remain:

- `worker_concurrency: number`
- `idle_workers: number`
- `busy_workers: number`
- `workers: TranslationWorkerStatus[]`

The response also includes desired runtime config fields:

- `target_general_worker_concurrency: number`
- `target_dedicated_worker_concurrency: number`
- `target_worker_concurrency: number`

`worker_concurrency` is the sum of the live `general_worker_concurrency + dedicated_worker_concurrency`.
`target_worker_concurrency` is the sum of the persisted desired config.

## New: `PATCH /api/admin/jobs/translations/runtime-config`

### Request

```json
{
  "general_worker_concurrency": 5,
  "dedicated_worker_concurrency": 2
}
```

### Response

Returns the refreshed `GET /api/admin/jobs/translations/status` payload after the new worker layout is persisted and applied in-process.

### Notes

- Both request fields must be positive integers.
- Expanding the config creates new idle worker slots immediately.
- Shrinking the config only prevents extra workers from claiming new batches; workers already executing a batch retire after that batch finishes.
