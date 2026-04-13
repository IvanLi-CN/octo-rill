ALTER TABLE admin_runtime_settings
  ADD COLUMN ai_model_context_limit INTEGER
  CHECK (ai_model_context_limit IS NULL OR ai_model_context_limit > 0);
