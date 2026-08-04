# HTTP API contracts

## `GET /api/admin/dashboard`

### Query params

- `window` (optional): `7d` or `30d`.
  - Omitted: defaults to `7d`
  - Invalid value: responds with validation error

### Response

```json
{
  "generated_at": "2026-04-24T10:47:00Z",
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
    "date": "2026-04-24",
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
    "business_counts": {
      "ok": 35,
      "partial": 4,
      "failed": 6,
      "disabled": 0
    },
    "items": [
      {
        "task_type": "summarize.release.smart.batch",
        "label": "润色",
        "queued": 0,
        "running": 0,
        "succeeded": 18,
        "failed": 0,
        "canceled": 0,
        "total": 18,
        "success_rate": 1.0,
        "business_counts": {
          "ok": 14,
          "partial": 3,
          "failed": 1,
          "disabled": 0
        },
        "business_success_rate": 0.78
      }
    ]
  },
  "task_share": [
    {
      "task_type": "summarize.release.smart.batch",
      "label": "润色",
      "total": 18,
      "share_ratio": 0.32,
      "success_rate": 1.0,
      "business_counts": {
        "ok": 14,
        "partial": 3,
        "failed": 1,
        "disabled": 0
      },
      "business_success_rate": 0.78
    }
  ],
  "trend_points": [
    {
      "date": "2026-04-24",
      "label": "04-24",
      "total_users": 128,
      "active_users": 36,
      "translations_total": 30,
      "translations_failed": 1,
      "translations_partial": 2,
      "translations_business_failed": 3,
      "summaries_total": 18,
      "summaries_failed": 0,
      "summaries_partial": 3,
      "summaries_business_failed": 1,
      "briefs_total": 9,
      "briefs_failed": 1,
      "briefs_partial": 0,
      "briefs_business_failed": 1
    }
  ],
  "llm_health": {
    "calls_24h": 2196,
    "failed_24h": 1390,
    "last_failure_at": "2026-04-24T10:42:31Z",
    "top_failure_reasons": [
      {
        "label": "429 No available accounts for this model tier",
        "count": 1198
      },
      {
        "label": "403 Chat upstream returned 403",
        "count": 159
      }
    ],
    "top_failure_sources": [
      {
        "label": "job.sync.subscriptions.auto_translate",
        "count": 648
      },
      {
        "label": "job.sync.subscriptions.auto_smart",
        "count": 480
      }
    ]
  },
  "window_meta": {
    "selected_window": "30d",
    "available_windows": ["7d", "30d"],
    "window_start": "2026-03-26",
    "window_end": "2026-04-24",
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
- `status_breakdown.business_counts` and every per-task `business_counts` are derived from task payload/result/error, not from `job_tasks.status` alone.
- For `translate.release.batch` and `summarize.release.smart.batch`, the backend must accept both legacy `result_json = { "items": [...] }` rows and summary-enriched rows carrying `total / ready / missing / disabled / error`.
- `business_success_rate` uses business terminal outcomes (`ok / partial / failed / disabled`) as denominator and only counts `ok` as full success.
- `llm_health` summarizes the latest 24h of `llm_calls` and is intended for operator triage rather than billing-grade analytics.
