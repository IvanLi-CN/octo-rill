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

use crate::admin_job_groups;
use crate::jobs;
use crate::postgres_scheduler::{
    AdminDashboardTaskCounts, load_admin_dashboard_task_counts, local_day_bounds_utc,
};
use crate::postgres_server::{PostgresServerState, require_admin};

const ADMIN_DASHBOARD_DEFAULT_WINDOW_DAYS: i64 = 7;
const ADMIN_DASHBOARD_WINDOW_OPTIONS: [(&str, i64); 2] = [("7d", 7), ("30d", 30)];

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
    sync: i64,
    briefs: i64,
    operations: i64,
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
    task_group: String,
    task_group_label: String,
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
    task_group: String,
    task_group_label: String,
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
    sync_total: i64,
    sync_failed: i64,
    sync_partial: i64,
    sync_business_failed: i64,
    briefs_total: i64,
    briefs_failed: i64,
    briefs_partial: i64,
    briefs_business_failed: i64,
    operations_total: i64,
    operations_failed: i64,
    operations_partial: i64,
    operations_business_failed: i64,
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
            sync_total: 0,
            sync_failed: 0,
            sync_partial: 0,
            sync_business_failed: 0,
            briefs_total: 0,
            briefs_failed: 0,
            briefs_partial: 0,
            briefs_business_failed: 0,
            operations_total: 0,
            operations_failed: 0,
            operations_partial: 0,
            operations_business_failed: 0,
        }
    }

    fn apply(&mut self, row: &AdminDashboardRollupRow) {
        let Some(group) = admin_job_groups::group_for_task_type(&row.task_type) else {
            return;
        };
        self.apply_group_counts(
            group.key,
            row.total_tasks(),
            row.failed_count,
            row.business_partial_count,
            row.business_failed_count,
        );
    }

    fn apply_status_item(&mut self, item: &AdminDashboardTaskStatusItem) {
        self.apply_group_counts(
            &item.task_group,
            item.total,
            item.failed,
            item.business_counts.partial,
            item.business_counts.failed,
        );
    }

    fn apply_group_counts(
        &mut self,
        group: &str,
        total: i64,
        failed: i64,
        partial: i64,
        business_failed: i64,
    ) {
        let (total_field, failed_field, partial_field, business_failed_field) = match group {
            "translation" => (
                &mut self.translations_total,
                &mut self.translations_failed,
                &mut self.translations_partial,
                &mut self.translations_business_failed,
            ),
            "sync" => (
                &mut self.sync_total,
                &mut self.sync_failed,
                &mut self.sync_partial,
                &mut self.sync_business_failed,
            ),
            "briefs" => (
                &mut self.briefs_total,
                &mut self.briefs_failed,
                &mut self.briefs_partial,
                &mut self.briefs_business_failed,
            ),
            "operations" => (
                &mut self.operations_total,
                &mut self.operations_failed,
                &mut self.operations_partial,
                &mut self.operations_business_failed,
            ),
            _ => return,
        };
        *total_field += total;
        *failed_field += failed;
        *partial_field += partial;
        *business_failed_field += business_failed;
    }

    /// Realtime status items replace the pre-aggregated rollup for today.
    fn reset_task_groups(&mut self) {
        self.translations_total = 0;
        self.translations_failed = 0;
        self.translations_partial = 0;
        self.translations_business_failed = 0;
        self.sync_total = 0;
        self.sync_failed = 0;
        self.sync_partial = 0;
        self.sync_business_failed = 0;
        self.briefs_total = 0;
        self.briefs_failed = 0;
        self.briefs_partial = 0;
        self.briefs_business_failed = 0;
        self.operations_total = 0;
        self.operations_failed = 0;
        self.operations_partial = 0;
        self.operations_business_failed = 0;
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
    today_point.reset_task_groups();
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
                task_group: item.task_group.clone(),
                task_group_label: item.task_group_label.clone(),
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
    let ongoing_tasks_total = ongoing_by_task.translations
        + ongoing_by_task.sync
        + ongoing_by_task.briefs
        + ongoing_by_task.operations;

    let specs = admin_job_groups::runtime_task_specs().collect::<Vec<_>>();
    let mut items = Vec::with_capacity(specs.len());
    for spec in specs {
        let counts =
            load_admin_dashboard_task_counts(pool, spec.task_type, start_at, now_utc).await?;
        let Some(group) = admin_job_groups::group_for_task_type(spec.task_type) else {
            continue;
        };
        items.push(build_status_item(
            spec.task_type,
            spec.label,
            group,
            &counts,
        ));
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
    group: &admin_job_groups::AdminJobTaskGroup,
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
        task_group: group.key.to_owned(),
        task_group_label: group.label.to_owned(),
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
        sync: 0,
        briefs: 0,
        operations: 0,
    };
    for (task_type, total) in rows {
        let Some(group) = admin_job_groups::group_for_task_type(&task_type) else {
            continue;
        };
        match group.key {
            "translation" => breakdown.translations += total,
            "sync" => breakdown.sync += total,
            "briefs" => breakdown.briefs += total,
            "operations" => breakdown.operations += total,
            _ => {}
        }
    }
    Ok(breakdown)
}

fn admin_dashboard_task_types() -> Vec<&'static str> {
    admin_job_groups::runtime_task_types().collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs;

    fn status_item(
        task_type: &str,
        task_group: &str,
        total: i64,
        failed: i64,
    ) -> AdminDashboardTaskStatusItem {
        AdminDashboardTaskStatusItem {
            task_type: task_type.to_owned(),
            task_group: task_group.to_owned(),
            task_group_label: String::new(),
            label: String::new(),
            queued: 0,
            running: 0,
            succeeded: total - failed,
            failed,
            canceled: 0,
            business_counts: AdminDashboardBusinessCounts::default(),
            total,
            success_rate: 0.0,
            business_success_rate: 0.0,
        }
    }

    #[test]
    fn postgres_admin_dashboard_rollup_types_match_runtime_groups() {
        let task_types = admin_dashboard_task_types();
        assert!(task_types.contains(&admin_job_groups::TASK_TRANSLATION_GENERATE));
        assert!(task_types.contains(&jobs::TASK_BRIEF_DAILY_SLOT));
        assert!(!task_types.contains(&jobs::TASK_SYNC_SUBSCRIPTIONS));
        assert!(!task_types.contains(&jobs::TASK_SUMMARIZE_RELEASE_SMART_BATCH));
        assert_eq!(
            task_types.len(),
            admin_job_groups::runtime_task_specs().count()
        );
    }

    #[test]
    fn postgres_admin_dashboard_trend_sums_task_types_per_group() {
        let mut point = AdminDashboardTrendPoint::empty("2026-11-22");
        point.apply_status_item(&status_item(
            admin_job_groups::TASK_TRANSLATION_GENERATE,
            "translation",
            3,
            1,
        ));
        point.apply_status_item(&status_item(
            jobs::TASK_TRANSLATE_RELEASE_BATCH,
            "translation",
            2,
            0,
        ));
        point.apply_status_item(&status_item(jobs::TASK_SYNC_RELEASES, "sync", 4, 2));
        assert_eq!(point.translations_total, 5);
        assert_eq!(point.translations_failed, 1);
        assert_eq!(point.sync_total, 4);
        assert_eq!(point.sync_failed, 2);
        assert_eq!(point.briefs_total, 0);
        assert_eq!(point.operations_total, 0);

        point.reset_task_groups();
        assert_eq!(point.translations_total, 0);
        assert_eq!(point.sync_total, 0);
    }
}
