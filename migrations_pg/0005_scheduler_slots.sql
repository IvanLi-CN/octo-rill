-- PostgreSQL-native scheduler configuration. The slot state is durable
-- control data and must not depend on the legacy SQLite scheduler tables.
CREATE TABLE IF NOT EXISTS public.daily_brief_hour_slots (
    hour_utc BIGINT PRIMARY KEY CHECK (hour_utc >= 0 AND hour_utc <= 23),
    enabled BIGINT NOT NULL DEFAULT 1,
    last_dispatch_at TEXT,
    updated_at TEXT NOT NULL
);

INSERT INTO public.daily_brief_hour_slots (hour_utc, enabled, updated_at)
SELECT hour_utc, 1, CURRENT_TIMESTAMP::text
FROM generate_series(0, 23) AS hour_utc
ON CONFLICT (hour_utc) DO NOTHING;
