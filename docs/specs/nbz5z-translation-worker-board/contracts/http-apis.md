# HTTP / SSE contracts

## Modified: `GET /api/admin/jobs/translations/status`

Response adds worker runtime fields:

- `worker_concurrency: number` = `4`
- `idle_workers: number`
- `busy_workers: number`
- `workers: TranslationWorkerStatus[]`

`TranslationWorkerStatus`:

- `worker_id: string`
- `worker_slot: number`
- `worker_kind: "general" | "user_dedicated"`
- `status: "idle" | "running" | "error"`
- `current_batch_id: string | null`
- `request_count: number`
- `work_item_count: number`
- `trigger_reason: string | null`
- `updated_at: string`
- `error_text: string | null`

## Modified: `GET /api/admin/jobs/translations/requests`

- Sorting changes to: active (`queued`, `running`) first, then `updated_at DESC, id DESC`.
- Existing filter and pagination semantics stay unchanged.

## Modified: `GET /api/admin/jobs/translations/batches`

Each batch item adds:

- `worker_slot: number`
- `request_count: number`

## Modified: `GET /api/admin/jobs/translations/batches/{batch_id}`

`batch` object adds:

- `worker_slot: number`
- `request_count: number`

## Modified: `GET /api/admin/jobs/events` SSE

`translation.event` payload expands:

- `resource_type: "request" | "batch" | "worker"`
- `resource_id: string`
- `status: string`
- `event_type: string`
- `created_at: string`

When `resource_type = worker`, `resource_id` is the stable `worker_id`.
