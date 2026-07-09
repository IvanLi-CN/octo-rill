CREATE INDEX IF NOT EXISTS idx_starred_repos_public_full_name_updated
ON starred_repos(lower(full_name), updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_owned_repo_star_baselines_public_full_name_updated
ON owned_repo_star_baselines(lower(repo_full_name), updated_at DESC);
