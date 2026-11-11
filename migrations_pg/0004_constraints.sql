-- Baseline invariants. These checks fail fast on a partial or incompatible schema.
DO $$
BEGIN
  IF to_regclass('public.users') IS NULL OR to_regclass('public.job_tasks') IS NULL
     OR to_regclass('public.search_documents') IS NULL THEN
    RAISE EXCEPTION 'PostgreSQL baseline is incomplete';
  END IF;
  IF to_regclass('tower_sessions.session') IS NULL THEN
    RAISE EXCEPTION 'tower_sessions.session is missing';
  END IF;
END
$$;
