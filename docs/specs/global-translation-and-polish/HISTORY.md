# 全局翻译与润色工作模型演进记录

## Lifecycle

- Lifecycle: active
- This topic owns global content-processing identity, authorization association, result projection, legacy evidence and cutover compatibility.
- This topic also owns the admin collection activity read model: bounded source-time buckets and composite status are read-only projections of existing collection and processing facts.
- The existing translation scheduler topic continues to own shared batching, lease, recovery and attempt-audit mechanics.

## Compatibility

- The former user-scoped scheduler and cache tables remain historical evidence and are never rewritten by this topic.
- A result found only in legacy evidence is not promoted to a current global result or work state.
- The migration-bearing compatibility version is the earliest application version permitted after the global schema exists. A migration-preceding binary is intentionally unsupported after that point.
- Admin activity indexes are additive DDL; the activity reader does not backfill or mutate retained collection, legacy or global work facts.
- Model-specific global work and projection rows are reconciled to model-independent identities without discarding attempt, model-call or requester-link history. The latest published valid projection is the current result; existing blocked work is requeued only when no matching valid projection exists.
- The model-independent identity upgrade is separate from the already completed global cutover: its compatibility release adds schema only, and its later identity cutover adds no new migration.
- Provider admission is now a terminal boundary for persisted diagnostic calls: both an explicit rejection and an admission-check failure finalize the call as transient before returning, so no provider request or dangling `running` diagnostic remains.
- Source revision ties are resolved consistently across synchronization, API, scheduler and admin reads; legacy release ingest timestamps remain non-authoritative until upstream revision metadata is available.
- Announcement synchronization now rejects stale or older equal-timestamp payloads, and post-admission mode transitions retain provider attribution while closing the superseded attempt.
- Legacy active work without a verified source revision is now superseded before provider admission, and retention cleanup commits all audit-table changes atomically.
- Global batch adapters now retain per-item conflict facts and stream initial progress without changing the existing async/wait/stream wire shapes.
