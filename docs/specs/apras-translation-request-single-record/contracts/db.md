# DB contracts

## Rebuilt tables

### `translation_requests`

- keep: `id`, `mode`, `source`, `request_origin`, `requested_by`, `scope_user_id`, `status`, `created_at`, `started_at`, `finished_at`, `updated_at`
- add / inline request payload: `producer_ref`, `kind`, `variant`, `entity_id`, `target_lang`, `max_wait_ms`, `source_hash`, `source_blocks_json`, `target_slots_json`
- add / inline execution binding: `work_item_id`
- add / inline result fields: `result_status`, `title_zh`, `summary_md`, `body_md`, `error_text`
- remove: `item_count`, `completed_item_count`
- `work_item_id` is nullable and points to `translation_work_items(id)`

### `translation_work_items`

- keep dedupe / scheduling / result fields
- no longer requires watcher table for request fan-out

### `translation_batches`

- keep `worker_slot`, `request_count`, `item_count`, `estimated_input_tokens`, status/timing fields
- `request_count` is computed from distinct `translation_requests.id` linked to selected `work_item_id`s

### `translation_batch_items`

- keep batch-to-work-item mapping and per-item execution result fields
- `producer_count` should represent the number of request rows attached to the work item

## Removed tables

- `translation_request_items`
- `translation_work_watchers`

## Migration behavior

- migration is destructive for translation scheduler state
- drop and recreate the translation scheduler domain tables
- keep unrelated tables such as `ai_translations` and `llm_calls`
