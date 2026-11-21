-- The daily-brief executor upserts one row per (user, date) with
-- `ON CONFLICT (user_id, date)`. The legacy SQLite table enforced that
-- uniqueness until the window-snapshot rebuild replaced it with a partial
-- window index, so neither a fresh nor an imported database carries a matching
-- arbiter and every scheduled brief failed with 42P10. Production holds no
-- duplicate (user_id, date) pair, so the constraint is safe to add.
CREATE UNIQUE INDEX IF NOT EXISTS idx_briefs_user_date_unique
    ON public.briefs USING btree (user_id, date);
