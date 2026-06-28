ALTER TABLE admin_runtime_settings
  ADD COLUMN llm_models_json TEXT NOT NULL DEFAULT '[]';
