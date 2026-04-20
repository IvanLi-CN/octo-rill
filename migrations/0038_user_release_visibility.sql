ALTER TABLE users
  ADD COLUMN include_own_releases INTEGER NOT NULL DEFAULT 0;

DROP VIEW IF EXISTS user_release_visible_repos;

CREATE VIEW user_release_visible_repos AS
SELECT
  sr.user_id AS user_id,
  sr.repo_id AS repo_id,
  sr.full_name AS full_name,
  sr.owner_login AS owner_login,
  sr.name AS name,
  sr.description AS description,
  sr.html_url AS html_url,
  sr.stargazed_at AS stargazed_at,
  sr.is_private AS is_private,
  sr.updated_at AS updated_at,
  sr.owner_avatar_url AS owner_avatar_url,
  sr.open_graph_image_url AS open_graph_image_url,
  sr.uses_custom_open_graph_image AS uses_custom_open_graph_image
FROM starred_repos sr

UNION ALL

SELECT
  ob.user_id AS user_id,
  ob.repo_id AS repo_id,
  ob.repo_full_name AS full_name,
  CASE
    WHEN instr(ob.repo_full_name, '/') > 0
      THEN substr(ob.repo_full_name, 1, instr(ob.repo_full_name, '/') - 1)
    ELSE ob.repo_full_name
  END AS owner_login,
  CASE
    WHEN instr(ob.repo_full_name, '/') > 0
      THEN substr(ob.repo_full_name, instr(ob.repo_full_name, '/') + 1)
    ELSE ob.repo_full_name
  END AS name,
  NULL AS description,
  'https://github.com/' || ob.repo_full_name AS html_url,
  NULL AS stargazed_at,
  0 AS is_private,
  ob.updated_at AS updated_at,
  ob.owner_avatar_url AS owner_avatar_url,
  ob.open_graph_image_url AS open_graph_image_url,
  ob.uses_custom_open_graph_image AS uses_custom_open_graph_image
FROM owned_repo_star_baselines ob
JOIN users u
  ON u.id = ob.user_id
WHERE u.include_own_releases != 0
  AND NOT EXISTS (
    SELECT 1
    FROM starred_repos sr
    WHERE sr.user_id = ob.user_id
      AND sr.repo_id = ob.repo_id
  );
