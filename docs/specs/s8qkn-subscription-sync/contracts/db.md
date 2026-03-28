# DB contract

## `repo_releases` (new)

Shared repo-level release cache.

Key columns:

- `id TEXT PRIMARY KEY`
- `repo_id INTEGER NOT NULL`
- `release_id INTEGER NOT NULL UNIQUE`
- `node_id TEXT NULL`
- `tag_name TEXT NOT NULL`
- `name TEXT NULL`
- `body TEXT NULL`
- `html_url TEXT NOT NULL`
- `published_at TEXT NULL`
- `created_at TEXT NULL`
- `is_prerelease INTEGER NOT NULL DEFAULT 0`
- `is_draft INTEGER NOT NULL DEFAULT 0`
- `updated_at TEXT NOT NULL`
- reaction counters:
  - `react_plus1`
  - `react_laugh`
  - `react_heart`
  - `react_hooray`
  - `react_rocket`
  - `react_eyes`

Notes:

- Backfilled from historical user-scoped `releases` with `release_id` de-duplication, keeping the most recently updated row.
- Runtime reads now use `starred_repos + repo_releases`.

## `repo_release_work_items` (new)

Shared repo-level fetch queue.

Key columns:

- `id TEXT PRIMARY KEY`
- `repo_id INTEGER NOT NULL UNIQUE`
- `repo_full_name TEXT NOT NULL`
- `status TEXT NOT NULL`
- `request_origin TEXT NOT NULL`
- `priority INTEGER NOT NULL DEFAULT 0`
- `has_new_repo_watchers INTEGER NOT NULL DEFAULT 0`
- `deadline_at TEXT NOT NULL`
- `last_release_count INTEGER NOT NULL DEFAULT 0`
- `last_candidate_failures INTEGER NOT NULL DEFAULT 0`
- `last_success_at TEXT NULL`
- `error_text TEXT NULL`
- `created_at TEXT NOT NULL`
- `started_at TEXT NULL`
- `finished_at TEXT NULL`
- `updated_at TEXT NOT NULL`
- `runtime_owner_id TEXT NULL`
- `lease_heartbeat_at TEXT NULL`

Indexes:

- queue ordering index on `(status, priority DESC, has_new_repo_watchers DESC, deadline_at ASC, created_at ASC)`
- runtime lease index on `(status, runtime_owner_id, lease_heartbeat_at)`

Notes:

- One repo keeps one deduplicated work item.
- `priority=interactive` beats `priority=system`.
- Runtime lease columns follow the same single-process recovery model as `job_tasks`.

## `repo_release_watchers` (new)

Producer task to shared work-item fan-in table.

Key columns:

- `id TEXT PRIMARY KEY`
- `work_item_id TEXT NOT NULL` (`FK -> repo_release_work_items.id`)
- `task_id TEXT NOT NULL` (`FK -> job_tasks.id`)
- `user_id TEXT NULL` (`FK -> users.id`)
- `origin TEXT NOT NULL`
- `priority INTEGER NOT NULL DEFAULT 0`
- `reason TEXT NOT NULL`
- `is_new_repo INTEGER NOT NULL DEFAULT 0`
- `status TEXT NOT NULL DEFAULT 'pending'`
- `error_text TEXT NULL`
- `created_at TEXT NOT NULL`
- `updated_at TEXT NOT NULL`

Constraints / indexes:

- `UNIQUE(task_id, work_item_id)`
- task status index on `(task_id, status, created_at ASC)`
- work item status index on `(work_item_id, status, created_at ASC)`

Notes:

- `sync.access_refresh` and `sync.subscriptions` both use watchers to attach repo demand to shared work items.
- Watchers are marked `succeeded` immediately when an existing fresh cache is reusable.

## Historical tables

### `releases`

Status:

- retained for migration compatibility / historical data
- no longer part of runtime read or write paths for Release visibility

### `job_tasks`

Relevant task types:

- existing:
  - `sync.starred`
  - `sync.releases`
  - `sync.notifications`
  - `sync.all`
  - `sync.subscriptions`
- new:
  - `sync.access_refresh`

Notes:

- `sync.access_refresh` remains observable through existing `job_tasks` / `job_task_events`.
- Shared repo release fetching itself is stored in `repo_release_work_items`, not as additional `job_tasks`.

## Existing scheduler tables

### `scheduled_task_dispatch_state`

Unchanged responsibility:

- scheduler dedupe state for `sync.subscriptions`

### `sync_subscription_events`

Unchanged responsibility:

- append-only key-event audit table for subscription sync runs
- `release` phase events now describe attachment / shared queue outcomes rather than user fan-out writes
