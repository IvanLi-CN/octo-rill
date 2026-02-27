PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS llm_call_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  call_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  status TEXT NOT NULL,
  source TEXT NOT NULL,
  requested_by INTEGER,
  parent_task_id TEXT,
  payload_json TEXT,
  created_at TEXT NOT NULL,
  CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
  FOREIGN KEY(call_id) REFERENCES llm_calls(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_llm_call_events_call_id_id
  ON llm_call_events(call_id, id);
