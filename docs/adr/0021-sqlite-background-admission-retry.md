# ADR 0021: SQLite Background Admission Bounded Retry

## Status

Accepted

## Context

SQLite writer contention is already serialized by an application coordinator with one background waiter and foreground priority. A background request that arrives while the waiter budget is occupied currently receives an admission error immediately. Under Social's four-user fan-out, that error is treated as a non-recoverable user failure even when the writer becomes available within the existing background deadline; the same pattern can also pressure heartbeat and finalize paths.

## Decision

- Retry background admission denial inside the coordinator using the original monotonic deadline and the existing 25/50/100ms capped retry schedule.
- Cap the actual retry sleep, including `BEGIN IMMEDIATE` busy retry, at that same absolute deadline; checking the remaining budget before sleeping is not sufficient if the clock advances between the check and the sleep.
- Do not register another background waiter, enlarge the queue, lower business worker concurrency, or reset the deadline between attempts. Foreground admission remains ahead of background work.
- Keep job task terminal finalization on the existing foreground lane so a completed or canceled task cannot be starved behind heartbeat admission; this is a lifecycle priority choice, not a new writer priority.
- When the deadline is exhausted, return the existing typed retryable writer deadline so the caller's existing worker backoff and lifecycle handling remain in control.
- Keep Social activity persistence in its existing fixed 64-row short transactions. Classify SQLite busy, writer deadline, and background admission exhaustion as `social_sync_failed` with `severity=warning`, `recoverable=true`, `error_kind=sqlite_write_backpressure`, and `retry_after=next_sync_subscriptions_run`. The user remains failed for the current run; upstream failures remain `error` and non-recoverable.
- Keep non-empty Social upstream `source_errors` in failed/non-success compatibility results, preserving their details even when a later SQLite write also fails. A completed-but-partially-degraded subscription snapshot retains the legacy `social_source_degraded` warning/recoverable event, while fatal upstream failures retain `social_sync_failed` error/non-recoverable semantics. Write the failure to the run log before `sync_subscription_events`; if only retryable SQLite pressure prevents the event-table append, keep the run log as the durable fallback and continue the remaining phases.
- Preserve any Social counters, including event rows committed by earlier 64-row chunks, and failed/partial repo diagnostics collected before a persistence failure in a typed error context, so compatibility callers do not replace a partial result with an empty or under-counted one.
- Carry a failed-but-partial Social worker outcome into `sync.subscriptions` summaries: increment `failed_users` and merge its committed counters; reserve the empty outcome for cancellation/incomplete work rather than using it to discard partial data.
- Finalize a `sync.all` Social business failure by committing the failed task row and its `task.completed` event in one foreground transaction; stop the task heartbeat before returning a finalization error so the lifecycle cannot retain a stale lease.
- Requeue or defer a running job after cancellation, webhook retry, or SQLite backpressure only when the original runtime owner still matches; a late worker must return no-op after lease recovery rather than requeueing a newer owner's task.
- Keep the `sync.all` compatibility completion payload complete: preserve `error_kind`, `severity`, `recoverable`, `retryable`, and `retry_after` for both SQLite backpressure and genuine upstream failures, including null retry metadata when the failure is non-recoverable.

## Consequences

Transient writer pressure can clear without creating a false Social failure, while persistent pressure still produces bounded partial results and caller backoff. The one-waiter cap and writer priority contract remain unchanged. Admin Jobs can distinguish recoverable SQLite pressure from a real GitHub/upstream failure through the existing event type and payload metadata.

## Verification

- `sqlite_write::tests::background_writer_admission_retries_without_growing_waiter_queue`
- `sqlite_write::tests::background_writer_admission_recovers_after_capacity_clears`
- `jobs::tests::runtime_owner_heartbeat_recovers_after_writer_pressure`
- `jobs::tests::job_task_heartbeat_recovers_after_writer_pressure`
- `jobs::tests::job_task_reschedule_keeps_foreground_priority_under_writer_pressure`
- `jobs::tests::job_task_finalize_keeps_foreground_priority_under_writer_pressure`
- `sync::tests::repo_release_heartbeat_recovers_after_writer_pressure`
- `sync::tests::social_sqlite_backpressure_is_recoverable_but_upstream_failure_is_not`
- `sync::tests::social_sync_sqlite_backpressure_emits_recoverable_event_and_counts_user_failed`
- `sync::tests::social_failure_event_falls_back_to_run_log_when_sqlite_is_busy`
- `sync::tests::social_upstream_source_error_is_nonrecoverable_and_counts_user_failed`
- `sync::tests::social_source_degraded_event_preserves_legacy_warning_contract`
- `jobs::tests::execute_sync_all_task_marks_social_source_errors_without_false_success`
- `jobs::tests::sync_all_social_failure_finalizes_task_failed_with_partial_result`
- `jobs::tests::sync_all_social_recoverable_failure_completion_payload_preserves_retry_metadata`
- `sync::tests::social_activity_failure_preserves_partial_result_and_upstream_classification`
- `sync::tests::social_activity_snapshot_preserves_committed_events_when_later_chunk_fails`
- `jobs::tests::owned_task_reschedule_does_not_reschedule_reclaimed_task`
- `sync::tests::social_activity_snapshot_deduplicates_cross_phase_source_errors`
- `sqlite_write::tests::background_retry_sleep_is_capped_at_absolute_deadline`
- `sqlite_write::tests::foreground_writer_succeeds_after_background_admission_denial`
- Mock-only Storybook: `Admin/Task Type Detail/SyncSubscriptionsRecoverableSocialWriterPressure`
