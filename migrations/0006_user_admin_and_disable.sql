PRAGMA foreign_keys = ON;

ALTER TABLE users ADD COLUMN is_admin INTEGER NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN is_disabled INTEGER NOT NULL DEFAULT 0;

UPDATE users
SET is_admin = 1
WHERE id = (
    SELECT id
    FROM users
    ORDER BY created_at ASC, id ASC
    LIMIT 1
)
AND NOT EXISTS (
    SELECT 1 FROM users WHERE is_admin = 1
);
