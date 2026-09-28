CREATE TABLE IF NOT EXISTS public_metrics_hourly_snapshots (
  sampled_hour TEXT PRIMARY KEY,
  observed_at TEXT NOT NULL,
  deduplicated_repositories INTEGER NOT NULL CHECK (deduplicated_repositories >= 0),
  pressure REAL NOT NULL CHECK (pressure >= 0)
);
