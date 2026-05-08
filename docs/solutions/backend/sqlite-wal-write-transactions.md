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

OctoRill runs a single SQLite database with WAL mode and a configurable main pool. Background workers can concurrently claim job tasks, attach repo release watchers, claim repo release work items, schedule translation batches, and touch lightweight HTTP-side user activity fields.

## Symptoms

- Scheduled `sync.subscriptions` tasks fail repeatedly with `database is locked`.
- Container logs show SQLite code `517` (`SQLITE_BUSY_SNAPSHOT`) on claim or attach paths.
- Increasing pool size improves read availability but makes read-then-write transaction races easier to hit.

## Root Cause

SQLite `BEGIN` starts a deferred transaction. If a transaction reads first, another connection commits a write, and the first transaction later tries to write, SQLite cannot safely upgrade the stale read snapshot to a writer. The busy timeout does not fix this class of failure because the snapshot is already invalid.

WAL also keeps SQLite's single-writer rule. Multiple app workers can keep doing network and AI work concurrently, but their SQLite write sections must not all race the database writer lock directly. If they do, lock contention leaks as `database is locked` instead of backpressure.

## Resolution

Use an application-level SQLite writer coordinator for high-contention write paths. The coordinator grants one writer permit at a time, records lane/wait/attempt/elapsed telemetry, and applies bounded retry for SQLite busy/locked failures. Reads still use the existing pool; do not set the whole app to pool size 1.

Use `BEGIN IMMEDIATE` inside the writer permit for high-contention transactions that are known to write after an initial read. This acquires the SQLite writer slot before the transaction observes mutable state, so competing writers wait in the app coordinator instead of causing a stale snapshot upgrade failure.

Keep this targeted to write-intent paths such as:

- job task claim
- repo release work item claim
- repo release demand attach / watcher upsert
- translation batch scheduling or stale batch recovery when the transaction reads rows and then updates them
- lightweight user activity writes such as `last_active_at`, which should be best-effort, should not wait behind background writer backlog, and must not turn read endpoints into 500 responses

## Guardrails / Reuse Notes

- Do not solve this by forcing `OCTORILL_SQLITE_POOL_MAX_CONNECTIONS=1`; that hides the race by serializing the whole app and can starve HTTP reads.
- Do not lower worker/LLM concurrency as the durable database fix. Keep network and AI concurrency high; coordinate only the SQLite write section.
- Do not convert pure read transactions to `BEGIN IMMEDIATE`; that would unnecessarily block writers.
- Do not hold the writer permit around GitHub API, AI calls, HTTP calls, or long computation. Prepare data first, acquire the permit, write quickly, and release it.
- When diagnosing production incidents, copy a SQLite backup to an isolated testbox and reproduce with the same pool/concurrency settings before changing live service configuration.
- A minimal probe is: one deferred transaction reads, a second connection commits, then the first writes. Seeing `517` confirms the stale snapshot upgrade pattern.

## References

- `src/jobs.rs`
- `src/sqlite_write.rs`
- `src/sync.rs`
- `src/translations.rs`
