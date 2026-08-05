# Database contract

## Table changes

`users` adds:

- `is_admin INTEGER NOT NULL DEFAULT 0`
- `is_disabled INTEGER NOT NULL DEFAULT 0`

## Backfill rules

After adding columns:

1. Ensure all existing rows have non-null defaults (SQLite handles via DEFAULT for new column).
2. If no admin exists, set admin for exactly one row:
   - first by earliest `created_at` ascending,
   - tie-break by smallest `id` ascending.
