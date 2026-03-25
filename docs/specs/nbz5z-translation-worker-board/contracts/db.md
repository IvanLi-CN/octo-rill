# DB contracts

## Modified tables

- `translation_requests`
  - add `request_origin TEXT NOT NULL CHECK (request_origin IN ('user', 'system'))`
  - current `/api/translate/requests*` writes `user`
  - future internal/system producers must write `system`

- `translation_batches`
  - add `worker_slot INTEGER NOT NULL`
  - add `request_count INTEGER NOT NULL`
  - add `runtime_owner_id TEXT NULL`
  - add `lease_heartbeat_at TEXT NULL`

## Runtime semantics

- Worker slots are fixed:
  - `1..3` => `general`
  - `4` => `user_dedicated`
- `system` requests can only be claimed by `general` workers.
- `user` requests can be claimed by any worker, but the dedicated worker only claims `user` requests.
- Running batch leases heartbeat every 10s.
- Startup recovery immediately reclaims batches owned by a previous runtime as `failed(runtime_lease_expired)`.
- Periodic sweep only reclaims rows with missing heartbeat or heartbeat older than 90s.
