CREATE TABLE IF NOT EXISTS user_repo_associations (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  repo_id INTEGER,
  repo_full_name TEXT NOT NULL,
  repo_full_name_lower TEXT NOT NULL,
  owner_login TEXT NOT NULL,
  repo_name TEXT NOT NULL,
  html_url TEXT,
  description TEXT,
  is_private INTEGER,
  owner_avatar_url TEXT,
  open_graph_image_url TEXT,
  uses_custom_open_graph_image INTEGER NOT NULL DEFAULT 0,
  first_source TEXT NOT NULL
    CHECK (first_source IN ('personal_owned', 'github_star', 'manual_feed')),
  first_associated_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL,
  is_following INTEGER NOT NULL DEFAULT 0,
  follow_state_source TEXT NOT NULL DEFAULT 'system_default'
    CHECK (follow_state_source IN ('system_default', 'user_explicit')),
  has_personal_owned_source INTEGER NOT NULL DEFAULT 0,
  has_github_star_source INTEGER NOT NULL DEFAULT 0,
  has_manual_feed_source INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, repo_full_name_lower),
  FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_user_repo_associations_user_following
  ON user_repo_associations(user_id, is_following, repo_full_name_lower);

CREATE INDEX IF NOT EXISTS idx_user_repo_associations_user_repo_id
  ON user_repo_associations(user_id, repo_id);

CREATE INDEX IF NOT EXISTS idx_user_repo_associations_repo_lower
  ON user_repo_associations(repo_full_name_lower);

INSERT INTO user_repo_associations (
  id,
  user_id,
  repo_id,
  repo_full_name,
  repo_full_name_lower,
  owner_login,
  repo_name,
  html_url,
  description,
  is_private,
  owner_avatar_url,
  open_graph_image_url,
  uses_custom_open_graph_image,
  first_source,
  first_associated_at,
  last_seen_at,
  is_following,
  follow_state_source,
  has_personal_owned_source,
  has_github_star_source,
  has_manual_feed_source,
  created_at,
  updated_at
)
SELECT
  lower(hex(randomblob(16))) AS id,
  ob.user_id,
  ob.repo_id,
  ob.repo_full_name,
  lower(ob.repo_full_name),
  CASE
    WHEN instr(ob.repo_full_name, '/') > 0
      THEN substr(ob.repo_full_name, 1, instr(ob.repo_full_name, '/') - 1)
    ELSE ob.repo_full_name
  END AS owner_login,
  CASE
    WHEN instr(ob.repo_full_name, '/') > 0
      THEN substr(ob.repo_full_name, instr(ob.repo_full_name, '/') + 1)
    ELSE ob.repo_full_name
  END AS repo_name,
  'https://github.com/' || ob.repo_full_name AS html_url,
  NULL AS description,
  ob.is_private,
  ob.owner_avatar_url,
  ob.open_graph_image_url,
  COALESCE(ob.uses_custom_open_graph_image, 0),
  'personal_owned' AS first_source,
  COALESCE(ob.initialized_at, ob.updated_at) AS first_associated_at,
  ob.updated_at AS last_seen_at,
  1 AS is_following,
  'system_default' AS follow_state_source,
  1 AS has_personal_owned_source,
  0 AS has_github_star_source,
  0 AS has_manual_feed_source,
  COALESCE(ob.initialized_at, ob.updated_at) AS created_at,
  ob.updated_at AS updated_at
FROM owned_repo_star_baselines ob
WHERE 1
ON CONFLICT(user_id, repo_full_name_lower) DO UPDATE SET
  repo_id = COALESCE(excluded.repo_id, user_repo_associations.repo_id),
  repo_full_name = excluded.repo_full_name,
  owner_login = excluded.owner_login,
  repo_name = excluded.repo_name,
  html_url = COALESCE(excluded.html_url, user_repo_associations.html_url),
  is_private = COALESCE(excluded.is_private, user_repo_associations.is_private),
  owner_avatar_url = COALESCE(excluded.owner_avatar_url, user_repo_associations.owner_avatar_url),
  open_graph_image_url = COALESCE(excluded.open_graph_image_url, user_repo_associations.open_graph_image_url),
  uses_custom_open_graph_image = CASE
    WHEN excluded.owner_avatar_url IS NOT NULL
      OR excluded.open_graph_image_url IS NOT NULL
      OR excluded.uses_custom_open_graph_image != 0
      THEN excluded.uses_custom_open_graph_image
    ELSE user_repo_associations.uses_custom_open_graph_image
  END,
  first_source = CASE
    WHEN excluded.first_associated_at < user_repo_associations.first_associated_at
      OR (
        excluded.first_associated_at = user_repo_associations.first_associated_at
        AND user_repo_associations.first_source != 'personal_owned'
      )
      THEN excluded.first_source
    ELSE user_repo_associations.first_source
  END,
  first_associated_at = CASE
    WHEN excluded.first_associated_at < user_repo_associations.first_associated_at
      THEN excluded.first_associated_at
    ELSE user_repo_associations.first_associated_at
  END,
  last_seen_at = MAX(user_repo_associations.last_seen_at, excluded.last_seen_at),
  is_following = CASE
    WHEN user_repo_associations.follow_state_source = 'system_default'
      THEN 1
    ELSE user_repo_associations.is_following
  END,
  has_personal_owned_source = 1,
  updated_at = excluded.updated_at;

INSERT INTO user_repo_associations (
  id,
  user_id,
  repo_id,
  repo_full_name,
  repo_full_name_lower,
  owner_login,
  repo_name,
  html_url,
  description,
  is_private,
  owner_avatar_url,
  open_graph_image_url,
  uses_custom_open_graph_image,
  first_source,
  first_associated_at,
  last_seen_at,
  is_following,
  follow_state_source,
  has_personal_owned_source,
  has_github_star_source,
  has_manual_feed_source,
  created_at,
  updated_at
)
SELECT
  lower(hex(randomblob(16))) AS id,
  sr.user_id,
  sr.repo_id,
  sr.full_name,
  lower(sr.full_name),
  sr.owner_login,
  sr.name,
  sr.html_url,
  sr.description,
  sr.is_private,
  sr.owner_avatar_url,
  sr.open_graph_image_url,
  COALESCE(sr.uses_custom_open_graph_image, 0),
  'github_star' AS first_source,
  COALESCE(sr.stargazed_at, sr.updated_at) AS first_associated_at,
  sr.updated_at AS last_seen_at,
  1 AS is_following,
  'system_default' AS follow_state_source,
  0 AS has_personal_owned_source,
  1 AS has_github_star_source,
  0 AS has_manual_feed_source,
  COALESCE(sr.stargazed_at, sr.updated_at) AS created_at,
  sr.updated_at AS updated_at
FROM starred_repos sr
WHERE 1
ON CONFLICT(user_id, repo_full_name_lower) DO UPDATE SET
  repo_id = COALESCE(excluded.repo_id, user_repo_associations.repo_id),
  repo_full_name = excluded.repo_full_name,
  owner_login = excluded.owner_login,
  repo_name = excluded.repo_name,
  html_url = COALESCE(excluded.html_url, user_repo_associations.html_url),
  description = COALESCE(excluded.description, user_repo_associations.description),
  is_private = COALESCE(excluded.is_private, user_repo_associations.is_private),
  owner_avatar_url = COALESCE(excluded.owner_avatar_url, user_repo_associations.owner_avatar_url),
  open_graph_image_url = COALESCE(excluded.open_graph_image_url, user_repo_associations.open_graph_image_url),
  uses_custom_open_graph_image = CASE
    WHEN excluded.owner_avatar_url IS NOT NULL
      OR excluded.open_graph_image_url IS NOT NULL
      OR excluded.uses_custom_open_graph_image != 0
      THEN excluded.uses_custom_open_graph_image
    ELSE user_repo_associations.uses_custom_open_graph_image
  END,
  first_source = CASE
    WHEN excluded.first_associated_at < user_repo_associations.first_associated_at
      THEN excluded.first_source
    ELSE user_repo_associations.first_source
  END,
  first_associated_at = CASE
    WHEN excluded.first_associated_at < user_repo_associations.first_associated_at
      THEN excluded.first_associated_at
    ELSE user_repo_associations.first_associated_at
  END,
  last_seen_at = MAX(user_repo_associations.last_seen_at, excluded.last_seen_at),
  is_following = CASE
    WHEN user_repo_associations.follow_state_source = 'system_default'
      THEN 1
    ELSE user_repo_associations.is_following
  END,
  has_github_star_source = 1,
  updated_at = excluded.updated_at;

DROP VIEW IF EXISTS user_following_repos;

CREATE VIEW user_following_repos AS
SELECT
  user_id,
  repo_id,
  repo_full_name AS full_name,
  owner_login,
  repo_name AS name,
  html_url,
  description,
  is_private,
  last_seen_at AS updated_at,
  owner_avatar_url,
  open_graph_image_url,
  uses_custom_open_graph_image,
  first_source,
  first_associated_at,
  follow_state_source,
  has_personal_owned_source,
  has_github_star_source,
  has_manual_feed_source
FROM user_repo_associations
WHERE is_following != 0
  AND repo_id IS NOT NULL

UNION ALL

SELECT
  sr.user_id AS user_id,
  sr.repo_id AS repo_id,
  sr.full_name AS full_name,
  sr.owner_login AS owner_login,
  sr.name AS name,
  sr.html_url AS html_url,
  sr.description AS description,
  sr.is_private AS is_private,
  sr.updated_at AS updated_at,
  sr.owner_avatar_url AS owner_avatar_url,
  sr.open_graph_image_url AS open_graph_image_url,
  sr.uses_custom_open_graph_image AS uses_custom_open_graph_image,
  'github_star' AS first_source,
  sr.stargazed_at AS first_associated_at,
  'system_default' AS follow_state_source,
  0 AS has_personal_owned_source,
  1 AS has_github_star_source,
  0 AS has_manual_feed_source
FROM starred_repos sr
WHERE NOT EXISTS (
  SELECT 1
  FROM user_repo_associations ura
  WHERE ura.user_id = sr.user_id
    AND ura.repo_full_name_lower = lower(sr.full_name)
)

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
  'https://github.com/' || ob.repo_full_name AS html_url,
  NULL AS description,
  ob.is_private AS is_private,
  ob.updated_at AS updated_at,
  ob.owner_avatar_url AS owner_avatar_url,
  ob.open_graph_image_url AS open_graph_image_url,
  ob.uses_custom_open_graph_image AS uses_custom_open_graph_image,
  'personal_owned' AS first_source,
  COALESCE(ob.initialized_at, ob.updated_at) AS first_associated_at,
  'system_default' AS follow_state_source,
  1 AS has_personal_owned_source,
  0 AS has_github_star_source,
  0 AS has_manual_feed_source
FROM owned_repo_star_baselines ob
WHERE NOT EXISTS (
  SELECT 1
  FROM user_repo_associations ura
  WHERE ura.user_id = ob.user_id
    AND ura.repo_full_name_lower = lower(ob.repo_full_name)
);

DROP VIEW IF EXISTS user_release_visible_repos;

CREATE VIEW user_release_visible_repos AS
SELECT
  ura.user_id AS user_id,
  ura.repo_id AS repo_id,
  ura.repo_full_name AS full_name,
  ura.owner_login AS owner_login,
  ura.repo_name AS name,
  ura.description AS description,
  COALESCE(ura.html_url, 'https://github.com/' || ura.repo_full_name) AS html_url,
  sr.stargazed_at AS stargazed_at,
  ura.is_private AS is_private,
  ura.last_seen_at AS updated_at,
  ura.owner_avatar_url AS owner_avatar_url,
  ura.open_graph_image_url AS open_graph_image_url,
  ura.uses_custom_open_graph_image AS uses_custom_open_graph_image
FROM user_repo_associations ura
JOIN users u
  ON u.id = ura.user_id
LEFT JOIN starred_repos sr
  ON sr.user_id = ura.user_id
 AND lower(sr.full_name) = ura.repo_full_name_lower
WHERE ura.is_following != 0
  AND ura.repo_id IS NOT NULL
  AND (
    ura.has_personal_owned_source = 0
    OR ura.has_github_star_source != 0
    OR u.include_own_releases != 0
  )

UNION ALL

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
WHERE NOT EXISTS (
  SELECT 1
  FROM user_repo_associations ura
  WHERE ura.user_id = sr.user_id
    AND ura.repo_full_name_lower = lower(sr.full_name)
)

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
  ob.is_private AS is_private,
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
    FROM user_repo_associations ura
    WHERE ura.user_id = ob.user_id
      AND ura.repo_full_name_lower = lower(ob.repo_full_name)
  )
  AND NOT EXISTS (
    SELECT 1
    FROM starred_repos sr
    WHERE sr.user_id = ob.user_id
      AND sr.repo_id = ob.repo_id
  );
