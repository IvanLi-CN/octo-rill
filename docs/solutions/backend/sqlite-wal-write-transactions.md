---
title: SQLite WAL write transactions under worker concurrency
module: backend-runtime
problem_type: sqlite-write-contention
component: jobs, repo-release-queue, translations
tags: [sqlite, wal, concurrency, jobs]
status: active
related_specs:
  - docs/specs/s8qkn-subscription-sync/SPEC.md
  - docs/specs/p8r3l-public-release-endpoints/SPEC.md
---

# SQLite WAL Write Transactions Under Worker Concurrency

## Context

OctoRill runs a single SQLite database with WAL mode and a configurable main pool. Background workers can concurrently claim job tasks, attach repo release watchers, claim repo release work items, and schedule translation batches.

## Symptoms

- Scheduled `sync.subscriptions` tasks fail repeatedly with `database is locked`.
- Container logs show SQLite code `517` (`SQLITE_BUSY_SNAPSHOT`) on claim or attach paths.
- Increasing pool size improves read availability but makes read-then-write transaction races easier to hit.

## Root Cause

SQLite `BEGIN` starts a deferred transaction. If a transaction reads first, another connection commits a write, and the first transaction later tries to write, SQLite cannot safely upgrade the stale read snapshot to a writer. The busy timeout does not fix this class of failure because the snapshot is already invalid.

## Resolution

Use `BEGIN IMMEDIATE` for high-contention transactions that are known to write after an initial read. This acquires the writer slot before the transaction observes mutable state, so competing writers wait instead of causing a stale snapshot upgrade failure.

Keep this targeted to write-intent paths such as:

- job task claim
- repo release work item claim
- repo release demand attach / watcher upsert
- translation batch scheduling or stale batch recovery when the transaction reads rows and then updates them

## Guardrails / Reuse Notes

- Do not solve this by forcing `OCTORILL_SQLITE_POOL_MAX_CONNECTIONS=1`; that hides the race by serializing the whole app and can starve HTTP reads.
- Do not convert pure read transactions to `BEGIN IMMEDIATE`; that would unnecessarily block writers.
- When diagnosing production incidents, copy a SQLite backup to an isolated testbox and reproduce with the same pool/concurrency settings before changing live service configuration.
- A minimal probe is: one deferred transaction reads, a second connection commits, then the first writes. Seeing `517` confirms the stale snapshot upgrade pattern.

## References

- `src/jobs.rs`
- `src/sync.rs`
- `src/translations.rs`
