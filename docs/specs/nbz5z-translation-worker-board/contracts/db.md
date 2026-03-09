# DB contracts

## Modified tables

- `translation_requests`
  - add `request_origin TEXT NOT NULL CHECK (request_origin IN ('user', 'system'))`
  - current `/api/translate/requests*` writes `user`
  - future internal/system producers must write `system`

- `translation_batches`
  - add `worker_slot INTEGER NOT NULL`
  - add `request_count INTEGER NOT NULL`

## Runtime semantics

- Worker slots are fixed:
  - `1..3` => `general`
  - `4` => `user_dedicated`
- `system` requests can only be claimed by `general` workers.
- `user` requests can be claimed by any worker, but the dedicated worker only claims `user` requests.
