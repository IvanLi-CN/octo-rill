ALTER TABLE owned_repo_star_baselines
ADD COLUMN members_snapshot_initialized INTEGER NOT NULL DEFAULT 1;
