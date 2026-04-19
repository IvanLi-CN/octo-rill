# HTTP API contracts

## `GET /api/admin/dashboard`

### Query params

- `window` (optional): `7d` or `30d`.
  - Omitted: defaults to `7d`
  - Invalid value: responds with validation error

### Response

```json
{
  "generated_at": "2026-04-18T03:00:00Z",
  "time_zone": "Asia/Shanghai",
  "summary": {
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
  "today_live": {
    "date": "2026-04-18",
    "total_users": 128,
    "active_users": 36,
    "ongoing_tasks_total": 9,
    "queued_tasks": 4,
    "running_tasks": 5
  },
  "status_breakdown": {
    "queued_total": 6,
    "running_total": 5,
    "succeeded_total": 42,
    "failed_total": 3,
    "canceled_total": 1,
    "total": 57,
    "items": [
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
  "task_share": [
    {
      "task_type": "translate.release.batch",
      "label": "翻译",
      "total": 30,
      "share_ratio": 0.52,
      "success_rate": 0.96
    }
  ],
  "trend_points": [
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
  ],
  "window_meta": {
    "selected_window": "30d",
    "available_windows": ["7d", "30d"],
    "window_start": "2026-03-20",
    "window_end": "2026-04-18",
    "point_count": 30
  }
}
```

### Notes

- Requires admin session.
- Statistics always use the configured system time zone, not the browser time zone.
- `summary` and `today_live` reflect today's real-time snapshot.
- `trend_points` come from rollups, but the point for today is overwritten by live stats.
- `status_breakdown.items`, `task_share`, and `trend_points` are limited to translation, smart summary, and daily brief tasks.
