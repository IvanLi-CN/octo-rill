-- Existing records without coverage predate known-zero tracking. Preserve their
-- unknown history, then mark future source records as known never-started.
INSERT OR IGNORE INTO admin_collection_processing_coverage
  (record_kind, record_id, pipeline, status_origin)
SELECT 'notification', thread_id, pipeline, 'historical_unknown'
FROM notifications
CROSS JOIN (SELECT 'translation' AS pipeline UNION ALL SELECT 'polish');

CREATE TRIGGER IF NOT EXISTS admin_collection_coverage_repo_release_insert
AFTER INSERT ON repo_releases
BEGIN
  INSERT OR IGNORE INTO admin_collection_processing_coverage
    (record_kind, record_id, pipeline, status_origin)
  VALUES
    ('release', CAST(NEW.release_id AS TEXT), 'translation', 'never_started'),
    ('release', CAST(NEW.release_id AS TEXT), 'polish', 'never_started');
END;

CREATE TRIGGER IF NOT EXISTS admin_collection_coverage_announcement_insert
AFTER INSERT ON social_activity_events
WHEN NEW.kind = 'announcement'
  AND NEW.repo_full_name IS NOT NULL
  AND NEW.discussion_number IS NOT NULL
BEGIN
  INSERT OR IGNORE INTO admin_collection_processing_coverage
    (record_kind, record_id, pipeline, status_origin)
  VALUES
    ('announcement', lower(NEW.repo_full_name) || '#' || CAST(NEW.discussion_number AS TEXT), 'translation', 'never_started'),
    ('announcement', lower(NEW.repo_full_name) || '#' || CAST(NEW.discussion_number AS TEXT), 'polish', 'never_started');
END;

CREATE TRIGGER IF NOT EXISTS admin_collection_coverage_notification_insert
AFTER INSERT ON notifications
BEGIN
  INSERT OR IGNORE INTO admin_collection_processing_coverage
    (record_kind, record_id, pipeline, status_origin)
  VALUES
    ('notification', NEW.thread_id, 'translation', 'never_started'),
    ('notification', NEW.thread_id, 'polish', 'never_started');
END;

CREATE TRIGGER IF NOT EXISTS admin_collection_coverage_brief_insert
AFTER INSERT ON briefs
BEGIN
  INSERT OR IGNORE INTO admin_collection_processing_coverage
    (record_kind, record_id, pipeline, status_origin)
  VALUES ('brief', NEW.id, 'polish', 'never_started');
END;
