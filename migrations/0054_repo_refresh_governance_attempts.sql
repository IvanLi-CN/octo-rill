ALTER TABLE repo_refresh_governance_snapshots
  ADD COLUMN system_last_attempt_at TEXT;

ALTER TABLE repo_refresh_governance_snapshots
  ADD COLUMN system_last_attempt_status TEXT;

ALTER TABLE repo_refresh_governance_snapshots
  ADD COLUMN system_last_attempt_error TEXT;

ALTER TABLE repo_refresh_governance_cycle_members
  ADD COLUMN attempt_status TEXT;

ALTER TABLE repo_refresh_governance_cycle_members
  ADD COLUMN attempt_error TEXT;
