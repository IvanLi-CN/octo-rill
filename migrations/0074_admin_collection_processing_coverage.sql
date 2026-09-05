-- Keep source-record coverage separate from task execution state so an absent
-- task can distinguish a newly seen record from a legacy record with unknown
-- processing history.
CREATE TABLE IF NOT EXISTS admin_collection_processing_coverage (
  record_kind TEXT NOT NULL,
  record_id TEXT NOT NULL,
  pipeline TEXT NOT NULL,
  status_origin TEXT NOT NULL CHECK (status_origin IN ('never_started', 'historical_unknown')),
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (record_kind, record_id, pipeline)
);

CREATE INDEX IF NOT EXISTS idx_admin_collection_processing_coverage_lookup
  ON admin_collection_processing_coverage(record_kind, record_id, pipeline);

INSERT OR IGNORE INTO admin_collection_processing_coverage
  (record_kind, record_id, pipeline, status_origin)
SELECT 'release', CAST(release_id AS TEXT), 'translation', 'historical_unknown'
FROM repo_releases;

INSERT OR IGNORE INTO admin_collection_processing_coverage
  (record_kind, record_id, pipeline, status_origin)
SELECT 'release', CAST(release_id AS TEXT), 'polish', 'historical_unknown'
FROM repo_releases;

INSERT OR IGNORE INTO admin_collection_processing_coverage
  (record_kind, record_id, pipeline, status_origin)
SELECT DISTINCT
  'announcement',
  lower(repo_full_name) || '#' || CAST(discussion_number AS TEXT),
  'translation',
  'historical_unknown'
FROM social_activity_events
WHERE kind = 'announcement'
  AND repo_full_name IS NOT NULL
  AND discussion_number IS NOT NULL;

INSERT OR IGNORE INTO admin_collection_processing_coverage
  (record_kind, record_id, pipeline, status_origin)
SELECT DISTINCT
  'announcement',
  lower(repo_full_name) || '#' || CAST(discussion_number AS TEXT),
  'polish',
  'historical_unknown'
FROM social_activity_events
WHERE kind = 'announcement'
  AND repo_full_name IS NOT NULL
  AND discussion_number IS NOT NULL;

INSERT OR IGNORE INTO admin_collection_processing_coverage
  (record_kind, record_id, pipeline, status_origin)
SELECT 'brief', id, 'polish', 'historical_unknown'
FROM briefs;
