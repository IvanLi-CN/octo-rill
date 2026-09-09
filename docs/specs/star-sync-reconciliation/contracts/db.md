# Star Sync Reconciliation Database Contract

## `admin_runtime_settings`

New global columns:

- `star_sync_delta_interval_minutes INTEGER NOT NULL DEFAULT 30 CHECK (star_sync_delta_interval_minutes BETWEEN 1 AND 120)`
- `star_sync_full_sweep_interval_minutes INTEGER NOT NULL DEFAULT 1440 CHECK (star_sync_full_sweep_interval_minutes BETWEEN 60 AND 10080)`

These settings are independent from `sync_auto_fetch_interval_minutes` and have no pending effective-boundary field. A scheduler reads the latest saved values on its next tick; it does not reset an active epoch.

## `starred_repo_connection_memberships`

One canonical GitHub Star observation per `(user_id, github_connection_id, repo_id)`.

- `id TEXT PRIMARY KEY`
- `user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE`
- `github_connection_id TEXT NOT NULL REFERENCES github_connections(id) ON DELETE CASCADE`
- `repo_id INTEGER NOT NULL`
- repository identity fields required to rebuild `starred_repos`
- `starred_at TEXT NULL`
- `last_seen_epoch_id TEXT NULL REFERENCES star_sync_epochs(id) ON DELETE SET NULL`
- `last_delta_seen_at TEXT NULL`
- `created_at TEXT NOT NULL`
- `updated_at TEXT NOT NULL`
- `UNIQUE(user_id, github_connection_id, repo_id)`

Indexes support a connection epoch prune and a user/repo aggregate recomputation.

## `star_sync_epochs`

One active or historical full reconciliation epoch per GitHub connection.

- `id TEXT PRIMARY KEY`
- `user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE`
- `github_connection_id TEXT NOT NULL REFERENCES github_connections(id) ON DELETE CASCADE`
- `status TEXT NOT NULL CHECK (status IN ('queued','running','succeeded','failed','cancelled'))`
- `started_at TEXT NOT NULL`
- `next_cursor TEXT NULL`
- `total_count_at_start INTEGER NULL`
- `processed_pages INTEGER NOT NULL DEFAULT 0`
- `processed_items INTEGER NOT NULL DEFAULT 0`
- `next_slice_not_before TEXT NULL`
- `lease_owner_id TEXT NULL`
- `lease_expires_at TEXT NULL`
- `completed_at TEXT NULL`
- `failure_reason TEXT NULL`

Constraint: at most one `queued|running` epoch per `github_connection_id`, enforced with a partial unique index. The terminal `succeeded` status is the only status that authorizes membership pruning.

## Scheduler state

Use existing `scheduled_task_dispatch_state` with namespace-prefixed Star delta/full keys. It records dispatch and recovery facts only; epoch cursor/lease state stays in `star_sync_epochs`.
