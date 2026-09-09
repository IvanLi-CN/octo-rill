# ADR 0005: Unify Subscription and Governance Windows

## Status

Superseded by [ADR 0006](./0006-decouple-star-sync-schedules.md).

## Context

`sync.subscriptions` previously had an administrator-controlled scheduler interval while Release governance independently used a fixed ten-minute budget window. The two clocks could disagree: a task could run at one cadence while repository selection and the UI described another. The second editable interval also allowed a non-interval task save to appear to change subscription scheduling.

## Decision

- Treat `admin_runtime_settings.sync_auto_fetch_interval_minutes` as the single window length `N` for both subscription scheduling and Release governance. The valid range remains 1-120 minutes.
- When `N` is saved, persist `sync_auto_fetch_effective_at` as the next UTC epoch-aligned boundary strictly after the save instant. The scheduler does not enqueue or catch up before that boundary.
- Freeze `N` and the budget `B` in every active governance cycle. Snapshot rebuilds may refresh candidates, but an active cycle keeps its original `window_minutes`, `window_budget`, and selection-window gate until completion.
- Add `window_minutes` and `last_selection_window_index` to governance cycles. Existing cycles are backfilled with `window_minutes=10` and their existing `window_index_started_at` as the historical last-selection window; tasks, members, and watchers are not rewritten.
- Claim at most `B` repositories once per cycle/window inside one SQLite writer transaction. A scheduler overlap records `skipped` through the existing in-flight path rather than starting concurrent subscription work.
- Keep the subscription settings dialog as the only editor for `N`, `B`, concurrency, and freshness. The task interval dialog renders the current `N` and its effective time read-only and links to the subscription settings.
- Administrator retry of a completed `sync.subscriptions` task reuses only that task's `repo_release_watchers` whose work is failed or incomplete. Retry does not rebuild candidates, perform a new budget selection, or exceed the active cycle budget.
- Persist actual `N`, `B`, cycle ID, and selection window in task results and diagnostics. Governance labels use `W{target_window} · {target_interval_minutes} 分钟`, where the interval is derived from the cycle's `N`.

## Compatibility

The historical migration remains additive. Existing cycle rows retain their status, budget, timestamps, members, tasks, and watchers. The successor keeps the Release-governance parts of this decision, but moves Star scheduling, membership reconciliation, and its runtime controls to an independent coordinator.

## Consequences

History remains factual even after administrators change the current interval. A new Release-governance cycle adopts the latest saved values only after the current cycle completes. Star synchronization no longer shares this interval or its task path.
