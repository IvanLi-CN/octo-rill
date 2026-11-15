-- PostgreSQL daily brief snapshots and release membership projection.
CREATE TABLE IF NOT EXISTS briefs (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    date text NOT NULL,
    window_start_utc timestamptz,
    window_end_utc timestamptz,
    effective_time_zone text,
    effective_local_boundary text,
    generation_source text NOT NULL DEFAULT 'legacy',
    content_markdown text NOT NULL,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_briefs_user_date_created_at ON briefs(user_id, date DESC, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_briefs_user_window_end_created_at ON briefs(user_id, window_end_utc DESC, created_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_briefs_user_window_unique ON briefs(user_id, window_start_utc, window_end_utc) WHERE window_start_utc IS NOT NULL AND window_end_utc IS NOT NULL;

CREATE TABLE IF NOT EXISTS brief_release_memberships (
    brief_id text NOT NULL REFERENCES briefs(id) ON DELETE CASCADE,
    release_id bigint NOT NULL,
    release_ts_utc timestamptz NOT NULL,
    ordinal integer NOT NULL,
    created_at timestamptz NOT NULL,
    PRIMARY KEY (brief_id, release_id)
);

CREATE INDEX IF NOT EXISTS idx_brief_release_memberships_release_id ON brief_release_memberships(release_id);
CREATE INDEX IF NOT EXISTS idx_brief_release_memberships_brief_ordinal ON brief_release_memberships(brief_id, ordinal ASC);
