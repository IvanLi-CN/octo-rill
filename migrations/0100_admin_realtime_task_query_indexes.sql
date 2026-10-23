CREATE INDEX IF NOT EXISTS idx_job_tasks_task_type_created_at_id
  ON job_tasks(task_type, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_job_task_events_task_id_event_type
  ON job_task_events(task_id, event_type);

CREATE INDEX IF NOT EXISTS idx_job_task_events_task_id_event_type_stage
  ON job_task_events(
    task_id,
    event_type,
    CASE WHEN json_valid(payload_json)
      THEN json_extract(payload_json, '$.stage')
      ELSE NULL
    END
  );
