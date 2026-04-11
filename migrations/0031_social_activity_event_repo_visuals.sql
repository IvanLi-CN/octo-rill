ALTER TABLE social_activity_events
  ADD COLUMN repo_owner_avatar_url TEXT;

ALTER TABLE social_activity_events
  ADD COLUMN repo_open_graph_image_url TEXT;

ALTER TABLE social_activity_events
  ADD COLUMN repo_uses_custom_open_graph_image INTEGER;
