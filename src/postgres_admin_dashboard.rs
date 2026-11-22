//! PostgreSQL admin dashboard read surface.
//!
//! Mirrors the legacy SQLite handler in `api::admin_dashboard`: windowed
//! pre-aggregated rollups for the trend, a live snapshot for the current local
//! day, the task status breakdown with business codes, the task-share ratios and
//! the 24h LLM health summary. Only the persistence layer differs.
//!
//! The rollups themselves are refreshed by
//! [`crate::postgres_scheduler::refresh_admin_dashboard_rollups`]; this module is
//! read-only.

use std::collections::HashMap;
use std::str::FromStr;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tower_sessions::Session;
use tracing::warn;

use crate::jobs;
use crate::postgres_scheduler::{
    AdminDashboardTaskCounts, load_admin_dashboard_task_counts, local_day_bounds_utc,
};
use crate::postgres_server::{PostgresServerState, require_admin};

const ADMIN_DASHBOARD_DEFAULT_WINDOW_DAYS: i64 = 7;
const ADMIN_DASHBOARD_WINDOW_OPTIONS: [(&str, i64); 2] = [("7d", 7), ("30d", 30)];
const ADMIN_DASHBOARD_TASK_TYPES: [(&str, &str); 3] = [
    (jobs::TASK_TRANSLATE_RELEASE_BATCH, "翻译"),
    (jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH, "润色"),
    (jobs::TASK_BRIEF_DAILY_SLOT, "日报"),
];

// ---------------------------------------------------------------------------
// Wire types (legacy response parity)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct AdminDashboardQuery {
    window: Option<String>,
}

#[derive(Debug, Serialize)]
struct AdminDashboardResponse {
    generated_at: String,
    time_zone: String,
    summary: AdminDashboardSummary,
    today_live: AdminDashboardTodayLive,
    status_breakdown: AdminDashboardStatusBreakdown,
    task_share: Vec<AdminDashboardTaskShareItem>,
    trend_points: Vec<AdminDashboardTrendPoint>,
    llm_health: AdminDashboardLlmHealth,
    window_meta: AdminDashboardWindowMeta,
}

#[derive(Debug, Serialize)]
struct AdminDashboardSummary {
    total_users: i64,
    active_users_today: i64,
    ongoing_tasks_total: i64,
    queued_tasks: i64,
    running_tasks: i64,
    ongoing_by_task: AdminDashboardOngoingTaskBreakdown,
}

#[derive(Debug, Serialize)]
struct AdminDashboardOngoingTaskBreakdown {
    translations: i64,
    summaries: i64,
    briefs: i64,
}

#[derive(Debug, Serialize)]
struct AdminDashboardTodayLive {
    date: String,
    total_users: i64,
    active_users: i64,
    ongoing_tasks_total: i64,
    queued_tasks: i64,
    running_tasks: i64,
}

#[derive(Debug, Serialize)]
struct AdminDashboardStatusBreakdown {
    queued_total: i64,
    running_total: i64,
    succeeded_total: i64,
    failed_total: i64,
    canceled_total: i64,
    business_counts: AdminDashboardBusinessCounts,
    total: i64,
    items: Vec<AdminDashboardTaskStatusItem>,
}

#[derive(Debug, Serialize, Clone, Default)]
struct AdminDashboardBusinessCounts {
    ok: i64,
    partial: i64,
    failed: i64,
    disabled: i64,
}

impl AdminDashboardBusinessCounts {
    fn terminal_total(&self) -> i64 {
        self.ok + self.partial + self.failed + self.disabled
    }

    fn success_rate(&self) -> f64 {
        let total = self.terminal_total();
        if total <= 0 {
            0.0
        } else {
            self.ok as f64 / total as f64
        }
    }
}

#[derive(Debug, Serialize, Clone)]
struct AdminDashboardTaskStatusItem {
    task_type: String,
    label: String,
    queued: i64,
    running: i64,
    succeeded: i64,
    failed: i64,
    canceled: i64,
    business_counts: AdminDashboardBusinessCounts,
    total: i64,
    success_rate: f64,
    business_success_rate: f64,
}

#[derive(Debug, Serialize)]
struct AdminDashboardTaskShareItem {
    task_type: String,
    label: String,
    total: i64,
    share_ratio: f64,
    success_rate: f64,
    business_counts: AdminDashboardBusinessCounts,
    business_success_rate: f64,
}

#[derive(Debug, Serialize, Clone)]
struct AdminDashboardTrendPoint {
    date: String,
    label: String,
    total_users: i64,
    active_users: i64,
    translations_total: i64,
    translations_failed: i64,
    translations_partial: i64,
    translations_business_failed: i64,
    summaries_total: i64,
    summaries_failed: i64,
    summaries_partial: i64,
    summaries_business_failed: i64,
    briefs_total: i64,
    briefs_failed: i64,
    briefs_partial: i64,
    briefs_business_failed: i64,
}

impl AdminDashboardTrendPoint {
    fn empty(date: &str) -> Self {
        Self {
            date: date.to_owned(),
            label: date.get(5..).unwrap_or(date).to_owned(),
            total_users: 0,
            active_users: 0,
            translations_total: 0,
            translations_failed: 0,
            translations_partial: 0,
            translations_business_failed: 0,
            summaries_total: 0,
            summaries_failed: 0,
            summaries_partial: 0,
            summaries_business_failed: 0,
            briefs_total: 0,
            briefs_failed: 0,
            briefs_partial: 0,
            briefs_business_failed: 0,
        }
    }

    fn apply(&mut self, row: &AdminDashboardRollupRow) {
        match row.task_type.as_str() {
            jobs::TASK_TRANSLATE_RELEASE_BATCH => {
                self.translations_total = row.total_tasks();
                self.translations_failed = row.failed_count;
                self.translations_partial = row.business_partial_count;
                self.translations_business_failed = row.business_failed_count;
            }
            jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH => {
                self.summaries_total = row.total_tasks();
                self.summaries_failed = row.failed_count;
                self.summaries_partial = row.business_partial_count;
                self.summaries_business_failed = row.business_failed_count;
            }
            jobs::TASK_BRIEF_DAILY_SLOT => {
                self.briefs_total = row.total_tasks();
                self.briefs_failed = row.failed_count;
                self.briefs_partial = row.business_partial_count;
                self.briefs_business_failed = row.business_failed_count;
            }
            _ => {}
        }
    }

    fn apply_status_item(&mut self, item: &AdminDashboardTaskStatusItem) {
        match item.task_type.as_str() {
            jobs::TASK_TRANSLATE_RELEASE_BATCH => {
                self.translations_total = item.total;
                self.translations_failed = item.failed;
                self.translations_partial = item.business_counts.partial;
                self.translations_business_failed = item.business_counts.failed;
            }
            jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH => {
                self.summaries_total = item.total;
                self.summaries_failed = item.failed;
                self.summaries_partial = item.business_counts.partial;
                self.summaries_business_failed = item.business_counts.failed;
            }
            jobs::TASK_BRIEF_DAILY_SLOT => {
                self.briefs_total = item.total;
                self.briefs_failed = item.failed;
                self.briefs_partial = item.business_counts.partial;
                self.briefs_business_failed = item.business_counts.failed;
            }
            _ => {}
        }
    }
}

#[derive(Debug, Serialize)]
struct AdminDashboardWindowMeta {
    selected_window: String,
    available_windows: Vec<String>,
    window_start: String,
    window_end: String,
    point_count: i64,
}

#[derive(Debug, Serialize)]
struct AdminDashboardLlmHealthBucket {
    label: String,
    count: i64,
}

#[derive(Debug, Serialize)]
struct AdminDashboardLlmHealth {
    calls_24h: i64,
    failed_24h: i64,
    last_failure_at: Option<String>,
    top_failure_reasons: Vec<AdminDashboardLlmHealthBucket>,
    top_failure_sources: Vec<AdminDashboardLlmHealthBucket>,
}

#[derive(Debug, sqlx::FromRow)]
struct AdminDashboardRollupRow {
    rollup_date: String,
    task_type: String,
    total_users: i64,
    active_users: i64,
    queued_count: i64,
    running_count: i64,
    succeeded_count: i64,
    failed_count: i64,
    canceled_count: i64,
    business_partial_count: i64,
    business_failed_count: i64,
}

impl AdminDashboardRollupRow {
    fn total_tasks(&self) -> i64 {
        self.queued_count
            + self.running_count
            + self.succeeded_count
            + self.failed_count
            + self.canceled_count
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

pub(crate) async fn admin_dashboard(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<AdminDashboardQuery>,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return (status, Json(serde_json::json!({"code": "admin_required"}))).into_response();
    }
    let window = match resolve_window_days(query.window.as_deref()) {
        Ok(window) => window,
        Err(()) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"code": "bad_request", "message": "invalid window"})),
            )
                .into_response();
        }
    };
    match build_dashboard(&state.runtime.pool, window, dashboard_time_zone(&state)).await {
        Ok(body) => Json(body).into_response(),
        Err(error) => {
            warn!(%error, "PostgreSQL admin dashboard query failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"code": "database_error"})),
            )
                .into_response()
        }
    }
}

fn dashboard_time_zone(state: &PostgresServerState) -> Tz {
    Tz::from_str(state.auth_ext.default_daily_brief_time_zone.as_str()).unwrap_or(chrono_tz::UTC)
}

fn resolve_window_days(raw: Option<&str>) -> Result<(String, i64), ()> {
    let trimmed = raw.unwrap_or_default().trim();
    if trimmed.is_empty() {
        return Ok(("7d".to_owned(), ADMIN_DASHBOARD_DEFAULT_WINDOW_DAYS));
    }
    ADMIN_DASHBOARD_WINDOW_OPTIONS
        .iter()
        .find_map(|(label, days)| (*label == trimmed).then(|| ((*label).to_owned(), *days)))
        .ok_or(())
}

async fn build_dashboard(
    pool: &PgPool,
    (selected_window, window_days): (String, i64),
    time_zone: Tz,
) -> anyhow::Result<AdminDashboardResponse> {
    let now_utc = Utc::now();
    let end_day = now_utc.with_timezone(&time_zone).date_naive();
    let start_day = end_day
        .checked_sub_signed(Duration::days(window_days - 1))
        .unwrap_or(end_day);

    let rollups = load_rollups(pool, time_zone, start_day, end_day).await?;
    let (today_live, status_breakdown) = load_today_live_snapshot(pool, time_zone, now_utc).await?;
    let ongoing_by_task = load_ongoing_counts(pool).await?;
    let llm_health = load_llm_health(pool, now_utc).await?;

    let today_value = today_live.date.clone();
    let mut trend_points_by_date = HashMap::<String, AdminDashboardTrendPoint>::new();
    let mut day = start_day;
    loop {
        let day_value = format_date(day);
        trend_points_by_date.insert(
            day_value.clone(),
            AdminDashboardTrendPoint::empty(&day_value),
        );
        if day >= end_day {
            break;
        }
        day = match day.succ_opt() {
            Some(next) => next,
            None => break,
        };
    }
    for row in &rollups {
        let point = trend_points_by_date
            .entry(row.rollup_date.clone())
            .or_insert_with(|| AdminDashboardTrendPoint::empty(&row.rollup_date));
        point.total_users = row.total_users;
        point.active_users = row.active_users;
        point.apply(row);
    }
    let today_point = trend_points_by_date
        .entry(today_value.clone())
        .or_insert_with(|| AdminDashboardTrendPoint::empty(&today_value));
    today_point.total_users = today_live.total_users;
    today_point.active_users = today_live.active_users;
    for item in &status_breakdown.items {
        today_point.apply_status_item(item);
    }
    let mut trend_points = trend_points_by_date.into_values().collect::<Vec<_>>();
    trend_points.sort_by(|left, right| left.date.cmp(&right.date));

    let task_share = status_breakdown
        .items
        .iter()
        .map(|item| {
            let share_ratio = if status_breakdown.total <= 0 {
                0.0
            } else {
                item.total as f64 / status_breakdown.total as f64
            };
            AdminDashboardTaskShareItem {
                task_type: item.task_type.clone(),
                label: item.label.clone(),
                total: item.total,
                share_ratio,
                success_rate: item.success_rate,
                business_counts: item.business_counts.clone(),
                business_success_rate: item.business_success_rate,
            }
        })
        .collect::<Vec<_>>();

    Ok(AdminDashboardResponse {
        generated_at: now_utc.to_rfc3339(),
        time_zone: time_zone.name().to_owned(),
        summary: AdminDashboardSummary {
            total_users: today_live.total_users,
            active_users_today: today_live.active_users,
            ongoing_tasks_total: today_live.ongoing_tasks_total,
            queued_tasks: today_live.queued_tasks,
            running_tasks: today_live.running_tasks,
            ongoing_by_task,
        },
        today_live,
        status_breakdown,
        task_share,
        trend_points,
        llm_health,
        window_meta: AdminDashboardWindowMeta {
            selected_window,
            available_windows: ADMIN_DASHBOARD_WINDOW_OPTIONS
                .iter()
                .map(|(label, _)| (*label).to_owned())
                .collect(),
            window_start: format_date(start_day),
            window_end: today_value,
            point_count: window_days,
        },
    })
}

fn format_date(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

async fn load_rollups(
    pool: &PgPool,
    time_zone: Tz,
    start_day: NaiveDate,
    end_day: NaiveDate,
) -> anyhow::Result<Vec<AdminDashboardRollupRow>> {
    sqlx::query_as::<_, AdminDashboardRollupRow>(
        r#"
        SELECT
          rollup_date,
          task_type,
          total_users,
          active_users,
          queued_count,
          running_count,
          succeeded_count,
          failed_count,
          canceled_count,
          business_partial_count,
          business_failed_count
        FROM public.admin_dashboard_daily_rollups
        WHERE time_zone = $1
          AND rollup_date >= $2
          AND rollup_date <= $3
        ORDER BY rollup_date ASC, task_type ASC
        "#,
    )
    .bind(time_zone.name())
    .bind(format_date(start_day))
    .bind(format_date(end_day))
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

/// Users timestamps are TEXT in both the imported production schema and the
/// fresh baseline, so every comparison validates the text before casting.
const USERS_LAST_ACTIVE_EXPR: &str = "CASE
        WHEN pg_input_is_valid(last_active_at, 'timestamptz')
        THEN last_active_at::timestamptz
        ELSE NULL
      END";

async fn load_today_live_snapshot(
    pool: &PgPool,
    time_zone: Tz,
    now_utc: DateTime<Utc>,
) -> anyhow::Result<(AdminDashboardTodayLive, AdminDashboardStatusBreakdown)> {
    let today = now_utc.with_timezone(&time_zone).date_naive();
    let start_at = local_day_bounds_utc(time_zone, today)
        .map(|(start, _)| start)
        .unwrap_or(now_utc);

    let total_users = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM public.users")
        .fetch_one(pool)
        .await?;
    let active_users = sqlx::query_scalar::<_, i64>(&format!(
        "SELECT COUNT(*)
         FROM public.users
         WHERE last_active_at IS NOT NULL
           AND {USERS_LAST_ACTIVE_EXPR} BETWEEN $1 AND $2"
    ))
    .bind(start_at)
    .bind(now_utc)
    .fetch_one(pool)
    .await?;
    let queued_tasks = count_live_tasks_by_status(pool, jobs::STATUS_QUEUED).await?;
    let running_tasks = count_live_tasks_by_status(pool, jobs::STATUS_RUNNING).await?;
    let ongoing_by_task = load_ongoing_counts(pool).await?;
    let ongoing_tasks_total =
        ongoing_by_task.translations + ongoing_by_task.summaries + ongoing_by_task.briefs;

    let mut items = Vec::with_capacity(ADMIN_DASHBOARD_TASK_TYPES.len());
    for (task_type, label) in ADMIN_DASHBOARD_TASK_TYPES {
        let counts = load_admin_dashboard_task_counts(pool, task_type, start_at, now_utc).await?;
        items.push(build_status_item(task_type, label, &counts));
    }

    let queued_total = items.iter().map(|item| item.queued).sum::<i64>();
    let running_total = items.iter().map(|item| item.running).sum::<i64>();
    let succeeded_total = items.iter().map(|item| item.succeeded).sum::<i64>();
    let failed_total = items.iter().map(|item| item.failed).sum::<i64>();
    let canceled_total = items.iter().map(|item| item.canceled).sum::<i64>();
    let business_counts =
        items
            .iter()
            .fold(AdminDashboardBusinessCounts::default(), |mut acc, item| {
                acc.ok += item.business_counts.ok;
                acc.partial += item.business_counts.partial;
                acc.failed += item.business_counts.failed;
                acc.disabled += item.business_counts.disabled;
                acc
            });
    let total = items.iter().map(|item| item.total).sum::<i64>();

    Ok((
        AdminDashboardTodayLive {
            date: format_date(today),
            total_users,
            active_users,
            ongoing_tasks_total,
            queued_tasks,
            running_tasks,
        },
        AdminDashboardStatusBreakdown {
            queued_total,
            running_total,
            succeeded_total,
            failed_total,
            canceled_total,
            business_counts,
            total,
            items,
        },
    ))
}

fn build_status_item(
    task_type: &str,
    label: &str,
    counts: &AdminDashboardTaskCounts,
) -> AdminDashboardTaskStatusItem {
    let total = counts.queued + counts.running + counts.succeeded + counts.failed + counts.canceled;
    let finished_total = counts.succeeded + counts.failed + counts.canceled;
    let success_rate = if finished_total <= 0 {
        0.0
    } else {
        counts.succeeded as f64 / finished_total as f64
    };
    let business_counts = AdminDashboardBusinessCounts {
        ok: counts.business_ok,
        partial: counts.business_partial,
        failed: counts.business_failed,
        disabled: counts.business_disabled,
    };
    AdminDashboardTaskStatusItem {
        task_type: task_type.to_owned(),
        label: label.to_owned(),
        queued: counts.queued,
        running: counts.running,
        succeeded: counts.succeeded,
        failed: counts.failed,
        canceled: counts.canceled,
        business_success_rate: business_counts.success_rate(),
        business_counts,
        total,
        success_rate,
    }
}

async fn count_live_tasks_by_status(pool: &PgPool, status: &str) -> anyhow::Result<i64> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)
         FROM public.job_tasks
         WHERE status = $1
           AND task_type = ANY($2)",
    )
    .bind(status)
    .bind(admin_dashboard_task_types())
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

async fn load_ongoing_counts(pool: &PgPool) -> anyhow::Result<AdminDashboardOngoingTaskBreakdown> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT task_type, COUNT(*) AS total
         FROM public.job_tasks
         WHERE status IN ('queued', 'running')
           AND task_type = ANY($1)
         GROUP BY task_type",
    )
    .bind(admin_dashboard_task_types())
    .fetch_all(pool)
    .await?;
    let mut breakdown = AdminDashboardOngoingTaskBreakdown {
        translations: 0,
        summaries: 0,
        briefs: 0,
    };
    for (task_type, total) in rows {
        match task_type.as_str() {
            jobs::TASK_TRANSLATE_RELEASE_BATCH => breakdown.translations = total,
            jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH => breakdown.summaries = total,
            jobs::TASK_BRIEF_DAILY_SLOT => breakdown.briefs = total,
            _ => {}
        }
    }
    Ok(breakdown)
}

fn admin_dashboard_task_types() -> Vec<&'static str> {
    ADMIN_DASHBOARD_TASK_TYPES
        .iter()
        .map(|(task_type, _)| *task_type)
        .collect()
}

async fn load_llm_health(
    pool: &PgPool,
    now_utc: DateTime<Utc>,
) -> anyhow::Result<AdminDashboardLlmHealth> {
    let since = now_utc - Duration::hours(24);
    let window_expr = "CASE
        WHEN pg_input_is_valid(COALESCE(finished_at, updated_at, created_at), 'timestamptz')
        THEN COALESCE(finished_at, updated_at, created_at)::timestamptz
        ELSE NULL
      END";
    let calls_24h = sqlx::query_scalar::<_, i64>(&format!(
        "SELECT COUNT(*) FROM public.llm_calls WHERE {window_expr} BETWEEN $1 AND $2"
    ))
    .bind(since)
    .bind(now_utc)
    .fetch_one(pool)
    .await?;
    let failed_24h = sqlx::query_scalar::<_, i64>(&format!(
        "SELECT COUNT(*)
         FROM public.llm_calls
         WHERE {window_expr} BETWEEN $1 AND $2
           AND status = 'failed'"
    ))
    .bind(since)
    .bind(now_utc)
    .fetch_one(pool)
    .await?;
    let last_failure_at = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(&format!(
        "SELECT MAX({window_expr})
         FROM public.llm_calls
         WHERE {window_expr} BETWEEN $1 AND $2
           AND status = 'failed'"
    ))
    .bind(since)
    .bind(now_utc)
    .fetch_one(pool)
    .await?;
    let top_failure_reasons = sqlx::query_as::<_, (String, i64)>(&format!(
        "SELECT
           COALESCE(NULLIF(TRIM(error_text), ''), '未记录错误文本') AS label,
           COUNT(*) AS count
         FROM public.llm_calls
         WHERE {window_expr} BETWEEN $1 AND $2
           AND status = 'failed'
         GROUP BY label
         ORDER BY count DESC, label ASC
         LIMIT 5"
    ))
    .bind(since)
    .bind(now_utc)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(label, count)| AdminDashboardLlmHealthBucket { label, count })
    .collect::<Vec<_>>();
    let top_failure_sources = sqlx::query_as::<_, (String, i64)>(&format!(
        "SELECT source AS label, COUNT(*) AS count
         FROM public.llm_calls
         WHERE {window_expr} BETWEEN $1 AND $2
           AND status = 'failed'
         GROUP BY source
         ORDER BY count DESC, label ASC
         LIMIT 5"
    ))
    .bind(since)
    .bind(now_utc)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(label, count)| AdminDashboardLlmHealthBucket { label, count })
    .collect::<Vec<_>>();

    Ok(AdminDashboardLlmHealth {
        calls_24h,
        failed_24h,
        last_failure_at: last_failure_at.map(|value| value.to_rfc3339()),
        top_failure_reasons,
        top_failure_sources,
    })
}
