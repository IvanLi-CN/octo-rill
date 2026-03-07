# DB contracts

## New tables

- `translation_requests`
  - producer-facing request envelope
- `translation_request_items`
  - per-request item status and returned fields
- `translation_work_items`
  - deduplicated scheduler work items keyed by scope + kind + variant + entity + source hash
- `translation_work_watchers`
  - request-item to work-item fan-out mapping
- `translation_batches`
  - actual scheduled batches with trigger reason and token estimate
- `translation_batch_items`
  - batch membership and terminal per-item status/error

## Modified tables

- `llm_calls`
  - add `parent_translation_batch_id`
  - add translation-specific linkage indexes for admin tracing
