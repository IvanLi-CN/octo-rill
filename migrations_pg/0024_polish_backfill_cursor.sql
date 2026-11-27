-- Cursor for the polishing audience-variant repair sweep (`polish.backfill`).
--
-- Releases admitted before polishing emitted `smart_public`/`smart_authorized`
-- keep only `smart`, and admission only runs for newly inserted Releases, so no
-- sync ever re-offers them. The sweep re-drives the existing admission over that
-- backlog one bounded page per run, and needs somewhere durable to remember how
-- far it got.
--
-- The cursor belongs here rather than in `content_processing_control`: that row
-- is the runtime processing mode, and a maintenance cursor has nothing to do
-- with it. One row is enough, so the primary key is pinned to `1` the same way
-- `content_processing_control` and `llm_recovery_flags` do.
--
-- `last_release_id` is the highest `repo_releases.release_id` a run *examined*,
-- not the highest it admitted, so a Release that legitimately admits nothing
-- cannot stall the sweep. `0` means "start from the beginning"; the executor
-- rewinds to `0` when a page comes back short, so the next trigger re-scans for
-- gaps that appeared since the last completed pass.
--
-- Additive and idempotent: a fresh deployment creates the row at the origin, and
-- an existing database keeps whatever progress it already recorded.
CREATE TABLE IF NOT EXISTS public.polish_backfill_cursor (
    id bigint PRIMARY KEY CHECK (id = 1),
    last_release_id bigint NOT NULL DEFAULT 0 CHECK (last_release_id >= 0),
    last_run_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO public.polish_backfill_cursor (id, last_release_id, last_run_at, updated_at)
VALUES (1, 0, NULL, CURRENT_TIMESTAMP)
ON CONFLICT (id) DO NOTHING;
