ALTER TABLE repo_release_work_items
ADD COLUMN last_fetched_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE repo_release_work_items
ADD COLUMN last_inserted_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE repo_release_work_items
ADD COLUMN last_updated_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE repo_release_work_items
ADD COLUMN last_unchanged_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE repo_release_work_items
ADD COLUMN last_pages_fetched INTEGER NOT NULL DEFAULT 0;

ALTER TABLE repo_release_work_items
ADD COLUMN last_stopped_reason TEXT;

ALTER TABLE repo_release_watchers
ADD COLUMN reused_fresh INTEGER NOT NULL DEFAULT 0;

ALTER TABLE repo_release_sync_state
ADD COLUMN last_page_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE repo_release_sync_state
ADD COLUMN last_fetched_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE repo_release_sync_state
ADD COLUMN last_inserted_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE repo_release_sync_state
ADD COLUMN last_updated_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE repo_release_sync_state
ADD COLUMN last_unchanged_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE repo_release_sync_state
ADD COLUMN last_stopped_reason TEXT;
