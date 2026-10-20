-- Keep realtime projection polling ordered while filtering both ready and expired leases.
CREATE INDEX IF NOT EXISTS idx_search_projection_outbox_poll
  ON search_projection_outbox(available_at, created_at, projection_key, lease_expires_at);
