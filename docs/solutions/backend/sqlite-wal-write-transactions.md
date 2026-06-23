---
title: SQLite WAL write transactions under worker concurrency
module: backend-runtime
problem_type: sqlite-write-contention
component: jobs, repo-release-queue, translations
tags: [sqlite, wal, concurrency, jobs]
status: active
related_specs:
  - docs/specs/5prh7-sqlite-writer-coordinator/SPEC.md
  - docs/specs/s8qkn-subscription-sync/SPEC.md
  - docs/specs/p8r3l-public-release-endpoints/SPEC.md
---

# SQLite WAL Write Transactions Under Worker Concurrency

## Context

OctoRill runs a single SQLite database with WAL mode and a configurable main pool. Background workers can concurrently claim job tasks, attach repo release watchers, claim repo release work items, schedule translation batches, run LLM calls, and touch lightweight HTTP-side user activity fields.

## Symptoms

- Scheduled `sync.subscriptions` tasks fail repeatedly with `database is locked`.
- Container logs show SQLite code `517` (`SQLITE_BUSY_SNAPSHOT`) on claim or attach paths.
- Foreground paths such as login/session writes or job enqueue can fail with 500 (`failed to insert job task`) while high-concurrency background workers are mostly waiting on network IO.
- Increasing pool size improves read availability but makes read-then-write transaction races easier to hit.

## Root Cause

SQLite `BEGIN` starts a deferred transaction. If a transaction reads first, another connection commits a write, and the first transaction later tries to write, SQLite cannot safely upgrade the stale read snapshot to a writer. The busy timeout does not fix this class of failure because the snapshot is already invalid.

WAL also keeps SQLite's single-writer rule. Multiple app workers can keep doing network and AI work concurrently, but their SQLite write sections must not all race the database writer lock directly. If they do, lock contention leaks as `database is locked` instead of backpressure.

High worker counts are not the root bug when most worker time is network IO. Lowering `repo_release_worker_concurrency`, translation worker counts, or LLM concurrency can reduce symptoms, but it also reduces useful throughput and does not provide a durable SQLite write contract.

## Resolution

Use an application-level SQLite writer coordinator for high-contention write paths. The coordinator grants one writer permit at a time, records lane/priority/wait/attempt/elapsed telemetry, and applies bounded retry for SQLite busy/locked failures. Reads still use the existing pool; do not set the whole app to pool size 1.

The coordinator distinguishes three write priorities:

- `foreground`: user-visible writes such as session create/save/delete, login-driven session persistence, job enqueue, task event insertion, and cancellation. Foreground writes wait for the current writer and then run before queued background writes.
- `background`: worker claim, heartbeat, finalize, LLM call lifecycle, repo release state, and translation scheduler writes. Background work keeps network/AI concurrency outside the permit and only queues the short SQLite section.
- `best_effort`: non-critical touches such as `last_active_at` and expired-session cleanup. These writes may skip when a writer is already active or foreground work is waiting.

Use `BEGIN IMMEDIATE` inside the writer permit for high-contention transactions that are known to write after an initial read. This acquires the SQLite writer slot before the transaction observes mutable state, so competing writers wait in the app coordinator instead of causing a stale snapshot upgrade failure.

Keep this targeted to write-intent paths such as:

- job task claim
- job task enqueue, task event insertion, cancellation, heartbeat, finalize
- session create/save/delete
- LLM call insert/event/running/requeue/finalize/heartbeat/recovery
- repo release work item claim
- repo release demand attach / watcher upsert
- repo release work item finalize/heartbeat/fail, shared release upsert, sync-state success/failure
- translation batch scheduling or stale batch recovery when the transaction reads rows and then updates them
- lightweight user activity writes such as `last_active_at`, which should be best-effort, should not wait behind background writer backlog, and must not turn read endpoints into 500 responses

## Guardrails / Reuse Notes

- For `replace_starred_repos` and similar full-replacement write paths, acquire the writer permit with `BEGIN IMMEDIATE` before deleting/replacing rows. That keeps the stale snapshot upgrade from surfacing as `failed to clear starred_repos` under concurrent access refresh or subscription sync.
- For `store_sync_state_value` and related sync watermark writes, use the same coordinator lane and `BEGIN IMMEDIATE` contract. These upserts are high-contention write-intent paths during access refresh and subscription sync, and they should not bypass the writer coordinator by writing straight to the pool.
- Do not solve this by forcing `OCTORILL_SQLITE_POOL_MAX_CONNECTIONS=1`; that hides the race by serializing the whole app and can starve HTTP reads.
- Do not lower worker/LLM concurrency as the durable database fix. Keep network and AI concurrency high; coordinate only the SQLite write section.
- Do not preserve direct high-frequency writes to the pool in worker lifecycle paths. If a path can run per worker or per heartbeat, it needs a coordinator lane.
- Do not convert pure read transactions to `BEGIN IMMEDIATE`; that would unnecessarily block writers.
- Do not hold the writer permit around GitHub API, AI calls, HTTP calls, or long computation. Prepare data first, acquire the permit, write quickly, and release it.
- When diagnosing production incidents, copy a SQLite backup to an isolated testbox and reproduce with the same pool/concurrency settings before changing live service configuration.
- A minimal probe is: one deferred transaction reads, a second connection commits, then the first writes. Seeing `517` confirms the stale snapshot upgrade pattern.

## References

- `src/jobs.rs`
- `src/sqlite_write.rs`
- `src/sync.rs`
- `src/translations.rs`
