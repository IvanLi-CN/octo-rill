# Star Sync Reconciliation API and Task Contract

## `GET /api/admin/jobs/sync/runtime-config`

The existing admin-only response adds:

```json
{
  "star_sync_delta_interval_minutes": 30,
  "star_sync_full_sweep_interval_minutes": 1440,
  "star_sync": {
    "last_delta_completed_at": "2026-09-08T08:00:00Z",
    "last_full_sweep_completed_at": "2026-09-07T08:00:00Z",
    "active_epochs": [
      {
        "github_connection_id": "conn_123",
        "processed_pages": 3,
        "processed_items": 300,
        "total_count_at_start": 721,
        "completion_percent": 41.6,
        "next_slice_not_before": "2026-09-08T10:30:00Z"
      }
    ]
  }
}
```

`completion_percent` is `null` when GitHub did not provide a reliable `totalCount`. `active_epochs` must not reveal tokens or private repo names.

## `PATCH /api/admin/jobs/sync/runtime-config`

The existing admin-only request accepts optional fields:

```json
{
  "star_sync_delta_interval_minutes": 30,
  "star_sync_full_sweep_interval_minutes": 1440
}
```

- Delta interval must be `1-120`.
- Full sweep interval must be `60-10080`.
- Updating either value changes future due calculations only. It does not clear an active epoch, reset its cursor, or start an immediate burst.
- The response uses the GET shape.

## `GET /api/admin/jobs/realtime?task_group=user_sync`

The existing admin-only task list accepts `task_group=user_sync` and returns only scheduled user-sync executions:

- `sync.starred.delta`
- `sync.starred.reconcile`

The response keeps the existing pagination and status-filter contract. `task_group=scheduled` excludes these two types, so the generic scheduled-task view and the user-sync view do not duplicate the same executions.

## `sync.starred.delta`

Input: optional `user_id` and `github_connection_id` for a targeted/manual enqueue; scheduled runs resolve due connections internally.

Result fields:

```json
{
  "mode": "delta",
  "connections_total": 2,
  "connections_succeeded": 2,
  "items_observed": 37,
  "membership_removed": 0
}
```

`membership_removed` is always zero for delta.

## `sync.starred.reconcile`

Input identifies one epoch connection and claims at most one page slice. Result fields include `epoch_id`, `processed_pages`, `processed_items`, `total_count_at_start`, `has_next_page`, `next_slice_not_before`, `status`, and `membership_removed`.

Only a successful terminal result with `has_next_page=false` may report a nonzero `membership_removed`.
