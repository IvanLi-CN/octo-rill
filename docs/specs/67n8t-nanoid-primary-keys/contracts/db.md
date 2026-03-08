# Database contract changes for #67n8t

## Scope

This document defines the local identifier policy for application tables.

## Identifier policy

- Local entity/relationship primary keys use `TEXT PRIMARY KEY` with a 16-character NanoID.
- Foreign keys targeting those primary keys must also use `TEXT`.
- Natural business-key primary keys remain unchanged:
  - `daily_brief_hour_slots.hour_utc`
  - `scheduled_task_dispatch_state.schedule_name`
- GitHub-origin business identifiers remain as their current integer/text shapes and are not promoted to local primary keys.

## Migration policy

- The repo uses a destructive rebuild migration for application tables.
- Compatibility with existing SQLite data files is explicitly out of scope.
- Empty database initialization through the full migration chain must end in the NanoID schema.

## Tables affected

- `users`, `user_tokens`, `starred_repos`, `releases`, `notifications`, `briefs`, `sync_state`, `ai_translations`, `reaction_pat_tokens`
- `job_tasks`, `job_task_events`, `llm_calls`, `llm_call_events`, `sync_subscription_events`
- `translation_requests`, `translation_request_items`, `translation_work_items`, `translation_work_watchers`, `translation_batches`, `translation_batch_items`

## Constraints to preserve

- Existing uniqueness rules, foreign-key cascade/set-null behaviors, and check constraints remain semantically unchanged unless they depend on numeric auto-increment behavior.
- Ordering semantics that previously used auto-increment event/item ids may either keep integer surrogate row ids for ordering-only rows or switch to NanoID plus explicit timestamps/indexes; the implementation must preserve deterministic ordering for current APIs and tests.
