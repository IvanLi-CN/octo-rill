//! PostgreSQL reads for the administrator job center.
//!
//! The legacy SQLite runtime in `api.rs` and `jobs.rs` remains the behavioural
//! reference for this surface. The overview reports the six KPI counters, the
//! task list keeps its `task_group` filter, exact total and pagination
//! contract, and `/api/admin/jobs/events` is a real SSE stream. This module
//! implements the same contract directly against the PostgreSQL pool.

use std::{collections::HashMap, convert::Infallible, path::Path, time::Duration};

use async_stream::stream;
use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sqlx::{FromRow, PgPool, Postgres, QueryBuilder};
use tokio::time::sleep;
use tower_sessions::Session;
use tracing::warn;

use crate::{
    jobs::{
        SCHEDULED_TASK_TYPES, STATUS_CANCELED, STATUS_FAILED, STATUS_QUEUED, STATUS_RUNNING,
        TASK_SYNC_STARRED_DELTA, TASK_SYNC_STARRED_RECONCILE, TASK_SYNC_SUBSCRIPTIONS,
        is_scheduled_task_type,
    },
    postgres_server::{PostgresServerState, require_admin},
};

const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 100;
const STREAM_POLL_INTERVAL: Duration = Duration::from_millis(500);
const STREAM_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(8);
/// TEXT-timestamp event sources are sampled every 4th job-event tick (2s).
const STREAM_TEXT_SOURCE_TICK_INTERVAL: u64 = 4;
const STREAM_BATCH_LIMIT: i64 = 200;
const SYNC_SUBSCRIPTION_EVENT_LIMIT: usize = 20;

/// `result_json` is written as SQLite JSON text. Only a value that PostgreSQL
/// can parse as JSONB, and whose `skipped` member is a valid boolean, counts as
/// skipped; anything else degrades to `false` exactly like the legacy
/// `json_extract(...) = 1` predicate.
const SKIPPED_EXPRESSION: &str = r#"CASE
    WHEN pg_input_is_valid(result_json, 'jsonb')
     AND jsonb_typeof(result_json::jsonb -> 'skipped') = 'boolean'
    THEN (result_json::jsonb -> 'skipped')::boolean
    WHEN pg_input_is_valid(result_json, 'jsonb')
     AND pg_input_is_valid(result_json::jsonb ->> 'skipped', 'boolean')
    THEN (result_json::jsonb ->> 'skipped')::boolean
    ELSE FALSE
END"#;

fn database_error(error: sqlx::Error, operation: &'static str) -> Response {
    warn!(%error, operation, "PostgreSQL job center read failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"code": "database_error"})),
    )
        .into_response()
}

fn require_admin_response(status: StatusCode) -> Response {
    (status, Json(json!({"code": "admin_required"}))).into_response()
}

// ---------------------------------------------------------------- overview

pub(super) async fn overview(
    State(state): State<PostgresServerState>,
    session: Session,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return require_admin_response(status);
    }
    let since = Utc::now() - chrono::Duration::hours(24);
    let counts = sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64)>(
        "SELECT
           COUNT(*) FILTER (WHERE status = 'queued'),
           COUNT(*) FILTER (WHERE status = 'running'),
           COUNT(*) FILTER (WHERE status = 'failed' AND finished_at >= $1),
           COUNT(*) FILTER (WHERE status = 'succeeded' AND finished_at >= $1),
           (SELECT COUNT(*) FROM public.daily_brief_hour_slots WHERE enabled = 1),
           (SELECT COUNT(*) FROM public.daily_brief_hour_slots)
         FROM public.job_tasks",
    )
    .bind(since)
    .fetch_one(&state.runtime.pool)
    .await;
    match counts {
        Ok((queued, running, failed_24h, succeeded_24h, enabled_slots, total_slots)) => {
            Json(json!({
                "queued": queued,
                "running": running,
                "failed_24h": failed_24h,
                "succeeded_24h": succeeded_24h,
                "enabled_scheduled_slots": enabled_slots,
                "total_scheduled_slots": total_slots,
            }))
            .into_response()
        }
        Err(error) => database_error(error, "admin jobs overview"),
    }
}

// ------------------------------------------------------------ task listing

#[derive(Debug, Deserialize)]
pub(super) struct RealtimeTasksQuery {
    status: Option<String>,
    task_type: Option<String>,
    exclude_task_type: Option<String>,
    task_group: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

#[derive(Debug)]
struct TaskFilters {
    status: String,
    task_type: String,
    exclude_task_type: String,
    task_group: String,
}

#[derive(Debug, FromRow)]
struct RealtimeTaskRow {
    id: String,
    task_type: String,
    status: String,
    source: String,
    skipped: bool,
    requested_by: Option<String>,
    parent_task_id: Option<String>,
    cancel_requested: bool,
    error_message: Option<String>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct RealtimeTaskItem {
    id: String,
    task_type: String,
    status: String,
    source: String,
    skipped: bool,
    requested_by: Option<String>,
    parent_task_id: Option<String>,
    cancel_requested: bool,
    error_message: Option<String>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<TaskDiagnostics>,
}

impl From<RealtimeTaskRow> for RealtimeTaskItem {
    fn from(row: RealtimeTaskRow) -> Self {
        Self {
            id: row.id,
            task_type: row.task_type,
            status: row.status,
            source: row.source,
            skipped: row.skipped,
            requested_by: row.requested_by,
            parent_task_id: row.parent_task_id,
            cancel_requested: row.cancel_requested,
            error_message: row.error_message,
            created_at: row.created_at,
            started_at: row.started_at,
            finished_at: row.finished_at,
            updated_at: row.updated_at,
            diagnostics: None,
        }
    }
}

pub(super) async fn realtime_tasks(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<RealtimeTasksQuery>,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return require_admin_response(status);
    }
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query
        .page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let Some(offset) = page
        .checked_sub(1)
        .and_then(|value| value.checked_mul(page_size))
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"code": "bad_request", "message": "page is too large"})),
        )
            .into_response();
    };
    let filters = TaskFilters {
        status: query.status.unwrap_or_else(|| "all".to_owned()),
        task_type: query.task_type.unwrap_or_default(),
        exclude_task_type: query.exclude_task_type.unwrap_or_default(),
        task_group: query.task_group.unwrap_or_else(|| "all".to_owned()),
    };
    match load_realtime_tasks(&state.runtime.pool, &filters, page_size, offset).await {
        Ok((items, total)) => Json(json!({
            "items": items,
            "page": page,
            "page_size": page_size,
            "total": total,
        }))
        .into_response(),
        Err(error) => database_error(error, "admin realtime task list"),
    }
}

async fn load_realtime_tasks(
    pool: &PgPool,
    filters: &TaskFilters,
    page_size: i64,
    offset: i64,
) -> Result<(Vec<RealtimeTaskItem>, i64), sqlx::Error> {
    let mut count_query =
        QueryBuilder::<Postgres>::new("SELECT COUNT(*) FROM public.job_tasks WHERE 1 = 1");
    push_task_filters(&mut count_query, filters);
    let total = count_query
        .build_query_scalar::<i64>()
        .fetch_one(pool)
        .await?;

    let mut items_query = QueryBuilder::<Postgres>::new(format!(
        "SELECT id, task_type, status, source, {SKIPPED_EXPRESSION} AS skipped, \
         requested_by, parent_task_id, (cancel_requested <> 0) AS cancel_requested, \
         error_message, created_at, started_at, finished_at, updated_at \
         FROM public.job_tasks WHERE 1 = 1"
    ));
    push_task_filters(&mut items_query, filters);
    items_query.push(" ORDER BY created_at DESC, id DESC LIMIT ");
    items_query.push_bind(page_size);
    items_query.push(" OFFSET ");
    items_query.push_bind(offset);
    let rows = items_query
        .build_query_as::<RealtimeTaskRow>()
        .fetch_all(pool)
        .await?;
    let mut items = rows
        .into_iter()
        .map(RealtimeTaskItem::from)
        .collect::<Vec<_>>();
    attach_subscription_diagnostics(pool, &mut items).await?;
    Ok((items, total))
}

fn push_task_filters(query: &mut QueryBuilder<'_, Postgres>, filters: &TaskFilters) {
    if filters.status != "all" {
        query
            .push(" AND status = ")
            .push_bind(filters.status.clone());
    }
    if !filters.task_type.is_empty() {
        query
            .push(" AND task_type = ")
            .push_bind(filters.task_type.clone());
    }
    if !filters.exclude_task_type.is_empty() {
        query
            .push(" AND task_type <> ")
            .push_bind(filters.exclude_task_type.clone());
    }
    let user_sync_task_types = [TASK_SYNC_STARRED_DELTA, TASK_SYNC_STARRED_RECONCILE];
    match filters.task_group.as_str() {
        "scheduled" => {
            push_in_list(
                query,
                " AND task_type IN ",
                SCHEDULED_TASK_TYPES
                    .iter()
                    .copied()
                    .filter(|task_type| is_scheduled_task_type(task_type)),
            );
            push_in_list(
                query,
                " AND task_type NOT IN ",
                user_sync_task_types.iter().copied(),
            );
        }
        "user_sync" => {
            push_in_list(
                query,
                " AND task_type IN ",
                user_sync_task_types.iter().copied(),
            );
        }
        "realtime" => {
            push_in_list(
                query,
                " AND task_type NOT IN ",
                SCHEDULED_TASK_TYPES.iter().copied(),
            );
        }
        _ => {}
    }
}

fn push_in_list<'a>(
    query: &mut QueryBuilder<'_, Postgres>,
    clause: &'static str,
    values: impl Iterator<Item = &'a str>,
) {
    query.push(clause).push("(");
    let mut separated = query.separated(", ");
    for value in values {
        separated.push_bind(value.to_owned());
    }
    separated.push_unseparated(")");
}

// --------------------------------------------------- subscription diagnostics

#[derive(Debug, FromRow)]
struct SubscriptionTaskRow {
    id: String,
    status: String,
    payload_json: String,
    result_json: Option<String>,
    error_message: Option<String>,
    log_file_path: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct TaskProgressEventRow {
    task_id: String,
    id: String,
    payload_json: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct SubscriptionEventRow {
    id: String,
    task_id: String,
    stage: String,
    event_type: String,
    severity: String,
    recoverable: i64,
    attempt: i64,
    user_id: Option<String>,
    repo_id: Option<i64>,
    repo_full_name: Option<String>,
    payload_json: Option<String>,
    created_at: String,
}

const PROGRESS_STAGE_LITERAL: &str = "'collect', 'repo_collect', 'release_attached', \
     'release_progress', 'release_summary', 'social_progress', 'social_summary', \
     'notifications_progress', 'notifications_summary'";
const SUMMARY_STAGE_LITERAL: &str =
    "'collect', 'release_summary', 'social_summary', 'notifications_summary'";

#[derive(Debug, Serialize)]
struct TaskDiagnostics {
    business_outcome: BusinessOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    sync_subscriptions: Option<SyncSubscriptionsDiagnostics>,
}

#[derive(Debug, Serialize)]
struct BusinessOutcome {
    code: String,
    label: String,
    message: String,
}

#[derive(Debug, Default, Serialize)]
struct CollectDiagnostics {
    total_users: i64,
}

#[derive(Debug, Default, Serialize)]
struct ReleaseDiagnostics {
    total_repos: i64,
    succeeded_repos: i64,
    failed_repos: i64,
    candidate_failures: i64,
    fetched_count: i64,
    inserted_count: i64,
    updated_count: i64,
    unchanged_count: i64,
    pages_fetched: i64,
}

#[derive(Debug, Serialize)]
struct GovernanceDiagnostics {
    window_minutes: i64,
    window_budget: i64,
    cycle_id: Option<String>,
    selection_window_index: Option<i64>,
    selected_repos: i64,
}

#[derive(Debug, Default, Serialize)]
struct SocialDiagnostics {
    total_users: i64,
    succeeded_users: i64,
    failed_users: i64,
    repo_stars: i64,
    followers: i64,
    events: i64,
}

#[derive(Debug, Default, Serialize)]
struct NotificationsDiagnostics {
    total_users: i64,
    succeeded_users: i64,
    failed_users: i64,
    notifications: i64,
}

#[derive(Debug, Clone, Serialize)]
struct SubscriptionEventItem {
    id: String,
    stage: String,
    event_type: String,
    severity: String,
    recoverable: bool,
    attempt: i64,
    user_id: Option<String>,
    repo_id: Option<i64>,
    repo_full_name: Option<String>,
    message: Option<String>,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct SyncSubscriptionsDiagnostics {
    trigger: Option<String>,
    schedule_key: Option<String>,
    skipped: bool,
    skip_reason: Option<String>,
    log_available: bool,
    log_download_path: Option<String>,
    collect: CollectDiagnostics,
    release: ReleaseDiagnostics,
    governance: Option<GovernanceDiagnostics>,
    social: SocialDiagnostics,
    notifications: NotificationsDiagnostics,
    releases_written: i64,
    critical_events: i64,
    recent_events: Vec<SubscriptionEventItem>,
}

async fn attach_subscription_diagnostics(
    pool: &PgPool,
    items: &mut [RealtimeTaskItem],
) -> Result<(), sqlx::Error> {
    let task_ids = items
        .iter()
        .filter(|item| item.task_type == TASK_SYNC_SUBSCRIPTIONS)
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    if task_ids.is_empty() {
        return Ok(());
    }
    let tasks = sqlx::query_as::<_, SubscriptionTaskRow>(
        "SELECT id, status, payload_json, result_json, error_message, log_file_path \
         FROM public.job_tasks WHERE id = ANY($1)",
    )
    .bind(&task_ids)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.id.clone(), row))
    .collect::<HashMap<_, _>>();
    let mut progress_events = load_progress_events(pool, &task_ids).await?;
    let mut subscription_events = load_subscription_events(pool, &task_ids).await?;
    for item in items
        .iter_mut()
        .filter(|item| item.task_type == TASK_SYNC_SUBSCRIPTIONS)
    {
        let Some(task) = tasks.get(item.id.as_str()) else {
            continue;
        };
        let progress = progress_events.remove(item.id.as_str()).unwrap_or_default();
        let events = subscription_events
            .remove(item.id.as_str())
            .unwrap_or_default();
        item.diagnostics = Some(build_subscription_diagnostics(task, &progress, &events));
    }
    Ok(())
}

async fn load_progress_events(
    pool: &PgPool,
    task_ids: &[String],
) -> Result<HashMap<String, Vec<TaskProgressEventRow>>, sqlx::Error> {
    let query = format!(
        "WITH ranked AS (
           SELECT task_id, id, payload_json, created_at,
                  ROW_NUMBER() OVER (PARTITION BY task_id ORDER BY created_at DESC, id DESC) AS rn,
                  CASE WHEN pg_input_is_valid(payload_json, 'jsonb')
                       THEN payload_json::jsonb ->> 'stage' END AS stage
           FROM public.job_task_events
           WHERE task_id = ANY($1)
             AND event_type = 'task.progress'
             AND CASE WHEN pg_input_is_valid(payload_json, 'jsonb')
                      THEN payload_json::jsonb ->> 'stage' IN ({PROGRESS_STAGE_LITERAL})
                      ELSE FALSE END
         ), retained AS (
           SELECT task_id, id, payload_json, created_at FROM ranked WHERE rn <= $2
           UNION
           SELECT task_id, id, payload_json, created_at FROM (
             SELECT DISTINCT ON (task_id, stage) task_id, id, payload_json, created_at, stage
             FROM ranked
             WHERE stage IN ({SUMMARY_STAGE_LITERAL})
             ORDER BY task_id, stage, created_at DESC, id DESC
           ) summaries
         )
         SELECT task_id, id, payload_json, created_at
         FROM retained
         ORDER BY task_id ASC, created_at ASC, id ASC"
    );
    let rows = sqlx::query_as::<_, TaskProgressEventRow>(&query)
        .bind(task_ids)
        .bind(SYNC_SUBSCRIPTION_EVENT_LIMIT as i64)
        .fetch_all(pool)
        .await?;
    let mut grouped = HashMap::<String, Vec<TaskProgressEventRow>>::new();
    for row in rows {
        grouped.entry(row.task_id.clone()).or_default().push(row);
    }
    // The fill logic must be independent of the outer query plan.
    for events in grouped.values_mut() {
        events.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
    }
    Ok(grouped)
}

async fn load_subscription_events(
    pool: &PgPool,
    task_ids: &[String],
) -> Result<HashMap<String, Vec<SubscriptionEventItem>>, sqlx::Error> {
    let rows = sqlx::query_as::<_, SubscriptionEventRow>(
        "SELECT id, task_id, stage, event_type, severity, recoverable, attempt, user_id, \
         repo_id, repo_full_name, payload_json, created_at
         FROM (
           SELECT id, task_id, stage, event_type, severity, recoverable, attempt, user_id,
                  repo_id, repo_full_name, payload_json, created_at,
                  ROW_NUMBER() OVER (PARTITION BY task_id ORDER BY created_at DESC, id DESC) AS rn
           FROM public.sync_subscription_events
           WHERE task_id = ANY($1)
         ) ranked
         WHERE rn <= $2
         ORDER BY task_id ASC, created_at DESC, id DESC",
    )
    .bind(task_ids)
    .bind(SYNC_SUBSCRIPTION_EVENT_LIMIT as i64)
    .fetch_all(pool)
    .await?;
    let mut grouped = HashMap::<String, Vec<SubscriptionEventItem>>::new();
    for row in rows {
        let payload = parse_json(row.payload_json.as_deref());
        let item = SubscriptionEventItem {
            id: row.id,
            stage: row.stage,
            event_type: row.event_type,
            severity: row.severity,
            recoverable: row.recoverable != 0,
            attempt: row.attempt,
            user_id: row.user_id,
            repo_id: row.repo_id,
            repo_full_name: row.repo_full_name,
            message: json_get_string(payload.as_ref().and_then(Value::as_object), "message"),
            created_at: row.created_at,
        };
        grouped.entry(row.task_id).or_default().push(item);
    }
    Ok(grouped)
}

fn build_subscription_diagnostics(
    task: &SubscriptionTaskRow,
    progress: &[TaskProgressEventRow],
    recent_events: &[SubscriptionEventItem],
) -> TaskDiagnostics {
    let payload_value = parse_json(Some(task.payload_json.as_str()));
    let payload_object = payload_value.as_ref().and_then(Value::as_object);
    let result_value = parse_json(task.result_json.as_deref());
    let result_object = result_value.as_ref().and_then(Value::as_object);
    let collect_object = json_get_object(result_object, "collect");
    let legacy_star_object = json_get_object(result_object, "star");
    let release_object = json_get_object(result_object, "release");
    let governance_object = json_get_object(result_object, "governance");
    let social_object = json_get_object(result_object, "social");
    let notifications_object = json_get_object(result_object, "notifications");

    let skipped = json_get_bool(result_object, "skipped").unwrap_or(false);
    let skip_reason = json_get_string(result_object, "skip_reason");
    let trigger = json_get_string(payload_object, "trigger");
    let schedule_key = json_get_string(payload_object, "schedule_key");

    let collect = CollectDiagnostics {
        total_users: json_get_i64(collect_object, "total_users")
            .or_else(|| json_get_i64(legacy_star_object, "total_users"))
            .unwrap_or(0),
    };
    let release = ReleaseDiagnostics {
        total_repos: json_get_i64(release_object, "total_repos").unwrap_or(0),
        succeeded_repos: json_get_i64(release_object, "succeeded_repos").unwrap_or(0),
        failed_repos: json_get_i64(release_object, "failed_repos").unwrap_or(0),
        candidate_failures: json_get_i64(release_object, "candidate_failures").unwrap_or(0),
        fetched_count: json_get_i64(release_object, "fetched_count").unwrap_or(0),
        inserted_count: json_get_i64(release_object, "inserted_count").unwrap_or(0),
        updated_count: json_get_i64(release_object, "updated_count").unwrap_or(0),
        unchanged_count: json_get_i64(release_object, "unchanged_count").unwrap_or(0),
        pages_fetched: json_get_i64(release_object, "pages_fetched").unwrap_or(0),
    };
    let governance = governance_object.map(|object| GovernanceDiagnostics {
        window_minutes: json_get_i64(Some(object), "window_minutes").unwrap_or(0),
        window_budget: json_get_i64(Some(object), "window_budget").unwrap_or(0),
        cycle_id: json_get_string(Some(object), "cycle_id"),
        selection_window_index: json_get_i64(Some(object), "selection_window_index"),
        selected_repos: json_get_i64(Some(object), "selected_repos").unwrap_or(0),
    });
    let social = SocialDiagnostics {
        total_users: json_get_i64(social_object, "total_users").unwrap_or(0),
        succeeded_users: json_get_i64(social_object, "succeeded_users").unwrap_or(0),
        failed_users: json_get_i64(social_object, "failed_users").unwrap_or(0),
        repo_stars: json_get_i64(social_object, "repo_stars").unwrap_or(0),
        followers: json_get_i64(social_object, "followers").unwrap_or(0),
        events: json_get_i64(social_object, "events").unwrap_or(0),
    };
    let notifications = NotificationsDiagnostics {
        total_users: json_get_i64(notifications_object, "total_users").unwrap_or(0),
        succeeded_users: json_get_i64(notifications_object, "succeeded_users").unwrap_or(0),
        failed_users: json_get_i64(notifications_object, "failed_users").unwrap_or(0),
        notifications: json_get_i64(notifications_object, "notifications").unwrap_or(0),
    };
    let releases_written = json_get_i64(result_object, "releases_written").unwrap_or(0);
    let critical_events = json_get_i64(result_object, "critical_events").unwrap_or(0);
    let mut snapshot = SubscriptionSnapshot {
        collect,
        release,
        social,
        notifications,
        releases_written,
    };
    fill_from_progress_events(
        progress,
        ProgressFill {
            all: result_object.is_none(),
            collect: collect_object.is_none(),
            release: release_object.is_none(),
            social: social_object.is_none(),
            notifications: notifications_object.is_none(),
            releases_written: json_get_i64(result_object, "releases_written").is_none(),
        },
        &mut snapshot,
    );
    let SubscriptionSnapshot {
        collect,
        release,
        social,
        notifications,
        releases_written,
    } = snapshot;

    let log_available = task
        .log_file_path
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(Path::new)
        .is_some_and(Path::is_file);
    let log_download_path = log_available.then(|| {
        format!(
            "/api/admin/jobs/realtime/{}/log",
            urlencoding::encode(&task.id)
        )
    });

    let release_pending =
        (release.total_repos - release.succeeded_repos - release.failed_repos).max(0);
    let social_pending = (social.total_users - social.succeeded_users - social.failed_users).max(0);
    let notifications_pending =
        (notifications.total_users - notifications.succeeded_users - notifications.failed_users)
            .max(0);
    let outcome = if task.status == STATUS_FAILED {
        business_outcome(
            "failed",
            "业务失败",
            task.error_message
                .clone()
                .unwrap_or_else(|| "订阅同步任务执行失败。".to_owned()),
        )
    } else if skipped {
        business_outcome(
            "disabled",
            "已跳过",
            match skip_reason.as_deref() {
                Some("previous_run_active") => {
                    "上一轮订阅同步仍在执行，本轮仅记录跳过结果。".to_owned()
                }
                Some(other) => format!("本轮任务已跳过：{other}"),
                None => "本轮任务已跳过。".to_owned(),
            },
        )
    } else if task.status == STATUS_CANCELED {
        business_outcome("partial", "已取消", "任务在执行中被取消，结果可能不完整。")
    } else if task.status == STATUS_QUEUED || task.status == STATUS_RUNNING {
        business_outcome("unknown", "处理中", "任务正在执行中，结果尚未稳定。")
    } else if collect.total_users == 0
        && release.total_repos == 0
        && social.total_users == 0
        && notifications.total_users == 0
    {
        business_outcome("ok", "无需执行", "当前没有可同步的启用用户。")
    } else if release.total_repos > 0 && release.succeeded_repos == 0 && release.failed_repos > 0 {
        business_outcome(
            "failed",
            "业务失败",
            "Release 阶段全部失败，未能写入任何仓库结果。",
        )
    } else if release.failed_repos > 0
        || social.failed_users > 0
        || notifications.failed_users > 0
        || release_pending > 0
        || social_pending > 0
        || notifications_pending > 0
        || critical_events > 0
    {
        business_outcome(
            "partial",
            "部分完成",
            "任务已结束，但仍存在失败、剩余工作项或关键告警，请查看阶段摘要与最近关键事件。",
        )
    } else if release.total_repos == 0 {
        business_outcome("ok", "业务成功", "本轮没有需要抓取 Release 的仓库。")
    } else {
        business_outcome("ok", "业务成功", "订阅同步任务已完成。")
    };

    TaskDiagnostics {
        business_outcome: outcome,
        sync_subscriptions: Some(SyncSubscriptionsDiagnostics {
            trigger,
            schedule_key,
            skipped,
            skip_reason,
            log_available,
            log_download_path,
            collect,
            release,
            governance,
            social,
            notifications,
            releases_written,
            critical_events,
            recent_events: recent_events.to_vec(),
        }),
    }
}

#[derive(Debug)]
struct SubscriptionSnapshot {
    collect: CollectDiagnostics,
    release: ReleaseDiagnostics,
    social: SocialDiagnostics,
    notifications: NotificationsDiagnostics,
    releases_written: i64,
}

#[derive(Debug, Clone, Copy)]
struct ProgressFill {
    all: bool,
    collect: bool,
    release: bool,
    social: bool,
    notifications: bool,
    releases_written: bool,
}

fn fill_from_progress_events(
    progress: &[TaskProgressEventRow],
    fill: ProgressFill,
    snapshot: &mut SubscriptionSnapshot,
) {
    for event in progress {
        if event.payload_json.is_empty() {
            continue;
        }
        let payload_value = parse_json(Some(event.payload_json.as_str()));
        let payload_object = payload_value.as_ref().and_then(Value::as_object);
        let Some(stage) = json_get_string(payload_object, "stage") else {
            continue;
        };
        let collect = &mut snapshot.collect;
        let release = &mut snapshot.release;
        let social = &mut snapshot.social;
        let notifications = &mut snapshot.notifications;
        let releases_written = &mut snapshot.releases_written;
        match stage.as_str() {
            "collect" => {
                if (fill.all || fill.collect)
                    && let Some(total_users) = json_get_i64(payload_object, "total_users")
                {
                    collect.total_users = total_users;
                }
            }
            "repo_collect" => {
                if (fill.all || fill.release)
                    && let Some(total_repos) = json_get_i64(payload_object, "total_repos")
                {
                    release.total_repos = total_repos;
                }
            }
            "release_attached" => {
                if fill.all || fill.release {
                    release.total_repos =
                        json_get_i64(payload_object, "repos").unwrap_or(release.total_repos);
                }
            }
            "release_progress" | "release_summary" => {
                if fill.all || fill.release {
                    release.total_repos =
                        json_get_i64(payload_object, "total_repos").unwrap_or(release.total_repos);
                    release.succeeded_repos = json_get_i64(payload_object, "succeeded_repos")
                        .unwrap_or(release.succeeded_repos);
                    release.failed_repos = json_get_i64(payload_object, "failed_repos")
                        .unwrap_or(release.failed_repos);
                    release.candidate_failures = json_get_i64(payload_object, "candidate_failures")
                        .unwrap_or(release.candidate_failures);
                    release.fetched_count = json_get_i64(payload_object, "fetched_count")
                        .unwrap_or(release.fetched_count);
                    release.inserted_count = json_get_i64(payload_object, "inserted_count")
                        .unwrap_or(release.inserted_count);
                    release.updated_count = json_get_i64(payload_object, "updated_count")
                        .unwrap_or(release.updated_count);
                    release.unchanged_count = json_get_i64(payload_object, "unchanged_count")
                        .unwrap_or(release.unchanged_count);
                    release.pages_fetched = json_get_i64(payload_object, "pages_fetched")
                        .unwrap_or(release.pages_fetched);
                }
                if fill.all || fill.releases_written {
                    *releases_written = json_get_i64(payload_object, "releases_written")
                        .unwrap_or(*releases_written);
                }
            }
            "social_progress" | "social_summary" => {
                if fill.all || fill.social {
                    social.total_users =
                        json_get_i64(payload_object, "total_users").unwrap_or(social.total_users);
                    social.succeeded_users = json_get_i64(payload_object, "succeeded_users")
                        .unwrap_or(social.succeeded_users);
                    social.failed_users =
                        json_get_i64(payload_object, "failed_users").unwrap_or(social.failed_users);
                    social.repo_stars =
                        json_get_i64(payload_object, "repo_stars").unwrap_or(social.repo_stars);
                    social.followers =
                        json_get_i64(payload_object, "followers").unwrap_or(social.followers);
                    social.events = json_get_i64(payload_object, "events").unwrap_or(social.events);
                }
            }
            "notifications_progress" | "notifications_summary" => {
                if fill.all || fill.notifications {
                    notifications.total_users = json_get_i64(payload_object, "total_users")
                        .unwrap_or(notifications.total_users);
                    notifications.succeeded_users = json_get_i64(payload_object, "succeeded_users")
                        .unwrap_or(notifications.succeeded_users);
                    notifications.failed_users = json_get_i64(payload_object, "failed_users")
                        .unwrap_or(notifications.failed_users);
                    notifications.notifications = json_get_i64(payload_object, "notifications")
                        .unwrap_or(notifications.notifications);
                }
            }
            _ => {}
        }
    }
}

fn business_outcome(code: &str, label: &str, message: impl Into<String>) -> BusinessOutcome {
    BusinessOutcome {
        code: code.to_owned(),
        label: label.to_owned(),
        message: message.into(),
    }
}

fn parse_json(raw: Option<&str>) -> Option<Value> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    serde_json::from_str(raw).ok()
}

fn json_value_to_i64(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(number);
    }
    if let Some(number) = value.as_u64() {
        return i64::try_from(number).ok();
    }
    value
        .as_str()
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| raw.parse::<i64>().ok())
}

fn json_value_to_string(value: &Value) -> Option<String> {
    if let Some(raw) = value.as_str() {
        let trimmed = raw.trim();
        return (!trimmed.is_empty()).then(|| trimmed.to_owned());
    }
    (value.is_number() || value.is_boolean()).then(|| value.to_string())
}

fn json_value_to_bool(value: &Value) -> Option<bool> {
    if let Some(boolean) = value.as_bool() {
        return Some(boolean);
    }
    if let Some(number) = value.as_i64() {
        return Some(number != 0);
    }
    value.as_str().map(str::trim).and_then(|raw| match raw {
        "1" | "true" | "TRUE" => Some(true),
        "0" | "false" | "FALSE" => Some(false),
        _ => None,
    })
}

fn json_get_i64(object: Option<&Map<String, Value>>, key: &str) -> Option<i64> {
    object
        .and_then(|object| object.get(key))
        .and_then(json_value_to_i64)
}

fn json_get_string(object: Option<&Map<String, Value>>, key: &str) -> Option<String> {
    object
        .and_then(|object| object.get(key))
        .and_then(json_value_to_string)
}

fn json_get_bool(object: Option<&Map<String, Value>>, key: &str) -> Option<bool> {
    object
        .and_then(|object| object.get(key))
        .and_then(json_value_to_bool)
}

fn json_get_object<'a>(
    object: Option<&'a Map<String, Value>>,
    key: &str,
) -> Option<&'a Map<String, Value>> {
    object
        .and_then(|object| object.get(key))
        .and_then(Value::as_object)
}

// ------------------------------------------------------------- task detail

const DETAIL_EVENT_LIMIT: i64 = 200;

#[derive(Debug, FromRow)]
struct RealtimeTaskDetailRow {
    id: String,
    task_type: String,
    status: String,
    source: String,
    requested_by: Option<String>,
    parent_task_id: Option<String>,
    cancel_requested: i64,
    error_message: Option<String>,
    payload_json: String,
    result_json: Option<String>,
    log_file_path: Option<String>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct RealtimeTaskDetailItem {
    id: String,
    task_type: String,
    status: String,
    source: String,
    requested_by: Option<String>,
    parent_task_id: Option<String>,
    cancel_requested: bool,
    error_message: Option<String>,
    payload_json: String,
    result_json: Option<String>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
struct TaskEventItem {
    id: String,
    event_type: String,
    payload_json: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct TaskEventMeta {
    returned: i64,
    total: i64,
    limit: i64,
    truncated: bool,
}

#[derive(Debug, Serialize)]
struct RealtimeTaskDetailResponse {
    task: RealtimeTaskDetailItem,
    events: Vec<TaskEventItem>,
    event_meta: TaskEventMeta,
    diagnostics: Option<TaskDiagnostics>,
}

pub(super) async fn task_detail(
    State(state): State<PostgresServerState>,
    session: Session,
    AxumPath(task_id): AxumPath<String>,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return require_admin_response(status);
    }
    match load_task_detail(&state.runtime.pool, &task_id).await {
        Ok(Some(body)) => Json(body).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"code": "not_found", "message": "task not found"})),
        )
            .into_response(),
        Err(error) => database_error(error, "admin realtime task detail"),
    }
}

async fn load_task_detail(
    pool: &PgPool,
    task_id: &str,
) -> Result<Option<RealtimeTaskDetailResponse>, sqlx::Error> {
    let Some(row) = sqlx::query_as::<_, RealtimeTaskDetailRow>(
        "SELECT id, task_type, status, source, requested_by, parent_task_id, cancel_requested, \
         error_message, payload_json, result_json, log_file_path, created_at, started_at, \
         finished_at, updated_at \
         FROM public.job_tasks WHERE id = $1",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let event_total = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.job_task_events WHERE task_id = $1",
    )
    .bind(task_id)
    .fetch_one(pool)
    .await?;
    let events = sqlx::query_as::<_, TaskEventItem>(
        "SELECT id, event_type, payload_json, created_at \
         FROM public.job_task_events WHERE task_id = $1 \
         ORDER BY created_at DESC, id DESC LIMIT $2",
    )
    .bind(task_id)
    .bind(DETAIL_EVENT_LIMIT)
    .fetch_all(pool)
    .await?;
    let returned = i64::try_from(events.len()).unwrap_or(DETAIL_EVENT_LIMIT);
    let event_total = event_total.max(returned);
    let event_meta = TaskEventMeta {
        returned,
        total: event_total,
        limit: DETAIL_EVENT_LIMIT,
        truncated: event_total > returned,
    };
    // Only the subscription task type has a legacy list/detail diagnostics
    // builder; the other task types keep the historical `null` payload.
    let diagnostics = if row.task_type == TASK_SYNC_SUBSCRIPTIONS {
        let ids = std::slice::from_ref(&row.id);
        let progress = load_progress_events(pool, ids)
            .await?
            .remove(&row.id)
            .unwrap_or_default();
        let subscription_events = load_subscription_events(pool, ids)
            .await?
            .remove(&row.id)
            .unwrap_or_default();
        let subscription_task = SubscriptionTaskRow {
            id: row.id.clone(),
            status: row.status.clone(),
            payload_json: row.payload_json.clone(),
            result_json: row.result_json.clone(),
            error_message: row.error_message.clone(),
            log_file_path: row.log_file_path.clone(),
        };
        Some(build_subscription_diagnostics(
            &subscription_task,
            &progress,
            &subscription_events,
        ))
    } else {
        None
    };
    Ok(Some(RealtimeTaskDetailResponse {
        task: RealtimeTaskDetailItem {
            id: row.id,
            task_type: row.task_type,
            status: row.status,
            source: row.source,
            requested_by: row.requested_by,
            parent_task_id: row.parent_task_id,
            cancel_requested: row.cancel_requested != 0,
            error_message: row.error_message,
            payload_json: row.payload_json,
            result_json: row.result_json,
            created_at: row.created_at,
            started_at: row.started_at,
            finished_at: row.finished_at,
            updated_at: row.updated_at,
        },
        events,
        event_meta,
        diagnostics,
    }))
}

// -------------------------------------------------------------- SSE stream

#[derive(Debug, Clone)]
struct StreamCursor {
    at: DateTime<Utc>,
    id: String,
}

#[derive(Debug, Clone)]
struct StreamCursors {
    job: StreamCursor,
    llm: StreamCursor,
    translation_request: StreamCursor,
    translation_batch: StreamCursor,
}

#[derive(Debug, FromRow)]
struct JobStreamRow {
    id: String,
    task_id: String,
    task_type: String,
    status: String,
    event_type: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct LlmStreamRow {
    id: String,
    call_id: String,
    status: String,
    source: String,
    requested_by: Option<String>,
    parent_task_id: Option<String>,
    event_type: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct TranslationStreamRow {
    id: String,
    status: String,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct JobStreamPayload {
    event_id: String,
    task_id: String,
    task_type: String,
    status: String,
    event_type: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct LlmStreamPayload {
    event_id: String,
    call_id: String,
    status: String,
    source: String,
    requested_by: Option<String>,
    parent_task_id: Option<String>,
    event_type: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct TranslationStreamPayload {
    event_id: String,
    resource_type: String,
    resource_id: String,
    status: String,
    event_type: String,
    created_at: String,
}

pub(super) async fn events_stream(
    State(state): State<PostgresServerState>,
    session: Session,
    headers: HeaderMap,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return require_admin_response(status);
    }
    let pool = state.runtime.pool.clone();
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let cursors = resolve_stream_cursors(&pool, last_event_id.as_deref()).await;
    let events = stream! {
        let _metrics_guard = crate::metrics::SseGuard::new("/api/admin/jobs/events");
        let StreamCursors {
            mut job,
            mut llm,
            mut translation_request,
            mut translation_batch,
        } = cursors;
        // Complete the SSE handshake immediately so the client connection
        // state settles before the first read interval.
        yield Ok::<Event, Infallible>(Event::default().comment("stream-ready"));
        let mut tick: u64 = 0;
        loop {
            for row in load_job_event_batch(&pool, &job).await {
                job = StreamCursor { at: row.created_at, id: row.id.clone() };
                let payload = JobStreamPayload {
                    event_id: row.id.clone(),
                    task_id: row.task_id,
                    task_type: row.task_type,
                    status: row.status,
                    event_type: row.event_type,
                    created_at: row.created_at.to_rfc3339(),
                };
                let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_owned());
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .id(format!("job-{}", row.id))
                        .event("job.event")
                        .data(data),
                );
            }

            // The TEXT timestamp columns behind these two sources cannot use an
            // index through the guarded timestamp cast the imported schemas
            // need, so each poll is a bounded sequential read. Sample them one
            // tick in `STREAM_TEXT_SOURCE_TICK_INTERVAL` instead of every
            // interval; the admin UI debounces refreshes far above that rate.
            if tick.is_multiple_of(STREAM_TEXT_SOURCE_TICK_INTERVAL) {
                for row in load_llm_event_batch(&pool, &llm).await {
                llm = StreamCursor { at: row.created_at, id: row.id.clone() };
                let payload = LlmStreamPayload {
                    event_id: row.id.clone(),
                    call_id: row.call_id,
                    status: row.status,
                    source: row.source,
                    requested_by: row.requested_by,
                    parent_task_id: row.parent_task_id,
                    event_type: row.event_type,
                    created_at: row.created_at.to_rfc3339(),
                };
                let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_owned());
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .id(format!("llm-{}", row.id))
                        .event("llm.call")
                        .data(data),
                );
            }

            for row in load_translation_batch(&pool, "translation_requests", &translation_request).await {
                translation_request = StreamCursor { at: row.updated_at, id: row.id.clone() };
                let event_id = format!("request:{}:{}", row.updated_at.to_rfc3339(), row.id);
                let payload = TranslationStreamPayload {
                    event_id: event_id.clone(),
                    resource_type: "request".to_owned(),
                    resource_id: row.id.clone(),
                    status: row.status,
                    event_type: "translation.request.updated".to_owned(),
                    created_at: row.updated_at.to_rfc3339(),
                };
                let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_owned());
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .id(format!(
                            "translation-request|{}|{}",
                            row.updated_at.to_rfc3339(),
                            row.id
                        ))
                        .event("translation.event")
                        .data(data),
                );
            }

            for row in load_translation_batch(&pool, "translation_batches", &translation_batch).await {
                translation_batch = StreamCursor { at: row.updated_at, id: row.id.clone() };
                let event_id = format!("batch:{}:{}", row.updated_at.to_rfc3339(), row.id);
                let payload = TranslationStreamPayload {
                    event_id,
                    resource_type: "batch".to_owned(),
                    resource_id: row.id.clone(),
                    status: row.status,
                    event_type: "translation.batch.updated".to_owned(),
                    created_at: row.updated_at.to_rfc3339(),
                };
                let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_owned());
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .id(format!(
                            "translation-batch|{}|{}",
                            row.updated_at.to_rfc3339(),
                            row.id
                        ))
                        .event("translation.event")
                        .data(data),
                );
            }
            }

            tick = tick.wrapping_add(1);
            sleep(STREAM_POLL_INTERVAL).await;
        }
    };

    Sse::new(events)
        .keep_alive(
            KeepAlive::new()
                .interval(STREAM_KEEP_ALIVE_INTERVAL)
                .text("keep-alive"),
        )
        .into_response()
}

async fn resolve_stream_cursors(pool: &PgPool, last_event_id: Option<&str>) -> StreamCursors {
    let mut cursors = StreamCursors {
        job: max_job_cursor(pool).await.unwrap_or_else(now_cursor),
        llm: max_llm_cursor(pool).await.unwrap_or_else(now_cursor),
        translation_request: max_translation_cursor(pool, "translation_requests")
            .await
            .unwrap_or_else(now_cursor),
        translation_batch: max_translation_cursor(pool, "translation_batches")
            .await
            .unwrap_or_else(now_cursor),
    };
    let Some(last_event_id) = last_event_id else {
        return cursors;
    };
    if let Some(event_id) = last_event_id.strip_prefix("job-") {
        if let Some(cursor) = job_cursor_by_id(pool, event_id).await {
            cursors.job = cursor;
        }
    } else if let Some(event_id) = last_event_id.strip_prefix("llm-") {
        if let Some(cursor) = llm_cursor_by_id(pool, event_id).await {
            cursors.llm = cursor;
        }
    } else if let Some(rest) = last_event_id.strip_prefix("translation-request|") {
        if let Some(cursor) = parse_translation_cursor(rest) {
            cursors.translation_request = cursor;
        }
    } else if let Some(rest) = last_event_id.strip_prefix("translation-batch|")
        && let Some(cursor) = parse_translation_cursor(rest)
    {
        cursors.translation_batch = cursor;
    }
    cursors
}

fn now_cursor() -> StreamCursor {
    StreamCursor {
        at: Utc::now(),
        id: String::new(),
    }
}

/// The SSE event id for translation rows carries the row timestamp and is split
/// on `|`; the RFC3339 timestamp itself contains `:` and `+`.
fn parse_translation_cursor(raw: &str) -> Option<StreamCursor> {
    let (at, id) = raw.split_once('|')?;
    if id.is_empty() {
        return None;
    }
    Some(StreamCursor {
        at: DateTime::parse_from_rfc3339(at).ok()?.with_timezone(&Utc),
        id: id.to_owned(),
    })
}

async fn max_job_cursor(pool: &PgPool) -> Option<StreamCursor> {
    sqlx::query_as::<_, (DateTime<Utc>, String)>(
        "SELECT created_at, id FROM public.job_task_events ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .map(|(at, id)| StreamCursor { at, id })
}

async fn max_llm_cursor(pool: &PgPool) -> Option<StreamCursor> {
    sqlx::query_as::<_, (DateTime<Utc>, String)>(
        "SELECT CASE WHEN pg_input_is_valid(created_at::text, 'timestamptz')
                     THEN created_at::timestamptz END AS created_at, id
         FROM public.llm_call_events
         WHERE pg_input_is_valid(created_at::text, 'timestamptz')
         ORDER BY created_at::timestamptz DESC, id DESC
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .map(|(at, id)| StreamCursor { at, id })
}

async fn max_translation_cursor(pool: &PgPool, table: &'static str) -> Option<StreamCursor> {
    let query = format!(
        "SELECT CASE WHEN pg_input_is_valid(updated_at::text, 'timestamptz')
                     THEN updated_at::timestamptz END AS updated_at, id
         FROM public.{table}
         WHERE pg_input_is_valid(updated_at::text, 'timestamptz')
         ORDER BY updated_at::timestamptz DESC, id DESC
         LIMIT 1"
    );
    sqlx::query_as::<_, (DateTime<Utc>, String)>(&query)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|(at, id)| StreamCursor { at, id })
}

async fn job_cursor_by_id(pool: &PgPool, event_id: &str) -> Option<StreamCursor> {
    sqlx::query_as::<_, (DateTime<Utc>, String)>(
        "SELECT created_at, id FROM public.job_task_events WHERE id = $1",
    )
    .bind(event_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .map(|(at, id)| StreamCursor { at, id })
}

async fn llm_cursor_by_id(pool: &PgPool, event_id: &str) -> Option<StreamCursor> {
    sqlx::query_as::<_, (DateTime<Utc>, String)>(
        "SELECT CASE WHEN pg_input_is_valid(created_at::text, 'timestamptz')
                     THEN created_at::timestamptz END AS created_at, id
         FROM public.llm_call_events WHERE id = $1",
    )
    .bind(event_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .map(|(at, id)| StreamCursor { at, id })
}

async fn load_job_event_batch(pool: &PgPool, cursor: &StreamCursor) -> Vec<JobStreamRow> {
    sqlx::query_as::<_, JobStreamRow>(
        "SELECT e.id, e.task_id, t.task_type, t.status, e.event_type, e.created_at
         FROM public.job_task_events e
         JOIN public.job_tasks t ON t.id = e.task_id
         WHERE (e.created_at, e.id) > ($1, $2)
         ORDER BY e.created_at ASC, e.id ASC
         LIMIT $3",
    )
    .bind(cursor.at)
    .bind(cursor.id.as_str())
    .bind(STREAM_BATCH_LIMIT)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

async fn load_llm_event_batch(pool: &PgPool, cursor: &StreamCursor) -> Vec<LlmStreamRow> {
    sqlx::query_as::<_, LlmStreamRow>(
        "SELECT id, call_id, status, source, requested_by, parent_task_id, event_type,
                CASE WHEN pg_input_is_valid(created_at::text, 'timestamptz')
                     THEN created_at::timestamptz END AS created_at
         FROM public.llm_call_events
         WHERE (CASE WHEN pg_input_is_valid(created_at::text, 'timestamptz')
                     THEN created_at::timestamptz END, id) > ($1, $2)
         ORDER BY created_at::timestamptz ASC, id ASC
         LIMIT $3",
    )
    .bind(cursor.at)
    .bind(cursor.id.as_str())
    .bind(STREAM_BATCH_LIMIT)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

async fn load_translation_batch(
    pool: &PgPool,
    table: &'static str,
    cursor: &StreamCursor,
) -> Vec<TranslationStreamRow> {
    let query = format!(
        "SELECT id, status,
                CASE WHEN pg_input_is_valid(updated_at::text, 'timestamptz')
                     THEN updated_at::timestamptz END AS updated_at
         FROM public.{table}
         WHERE (CASE WHEN pg_input_is_valid(updated_at::text, 'timestamptz')
                     THEN updated_at::timestamptz END, id) > ($1, $2)
         ORDER BY updated_at::timestamptz ASC, id ASC
         LIMIT $3"
    );
    sqlx::query_as::<_, TranslationStreamRow>(&query)
        .bind(cursor.at)
        .bind(cursor.id.as_str())
        .bind(STREAM_BATCH_LIMIT)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
}
