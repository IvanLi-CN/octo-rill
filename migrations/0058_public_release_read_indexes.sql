CREATE INDEX IF NOT EXISTS idx_ai_translations_public_lookup
ON ai_translations(entity_type, lang, entity_id, status, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_starred_repos_repo_public_updated
ON starred_repos(repo_id, is_private, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_owned_repo_star_baselines_repo_updated
ON owned_repo_star_baselines(repo_id, updated_at DESC);
