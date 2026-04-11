ALTER TABLE owned_repo_star_baselines
  ADD COLUMN owner_avatar_url TEXT;

ALTER TABLE owned_repo_star_baselines
  ADD COLUMN open_graph_image_url TEXT;

ALTER TABLE owned_repo_star_baselines
  ADD COLUMN uses_custom_open_graph_image INTEGER NOT NULL DEFAULT 0;
