# Webhook Push Desired State

Webhook Push stores the user's intended lifecycle separately from the observed state of each GitHub Hook. The durable target is `enabled`, `paused`, or `deleted`; local delivery is disabled before a pause or delete operation reaches GitHub, and background work retries remote reconciliation. This preserves user intent across transient failures and avoids treating a missing observation row as an abnormal state.

Remote mutations are restricted to Hooks that match the stored Hook ID, callback URL, and `release` event. A user has at most one active management or audit operation, so a later manual request cannot race an earlier target-alignment attempt.

The scheduled audit is a dispatcher, not a second remote worker. It records the audit start and enqueues one `webhook.push.manage` task per eligible user with `scheduled=true`; the user task owns GitHub calls, the user lease, cancellation boundaries, and retry timing. This keeps scheduled and manual reconciliation on one idempotent path.

All webhook worker persistence goes through `SqliteWriteCoordinator` for short local write sections. A busy error that survives coordinator retries is rescheduled with the existing task `available_at` and `retry_count` backoff, so a transient SQLite outage cannot replace the user's desired state with an observation failure.

An archived or read-only GitHub repository is represented as an `archived` observation error. It is not a PAT permission failure: the UI explains that GitHub disallows Hook changes for the archived repository and does not direct the user to change credentials. Because the deployed `0066` SQLite CHECK predates this observation label, the row persists with `status='error'` and `error_kind='archived'`; the API derives `status='archived'` for clients.

A reconciliation completes after every eligible repository has an independent disposition. Archived repositories and repository-local action-required outcomes remain visible but neither block completion nor cause healthy repositories to be retried. Transient GitHub and SQLite failures retain only their affected work for bounded retries.

Newly persisted eligible owned-repository baselines create a durable, generation-based Webhook 对齐需求 in the same transaction. A dispatcher consumes that demand after commit and coalesces it with the single user-scoped operation; a demand that arrives during an operation causes one follow-up reconciliation. A post-commit best-effort enqueue was rejected because a process failure could defer registration until the periodic audit.
