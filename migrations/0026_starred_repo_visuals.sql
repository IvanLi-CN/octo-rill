ALTER TABLE starred_repos
  ADD COLUMN owner_avatar_url TEXT;

ALTER TABLE starred_repos
  ADD COLUMN open_graph_image_url TEXT;

ALTER TABLE starred_repos
  ADD COLUMN uses_custom_open_graph_image INTEGER NOT NULL DEFAULT 0;
