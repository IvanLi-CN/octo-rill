CREATE INDEX IF NOT EXISTS idx_repo_refresh_governance_cycles_retention
  ON repo_refresh_governance_cycles(status, completed_at ASC, id ASC)
  WHERE status = 'completed';
