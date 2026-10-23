# Admin Jobs Read Hotpaths

## Problem

The Admin Jobs read surface has two independent SQLite hotpaths. The realtime task list combines an exact count, filtered pagination, and subscription diagnostics over `job_tasks` and related event tables. The sync runtime configuration endpoint selects the latest non-skipped subscription tasks by parsing `result_json`, then rolls up their child task chain.

These reads are separate from scheduler writer admission and transaction cleanup. They must remain read-only and must not change task creation, task history, search projection, or foreground writer behavior.

## Contract

`GET /api/admin/jobs/realtime` preserves its response shape, exact `total`, page/page_size semantics, filter combinations, newest-first ordering, and subscription diagnostics. Diagnostics are bounded per task by the existing event limit.

`GET /api/admin/jobs/sync/runtime-config` preserves the recent task list shape, newest three non-skipped subscription roots, child-chain duration calculation, unfinished-chain behavior, and empty-result behavior.

## Performance Boundary

The page, count, diagnostic, root-selection, and chain-rollup phases are observable as low-cardinality route and phase measurements. Production-shaped fixtures must target p95 below one second and p99 below two seconds for both endpoints, with no temporary sort for the bounded page or root-selection paths.

Realtime subscription diagnostics read at most the configured event limit per task through one bounded index-range branch per selected task, using the existing `(task_id, created_at, id)` index and deterministic newest-first ordering. The list and detail paths share this ordering contract, so event history growth does not expand a window-function intermediate result.

Index changes are delivered only through ordered migrations. The request path never creates or rebuilds an index. Any partial index over the JSON skipped predicate must be proven by `EXPLAIN QUERY PLAN` before release.

## Verification

Validation covers historical task volume, every supported filter combination, skipped-task density, child-chain states, authenticated requests, and concurrent background writer pressure. The read-path change is accepted only when foreground writes retain their existing success and bounded-wait behavior and production observation shows no new lock, deadline, or cleanup errors.
