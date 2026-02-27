PRAGMA foreign_keys = ON;

ALTER TABLE llm_calls
  ADD COLUMN input_messages_json TEXT;

ALTER TABLE llm_calls
  ADD COLUMN output_messages_json TEXT;

ALTER TABLE llm_calls
  ADD COLUMN input_tokens INTEGER;

ALTER TABLE llm_calls
  ADD COLUMN output_tokens INTEGER;

ALTER TABLE llm_calls
  ADD COLUMN cached_input_tokens INTEGER;

ALTER TABLE llm_calls
  ADD COLUMN total_tokens INTEGER;
