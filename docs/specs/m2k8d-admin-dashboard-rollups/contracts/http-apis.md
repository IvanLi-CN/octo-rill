# HTTP API contracts

## New: `GET /api/admin/dashboard`

### Query params

- `time_zone` (optional): IANA time zone string. When omitted, server falls back to the configured default daily brief time zone.

### Response

```json
{
  "generated_at": "2026-04-18T02:30:00Z",
  "time_zone": "Asia/Shanghai",
  "window_start": "2026-04-12",
  "window_end": "2026-04-18",
  "kpis": {
    "total_users": 128,
    "active_users_today": 36,
    "ongoing_tasks_total": 9,
    "queued_tasks": 4,
    "running_tasks": 5,
    "ongoing_by_task": {
      "translations": 3,
      "summaries": 4,
      "briefs": 2
    }
  },
  "today": {
    "queued_total": 6,
    "running_total": 5,
    "succeeded_total": 42,
    "failed_total": 3,
    "canceled_total": 1,
    "total": 57,
    "task_status": [
      {
        "task_type": "translate.release.batch",
        "label": "翻译",
        "queued": 3,
        "running": 2,
        "succeeded": 24,
        "failed": 1,
        "canceled": 0,
        "total": 30,
        "success_rate": 0.96
      }
    ]
  },
  "trends": [
    {
      "date": "2026-04-18",
      "label": "04-18",
      "total_users": 128,
      "active_users": 36,
      "translations_total": 30,
      "translations_failed": 1,
      "summaries_total": 18,
      "summaries_failed": 1,
      "briefs_total": 9,
      "briefs_failed": 1
    }
  ]
}
```

### Notes

- Requires admin session.
- Response always covers a rolling 7-day window ending at the current local date under the resolved time zone.
- `task_status` and `trends` are limited to translation, smart summary, and daily brief tasks.
