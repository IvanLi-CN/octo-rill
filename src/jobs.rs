use std::{
    collections::HashMap,
    convert::Infallible,
    future::Future,
    path::PathBuf,
    pin::Pin,
    str::FromStr,
    sync::{Arc, OnceLock},
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use async_stream::stream;
use axum::response::{
    IntoResponse, Response,
    sse::{Event, KeepAlive, Sse},
};
use chrono::{DateTime, NaiveDate, Timelike, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::{ai, api, briefs, local_id, runtime, state::AppState, sync, translations};

pub const STATUS_QUEUED: &str = "queued";
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_SUCCEEDED: &str = "succeeded";
pub const STATUS_FAILED: &str = "failed";
pub const STATUS_CANCELED: &str = "canceled";

pub const TASK_SYNC_STARRED: &str = "sync.starred";
pub const TASK_SYNC_RELEASES: &str = "sync.releases";
pub const TASK_SYNC_NOTIFICATIONS: &str = "sync.notifications";
pub const TASK_SYNC_ALL: &str = "sync.all";
pub const TASK_SYNC_ACCESS_REFRESH: &str = "sync.access_refresh";
pub const TASK_SYNC_SUBSCRIPTIONS: &str = "sync.subscriptions";
pub const TASK_BRIEF_GENERATE: &str = "brief.generate";
pub const TASK_BRIEF_DAILY_SLOT: &str = "brief.daily_slot";
pub const TASK_BRIEF_HISTORY_RECOMPUTE: &str = "brief.history_recompute";
pub const TASK_BRIEF_REFRESH_CONTENT: &str = "brief.refresh_content";
pub const TASK_TRANSLATE_RELEASE: &str = "translate.release";
pub const TASK_TRANSLATE_RELEASE_BATCH: &str = "translate.release.batch";
pub const TASK_SUMMARIZE_RELEASE_SMART_BATCH: &str = "summarize.release.smart.batch";
pub const TASK_TRANSLATE_RELEASE_DETAIL: &str = "translate.release_detail";
pub const TASK_TRANSLATE_NOTIFICATION: &str = "translate.notification";

pub const SCHEDULED_TASK_TYPES: &[&str] = &[TASK_BRIEF_DAILY_SLOT, TASK_SYNC_SUBSCRIPTIONS];

#[derive(Debug, Clone)]
pub struct NewTask {
    pub task_type: String,
    pub payload: Value,
    pub source: String,
    pub requested_by: Option<String>,
    pub parent_task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnqueuedTask {
    pub task_id: String,
    pub task_type: String,
    pub status: String,
}

#[derive(Debug, sqlx::FromRow)]
struct TaskRow {
    id: String,
    task_type: String,
    source: String,
    requested_by: Option<String>,
    payload_json: String,
    cancel_requested: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct SlotRow {
    enabled: i64,
    last_dispatch_at: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct DispatchStateRow {
    last_dispatch_key: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct DailySlotUserRow {
    id: String,
    daily_brief_local_time: Option<String>,
    daily_brief_time_zone: Option<String>,
    daily_brief_utc_time: String,
    last_active_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DailySlotUserSnapshot {
    user_id: String,
    last_active_at: Option<String>,
    key_date: String,
    local_boundary: String,
    #[serde(default)]
    effective_local_boundary: Option<String>,
    time_zone: String,
    window_start_utc: String,
    window_end_utc: String,
}

#[derive(Debug)]
struct DueDailySlotUser {
    user_id: String,
    last_active_at: Option<String>,
    preferences: briefs::DailyBriefPreferences,
    window: briefs::DailyWindow,
}

const SUBSCRIPTION_SCHEDULE_NAME: &str = "sync.subscriptions";
static TASK_CLAIM_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
static TASK_SINGLETON_ENQUEUE_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

pub fn is_scheduled_task_type(task_type: &str) -> bool {
    SCHEDULED_TASK_TYPES.contains(&task_type)
}

fn task_claim_lock() -> &'static tokio::sync::Mutex<()> {
    TASK_CLAIM_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn task_singleton_enqueue_lock() -> &'static tokio::sync::Mutex<()> {
    TASK_SINGLETON_ENQUEUE_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub fn spawn_task_workers(state: Arc<AppState>, count: usize) {
    for _ in 0..count.max(1) {
        spawn_task_worker(state.clone());
    }
}

pub fn spawn_task_recovery_worker(state: Arc<AppState>) -> tokio::task::AbortHandle {
    tokio::spawn(async move {
        loop {
            if let Err(err) = recover_runtime_state(state.as_ref()).await {
                tracing::warn!(?err, "task worker: recover stale runtime tasks failed");
            }
            tokio::time::sleep(runtime::RUNTIME_LEASE_HEARTBEAT_INTERVAL).await;
        }
    })
    .abort_handle()
}

pub async fn recover_runtime_state_on_startup(state: &AppState) -> Result<()> {
    recover_runtime_state_with_mode(state, runtime::RuntimeRecoveryMode::Startup).await
}

pub fn spawn_task_worker(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            match claim_next_queued_task(state.as_ref()).await {
                Ok(Some(task)) => {
                    if let Err(err) = process_task(Arc::clone(&state), task).await {
                        tracing::warn!(?err, "task worker: process task failed");
                    }
                }
                Ok(None) => {
                    tokio::time::sleep(Duration::from_millis(450)).await;
                }
                Err(err) => {
                    tracing::warn!(?err, "task worker: claim task failed");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    });
}

pub fn spawn_hourly_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let now = Utc::now();
            if let Err(err) = enqueue_hour_slot_if_due(state.as_ref(), now).await {
                tracing::warn!(?err, "hourly scheduler: enqueue due slot failed");
            }
            tokio::time::sleep(Duration::from_secs(45)).await;
        }
    });
}

pub fn spawn_subscription_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let now = Utc::now();
            if let Err(err) = enqueue_subscription_run_if_due(state.as_ref(), now).await {
                tracing::warn!(?err, "subscription scheduler: enqueue due run failed");
            }
            tokio::time::sleep(Duration::from_secs(20)).await;
        }
    });
}

pub async fn enqueue_hour_slot_if_due(
    state: &AppState,
    now: DateTime<Utc>,
) -> Result<Option<String>> {
    let hour_utc = i64::from(now.hour());
    let hour_key = now.format("%Y-%m-%dT%H").to_string();
    let slot = sqlx::query_as::<_, SlotRow>(
        r#"
        SELECT enabled, last_dispatch_at
        FROM daily_brief_hour_slots
        WHERE hour_utc = ?
        LIMIT 1
        "#,
    )
    .bind(hour_utc)
    .fetch_optional(&state.pool)
    .await
    .context("failed to query daily brief hour slot")?;

    let Some(slot) = slot else {
        return Ok(None);
    };

    if slot.enabled == 0 {
        return Ok(None);
    }

    let already_dispatched = slot
        .last_dispatch_at
        .as_deref()
        .is_some_and(|value| value.starts_with(&hour_key));
    if already_dispatched {
        return Ok(None);
    }

    let due_users = collect_due_daily_slot_user_snapshots(state, now, &hour_key).await?;

    let task = enqueue_task(
        state,
        NewTask {
            task_type: TASK_BRIEF_DAILY_SLOT.to_owned(),
            payload: json!({
                "hour_utc": hour_utc,
                "hour_key": hour_key,
                "users": due_users,
            }),
            source: "scheduler".to_owned(),
            requested_by: None,
            parent_task_id: None,
        },
    )
    .await?;

    let dispatch_at = now.to_rfc3339();
    sqlx::query(
        r#"
        UPDATE daily_brief_hour_slots
        SET last_dispatch_at = ?, updated_at = ?
        WHERE hour_utc = ?
        "#,
    )
    .bind(dispatch_at.as_str())
    .bind(dispatch_at.as_str())
    .bind(hour_utc)
    .execute(&state.pool)
    .await
    .context("failed to update slot last_dispatch_at")?;

    Ok(Some(task.task_id))
}

pub async fn enqueue_subscription_run_if_due(
    state: &AppState,
    now: DateTime<Utc>,
) -> Result<Option<String>> {
    let schedule_key = current_subscription_schedule_key(now);
    let row = sqlx::query_as::<_, DispatchStateRow>(
        r#"
        SELECT last_dispatch_key
        FROM scheduled_task_dispatch_state
        WHERE schedule_name = ?
        LIMIT 1
        "#,
    )
    .bind(SUBSCRIPTION_SCHEDULE_NAME)
    .fetch_optional(&state.pool)
    .await
    .context("failed to query subscription dispatch state")?;

    if row
        .as_ref()
        .and_then(|current| current.last_dispatch_key.as_deref())
        == Some(schedule_key.as_str())
    {
        return Ok(None);
    }

    let payload = json!({
        "trigger": "schedule",
        "schedule_key": schedule_key,
    });

    let task = if subscription_run_in_flight(state).await? {
        record_skipped_subscription_run(state, payload).await?
    } else {
        enqueue_task(
            state,
            NewTask {
                task_type: TASK_SYNC_SUBSCRIPTIONS.to_owned(),
                payload,
                source: "scheduler".to_owned(),
                requested_by: None,
                parent_task_id: None,
            },
        )
        .await?
    };

    upsert_dispatch_state(state, &schedule_key, &task.task_id).await?;
    Ok(Some(task.task_id))
}

pub async fn enqueue_brief_history_recompute_if_needed(state: &AppState) -> Result<Option<String>> {
    if ai::legacy_brief_count(state).await? == 0 {
        return Ok(None);
    }

    if let Some(existing) = find_inflight_task_by_type(state, TASK_BRIEF_HISTORY_RECOMPUTE).await? {
        return Ok(Some(existing.task_id));
    }

    let task = enqueue_task(
        state,
        NewTask {
            task_type: TASK_BRIEF_HISTORY_RECOMPUTE.to_owned(),
            payload: json!({}),
            source: "migration.bootstrap".to_owned(),
            requested_by: None,
            parent_task_id: None,
        },
    )
    .await?;

    Ok(Some(task.task_id))
}

pub async fn enqueue_brief_refresh_content_if_needed(state: &AppState) -> Result<Option<String>> {
    if ai::brief_content_refresh_candidate_count(state).await? == 0 {
        return Ok(None);
    }

    if let Some(existing) = find_inflight_task_by_type(state, TASK_BRIEF_REFRESH_CONTENT).await? {
        return Ok(Some(existing.task_id));
    }

    let task = enqueue_task(
        state,
        NewTask {
            task_type: TASK_BRIEF_REFRESH_CONTENT.to_owned(),
            payload: json!({}),
            source: "migration.bootstrap".to_owned(),
            requested_by: None,
            parent_task_id: None,
        },
    )
    .await?;

    Ok(Some(task.task_id))
}

pub(crate) fn current_subscription_schedule_key(now: DateTime<Utc>) -> String {
    let minute = if now.minute() < 30 { 0 } else { 30 };
    format!("{}:{minute:02}", now.format("%Y-%m-%dT%H"))
}

async fn subscription_run_in_flight(state: &AppState) -> Result<bool> {
    let count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM job_tasks
        WHERE task_type = ?
          AND status IN (?, ?)
        "#,
    )
    .bind(TASK_SYNC_SUBSCRIPTIONS)
    .bind(STATUS_QUEUED)
    .bind(STATUS_RUNNING)
    .fetch_one(&state.pool)
    .await
    .context("failed to query in-flight subscription runs")?;

    Ok(count > 0)
}

async fn upsert_dispatch_state(state: &AppState, schedule_key: &str, task_id: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO scheduled_task_dispatch_state (
          schedule_name,
          last_dispatch_key,
          last_task_id,
          updated_at
        ) VALUES (?, ?, ?, ?)
        ON CONFLICT(schedule_name) DO UPDATE SET
          last_dispatch_key = excluded.last_dispatch_key,
          last_task_id = excluded.last_task_id,
          updated_at = excluded.updated_at
        "#,
    )
    .bind(SUBSCRIPTION_SCHEDULE_NAME)
    .bind(schedule_key)
    .bind(task_id)
    .bind(now.as_str())
    .execute(&state.pool)
    .await
    .context("failed to upsert subscription dispatch state")?;
    Ok(())
}

async fn record_skipped_subscription_run(state: &AppState, payload: Value) -> Result<EnqueuedTask> {
    let schedule_key = payload
        .get("schedule_key")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let task = start_inline_task(
        state,
        NewTask {
            task_type: TASK_SYNC_SUBSCRIPTIONS.to_owned(),
            payload,
            source: "scheduler".to_owned(),
            requested_by: None,
            parent_task_id: None,
        },
    )
    .await?;

    append_task_event(
        state,
        &task.task_id,
        "task.progress",
        json!({
            "task_id": task.task_id,
            "stage": "skipped",
            "schedule_key": schedule_key,
            "skip_reason": "previous_run_active",
        }),
    )
    .await?;
    append_task_log_entry(
        state,
        &task.task_id,
        json!({
            "at": Utc::now().to_rfc3339(),
            "level": "warning",
            "stage": "scheduler",
            "event_type": "run_skipped",
            "task_id": task.task_id,
            "schedule_key": schedule_key,
            "message": "subscription sync run skipped because a previous run is still active",
            "skip_reason": "previous_run_active"
        }),
    )
    .await?;

    complete_task(
        state,
        &task.task_id,
        STATUS_SUCCEEDED,
        Some(sync::skipped_subscription_result(
            &schedule_key,
            "previous_run_active",
        )),
        None,
    )
    .await?;
    append_task_event(
        state,
        &task.task_id,
        "task.completed",
        json!({
            "task_id": task.task_id,
            "status": STATUS_SUCCEEDED,
            "skipped": true,
        }),
    )
    .await?;

    Ok(task)
}

fn task_supports_log_file(task_type: &str) -> bool {
    task_type == TASK_SYNC_SUBSCRIPTIONS
}

fn task_log_relative_path(task_type: &str, task_id: &str, now: DateTime<Utc>) -> PathBuf {
    PathBuf::from(task_type.replace('.', "-"))
        .join(now.format("%Y-%m-%d").to_string())
        .join(format!("{task_id}.ndjson"))
}

fn build_task_log_path(state: &AppState, task_type: &str, task_id: &str) -> Result<Option<String>> {
    if !task_supports_log_file(task_type) {
        return Ok(None);
    }

    let relative_path = task_log_relative_path(task_type, task_id, Utc::now());
    let full_path = state.config.task_log_dir.join(relative_path);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).context("failed to create task log parent directory")?;
    }
    std::fs::File::create(&full_path).context("failed to create task log file")?;
    Ok(Some(full_path.to_string_lossy().into_owned()))
}

pub async fn load_task_log_path(state: &AppState, task_id: &str) -> Result<Option<String>> {
    let log_file_path = sqlx::query_scalar::<_, Option<String>>(
        r#"
        SELECT log_file_path
        FROM job_tasks
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(task_id)
    .fetch_optional(&state.pool)
    .await
    .context("failed to query task log path")?;
    Ok(log_file_path.flatten())
}

pub async fn append_task_log_entry(state: &AppState, task_id: &str, entry: Value) -> Result<()> {
    let Some(log_file_path) = load_task_log_path(state, task_id).await? else {
        return Ok(());
    };
    let line = serde_json::to_vec(&entry).context("serialize task log entry")?;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)
        .await
        .with_context(|| format!("failed to open task log file {log_file_path}"))?;
    file.write_all(&line)
        .await
        .with_context(|| format!("failed to write task log file {log_file_path}"))?;
    file.write_all(b"\n")
        .await
        .with_context(|| format!("failed to write task log newline {log_file_path}"))?;
    file.flush()
        .await
        .with_context(|| format!("failed to flush task log file {log_file_path}"))?;
    Ok(())
}

async fn insert_task_record(
    state: &AppState,
    new_task: &NewTask,
    status: &str,
    started_at: Option<&str>,
    runtime_owner_id: Option<&str>,
    lease_heartbeat_at: Option<&str>,
) -> Result<String> {
    let task_id = crate::local_id::generate_local_id();
    let now = Utc::now().to_rfc3339();
    let payload_json = serde_json::to_string(&new_task.payload).context("serialize payload")?;
    let log_file_path = build_task_log_path(state, &new_task.task_type, &task_id)?;

    sqlx::query(
        r#"
        INSERT INTO job_tasks (
          id,
          task_type,
          status,
          source,
          requested_by,
          parent_task_id,
          payload_json,
          log_file_path,
          created_at,
          started_at,
          runtime_owner_id,
          lease_heartbeat_at,
          updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&task_id)
    .bind(&new_task.task_type)
    .bind(status)
    .bind(&new_task.source)
    .bind(new_task.requested_by.as_deref())
    .bind(new_task.parent_task_id.as_deref())
    .bind(payload_json)
    .bind(log_file_path.as_deref())
    .bind(now.as_str())
    .bind(started_at)
    .bind(runtime_owner_id)
    .bind(lease_heartbeat_at)
    .bind(now.as_str())
    .execute(&state.pool)
    .await
    .context("failed to insert job task")?;

    Ok(task_id)
}

pub async fn enqueue_task(state: &AppState, new_task: NewTask) -> Result<EnqueuedTask> {
    let task_id = insert_task_record(state, &new_task, STATUS_QUEUED, None, None, None).await?;

    append_task_event(
        state,
        &task_id,
        "task.created",
        json!({
            "task_id": task_id,
            "task_type": new_task.task_type,
            "status": STATUS_QUEUED,
            "source": new_task.source,
        }),
    )
    .await?;

    Ok(EnqueuedTask {
        task_id,
        task_type: new_task.task_type,
        status: STATUS_QUEUED.to_owned(),
    })
}

pub async fn find_inflight_task_for_requester(
    state: &AppState,
    task_type: &str,
    requested_by: &str,
) -> Result<Option<EnqueuedTask>> {
    #[derive(Debug, sqlx::FromRow)]
    struct InflightTaskRow {
        id: String,
        task_type: String,
        status: String,
    }

    let row = sqlx::query_as::<_, InflightTaskRow>(
        r#"
        SELECT id, task_type, status
        FROM job_tasks
        WHERE task_type = ?
          AND requested_by = ?
          AND status IN (?, ?)
        ORDER BY created_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(task_type)
    .bind(requested_by)
    .bind(STATUS_QUEUED)
    .bind(STATUS_RUNNING)
    .fetch_optional(&state.pool)
    .await
    .context("failed to find inflight task for requester")?;

    Ok(row.map(|row| EnqueuedTask {
        task_id: row.id,
        task_type: row.task_type,
        status: row.status,
    }))
}

pub async fn find_inflight_task_by_type(
    state: &AppState,
    task_type: &str,
) -> Result<Option<EnqueuedTask>> {
    #[derive(Debug, sqlx::FromRow)]
    struct InflightTaskRow {
        id: String,
        task_type: String,
        status: String,
    }

    let row = sqlx::query_as::<_, InflightTaskRow>(
        r#"
        SELECT id, task_type, status
        FROM job_tasks
        WHERE task_type = ?
          AND status IN (?, ?)
        ORDER BY created_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(task_type)
    .bind(STATUS_QUEUED)
    .bind(STATUS_RUNNING)
    .fetch_optional(&state.pool)
    .await
    .context("failed to find inflight task by type")?;

    Ok(row.map(|row| EnqueuedTask {
        task_id: row.id,
        task_type: row.task_type,
        status: row.status,
    }))
}

pub async fn enqueue_singleton_task_for_requester(
    state: &AppState,
    new_task: NewTask,
) -> Result<EnqueuedTask> {
    let _guard = task_singleton_enqueue_lock().lock().await;
    if let Some(requested_by) = new_task.requested_by.as_deref()
        && let Some(existing) =
            find_inflight_task_for_requester(state, &new_task.task_type, requested_by).await?
    {
        return Ok(existing);
    }
    enqueue_task(state, new_task).await
}

pub async fn start_inline_task(state: &AppState, new_task: NewTask) -> Result<EnqueuedTask> {
    let now = Utc::now().to_rfc3339();
    let task_id = insert_task_record(
        state,
        &new_task,
        STATUS_RUNNING,
        Some(now.as_str()),
        Some(state.runtime_owner_id.as_str()),
        Some(now.as_str()),
    )
    .await?;

    append_task_event(
        state,
        &task_id,
        "task.created",
        json!({
            "task_id": task_id,
            "task_type": new_task.task_type,
            "status": STATUS_QUEUED,
            "source": new_task.source,
        }),
    )
    .await?;
    append_task_event(
        state,
        &task_id,
        "task.running",
        json!({
            "task_id": task_id,
            "status": STATUS_RUNNING,
        }),
    )
    .await?;

    Ok(EnqueuedTask {
        task_id,
        task_type: new_task.task_type,
        status: STATUS_RUNNING.to_owned(),
    })
}

pub fn spawn_task_lease_heartbeat(
    state: Arc<AppState>,
    task_id: String,
) -> runtime::LeaseHeartbeat {
    runtime::spawn_lease_heartbeat(
        "job_tasks",
        runtime::RUNTIME_LEASE_HEARTBEAT_INTERVAL,
        move || {
            let state = state.clone();
            let task_id = task_id.clone();
            async move { heartbeat_task_lease(state.as_ref(), task_id.as_str()).await }
        },
    )
}

pub async fn complete_task(
    state: &AppState,
    task_id: &str,
    status: &str,
    result: Option<Value>,
    error_message: Option<String>,
) -> Result<()> {
    finalize_task(state, task_id, status, result, error_message).await
}

pub async fn retry_task(
    state: &AppState,
    task_id: &str,
    requested_by: String,
) -> Result<EnqueuedTask> {
    #[derive(Debug, sqlx::FromRow)]
    struct RetrySourceRow {
        task_type: String,
        payload_json: String,
        status: String,
    }

    let source = sqlx::query_as::<_, RetrySourceRow>(
        r#"
        SELECT task_type, payload_json, status
        FROM job_tasks
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(task_id)
    .fetch_optional(&state.pool)
    .await
    .context("failed to load retry source task")?
    .ok_or_else(|| anyhow!("task not found"))?;

    if source.status == STATUS_RUNNING || source.status == STATUS_QUEUED {
        return Err(anyhow!("only finished tasks can be retried"));
    }

    let payload: Value =
        serde_json::from_str(&source.payload_json).context("invalid source payload")?;
    let new_task = enqueue_task(
        state,
        NewTask {
            task_type: source.task_type,
            payload,
            source: "retry".to_owned(),
            requested_by: Some(requested_by),
            parent_task_id: Some(task_id.to_owned()),
        },
    )
    .await?;

    Ok(new_task)
}

pub async fn cancel_task(state: &AppState, task_id: &str) -> Result<String> {
    let now = Utc::now().to_rfc3339();

    let canceled_queued = sqlx::query(
        r#"
        UPDATE job_tasks
        SET status = ?, cancel_requested = 1, finished_at = ?, updated_at = ?
        WHERE id = ? AND status = ?
        "#,
    )
    .bind(STATUS_CANCELED)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(task_id)
    .bind(STATUS_QUEUED)
    .execute(&state.pool)
    .await
    .context("failed to cancel queued task")?;

    if canceled_queued.rows_affected() > 0 {
        append_task_event(
            state,
            task_id,
            "task.canceled",
            json!({"task_id": task_id, "status": STATUS_CANCELED}),
        )
        .await?;
        return Ok(STATUS_CANCELED.to_owned());
    }

    let requested_running = sqlx::query(
        r#"
        UPDATE job_tasks
        SET cancel_requested = 1, updated_at = ?
        WHERE id = ? AND status = ?
        "#,
    )
    .bind(now.as_str())
    .bind(task_id)
    .bind(STATUS_RUNNING)
    .execute(&state.pool)
    .await
    .context("failed to request cancellation")?;

    if requested_running.rows_affected() > 0 {
        append_task_event(
            state,
            task_id,
            "task.cancel_requested",
            json!({"task_id": task_id, "status": STATUS_RUNNING}),
        )
        .await?;
        return Ok(STATUS_RUNNING.to_owned());
    }

    let existing =
        sqlx::query_scalar::<_, String>(r#"SELECT status FROM job_tasks WHERE id = ? LIMIT 1"#)
            .bind(task_id)
            .fetch_optional(&state.pool)
            .await
            .context("failed to query task status")?;

    existing.ok_or_else(|| anyhow!("task not found"))
}

pub async fn append_task_event(
    state: &AppState,
    task_id: &str,
    event_type: &str,
    payload: Value,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let payload_json = serde_json::to_string(&payload).context("serialize task event payload")?;

    sqlx::query(
        r#"
        INSERT INTO job_task_events (id, task_id, event_type, payload_json, created_at)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(local_id::generate_local_id())
    .bind(task_id)
    .bind(event_type)
    .bind(payload_json)
    .bind(now.as_str())
    .execute(&state.pool)
    .await
    .context("failed to insert task event")?;

    Ok(())
}

pub fn task_sse_response(state: Arc<AppState>, task_id: String) -> Response {
    let events = stream! {
        let mut last_event_seq = 0_i64;
        loop {
            #[derive(Debug, sqlx::FromRow)]
            struct EventRow {
                seq: i64,
                id: String,
                event_type: String,
                payload_json: String,
            }

            let rows = sqlx::query_as::<_, EventRow>(
                r#"
                SELECT rowid AS seq, id, event_type, payload_json
                FROM job_task_events
                WHERE task_id = ? AND rowid > ?
                ORDER BY rowid ASC
                LIMIT 100
                "#,
            )
            .bind(&task_id)
            .bind(last_event_seq)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

            for row in rows {
                last_event_seq = row.seq;
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .id(row.id)
                        .event(row.event_type)
                        .data(row.payload_json),
                );
            }

            let status = sqlx::query_scalar::<_, String>(
                r#"SELECT status FROM job_tasks WHERE id = ? LIMIT 1"#,
            )
            .bind(&task_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();

            let Some(status) = status else {
                break;
            };
            if is_terminal_status(&status) {
                // Allow one more quick poll to flush late events.
                tokio::time::sleep(Duration::from_millis(120)).await;
                let rows = sqlx::query_as::<_, EventRow>(
                    r#"
                    SELECT rowid AS seq, id, event_type, payload_json
                    FROM job_task_events
                    WHERE task_id = ? AND rowid > ?
                    ORDER BY rowid ASC
                    LIMIT 100
                    "#,
                )
                .bind(&task_id)
                .bind(last_event_seq)
                .fetch_all(&state.pool)
                .await
                .unwrap_or_default();

                for row in rows {
                    yield Ok::<Event, Infallible>(
                        Event::default()
                            .id(row.id)
                            .event(row.event_type)
                            .data(row.payload_json),
                    );
                }
                break;
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    };

    Sse::new(events)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(8))
                .text("keep-alive"),
        )
        .into_response()
}

#[derive(Debug, Serialize)]
struct AdminJobEventStreamItem {
    event_id: String,
    task_id: String,
    task_type: String,
    status: String,
    event_type: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct AdminLlmCallEventStreamItem {
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
struct AdminLlmSchedulerEventStreamItem {
    event_id: String,
    max_concurrency: i64,
    available_slots: i64,
    waiting_calls: i64,
    in_flight_calls: i64,
    event_type: String,
    created_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TranslationStreamCursor {
    updated_at: String,
    entity_id: String,
}

#[derive(Debug, Clone, Serialize)]
struct AdminTranslationEventStreamItem {
    event_id: String,
    resource_type: String,
    resource_id: String,
    status: String,
    event_type: String,
    created_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct TranslationStreamCursorRow {
    id: String,
    updated_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct TranslationStreamRow {
    id: String,
    status: String,
    updated_at: String,
}

fn next_llm_scheduler_stream_event(
    last_max_concurrency: &mut i64,
    runtime_status: ai::LlmSchedulerRuntimeStatus,
    created_at: DateTime<Utc>,
) -> Option<AdminLlmSchedulerEventStreamItem> {
    if runtime_status.max_concurrency == *last_max_concurrency {
        return None;
    }
    *last_max_concurrency = runtime_status.max_concurrency;
    let created_at = created_at.to_rfc3339();
    Some(AdminLlmSchedulerEventStreamItem {
        event_id: format!(
            "{}:{}:{}:{}:{}",
            created_at,
            runtime_status.max_concurrency,
            runtime_status.available_slots,
            runtime_status.waiting_calls,
            runtime_status.in_flight_calls
        ),
        max_concurrency: runtime_status.max_concurrency,
        available_slots: runtime_status.available_slots,
        waiting_calls: runtime_status.waiting_calls,
        in_flight_calls: runtime_status.in_flight_calls,
        event_type: "llm.scheduler.updated".to_owned(),
        created_at,
    })
}

async fn load_translation_stream_cursor(
    pool: &sqlx::SqlitePool,
    table_name: &str,
) -> std::result::Result<TranslationStreamCursor, sqlx::Error> {
    let query = format!(
        "SELECT id, updated_at FROM {table_name} ORDER BY updated_at DESC, id DESC LIMIT 1"
    );
    let row = sqlx::query_as::<_, TranslationStreamCursorRow>(&query)
        .fetch_optional(pool)
        .await?;
    Ok(row
        .map(|value| TranslationStreamCursor {
            updated_at: value.updated_at,
            entity_id: value.id,
        })
        .unwrap_or_default())
}

async fn load_translation_stream_rows(
    pool: &sqlx::SqlitePool,
    table_name: &str,
    cursor: &TranslationStreamCursor,
) -> std::result::Result<Vec<TranslationStreamRow>, sqlx::Error> {
    let query = format!(
        "SELECT id, status, updated_at FROM {table_name} WHERE updated_at > ? OR (updated_at = ? AND id > ?) ORDER BY updated_at ASC, id ASC LIMIT 200"
    );
    sqlx::query_as::<_, TranslationStreamRow>(&query)
        .bind(cursor.updated_at.as_str())
        .bind(cursor.updated_at.as_str())
        .bind(cursor.entity_id.as_str())
        .fetch_all(pool)
        .await
}

pub fn admin_jobs_sse_response(state: Arc<AppState>) -> Response {
    let events = stream! {
        #[derive(Debug, sqlx::FromRow)]
        struct EventRow {
            seq: i64,
            id: String,
            task_id: String,
            task_type: String,
            status: String,
            event_type: String,
            created_at: String,
        }

        #[derive(Debug, sqlx::FromRow)]
        struct LlmEventRow {
            seq: i64,
            id: String,
            call_id: String,
            status: String,
            source: String,
            requested_by: Option<String>,
            parent_task_id: Option<String>,
            event_type: String,
            created_at: String,
        }

        let mut last_job_event_seq = sqlx::query_scalar::<_, i64>(
            r#"SELECT COALESCE(MAX(rowid), 0) FROM job_task_events"#,
        )
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
        let mut last_llm_event_seq = sqlx::query_scalar::<_, i64>(
            r#"SELECT COALESCE(MAX(rowid), 0) FROM llm_call_events"#,
        )
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);
        let mut last_translation_request_cursor = load_translation_stream_cursor(
            &state.pool,
            "translation_requests",
        )
        .await
        .unwrap_or_default();
        let mut last_translation_batch_cursor = load_translation_stream_cursor(
            &state.pool,
            "translation_batches",
        )
        .await
        .unwrap_or_default();
        let mut last_llm_scheduler_max_concurrency = state.llm_scheduler.runtime_status().max_concurrency;
        let mut last_translation_worker_updated_at = HashMap::<String, String>::new();

        // Emit one lightweight frame immediately so proxies/browsers can
        // complete SSE handshake and update client connection state promptly.
        yield Ok::<Event, Infallible>(Event::default().comment("stream-ready"));

        loop {
            let job_rows = sqlx::query_as::<_, EventRow>(
                r#"
                SELECT
                  e.rowid AS seq,
                  e.id,
                  e.task_id,
                  t.task_type,
                  t.status,
                  e.event_type,
                  e.created_at
                FROM job_task_events e
                JOIN job_tasks t ON t.id = e.task_id
                WHERE e.rowid > ?
                ORDER BY e.rowid ASC
                LIMIT 200
                "#,
            )
            .bind(last_job_event_seq)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

            for row in job_rows {
                last_job_event_seq = row.seq;
                let payload = AdminJobEventStreamItem {
                    event_id: row.id.clone(),
                    task_id: row.task_id,
                    task_type: row.task_type,
                    status: row.status,
                    event_type: row.event_type,
                    created_at: row.created_at,
                };
                let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_owned());
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .id(format!("job-{}", row.id))
                        .event("job.event")
                        .data(data),
                );
            }

            let llm_rows = sqlx::query_as::<_, LlmEventRow>(
                r#"
                SELECT
                  rowid AS seq,
                  id,
                  call_id,
                  status,
                  source,
                  requested_by,
                  parent_task_id,
                  event_type,
                  created_at
                FROM llm_call_events
                WHERE rowid > ?
                ORDER BY rowid ASC
                LIMIT 200
                "#,
            )
            .bind(last_llm_event_seq)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

            for row in llm_rows {
                last_llm_event_seq = row.seq;
                let payload = AdminLlmCallEventStreamItem {
                    event_id: row.id.clone(),
                    call_id: row.call_id,
                    status: row.status,
                    source: row.source,
                    requested_by: row.requested_by,
                    parent_task_id: row.parent_task_id,
                    event_type: row.event_type,
                    created_at: row.created_at,
                };
                let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_owned());
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .id(format!("llm-{}", row.id))
                        .event("llm.call")
                        .data(data),
                );
            }

            if let Some(payload) = next_llm_scheduler_stream_event(
                &mut last_llm_scheduler_max_concurrency,
                state.llm_scheduler.runtime_status(),
                Utc::now(),
            ) {
                let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_owned());
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .id(format!("llm-scheduler-{}", payload.event_id))
                        .event("llm.scheduler")
                        .data(data),
                );
            }

            let request_rows = load_translation_stream_rows(
                &state.pool,
                "translation_requests",
                &last_translation_request_cursor,
            )
            .await
            .unwrap_or_default();

            for row in request_rows {
                let event_id = format!("request:{}:{}", row.updated_at, row.id);
                last_translation_request_cursor = TranslationStreamCursor {
                    updated_at: row.updated_at.clone(),
                    entity_id: row.id.clone(),
                };
                let payload = AdminTranslationEventStreamItem {
                    event_id: event_id.clone(),
                    resource_type: "request".to_owned(),
                    resource_id: row.id,
                    status: row.status,
                    event_type: "translation.request.updated".to_owned(),
                    created_at: row.updated_at,
                };
                let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_owned());
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .id(format!("translation-{}", event_id))
                        .event("translation.event")
                        .data(data),
                );
            }

            let batch_rows = load_translation_stream_rows(
                &state.pool,
                "translation_batches",
                &last_translation_batch_cursor,
            )
            .await
            .unwrap_or_default();

            for row in batch_rows {
                let event_id = format!("batch:{}:{}", row.updated_at, row.id);
                last_translation_batch_cursor = TranslationStreamCursor {
                    updated_at: row.updated_at.clone(),
                    entity_id: row.id.clone(),
                };
                let payload = AdminTranslationEventStreamItem {
                    event_id: event_id.clone(),
                    resource_type: "batch".to_owned(),
                    resource_id: row.id,
                    status: row.status,
                    event_type: "translation.batch.updated".to_owned(),
                    created_at: row.updated_at,
                };
                let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_owned());
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .id(format!("translation-{}", event_id))
                        .event("translation.event")
                        .data(data),
                );
            }

            for worker in translations::translation_worker_runtime_statuses(state.as_ref()).await {
                let last_seen = last_translation_worker_updated_at
                    .get(worker.worker_id.as_str())
                    .cloned();
                if last_seen.as_deref() == Some(worker.updated_at.as_str()) {
                    continue;
                }
                last_translation_worker_updated_at
                    .insert(worker.worker_id.clone(), worker.updated_at.clone());

                let event_id = format!("worker:{}:{}", worker.updated_at, worker.worker_id);
                let payload = AdminTranslationEventStreamItem {
                    event_id: event_id.clone(),
                    resource_type: "worker".to_owned(),
                    resource_id: worker.worker_id,
                    status: worker.status,
                    event_type: "translation.worker.updated".to_owned(),
                    created_at: worker.updated_at,
                };
                let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_owned());
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .id(format!("translation-{}", event_id))
                        .event("translation.event")
                        .data(data),
                );
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    };

    Sse::new(events)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(8))
                .text("keep-alive"),
        )
        .into_response()
}

async fn claim_next_queued_task(state: &AppState) -> Result<Option<TaskRow>> {
    let _claim_guard = task_claim_lock().lock().await;
    let mut tx = state.pool.begin().await.context("begin task claim tx")?;

    let task_id = sqlx::query_scalar::<_, String>(
        r#"
        SELECT id
        FROM job_tasks
        WHERE status = ?
          AND (
            task_type != ?
            OR NOT EXISTS (
              SELECT 1
              FROM job_tasks running
              WHERE running.task_type = ?
                AND running.status = ?
            )
          )
        ORDER BY created_at ASC
        LIMIT 1
        "#,
    )
    .bind(STATUS_QUEUED)
    .bind(TASK_SYNC_SUBSCRIPTIONS)
    .bind(TASK_SYNC_SUBSCRIPTIONS)
    .bind(STATUS_RUNNING)
    .fetch_optional(&mut *tx)
    .await
    .context("select queued task")?;

    let Some(task_id) = task_id else {
        tx.commit().await.context("commit empty claim tx")?;
        return Ok(None);
    };

    let now = Utc::now().to_rfc3339();
    let updated = sqlx::query(
        r#"
        UPDATE job_tasks
        SET status = ?, started_at = ?, runtime_owner_id = ?, lease_heartbeat_at = ?, updated_at = ?
        WHERE id = ? AND status = ?
        "#,
    )
    .bind(STATUS_RUNNING)
    .bind(now.as_str())
    .bind(state.runtime_owner_id.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(&task_id)
    .bind(STATUS_QUEUED)
    .execute(&mut *tx)
    .await
    .context("update queued task to running")?;

    if updated.rows_affected() == 0 {
        tx.commit().await.context("commit failed claim tx")?;
        return Ok(None);
    }

    let task = sqlx::query_as::<_, TaskRow>(
        r#"
        SELECT id, task_type, source, requested_by, payload_json, cancel_requested
        FROM job_tasks
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(&task_id)
    .fetch_one(&mut *tx)
    .await
    .context("reload claimed task")?;

    tx.commit().await.context("commit claim tx")?;

    append_task_event(
        state,
        &task.id,
        "task.running",
        json!({"task_id": task.id, "status": STATUS_RUNNING}),
    )
    .await?;

    Ok(Some(task))
}

async fn process_task(state: Arc<AppState>, task: TaskRow) -> Result<()> {
    if task.cancel_requested != 0 {
        finalize_task(state.as_ref(), &task.id, STATUS_CANCELED, None, None).await?;
        append_task_event(
            state.as_ref(),
            &task.id,
            "task.completed",
            json!({"task_id": task.id, "status": STATUS_CANCELED}),
        )
        .await?;
        return Ok(());
    }

    let payload: Value = serde_json::from_str(&task.payload_json)
        .with_context(|| format!("invalid payload json for task {}", task.id))?;

    let context = ai::LlmCallContext {
        source: format!("job.{}", task.source),
        requested_by: task.requested_by,
        parent_task_id: Some(task.id.clone()),
        parent_task_type: Some(task.task_type.clone()),
        parent_translation_batch_id: None,
    };
    let heartbeat = spawn_task_lease_heartbeat(state.clone(), task.id.clone());
    let result = ai::with_llm_call_context(
        context,
        execute_task(state.as_ref(), &task.id, &task.task_type, &payload),
    )
    .await;

    if is_task_cancel_requested(state.as_ref(), &task.id)
        .await
        .unwrap_or(false)
    {
        finalize_task(state.as_ref(), &task.id, STATUS_CANCELED, None, None).await?;
        heartbeat.stop().await;
        append_task_event(
            state.as_ref(),
            &task.id,
            "task.completed",
            json!({"task_id": task.id, "status": STATUS_CANCELED}),
        )
        .await?;
        return Ok(());
    }

    match result {
        Ok(result_json) => {
            finalize_task(
                state.as_ref(),
                &task.id,
                STATUS_SUCCEEDED,
                Some(result_json),
                None,
            )
            .await?;
            heartbeat.stop().await;
            append_task_event(
                state.as_ref(),
                &task.id,
                "task.completed",
                json!({"task_id": task.id, "status": STATUS_SUCCEEDED}),
            )
            .await?;
        }
        Err(err) => {
            let message = err.to_string();
            finalize_task(
                state.as_ref(),
                &task.id,
                STATUS_FAILED,
                None,
                Some(message.clone()),
            )
            .await?;
            heartbeat.stop().await;
            append_task_event(
                state.as_ref(),
                &task.id,
                "task.completed",
                json!({"task_id": task.id, "status": STATUS_FAILED, "error": message}),
            )
            .await?;
        }
    }

    Ok(())
}

async fn execute_task(
    state: &AppState,
    task_id: &str,
    task_type: &str,
    payload: &Value,
) -> Result<Value> {
    match task_type {
        TASK_SYNC_STARRED => {
            let user_id = payload_local_id(payload, "user_id")?;
            let res = sync::sync_starred(state, user_id.as_str()).await?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_SYNC_RELEASES => {
            let user_id = payload_local_id(payload, "user_id")?;
            let res = sync::sync_releases(state, user_id.as_str()).await?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_SYNC_ACCESS_REFRESH => {
            let user_id = payload_local_id(payload, "user_id")?;
            let res = sync::sync_access_refresh(state, task_id, user_id.as_str()).await?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_SYNC_NOTIFICATIONS => {
            let user_id = payload_local_id(payload, "user_id")?;
            let res = sync::sync_notifications(state, user_id.as_str()).await?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_SYNC_ALL => {
            let user_id = payload_local_id(payload, "user_id")?;
            execute_sync_all_task_with(
                state,
                task_id,
                user_id.as_str(),
                |state, user_id| Box::pin(sync::sync_starred(state, user_id)),
                |state, user_id| Box::pin(sync::sync_releases(state, user_id)),
                |state, user_id| Box::pin(sync::sync_social_activity(state, user_id)),
                |state, user_id| Box::pin(sync::sync_notifications(state, user_id)),
            )
            .await
        }
        TASK_SYNC_SUBSCRIPTIONS => {
            let res = sync::sync_subscriptions(state, task_id, payload).await?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_BRIEF_GENERATE => {
            let user_id = payload_local_id(payload, "user_id")?;
            let key_date = payload_date(payload, "key_date")?;
            let snapshot = if let Some(key_date) = key_date {
                ai::generate_daily_brief_snapshot_for_key_date(state, user_id.as_str(), key_date)
                    .await?
            } else {
                ai::generate_daily_brief_snapshot_for_current(state, user_id.as_str()).await?
            };
            Ok(json!({
                "brief_id": snapshot.id,
                "content_length": snapshot.content_markdown.chars().count(),
                "date": snapshot.date,
                "window_start_utc": snapshot.window_start,
                "window_end_utc": snapshot.window_end,
                "effective_time_zone": snapshot.effective_time_zone,
                "effective_local_boundary": snapshot.effective_local_boundary,
                "release_count": snapshot.release_ids.len(),
            }))
        }
        TASK_BRIEF_DAILY_SLOT => execute_daily_slot_task(state, task_id, payload).await,
        TASK_BRIEF_HISTORY_RECOMPUTE => execute_brief_history_recompute_task(state, task_id).await,
        TASK_BRIEF_REFRESH_CONTENT => execute_brief_refresh_content_task(state, task_id).await,
        TASK_TRANSLATE_RELEASE => {
            let user_id = payload_local_id(payload, "user_id")?;
            let release_id = payload_string(payload, "release_id")?;
            let res = api::translate_release_for_user(state, user_id.as_str(), &release_id)
                .await
                .map_err(|err| anyhow!("translate_release failed: {}", err.code()))?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_TRANSLATE_RELEASE_BATCH => {
            let user_id = payload_local_id(payload, "user_id")?;
            let release_ids = payload_i64_array(payload, "release_ids")?;
            let res = api::translate_releases_batch_for_user(state, user_id.as_str(), &release_ids)
                .await
                .map_err(|err| anyhow!("translate_releases_batch failed: {}", err.code()))?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_SUMMARIZE_RELEASE_SMART_BATCH => {
            let user_id = payload_local_id(payload, "user_id")?;
            let release_ids = payload_i64_array(payload, "release_ids")?;
            let res =
                api::summarize_releases_smart_batch_for_user(state, user_id.as_str(), &release_ids)
                    .await
                    .map_err(|err| {
                        anyhow!("summarize_releases_smart_batch failed: {}", err.code())
                    })?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_TRANSLATE_RELEASE_DETAIL => {
            let user_id = payload_local_id(payload, "user_id")?;
            let release_id = payload_string(payload, "release_id")?;
            let res = api::translate_release_detail_for_user(state, user_id.as_str(), &release_id)
                .await
                .map_err(|err| anyhow!("translate_release_detail failed: {}", err.code()))?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_TRANSLATE_NOTIFICATION => {
            let user_id = payload_local_id(payload, "user_id")?;
            let thread_id = payload_string(payload, "thread_id")?;
            let res = api::translate_notification_for_user(state, user_id, &thread_id)
                .await
                .map_err(|err| anyhow!("translate_notification failed: {}", err.code()))?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        _ => Err(anyhow!("unsupported task_type: {task_type}")),
    }
}

type TaskStepFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

async fn execute_sync_all_task_with<SyncStarred, SyncReleases, SyncSocial, SyncNotifications>(
    state: &AppState,
    task_id: &str,
    user_id: &str,
    sync_starred: SyncStarred,
    sync_releases: SyncReleases,
    sync_social: SyncSocial,
    sync_notifications: SyncNotifications,
) -> Result<Value>
where
    SyncStarred: for<'a> Fn(&'a AppState, &'a str) -> TaskStepFuture<'a, sync::SyncStarredResult>,
    SyncReleases: for<'a> Fn(&'a AppState, &'a str) -> TaskStepFuture<'a, sync::SyncReleasesResult>,
    SyncSocial:
        for<'a> Fn(&'a AppState, &'a str) -> TaskStepFuture<'a, sync::SyncSocialActivityResult>,
    SyncNotifications:
        for<'a> Fn(&'a AppState, &'a str) -> TaskStepFuture<'a, sync::SyncNotificationsResult>,
{
    let starred = sync_starred(state, user_id).await?;
    if is_task_cancel_requested(state, task_id)
        .await
        .unwrap_or(false)
    {
        return Ok(json!({"canceled": true}));
    }

    let releases = sync_releases(state, user_id).await?;
    if is_task_cancel_requested(state, task_id)
        .await
        .unwrap_or(false)
    {
        return Ok(json!({"canceled": true}));
    }

    let (social, social_error) = match sync_social(state, user_id).await {
        Ok(result) => (result, None),
        Err(err) => {
            tracing::warn!(
                ?err,
                user_id,
                "task sync_all: social activity sync failed, continuing with notifications"
            );
            (
                sync::SyncSocialActivityResult::default(),
                Some(err.to_string()),
            )
        }
    };
    if is_task_cancel_requested(state, task_id)
        .await
        .unwrap_or(false)
    {
        return Ok(json!({"canceled": true}));
    }

    let notifications = sync_notifications(state, user_id).await?;
    let mut result = json!({
        "starred": starred,
        "releases": releases,
        "social": social,
        "notifications": notifications,
    });
    if let Some(error) = social_error {
        result["social_error"] = Value::String(error);
    }
    Ok(result)
}

async fn execute_daily_slot_task(
    state: &AppState,
    task_id: &str,
    payload: &Value,
) -> Result<Value> {
    let hour_utc = payload_i64(payload, "hour_utc")?;
    let task_created_at = sqlx::query_scalar::<_, Option<String>>(
        r#"
        SELECT created_at
        FROM job_tasks
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(task_id)
    .fetch_one(&state.pool)
    .await
    .context("failed to load daily slot task metadata")?;
    let now_utc = Utc::now();
    let slot_reference_utc =
        payload_slot_reference_utc(payload, task_created_at.as_deref(), now_utc);
    let target_hour_key = slot_reference_utc.format("%Y-%m-%dT%H").to_string();
    let due_users =
        load_due_daily_slot_users(state, payload, slot_reference_utc, target_hour_key.as_str())
            .await?;

    append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "collect",
            "hour_utc": hour_utc,
            "hour_key": target_hour_key,
            "total_users": due_users.len(),
        }),
    )
    .await?;

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut canceled = false;

    for (index, user) in due_users.iter().enumerate() {
        if is_task_cancel_requested(state, task_id)
            .await
            .unwrap_or(false)
        {
            canceled = true;
            break;
        }

        append_task_event(
            state,
            task_id,
            "task.progress",
            json!({
                "task_id": task_id,
                "stage": "generate",
                "index": index + 1,
                "total": due_users.len(),
                "user_id": user.user_id,
                "last_active_at": user.last_active_at,
                "key_date": user.window.key_date,
                "local_boundary": user.window.effective_local_boundary,
                "time_zone": user.preferences.time_zone,
                "window_start_utc": user.window.start_utc.to_rfc3339(),
                "window_end_utc": user.window.end_utc.to_rfc3339(),
            }),
        )
        .await?;

        match ai::generate_daily_brief_snapshot_for_window(
            state,
            user.user_id.as_str(),
            &user.window,
            "scheduled",
        )
        .await
        {
            Ok(snapshot) => {
                succeeded += 1;
                append_task_event(
                    state,
                    task_id,
                    "task.progress",
                    json!({
                        "task_id": task_id,
                        "stage": "user_succeeded",
                        "user_id": user.user_id,
                        "key_date": user.window.key_date,
                        "brief_id": snapshot.id,
                        "content_length": snapshot.content_markdown.chars().count(),
                        "local_boundary": snapshot.effective_local_boundary,
                        "time_zone": snapshot.effective_time_zone,
                        "window_start_utc": snapshot.window_start,
                        "window_end_utc": snapshot.window_end,
                        "release_count": snapshot.release_ids.len(),
                    }),
                )
                .await?;
            }
            Err(err) => {
                failed += 1;
                append_task_event(
                    state,
                    task_id,
                    "task.progress",
                    json!({
                        "task_id": task_id,
                        "stage": "user_failed",
                        "user_id": user.user_id,
                        "key_date": user.window.key_date,
                        "local_boundary": user.window.effective_local_boundary,
                        "time_zone": user.preferences.time_zone,
                        "error": err.to_string(),
                    }),
                )
                .await?;
            }
        }
    }

    append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "summary",
            "total": due_users.len(),
            "succeeded": succeeded,
            "failed": failed,
            "canceled": canceled,
        }),
    )
    .await?;

    if canceled {
        return Ok(json!({
            "hour_utc": hour_utc,
            "total": due_users.len(),
            "succeeded": succeeded,
            "failed": failed,
            "canceled": true,
        }));
    }

    if failed > 0 && succeeded == 0 {
        return Err(anyhow!(
            "daily slot {hour_utc:02} failed for all users (failed={failed}, total={})",
            due_users.len()
        ));
    }

    Ok(json!({
        "hour_utc": hour_utc,
        "total": due_users.len(),
        "succeeded": succeeded,
        "failed": failed,
    }))
}

async fn collect_due_daily_slot_user_snapshots(
    state: &AppState,
    slot_reference_utc: DateTime<Utc>,
    target_hour_key: &str,
) -> Result<Vec<DailySlotUserSnapshot>> {
    let users = sqlx::query_as::<_, DailySlotUserRow>(
        r#"
        SELECT
          id,
          daily_brief_local_time,
          daily_brief_time_zone,
          daily_brief_utc_time,
          last_active_at
        FROM users
        WHERE is_disabled = 0
        ORDER BY
          CASE WHEN last_active_at IS NULL THEN 1 ELSE 0 END ASC,
          last_active_at DESC,
          id ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .context("failed to query users for daily slot")?;

    let mut due_users = Vec::new();
    for row in users {
        let preferences = briefs::derive_daily_brief_preferences(
            &state.config,
            row.daily_brief_local_time.as_deref(),
            row.daily_brief_time_zone.as_deref(),
            Some(row.daily_brief_utc_time.as_str()),
            slot_reference_utc,
        );
        let window = briefs::compute_current_daily_window(&preferences, slot_reference_utc)
            .with_context(|| format!("failed to compute current window for user {}", row.id))?;
        if window.end_utc.format("%Y-%m-%dT%H").to_string() != target_hour_key {
            continue;
        }
        due_users.push(DailySlotUserSnapshot {
            user_id: row.id,
            last_active_at: row.last_active_at,
            key_date: window.key_date.to_string(),
            local_boundary: briefs::format_daily_brief_local_time(preferences.local_time),
            effective_local_boundary: Some(window.effective_local_boundary.clone()),
            time_zone: preferences.time_zone,
            window_start_utc: window.start_utc.to_rfc3339(),
            window_end_utc: window.end_utc.to_rfc3339(),
        });
    }

    Ok(due_users)
}

async fn load_due_daily_slot_users(
    state: &AppState,
    payload: &Value,
    slot_reference_utc: DateTime<Utc>,
    target_hour_key: &str,
) -> Result<Vec<DueDailySlotUser>> {
    if let Some(snapshots) = payload_daily_slot_user_snapshots(payload)? {
        return snapshots
            .into_iter()
            .map(due_daily_slot_user_from_snapshot)
            .collect();
    }

    collect_due_daily_slot_user_snapshots(state, slot_reference_utc, target_hour_key)
        .await?
        .into_iter()
        .map(due_daily_slot_user_from_snapshot)
        .collect()
}

fn payload_daily_slot_user_snapshots(
    payload: &Value,
) -> Result<Option<Vec<DailySlotUserSnapshot>>> {
    let Some(users) = payload.get("users") else {
        return Ok(None);
    };
    serde_json::from_value::<Vec<DailySlotUserSnapshot>>(users.clone())
        .map(Some)
        .context("payload field users must be daily slot snapshot array")
}

fn due_daily_slot_user_from_snapshot(snapshot: DailySlotUserSnapshot) -> Result<DueDailySlotUser> {
    let key_date =
        NaiveDate::parse_from_str(snapshot.key_date.as_str(), "%Y-%m-%d").with_context(|| {
            format!(
                "invalid daily slot snapshot key_date for user {}",
                snapshot.user_id
            )
        })?;
    let local_time = briefs::parse_daily_brief_local_time(snapshot.local_boundary.as_str())
        .with_context(|| {
            format!(
                "invalid daily slot snapshot local_boundary for user {}",
                snapshot.user_id
            )
        })?;
    let effective_local_boundary = snapshot
        .effective_local_boundary
        .unwrap_or_else(|| snapshot.local_boundary.clone());
    briefs::parse_daily_brief_local_time(effective_local_boundary.as_str()).with_context(|| {
        format!(
            "invalid daily slot snapshot effective_local_boundary for user {}",
            snapshot.user_id
        )
    })?;
    let time_zone =
        briefs::parse_daily_brief_time_zone(snapshot.time_zone.as_str()).with_context(|| {
            format!(
                "invalid daily slot snapshot time_zone for user {}",
                snapshot.user_id
            )
        })?;
    let tz = Tz::from_str(time_zone.as_str()).with_context(|| {
        format!(
            "invalid daily slot snapshot time_zone for user {}",
            snapshot.user_id
        )
    })?;
    let start_utc = DateTime::parse_from_rfc3339(snapshot.window_start_utc.as_str())
        .with_context(|| {
            format!(
                "invalid daily slot snapshot window_start_utc for user {}",
                snapshot.user_id
            )
        })?
        .with_timezone(&Utc);
    let end_utc = DateTime::parse_from_rfc3339(snapshot.window_end_utc.as_str())
        .with_context(|| {
            format!(
                "invalid daily slot snapshot window_end_utc for user {}",
                snapshot.user_id
            )
        })?
        .with_timezone(&Utc);

    Ok(DueDailySlotUser {
        user_id: snapshot.user_id,
        last_active_at: snapshot.last_active_at,
        preferences: briefs::DailyBriefPreferences {
            local_time,
            time_zone: time_zone.clone(),
        },
        window: briefs::DailyWindow {
            key_date,
            display_date: key_date.to_string(),
            end_local: end_utc.with_timezone(&tz).fixed_offset(),
            start_utc,
            end_utc,
            effective_time_zone: time_zone,
            effective_local_boundary,
        },
    })
}

async fn execute_brief_history_recompute_task(state: &AppState, task_id: &str) -> Result<Value> {
    #[derive(Debug, sqlx::FromRow)]
    struct LegacyBriefRow {
        id: String,
        user_id: String,
        date: String,
    }

    let total = ai::legacy_brief_count(state).await?;
    append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "collect",
            "total_briefs": total,
        }),
    )
    .await?;

    if total == 0 {
        append_task_event(
            state,
            task_id,
            "task.progress",
            json!({
                "task_id": task_id,
                "stage": "summary",
                "total": 0,
                "processed": 0,
                "succeeded": 0,
                "failed": 0,
                "canceled": false,
            }),
        )
        .await?;
        return Ok(json!({
            "total": 0,
            "processed": 0,
            "succeeded": 0,
            "failed": 0,
            "canceled": false,
        }));
    }

    let rows = sqlx::query_as::<_, LegacyBriefRow>(
        r#"
        SELECT id, user_id, date
        FROM briefs
        WHERE generation_source IN ('legacy', 'history_recompute_failed')
        ORDER BY date DESC, created_at DESC, id DESC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .context("failed to query legacy briefs for recompute")?;

    let mut processed = 0usize;
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut canceled = false;

    for row in rows {
        if is_task_cancel_requested(state, task_id)
            .await
            .unwrap_or(false)
        {
            canceled = true;
            break;
        }

        processed += 1;
        append_task_event(
            state,
            task_id,
            "task.progress",
            json!({
                "task_id": task_id,
                "stage": "recompute",
                "index": processed,
                "total": total,
                "brief_id": row.id,
                "user_id": row.user_id,
                "date": row.date,
            }),
        )
        .await?;

        match ai::recompute_legacy_brief_snapshot(state, row.id.as_str()).await {
            Ok(snapshot) => {
                succeeded += 1;
                append_task_event(
                    state,
                    task_id,
                    "task.progress",
                    json!({
                        "task_id": task_id,
                        "stage": "brief_succeeded",
                        "index": processed,
                        "total": total,
                        "brief_id": snapshot.id,
                        "date": snapshot.date,
                        "window_start_utc": snapshot.window_start,
                        "window_end_utc": snapshot.window_end,
                        "time_zone": snapshot.effective_time_zone,
                        "local_boundary": snapshot.effective_local_boundary,
                        "release_count": snapshot.release_ids.len(),
                    }),
                )
                .await?;
            }
            Err(err) => {
                let failed_at = Utc::now().to_rfc3339();
                sqlx::query(
                    r#"
                    UPDATE briefs
                    SET generation_source = 'history_recompute_failed',
                        updated_at = ?
                    WHERE id = ?
                    "#,
                )
                .bind(failed_at.as_str())
                .bind(&row.id)
                .execute(&state.pool)
                .await
                .with_context(|| {
                    format!(
                        "failed to mark legacy brief {} as history_recompute_failed",
                        row.id
                    )
                })?;
                failed += 1;
                append_task_event(
                    state,
                    task_id,
                    "task.progress",
                    json!({
                        "task_id": task_id,
                        "stage": "brief_failed",
                        "index": processed,
                        "total": total,
                        "brief_id": row.id,
                        "user_id": row.user_id,
                        "date": row.date,
                        "error": err.to_string(),
                    }),
                )
                .await?;
            }
        }
    }

    append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "summary",
            "total": total,
            "processed": processed,
            "succeeded": succeeded,
            "failed": failed,
            "canceled": canceled,
        }),
    )
    .await?;

    if canceled {
        return Ok(json!({
            "total": total,
            "processed": processed,
            "succeeded": succeeded,
            "failed": failed,
            "canceled": true,
        }));
    }

    if failed > 0 && succeeded == 0 {
        return Err(anyhow!(
            "brief history recompute failed for all legacy briefs (failed={failed}, total={total})"
        ));
    }

    if let Err(err) = enqueue_brief_refresh_content_if_needed(state).await {
        tracing::warn!(
            ?err,
            "failed to enqueue brief content refresh after history recompute"
        );
    }

    Ok(json!({
        "total": total,
        "processed": processed,
        "succeeded": succeeded,
        "failed": failed,
        "canceled": false,
    }))
}

async fn execute_brief_refresh_content_task(state: &AppState, task_id: &str) -> Result<Value> {
    let rows = ai::load_brief_content_refresh_candidates(state).await?;
    let total =
        i64::try_from(rows.len()).context("brief refresh candidate count overflowed i64")?;
    append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "collect",
            "total_briefs": total,
        }),
    )
    .await?;

    if total == 0 {
        append_task_event(
            state,
            task_id,
            "task.progress",
            json!({
                "task_id": task_id,
                "stage": "summary",
                "total": 0,
                "processed": 0,
                "succeeded": 0,
                "failed": 0,
                "canceled": false,
            }),
        )
        .await?;
        return Ok(json!({
            "total": 0,
            "processed": 0,
            "succeeded": 0,
            "failed": 0,
            "canceled": false,
        }));
    }

    let mut processed = 0usize;
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut canceled = false;

    for row in rows {
        if is_task_cancel_requested(state, task_id)
            .await
            .unwrap_or(false)
        {
            canceled = true;
            break;
        }

        processed += 1;
        append_task_event(
            state,
            task_id,
            "task.progress",
            json!({
                "task_id": task_id,
                "stage": "refresh",
                "index": processed,
                "total": total,
                "brief_id": row.id,
                "user_id": row.user_id,
                "date": row.date,
            }),
        )
        .await?;

        match ai::refresh_existing_brief_snapshot_content(state, row.id.as_str(), "content_refresh")
            .await
        {
            Ok(snapshot) => {
                succeeded += 1;
                append_task_event(
                    state,
                    task_id,
                    "task.progress",
                    json!({
                        "task_id": task_id,
                        "stage": "brief_succeeded",
                        "index": processed,
                        "total": total,
                        "brief_id": snapshot.id,
                        "date": snapshot.date,
                        "window_start_utc": snapshot.window_start,
                        "window_end_utc": snapshot.window_end,
                        "time_zone": snapshot.effective_time_zone,
                        "local_boundary": snapshot.effective_local_boundary,
                        "release_count": snapshot.release_ids.len(),
                    }),
                )
                .await?;
            }
            Err(err) => {
                let failed_at = Utc::now().to_rfc3339();
                sqlx::query(
                    r#"
                    UPDATE briefs
                    SET generation_source = 'content_refresh_failed',
                        updated_at = ?
                    WHERE id = ?
                    "#,
                )
                .bind(failed_at.as_str())
                .bind(&row.id)
                .execute(&state.pool)
                .await
                .with_context(|| {
                    format!(
                        "failed to mark normalized brief {} as content_refresh_failed",
                        row.id
                    )
                })?;
                failed += 1;
                append_task_event(
                    state,
                    task_id,
                    "task.progress",
                    json!({
                        "task_id": task_id,
                        "stage": "brief_failed",
                        "index": processed,
                        "total": total,
                        "brief_id": row.id,
                        "user_id": row.user_id,
                        "date": row.date,
                        "error": err.to_string(),
                    }),
                )
                .await?;
            }
        }
    }

    append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "summary",
            "total": total,
            "processed": processed,
            "succeeded": succeeded,
            "failed": failed,
            "canceled": canceled,
        }),
    )
    .await?;

    if canceled {
        return Ok(json!({
            "total": total,
            "processed": processed,
            "succeeded": succeeded,
            "failed": failed,
            "canceled": true,
        }));
    }

    if failed > 0 && succeeded == 0 {
        return Err(anyhow!(
            "brief content refresh failed for all candidate briefs (failed={failed}, total={total})"
        ));
    }

    Ok(json!({
        "total": total,
        "processed": processed,
        "succeeded": succeeded,
        "failed": failed,
        "canceled": false,
    }))
}

async fn finalize_task(
    state: &AppState,
    task_id: &str,
    status: &str,
    result: Option<Value>,
    error_message: Option<String>,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let result_json = result
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .context("serialize task result")?;

    sqlx::query(
        r#"
        UPDATE job_tasks
        SET status = ?,
            result_json = ?,
            error_message = ?,
            finished_at = ?,
            runtime_owner_id = NULL,
            lease_heartbeat_at = NULL,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(status)
    .bind(result_json.as_deref())
    .bind(error_message.as_deref())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(task_id)
    .execute(&state.pool)
    .await
    .context("failed to finalize task")?;

    Ok(())
}

async fn heartbeat_task_lease(state: &AppState, task_id: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE job_tasks
        SET lease_heartbeat_at = ?, updated_at = ?
        WHERE id = ?
          AND status = ?
          AND runtime_owner_id = ?
        "#,
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(task_id)
    .bind(STATUS_RUNNING)
    .bind(state.runtime_owner_id.as_str())
    .execute(&state.pool)
    .await
    .context("failed to heartbeat task lease")?;
    Ok(())
}

pub async fn recover_runtime_state(state: &AppState) -> Result<()> {
    recover_runtime_state_with_mode(state, runtime::RuntimeRecoveryMode::Sweep).await
}

async fn recover_runtime_state_with_mode(
    state: &AppState,
    mode: runtime::RuntimeRecoveryMode,
) -> Result<()> {
    #[derive(Debug, sqlx::FromRow)]
    struct StaleTaskRow {
        id: String,
        runtime_owner_id: Option<String>,
        lease_heartbeat_at: Option<String>,
    }

    let now = Utc::now();
    let cutoff = runtime::stale_cutoff_timestamp(now);
    let stale_tasks = match mode {
        runtime::RuntimeRecoveryMode::Startup => {
            sqlx::query_as::<_, StaleTaskRow>(
                r#"
                SELECT id, runtime_owner_id, lease_heartbeat_at
                FROM job_tasks
                WHERE status = ?
                  AND (
                    runtime_owner_id IS NULL
                    OR lease_heartbeat_at IS NULL
                    OR julianday(lease_heartbeat_at) <= julianday(?)
                    OR (
                      runtime_owner_id != ?
                      AND NOT EXISTS (
                        SELECT 1
                        FROM runtime_owners
                        WHERE runtime_owner_id = job_tasks.runtime_owner_id
                          AND julianday(lease_heartbeat_at) > julianday(?)
                      )
                    )
                  )
                ORDER BY created_at ASC, id ASC
                "#,
            )
            .bind(STATUS_RUNNING)
            .bind(cutoff.as_str())
            .bind(state.runtime_owner_id.as_str())
            .bind(cutoff.as_str())
            .fetch_all(&state.pool)
            .await
        }
        runtime::RuntimeRecoveryMode::Sweep => {
            sqlx::query_as::<_, StaleTaskRow>(
                r#"
                SELECT id, runtime_owner_id, lease_heartbeat_at
                FROM job_tasks
                WHERE status = ?
                  AND (
                    runtime_owner_id IS NULL
                    OR lease_heartbeat_at IS NULL
                    OR julianday(lease_heartbeat_at) <= julianday(?)
                  )
                ORDER BY created_at ASC, id ASC
                "#,
            )
            .bind(STATUS_RUNNING)
            .bind(cutoff.as_str())
            .fetch_all(&state.pool)
            .await
        }
    }
    .context("failed to load stale runtime tasks")?;

    for task in stale_tasks {
        finalize_task(
            state,
            task.id.as_str(),
            STATUS_FAILED,
            None,
            Some(runtime::RUNTIME_LEASE_EXPIRED_ERROR.to_owned()),
        )
        .await?;
        append_task_event(
            state,
            task.id.as_str(),
            "task.recovered_failed",
            json!({
                "task_id": task.id,
                "status": STATUS_FAILED,
                "error": runtime::RUNTIME_LEASE_EXPIRED_ERROR,
                "previous_runtime_owner_id": task.runtime_owner_id,
                "previous_lease_heartbeat_at": task.lease_heartbeat_at,
            }),
        )
        .await?;
    }

    Ok(())
}

async fn is_task_cancel_requested(state: &AppState, task_id: &str) -> Result<bool> {
    let flag = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT cancel_requested
        FROM job_tasks
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(task_id)
    .fetch_optional(&state.pool)
    .await
    .context("failed to query cancel_requested")?;

    Ok(flag.unwrap_or(0) != 0)
}

fn payload_i64(payload: &Value, key: &str) -> Result<i64> {
    payload
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("payload missing integer field: {key}"))
}

fn payload_slot_hour_key(
    payload: &Value,
    task_created_at: Option<&str>,
    now_utc: DateTime<Utc>,
) -> String {
    if let Some(hour_key) = payload
        .get("hour_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
    {
        return hour_key;
    }

    let legacy_hour = payload
        .get("hour_utc")
        .and_then(Value::as_i64)
        .filter(|value| (0..=23).contains(value));
    let legacy_created_at = task_created_at
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));
    if let (Some(hour_utc), Some(created_at)) = (legacy_hour, legacy_created_at) {
        return format!("{}T{:02}", created_at.format("%Y-%m-%d"), hour_utc);
    }

    now_utc.format("%Y-%m-%dT%H").to_string()
}

fn payload_slot_reference_utc(
    payload: &Value,
    task_created_at: Option<&str>,
    now_utc: DateTime<Utc>,
) -> DateTime<Utc> {
    let hour_key = payload_slot_hour_key(payload, task_created_at, now_utc);
    let rfc3339_hour = format!("{hour_key}:00:00Z");
    DateTime::parse_from_rfc3339(&rfc3339_hour)
        .ok()
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(|| {
            now_utc
                .with_minute(0)
                .and_then(|value| value.with_second(0))
                .and_then(|value| value.with_nanosecond(0))
                .unwrap_or(now_utc)
        })
}

fn payload_local_id(payload: &Value, key: &str) -> Result<String> {
    let value = payload
        .get(key)
        .ok_or_else(|| anyhow!("payload missing field: {key}"))?;
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|id| id.to_string()))
        .ok_or_else(|| anyhow!("payload field {key} must be string"))
}

fn payload_string(payload: &Value, key: &str) -> Result<String> {
    let value = payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("payload missing string field: {key}"))?;
    Ok(value.to_owned())
}

fn payload_date(payload: &Value, key: &str) -> Result<Option<NaiveDate>> {
    let Some(raw) = payload.get(key).and_then(Value::as_str) else {
        return Ok(None);
    };
    let value = raw.trim();
    if value.is_empty() {
        return Ok(None);
    }
    Ok(Some(NaiveDate::parse_from_str(value, "%Y-%m-%d")?))
}

fn payload_i64_array(payload: &Value, key: &str) -> Result<Vec<i64>> {
    let values = payload
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("payload missing array field: {key}"))?;
    let mut result = Vec::with_capacity(values.len());
    for value in values {
        let Some(id) = value.as_i64() else {
            return Err(anyhow!("payload field {key} must be integer array"));
        };
        result.push(id);
    }
    Ok(result)
}

fn is_terminal_status(status: &str) -> bool {
    matches!(status, STATUS_SUCCEEDED | STATUS_FAILED | STATUS_CANCELED)
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, sync::Arc};

    use super::{
        STATUS_FAILED, STATUS_QUEUED, STATUS_RUNNING, TASK_BRIEF_DAILY_SLOT,
        TASK_BRIEF_HISTORY_RECOMPUTE, TASK_BRIEF_REFRESH_CONTENT,
        TASK_SUMMARIZE_RELEASE_SMART_BATCH, TASK_SYNC_ALL, TASK_SYNC_RELEASES,
        TASK_SYNC_SUBSCRIPTIONS, TranslationStreamCursor, claim_next_queued_task,
        current_subscription_schedule_key, enqueue_brief_history_recompute_if_needed,
        enqueue_brief_refresh_content_if_needed, enqueue_hour_slot_if_due,
        execute_brief_history_recompute_task, execute_brief_refresh_content_task,
        execute_daily_slot_task, execute_sync_all_task_with, is_scheduled_task_type,
        load_due_daily_slot_users, load_translation_stream_cursor, load_translation_stream_rows,
        next_llm_scheduler_stream_event, payload_slot_hour_key, payload_slot_reference_utc,
        recover_runtime_state, recover_runtime_state_on_startup,
    };
    use chrono::{TimeZone, Utc};
    use serde_json::{Value, json};
    use sqlx::{
        Row, SqlitePool,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };
    use url::Url;

    use crate::{
        config::{AiConfig, AppConfig, GitHubOAuthConfig},
        crypto::EncryptionKey,
        state::{AppState, build_oauth_client},
        sync,
    };

    #[test]
    fn current_subscription_schedule_key_uses_half_hour_buckets() {
        let on_the_hour = Utc
            .with_ymd_and_hms(2026, 3, 6, 14, 5, 12)
            .single()
            .expect("valid datetime");
        let on_the_half_hour = Utc
            .with_ymd_and_hms(2026, 3, 6, 14, 45, 59)
            .single()
            .expect("valid datetime");

        assert_eq!(
            current_subscription_schedule_key(on_the_hour),
            "2026-03-06T14:00"
        );
        assert_eq!(
            current_subscription_schedule_key(on_the_half_hour),
            "2026-03-06T14:30"
        );
    }

    #[test]
    fn payload_slot_hour_key_prefers_enqueued_hour_key() {
        let fallback_now = Utc
            .with_ymd_and_hms(2026, 4, 13, 10, 45, 0)
            .single()
            .expect("valid datetime");

        assert_eq!(
            payload_slot_hour_key(
                &json!({
                    "hour_utc": 9,
                    "hour_key": "2026-04-13T09",
                }),
                None,
                fallback_now,
            ),
            "2026-04-13T09"
        );
        assert_eq!(
            payload_slot_hour_key(&json!({ "hour_utc": 10 }), None, fallback_now),
            "2026-04-13T10"
        );
    }

    #[test]
    fn payload_slot_hour_key_reconstructs_legacy_slot_from_task_created_at() {
        let fallback_now = Utc
            .with_ymd_and_hms(2026, 4, 13, 10, 45, 0)
            .single()
            .expect("valid datetime");

        assert_eq!(
            payload_slot_hour_key(
                &json!({ "hour_utc": 9 }),
                Some("2026-04-12T09:00:03Z"),
                fallback_now,
            ),
            "2026-04-12T09"
        );
    }

    #[test]
    fn payload_slot_reference_utc_uses_enqueued_hour_key() {
        let fallback_now = Utc
            .with_ymd_and_hms(2026, 4, 13, 10, 45, 27)
            .single()
            .expect("valid datetime");

        assert_eq!(
            payload_slot_reference_utc(
                &json!({
                    "hour_utc": 9,
                    "hour_key": "2026-04-12T09",
                }),
                None,
                fallback_now,
            ),
            Utc.with_ymd_and_hms(2026, 4, 12, 9, 0, 0)
                .single()
                .expect("valid datetime")
        );
        assert_eq!(
            payload_slot_reference_utc(&json!({ "hour_utc": 10 }), None, fallback_now),
            Utc.with_ymd_and_hms(2026, 4, 13, 10, 0, 0)
                .single()
                .expect("valid datetime")
        );
    }

    #[test]
    fn payload_slot_reference_utc_uses_task_created_at_for_legacy_payload() {
        let fallback_now = Utc
            .with_ymd_and_hms(2026, 4, 13, 10, 45, 27)
            .single()
            .expect("valid datetime");

        assert_eq!(
            payload_slot_reference_utc(
                &json!({ "hour_utc": 9 }),
                Some("2026-04-12T09:00:03Z"),
                fallback_now,
            ),
            Utc.with_ymd_and_hms(2026, 4, 12, 9, 0, 0)
                .single()
                .expect("valid datetime")
        );
    }

    #[tokio::test]
    async fn enqueue_hour_slot_if_due_snapshots_due_users_into_payload() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let now = "2026-04-13T00:00:00Z";

        sqlx::query(
            r#"
            UPDATE daily_brief_hour_slots
            SET enabled = 1, last_dispatch_at = NULL, updated_at = ?
            WHERE hour_utc = 0
            "#,
        )
        .bind(now)
        .execute(&pool)
        .await
        .expect("enable midnight slot");

        sqlx::query(
            r#"
            INSERT INTO users (
              id, github_user_id, login,
              daily_brief_local_time, daily_brief_time_zone, daily_brief_utc_time,
              last_active_at, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("slot-user")
        .bind(1001_i64)
        .bind("slot-user")
        .bind("08:00")
        .bind("Asia/Shanghai")
        .bind("00:00")
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert slot user");

        let task_id = enqueue_hour_slot_if_due(
            state.as_ref(),
            Utc.with_ymd_and_hms(2026, 4, 13, 0, 0, 0)
                .single()
                .expect("valid datetime"),
        )
        .await
        .expect("enqueue slot task")
        .expect("task id");

        let payload = sqlx::query_scalar::<_, String>(
            r#"
            SELECT payload_json
            FROM job_tasks
            WHERE id = ?
            "#,
        )
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .expect("load slot task payload");
        let payload: Value = serde_json::from_str(&payload).expect("parse payload");
        let users = payload["users"].as_array().expect("users array");
        assert_eq!(users.len(), 1);
        assert_eq!(payload["hour_key"], json!("2026-04-13T00"));
        assert_eq!(users[0]["user_id"], json!("slot-user"));
        assert_eq!(users[0]["key_date"], json!("2026-04-13"));
        assert_eq!(users[0]["local_boundary"], json!("08:00"));
        assert_eq!(users[0]["effective_local_boundary"], json!("08:00"));
        assert_eq!(users[0]["time_zone"], json!("Asia/Shanghai"));
        assert_eq!(
            users[0]["window_start_utc"],
            json!("2026-04-12T00:00:00+00:00")
        );
        assert_eq!(
            users[0]["window_end_utc"],
            json!("2026-04-13T00:00:00+00:00")
        );
    }

    #[tokio::test]
    async fn enqueue_hour_slot_if_due_snapshots_resolved_dst_gap_boundary() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let now = "2026-03-08T00:00:00Z";

        sqlx::query(
            r#"
            UPDATE daily_brief_hour_slots
            SET enabled = 1,
                last_dispatch_at = NULL,
                updated_at = ?
            WHERE hour_utc = ?
            "#,
        )
        .bind(now)
        .bind(7_i64)
        .execute(&pool)
        .await
        .expect("reset dst slot");

        sqlx::query(
            r#"
            INSERT INTO users (
              id, github_user_id, login,
              daily_brief_local_time, daily_brief_time_zone, daily_brief_utc_time,
              last_active_at, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("slot-user-dst-gap")
        .bind(1003_i64)
        .bind("slot-user-dst-gap")
        .bind("02:00")
        .bind("America/New_York")
        .bind("07:00")
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert dst-gap user");

        let task_id = enqueue_hour_slot_if_due(
            state.as_ref(),
            Utc.with_ymd_and_hms(2026, 3, 8, 7, 0, 0)
                .single()
                .expect("valid datetime"),
        )
        .await
        .expect("enqueue dst-gap slot task")
        .expect("task id");

        let payload = sqlx::query_scalar::<_, String>(
            r#"
            SELECT payload_json
            FROM job_tasks
            WHERE id = ?
            "#,
        )
        .bind(&task_id)
        .fetch_one(&pool)
        .await
        .expect("load dst-gap slot task payload");
        let payload: Value = serde_json::from_str(&payload).expect("parse payload");
        let users = payload["users"].as_array().expect("users array");
        assert_eq!(users.len(), 1);
        assert_eq!(users[0]["local_boundary"], json!("02:00"));
        assert_eq!(users[0]["effective_local_boundary"], json!("03:00"));

        let due_users = load_due_daily_slot_users(
            state.as_ref(),
            &payload,
            Utc.with_ymd_and_hms(2026, 3, 8, 7, 0, 0)
                .single()
                .expect("valid datetime"),
            "2026-03-08T07",
        )
        .await
        .expect("load due users from payload");
        assert_eq!(due_users.len(), 1);
        assert_eq!(due_users[0].preferences.local_time.to_string(), "02:00:00");
        assert_eq!(due_users[0].window.effective_local_boundary, "03:00");
    }

    #[tokio::test]
    async fn execute_daily_slot_task_prefers_payload_snapshots_over_live_user_settings() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let now = "2026-04-13T00:00:00Z";

        sqlx::query(
            r#"
            INSERT INTO users (
              id, github_user_id, login,
              daily_brief_local_time, daily_brief_time_zone, daily_brief_utc_time,
              last_active_at, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("slot-user-live")
        .bind(1002_i64)
        .bind("slot-user-live")
        .bind("08:00")
        .bind("Asia/Shanghai")
        .bind("00:00")
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert live due user");

        seed_task(
            &pool,
            "task-daily-slot-payload-snapshots",
            TASK_BRIEF_DAILY_SLOT,
            STATUS_RUNNING,
            0,
        )
        .await;

        let result = execute_daily_slot_task(
            state.as_ref(),
            "task-daily-slot-payload-snapshots",
            &json!({
                "hour_utc": 0,
                "hour_key": "2026-04-13T00",
                "users": [],
            }),
        )
        .await
        .expect("execute daily slot task");

        assert_eq!(result["total"], json!(0));
        assert_eq!(result["succeeded"], json!(0));
        assert_eq!(result["failed"], json!(0));
    }

    #[test]
    fn scheduled_task_types_include_subscription_sync() {
        assert!(is_scheduled_task_type(TASK_BRIEF_DAILY_SLOT));
        assert!(is_scheduled_task_type(TASK_SYNC_SUBSCRIPTIONS));
        assert!(!is_scheduled_task_type("translate.release"));
        assert!(!is_scheduled_task_type(TASK_SUMMARIZE_RELEASE_SMART_BATCH));
    }

    #[test]
    fn next_llm_scheduler_stream_event_only_emits_when_max_concurrency_changes() {
        let changed_at = Utc
            .with_ymd_and_hms(2026, 3, 28, 10, 0, 0)
            .single()
            .expect("valid datetime");
        let unchanged_at = Utc
            .with_ymd_and_hms(2026, 3, 28, 10, 1, 0)
            .single()
            .expect("valid datetime");
        let mut last_max_concurrency = 2_i64;

        assert!(
            next_llm_scheduler_stream_event(
                &mut last_max_concurrency,
                crate::ai::LlmSchedulerRuntimeStatus {
                    max_concurrency: 2,
                    available_slots: 0,
                    waiting_calls: 3,
                    in_flight_calls: 2,
                },
                unchanged_at,
            )
            .is_none()
        );

        let payload = next_llm_scheduler_stream_event(
            &mut last_max_concurrency,
            crate::ai::LlmSchedulerRuntimeStatus {
                max_concurrency: 5,
                available_slots: 3,
                waiting_calls: 1,
                in_flight_calls: 2,
            },
            changed_at,
        )
        .expect("scheduler change should emit event");

        assert_eq!(last_max_concurrency, 5);
        assert_eq!(payload.max_concurrency, 5);
        assert_eq!(payload.available_slots, 3);
        assert_eq!(payload.waiting_calls, 1);
        assert_eq!(payload.in_flight_calls, 2);
        assert_eq!(payload.event_type, "llm.scheduler.updated");
        assert_eq!(payload.created_at, "2026-03-28T10:00:00+00:00");
    }

    #[tokio::test]
    async fn claim_next_queued_task_defers_subscription_when_one_is_running() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());

        seed_task(
            &pool,
            "sync-running",
            TASK_SYNC_SUBSCRIPTIONS,
            STATUS_RUNNING,
            0,
        )
        .await;
        seed_task(
            &pool,
            "sync-queued",
            TASK_SYNC_SUBSCRIPTIONS,
            STATUS_QUEUED,
            1,
        )
        .await;
        seed_task(
            &pool,
            "brief-queued",
            TASK_BRIEF_DAILY_SLOT,
            STATUS_QUEUED,
            2,
        )
        .await;

        let claimed = claim_next_queued_task(state.as_ref())
            .await
            .expect("claim queued task")
            .expect("task claimed");
        assert_eq!(claimed.id, "brief-queued");
        assert_eq!(claimed.task_type, TASK_BRIEF_DAILY_SLOT);
    }

    #[tokio::test]
    async fn claim_next_queued_task_allows_subscription_when_no_run_is_active() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());

        seed_task(
            &pool,
            "sync-queued",
            TASK_SYNC_SUBSCRIPTIONS,
            STATUS_QUEUED,
            0,
        )
        .await;

        let claimed = claim_next_queued_task(state.as_ref())
            .await
            .expect("claim queued task")
            .expect("task claimed");
        assert_eq!(claimed.id, "sync-queued");
        assert_eq!(claimed.task_type, TASK_SYNC_SUBSCRIPTIONS);
    }

    #[tokio::test]
    async fn recover_runtime_state_marks_stale_running_tasks_failed() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());

        seed_task(&pool, "stale-task", TASK_SYNC_RELEASES, STATUS_RUNNING, 0).await;
        sqlx::query(
            r#"
            UPDATE job_tasks
            SET runtime_owner_id = ?, lease_heartbeat_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind("old-runtime-owner")
        .bind("2026-03-06T00:00:00Z")
        .bind("2026-03-06T00:00:00Z")
        .bind("stale-task")
        .execute(&pool)
        .await
        .expect("mark task stale");

        recover_runtime_state(state.as_ref())
            .await
            .expect("recover runtime state");

        let row = sqlx::query(
            r#"
            SELECT status, error_message, runtime_owner_id, lease_heartbeat_at
            FROM job_tasks
            WHERE id = ?
            "#,
        )
        .bind("stale-task")
        .fetch_one(&pool)
        .await
        .expect("load recovered task");

        assert_eq!(row.get::<String, _>("status"), STATUS_FAILED);
        assert_eq!(
            row.get::<Option<String>, _>("error_message").as_deref(),
            Some(crate::runtime::RUNTIME_LEASE_EXPIRED_ERROR)
        );
        assert_eq!(row.get::<Option<String>, _>("runtime_owner_id"), None);
        assert_eq!(row.get::<Option<String>, _>("lease_heartbeat_at"), None);

        let event_type = sqlx::query_scalar::<_, String>(
            r#"
            SELECT event_type
            FROM job_task_events
            WHERE task_id = ?
            ORDER BY rowid DESC
            LIMIT 1
            "#,
        )
        .bind("stale-task")
        .fetch_one(&pool)
        .await
        .expect("load recovery event");
        assert_eq!(event_type, "task.recovered_failed");
    }

    #[tokio::test]
    async fn recover_runtime_state_keeps_live_current_owner_tasks_running() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());

        seed_task(&pool, "live-task", TASK_SYNC_RELEASES, STATUS_RUNNING, 0).await;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            UPDATE job_tasks
            SET runtime_owner_id = ?, lease_heartbeat_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(state.runtime_owner_id.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .bind("live-task")
        .execute(&pool)
        .await
        .expect("mark task live");

        recover_runtime_state(state.as_ref())
            .await
            .expect("recover runtime state");

        let status =
            sqlx::query_scalar::<_, String>(r#"SELECT status FROM job_tasks WHERE id = ?"#)
                .bind("live-task")
                .fetch_one(&pool)
                .await
                .expect("load live task status");
        assert_eq!(status, STATUS_RUNNING);
    }

    #[tokio::test]
    async fn recover_runtime_state_keeps_live_foreign_owner_tasks_running() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());

        seed_task(
            &pool,
            "foreign-live-task",
            TASK_SYNC_RELEASES,
            STATUS_RUNNING,
            0,
        )
        .await;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            UPDATE job_tasks
            SET runtime_owner_id = ?, lease_heartbeat_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind("other-runtime-owner")
        .bind(now.as_str())
        .bind(now.as_str())
        .bind("foreign-live-task")
        .execute(&pool)
        .await
        .expect("mark foreign-owner task live");

        recover_runtime_state(state.as_ref())
            .await
            .expect("recover runtime state");

        let row = sqlx::query(
            r#"
            SELECT status, runtime_owner_id, lease_heartbeat_at
            FROM job_tasks
            WHERE id = ?
            "#,
        )
        .bind("foreign-live-task")
        .fetch_one(&pool)
        .await
        .expect("load foreign-owner task status");

        assert_eq!(row.get::<String, _>("status"), STATUS_RUNNING);
        assert_eq!(
            row.get::<Option<String>, _>("runtime_owner_id").as_deref(),
            Some("other-runtime-owner")
        );
        assert_eq!(
            row.get::<Option<String>, _>("lease_heartbeat_at")
                .as_deref(),
            Some(now.as_str())
        );
    }

    #[tokio::test]
    async fn recover_runtime_state_on_startup_reclaims_foreign_owner_tasks_without_live_owner_lease()
     {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());

        seed_task(
            &pool,
            "startup-foreign-task",
            TASK_SYNC_RELEASES,
            STATUS_RUNNING,
            0,
        )
        .await;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            UPDATE job_tasks
            SET runtime_owner_id = ?, lease_heartbeat_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind("other-runtime-owner")
        .bind(now.as_str())
        .bind(now.as_str())
        .bind("startup-foreign-task")
        .execute(&pool)
        .await
        .expect("mark startup foreign-owner task live");

        recover_runtime_state_on_startup(state.as_ref())
            .await
            .expect("startup recover runtime state");

        let row = sqlx::query(
            r#"
            SELECT status, error_message, runtime_owner_id, lease_heartbeat_at
            FROM job_tasks
            WHERE id = ?
            "#,
        )
        .bind("startup-foreign-task")
        .fetch_one(&pool)
        .await
        .expect("load recovered startup task");

        assert_eq!(row.get::<String, _>("status"), STATUS_FAILED);
        assert_eq!(
            row.get::<Option<String>, _>("error_message").as_deref(),
            Some(crate::runtime::RUNTIME_LEASE_EXPIRED_ERROR)
        );
        assert_eq!(row.get::<Option<String>, _>("runtime_owner_id"), None);
        assert_eq!(row.get::<Option<String>, _>("lease_heartbeat_at"), None);
    }

    #[tokio::test]
    async fn recover_runtime_state_on_startup_keeps_live_foreign_owner_tasks_running() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());

        seed_task(
            &pool,
            "startup-live-foreign-task",
            TASK_SYNC_RELEASES,
            STATUS_RUNNING,
            0,
        )
        .await;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            UPDATE job_tasks
            SET runtime_owner_id = ?, lease_heartbeat_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind("other-runtime-owner")
        .bind(now.as_str())
        .bind(now.as_str())
        .bind("startup-live-foreign-task")
        .execute(&pool)
        .await
        .expect("mark startup foreign-owner task live");
        crate::runtime::upsert_runtime_owner_for_tests(
            state.as_ref(),
            "other-runtime-owner",
            now.as_str(),
        )
        .await
        .expect("upsert runtime owner");

        recover_runtime_state_on_startup(state.as_ref())
            .await
            .expect("startup recover runtime state");

        let row = sqlx::query(
            r#"
            SELECT status, runtime_owner_id, lease_heartbeat_at
            FROM job_tasks
            WHERE id = ?
            "#,
        )
        .bind("startup-live-foreign-task")
        .fetch_one(&pool)
        .await
        .expect("load startup live foreign task");

        assert_eq!(row.get::<String, _>("status"), STATUS_RUNNING);
        assert_eq!(
            row.get::<Option<String>, _>("runtime_owner_id").as_deref(),
            Some("other-runtime-owner")
        );
        assert_eq!(
            row.get::<Option<String>, _>("lease_heartbeat_at")
                .as_deref(),
            Some(now.as_str())
        );
    }

    #[tokio::test]
    async fn translation_stream_cursor_defaults_for_empty_tables() {
        let pool = setup_pool().await;

        let cursor = load_translation_stream_cursor(&pool, "translation_requests")
            .await
            .expect("load empty translation cursor");

        assert_eq!(cursor, TranslationStreamCursor::default());
    }

    #[tokio::test]
    async fn translation_stream_rows_follow_updated_at_and_id_cursor() {
        let pool = setup_pool().await;
        seed_user(&pool, 1, "octo").await;
        seed_translation_request(&pool, "req-1", "queued", "2026-03-07T00:00:00Z").await;
        seed_translation_request(&pool, "req-2", "running", "2026-03-07T00:00:00Z").await;
        seed_translation_request(&pool, "req-3", "completed", "2026-03-07T00:00:01Z").await;

        let rows = load_translation_stream_rows(
            &pool,
            "translation_requests",
            &TranslationStreamCursor {
                updated_at: "2026-03-07T00:00:00Z".to_owned(),
                entity_id: "req-1".to_owned(),
            },
        )
        .await
        .expect("load translation rows from cursor");

        let ids = rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>();
        let statuses = rows
            .iter()
            .map(|row| row.status.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["req-2", "req-3"]);
        assert_eq!(statuses, vec!["running", "completed"]);
    }

    #[tokio::test]
    async fn execute_sync_all_task_continues_notifications_when_social_sync_fails() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_task(
            &pool,
            "task-sync-all-social-fail",
            TASK_SYNC_ALL,
            STATUS_RUNNING,
            0,
        )
        .await;

        let result = execute_sync_all_task_with(
            state.as_ref(),
            "task-sync-all-social-fail",
            "1",
            |_, _| Box::pin(async { Ok(sync::SyncStarredResult { repos: 4 }) }),
            |_, _| {
                Box::pin(async {
                    Ok(sync::SyncReleasesResult {
                        repos: 2,
                        releases: 5,
                    })
                })
            },
            |_, _| Box::pin(async { Err(anyhow::anyhow!("social unavailable")) }),
            |_, _| {
                Box::pin(async {
                    Ok(sync::SyncNotificationsResult {
                        notifications: 7,
                        since: Some("2026-03-07T00:00:00Z".to_owned()),
                    })
                })
            },
        )
        .await
        .expect("sync_all result");

        assert_eq!(result["starred"]["repos"], json!(4));
        assert_eq!(result["releases"]["releases"], json!(5));
        assert_eq!(result["social"]["events"], json!(0));
        assert_eq!(result["notifications"]["notifications"], json!(7));
        assert_eq!(result["social_error"], json!("social unavailable"));
    }

    async fn setup_pool() -> SqlitePool {
        let database_path = std::env::temp_dir().join(format!(
            "octo-rill-test-{}.db",
            crate::local_id::generate_local_id(),
        ));
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("create sqlite memory db");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");
        pool
    }

    fn setup_state(pool: SqlitePool) -> Arc<AppState> {
        let encryption_key =
            EncryptionKey::from_base64("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
                .expect("build encryption key");
        let config = AppConfig {
            bind_addr: "127.0.0.1:58090"
                .parse::<SocketAddr>()
                .expect("parse bind addr"),
            public_base_url: Url::parse("http://127.0.0.1:58090").expect("parse public base url"),
            database_url: "sqlite::memory:".to_owned(),
            static_dir: None,
            task_log_dir: std::env::temp_dir().join("octo-rill-task-logs-jobs-tests"),
            job_worker_concurrency: 4,
            encryption_key: encryption_key.clone(),
            github: GitHubOAuthConfig {
                client_id: "test-client-id".to_owned(),
                client_secret: "test-client-secret".to_owned(),
                redirect_url: Url::parse("http://127.0.0.1:58090/auth/callback")
                    .expect("parse github redirect"),
            },
            ai: None,
            ai_max_concurrency: 1,
            ai_daily_at_local: None,
            app_default_time_zone: crate::briefs::DEFAULT_DAILY_BRIEF_TIME_ZONE.to_owned(),
        };
        let oauth = build_oauth_client(&config).expect("build oauth client");
        Arc::new(AppState {
            llm_scheduler: Arc::new(crate::ai::LlmScheduler::new(config.ai_max_concurrency)),
            translation_scheduler: Arc::new(
                crate::translations::TranslationSchedulerController::new(
                    crate::translations::TranslationRuntimeConfig::default(),
                ),
            ),
            config,
            pool,
            http: reqwest::Client::new(),
            oauth,
            encryption_key,
            runtime_owner_id: "jobs-test-runtime-owner".to_owned(),
        })
    }

    async fn seed_user(pool: &SqlitePool, id: i64, login: &str) {
        let now = "2026-03-07T00:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO users (id, github_user_id, login, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(id.to_string())
        .bind(40_000_000_i64 + id)
        .bind(login)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .expect("seed user");
    }

    async fn seed_translation_request(
        pool: &SqlitePool,
        request_id: &str,
        status: &str,
        updated_at: &str,
    ) {
        let started_at = match status {
            "queued" => None,
            _ => Some(updated_at),
        };
        let finished_at = match status {
            "completed" | "failed" => Some(updated_at),
            _ => None,
        };
        let result_status = match status {
            "completed" => Some("ready"),
            "failed" => Some("error"),
            _ => None,
        };
        let error_text = matches!(status, "failed").then_some("boom");

        sqlx::query(
            r#"
            INSERT INTO translation_requests (
              id, mode, source, request_origin, requested_by, scope_user_id, producer_ref, kind,
              variant, entity_id, target_lang, max_wait_ms, source_hash, source_blocks_json,
              target_slots_json, status, result_status, title_zh, summary_md, body_md, error_text,
              created_at, started_at, finished_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(request_id)
        .bind("async")
        .bind("feed.auto_translate")
        .bind("user")
        .bind(1_i64)
        .bind(1_i64)
        .bind(format!("feed.auto_translate:release:{request_id}"))
        .bind("release_summary")
        .bind("feed_card")
        .bind(request_id)
        .bind("zh-CN")
        .bind(1000_i64)
        .bind("hash")
        .bind("[]")
        .bind("[]")
        .bind(status)
        .bind(result_status)
        .bind(matches!(status, "completed").then_some("标题"))
        .bind(matches!(status, "completed").then_some("摘要"))
        .bind(Option::<&str>::None)
        .bind(error_text)
        .bind(updated_at)
        .bind(started_at)
        .bind(finished_at)
        .bind(updated_at)
        .execute(pool)
        .await
        .expect("seed translation request");
    }

    async fn seed_task(
        pool: &SqlitePool,
        task_id: &str,
        task_type: &str,
        status: &str,
        offset_seconds: i64,
    ) {
        let created_at = format!("2026-03-06T00:00:{offset_seconds:02}Z");
        sqlx::query(
            r#"
            INSERT INTO job_tasks (
              id,
              task_type,
              status,
              source,
              requested_by,
              parent_task_id,
              payload_json,
              result_json,
              error_message,
              cancel_requested,
              created_at,
              started_at,
              finished_at,
              updated_at,
              log_file_path
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(task_id)
        .bind(task_type)
        .bind(status)
        .bind("test")
        .bind(Option::<i64>::None)
        .bind(Option::<String>::None)
        .bind("{}")
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(0_i64)
        .bind(created_at.as_str())
        .bind((status == STATUS_RUNNING).then_some(created_at.as_str()))
        .bind(Option::<String>::None)
        .bind(created_at.as_str())
        .bind(Option::<String>::None)
        .execute(pool)
        .await
        .expect("seed task");
    }

    #[tokio::test]
    async fn enqueue_brief_history_recompute_preserves_legacy_snapshot_seed() {
        let pool = setup_pool().await;
        let mut state = setup_state(pool.clone());
        Arc::get_mut(&mut state)
            .expect("unique app state")
            .config
            .ai = Some(AiConfig {
            base_url: Url::parse("https://example.invalid/").expect("ai base url"),
            model: "test-model".to_owned(),
            api_key: "test-key".to_owned(),
        });
        let now = "2026-03-07T00:00:00Z";

        sqlx::query(
            r#"
            INSERT INTO users (
              id, github_user_id, login,
              daily_brief_utc_time, daily_brief_time_zone,
              created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("user-legacy-seed")
        .bind(1001_i64)
        .bind("legacy-seed")
        .bind("13:00")
        .bind("America/New_York")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert legacy user");

        sqlx::query(
            r#"
            INSERT INTO briefs (
              id, user_id, date,
              window_start_utc, window_end_utc,
              effective_time_zone, effective_local_boundary,
              generation_source, content_markdown, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("brief-legacy-seed")
        .bind("user-legacy-seed")
        .bind("2026-03-07")
        .bind("2026-03-06T13:00:00Z")
        .bind("2026-03-07T13:00:00Z")
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind("legacy")
        .bind("legacy body")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert legacy brief");

        let task_id = enqueue_brief_history_recompute_if_needed(state.as_ref())
            .await
            .expect("enqueue history recompute")
            .expect("task id");

        assert!(!task_id.is_empty());

        let normalized =
            sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>, Option<String>)>(
                r#"
                SELECT window_start_utc, window_end_utc, effective_time_zone, effective_local_boundary
                FROM briefs
                WHERE id = ?
                "#,
            )
            .bind("brief-legacy-seed")
            .fetch_one(&pool)
            .await
            .expect("load legacy brief");

        assert_eq!(normalized.0.as_deref(), Some("2026-03-06T13:00:00Z"));
        assert_eq!(normalized.1.as_deref(), Some("2026-03-07T13:00:00Z"));
        assert_eq!(normalized.2, None);
        assert_eq!(normalized.3, None);
    }

    #[tokio::test]
    async fn enqueue_brief_history_recompute_runs_even_when_ai_is_disabled() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let now = "2026-03-07T00:00:00Z";

        sqlx::query(
            r#"
            INSERT INTO users (
              id, github_user_id, login,
              daily_brief_utc_time, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("user-legacy-no-ai")
        .bind(1002_i64)
        .bind("legacy-no-ai")
        .bind("08:00")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert legacy user");

        sqlx::query(
            r#"
            INSERT INTO briefs (
              id, user_id, date,
              generation_source, content_markdown, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("brief-legacy-no-ai")
        .bind("user-legacy-no-ai")
        .bind("2026-03-07")
        .bind("legacy")
        .bind("legacy body")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert legacy brief");

        let task_id = enqueue_brief_history_recompute_if_needed(state.as_ref())
            .await
            .expect("enqueue history recompute without ai");

        assert!(task_id.is_some());

        let queued = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM job_tasks
            WHERE task_type = ?
            "#,
        )
        .bind(TASK_BRIEF_HISTORY_RECOMPUTE)
        .fetch_one(&pool)
        .await
        .expect("count history recompute tasks");

        assert_eq!(queued, 1);
    }

    #[tokio::test]
    async fn execute_brief_history_recompute_marks_failed_rows_out_of_legacy_bootstrap() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let now = "2026-03-07T00:00:00Z";

        sqlx::query(
            r#"
            INSERT INTO users (
              id, github_user_id, login,
              daily_brief_utc_time, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("user-legacy-failed-bootstrap")
        .bind(1003_i64)
        .bind("legacy-failed-bootstrap")
        .bind("08:00")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert legacy user");

        sqlx::query(
            r#"
            INSERT INTO briefs (
              id, user_id, date,
              generation_source, content_markdown, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("brief-legacy-failed-bootstrap")
        .bind("user-legacy-failed-bootstrap")
        .bind("2026-03-07")
        .bind("legacy")
        .bind("legacy body")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert legacy brief");

        let task_id = enqueue_brief_history_recompute_if_needed(state.as_ref())
            .await
            .expect("enqueue history recompute")
            .expect("task id");

        let err = execute_brief_history_recompute_task(state.as_ref(), &task_id)
            .await
            .expect_err("irrecoverable legacy brief should fail recompute");
        assert!(
            err.to_string()
                .contains("brief history recompute failed for all legacy briefs"),
            "{err:#}"
        );

        let generation_source = sqlx::query_scalar::<_, String>(
            r#"
            SELECT generation_source
            FROM briefs
            WHERE id = ?
            "#,
        )
        .bind("brief-legacy-failed-bootstrap")
        .fetch_one(&pool)
        .await
        .expect("load failed brief generation source");
        assert_eq!(generation_source, "history_recompute_failed");

        sqlx::query(
            r#"
            UPDATE job_tasks
            SET status = ?, finished_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(STATUS_FAILED)
        .bind(now)
        .bind(now)
        .bind(&task_id)
        .execute(&pool)
        .await
        .expect("mark original recompute task finished");

        let next_task_id = enqueue_brief_history_recompute_if_needed(state.as_ref())
            .await
            .expect("requeue bootstrap after failed rows are marked");

        assert!(next_task_id.is_some());
        assert_ne!(next_task_id.as_deref(), Some(task_id.as_str()));

        let queued = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM job_tasks
            WHERE task_type = ?
            "#,
        )
        .bind(TASK_BRIEF_HISTORY_RECOMPUTE)
        .fetch_one(&pool)
        .await
        .expect("count history recompute tasks");

        assert_eq!(queued, 2);
    }

    #[tokio::test]
    async fn enqueue_brief_refresh_content_if_needed_detects_outdated_normalized_briefs() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let now = "2026-03-07T00:00:00Z";

        seed_user(&pool, 1004, "brief-refresh").await;
        sqlx::query(
            r#"
            INSERT INTO briefs (
              id, user_id, date,
              window_start_utc, window_end_utc,
              effective_time_zone, effective_local_boundary,
              generation_source, content_markdown, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("brief-refresh-candidate")
        .bind("1004")
        .bind("2026-03-07")
        .bind("2026-03-06T00:00:00Z")
        .bind("2026-03-07T00:00:00Z")
        .bind("Asia/Shanghai")
        .bind("08:00")
        .bind("scheduled")
        .bind("## 概览\n\n- 旧格式\n")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert refresh candidate");

        let task_id = enqueue_brief_refresh_content_if_needed(state.as_ref())
            .await
            .expect("enqueue brief refresh")
            .expect("task id");

        assert!(!task_id.is_empty());
        let queued = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM job_tasks
            WHERE task_type = ?
            "#,
        )
        .bind(TASK_BRIEF_REFRESH_CONTENT)
        .fetch_one(&pool)
        .await
        .expect("count refresh tasks");
        assert_eq!(queued, 1);
    }

    #[tokio::test]
    async fn enqueue_brief_refresh_content_if_needed_skips_unrefreshable_time_zones() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let now = "2026-03-07T00:00:00Z";

        seed_user(&pool, 1014, "brief-refresh-unsupported-tz").await;
        sqlx::query(
            r#"
            INSERT INTO briefs (
              id, user_id, date,
              window_start_utc, window_end_utc,
              effective_time_zone, effective_local_boundary,
              generation_source, content_markdown, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("brief-refresh-unsupported-tz")
        .bind("1014")
        .bind("2026-03-07")
        .bind("2026-03-06T00:00:00Z")
        .bind("2026-03-07T00:00:00Z")
        .bind("Mars/Olympus")
        .bind("08:00")
        .bind("scheduled")
        .bind("## 概览\n\n- 旧格式\n")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert unsupported time zone candidate");

        let task_id = enqueue_brief_refresh_content_if_needed(state.as_ref())
            .await
            .expect("enqueue brief refresh");

        assert!(task_id.is_none());
        let queued = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM job_tasks
            WHERE task_type = ?
            "#,
        )
        .bind(TASK_BRIEF_REFRESH_CONTENT)
        .fetch_one(&pool)
        .await
        .expect("count refresh tasks");
        assert_eq!(queued, 0);
    }

    #[tokio::test]
    async fn enqueue_brief_refresh_content_if_needed_ignores_heading_text_inside_code_block() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let now = "2026-03-07T00:00:00Z";

        seed_user(&pool, 1016, "brief-refresh-code-fence").await;
        sqlx::query(
            r#"
            INSERT INTO briefs (
              id, user_id, date,
              window_start_utc, window_end_utc,
              effective_time_zone, effective_local_boundary,
              generation_source, content_markdown, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("brief-refresh-code-fence")
        .bind("1016")
        .bind("2026-03-07")
        .bind("2026-03-06T00:00:00Z")
        .bind("2026-03-07T00:00:00Z")
        .bind("Asia/Shanghai")
        .bind("08:00")
        .bind("scheduled")
        .bind(
            "## 项目更新\n\n```md\n## 概览\n```\n\n## 获星与关注\n\n### 获星\n\n- 本时间窗口内没有新的获星动态。\n\n### 关注\n\n- 本时间窗口内没有新的关注动态。\n",
        )
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert valid v2 brief with code block");

        let task_id = enqueue_brief_refresh_content_if_needed(state.as_ref())
            .await
            .expect("enqueue brief refresh");

        assert!(task_id.is_none());
        let queued = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM job_tasks
            WHERE task_type = ?
            "#,
        )
        .bind(TASK_BRIEF_REFRESH_CONTENT)
        .fetch_one(&pool)
        .await
        .expect("count refresh tasks");
        assert_eq!(queued, 0);
    }

    #[tokio::test]
    async fn execute_brief_refresh_content_task_updates_existing_snapshot_in_place() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let now = "2026-03-07T00:00:00Z";

        seed_user(&pool, 1005, "brief-refresh-run").await;
        sqlx::query(
            r#"
            UPDATE users
            SET daily_brief_utc_time = ?, daily_brief_local_time = ?, daily_brief_time_zone = ?
            WHERE id = ?
            "#,
        )
        .bind("00:00")
        .bind("08:00")
        .bind("Asia/Shanghai")
        .bind("1005")
        .execute(&pool)
        .await
        .expect("update brief refresh user prefs");

        sqlx::query(
            r#"
            INSERT INTO starred_repos (
              id, user_id, repo_id, full_name, owner_login, name,
              description, html_url, stargazed_at, is_private, updated_at,
              owner_avatar_url, open_graph_image_url, uses_custom_open_graph_image
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?)
            "#,
        )
        .bind("star-refresh-run")
        .bind("1005")
        .bind(1_i64)
        .bind("acme/rocket")
        .bind("acme")
        .bind("rocket")
        .bind(Option::<String>::None)
        .bind("https://github.com/acme/rocket")
        .bind(now)
        .bind(now)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(0_i64)
        .execute(&pool)
        .await
        .expect("insert starred repo");

        sqlx::query(
            r#"
            INSERT INTO repo_releases (
              id, repo_id, release_id, node_id, tag_name, name, body, html_url,
              published_at, created_at, is_prerelease, is_draft, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("repo-refresh-run")
        .bind(1_i64)
        .bind(501_i64)
        .bind("node-refresh-run")
        .bind("v1.0.0")
        .bind("v1.0.0")
        .bind("fix: tighten release brief formatting")
        .bind("https://github.com/acme/rocket/releases/tag/v1.0.0")
        .bind("2026-03-06T12:00:00Z")
        .bind("2026-03-06T12:00:00Z")
        .bind(0_i64)
        .bind(0_i64)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert repo release");

        sqlx::query(
            r#"
            INSERT INTO repo_releases (
              id, repo_id, release_id, node_id, tag_name, name, body, html_url,
              published_at, created_at, is_prerelease, is_draft, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("repo-refresh-stale")
        .bind(1_i64)
        .bind(999_i64)
        .bind("node-refresh-stale")
        .bind("v0.9.0")
        .bind("v0.9.0")
        .bind("old release")
        .bind("https://github.com/acme/rocket/releases/tag/v0.9.0")
        .bind("2026-03-05T12:00:00Z")
        .bind("2026-03-05T12:00:00Z")
        .bind(0_i64)
        .bind(0_i64)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert stale repo release");

        sqlx::query(
            r#"
            INSERT INTO social_activity_events (
              id, user_id, kind, repo_id, repo_full_name, actor_github_user_id,
              actor_login, actor_avatar_url, actor_html_url, occurred_at, detected_at, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("social-refresh-run")
        .bind("1005")
        .bind("follower_received")
        .bind(Option::<i64>::None)
        .bind(Option::<String>::None)
        .bind(7001_i64)
        .bind("alice")
        .bind(Option::<String>::None)
        .bind("https://github.com/alice")
        .bind("2026-03-06T18:00:00Z")
        .bind("2026-03-06T18:00:00Z")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert social event");

        sqlx::query(
            r#"
            INSERT INTO briefs (
              id, user_id, date,
              window_start_utc, window_end_utc,
              effective_time_zone, effective_local_boundary,
              generation_source, content_markdown, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("brief-refresh-existing")
        .bind("1005")
        .bind("2026-03-07")
        .bind("2026-03-06T00:00:00Z")
        .bind("2026-03-07T00:00:00Z")
        .bind("Asia/Shanghai")
        .bind("08:00")
        .bind("scheduled")
        .bind("```markdown\n## 概览\n\n- 旧日报\n```")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert existing brief");

        sqlx::query(
            r#"
            INSERT INTO brief_release_memberships (
              brief_id, release_id, release_ts_utc, ordinal, created_at
            )
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind("brief-refresh-existing")
        .bind(999_i64)
        .bind("2026-03-06T08:00:00Z")
        .bind(0_i64)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert stale membership");

        let task_id = enqueue_brief_refresh_content_if_needed(state.as_ref())
            .await
            .expect("enqueue refresh")
            .expect("task id");

        let result = execute_brief_refresh_content_task(state.as_ref(), &task_id)
            .await
            .expect("execute refresh task");
        assert_eq!(result["succeeded"].as_u64(), Some(1));
        assert_eq!(result["failed"].as_u64(), Some(0));

        let row = sqlx::query_as::<_, (String, String, String)>(
            r#"
            SELECT id, generation_source, content_markdown
            FROM briefs
            WHERE id = ?
            "#,
        )
        .bind("brief-refresh-existing")
        .fetch_one(&pool)
        .await
        .expect("load refreshed brief");
        assert_eq!(row.0, "brief-refresh-existing");
        assert_eq!(row.1, "content_refresh");
        assert!(row.2.contains("## 项目更新"));
        assert!(row.2.contains("## 获星与关注"));
        assert!(!row.2.contains("## 概览"));
        assert!(!row.2.trim_start().starts_with("```"));
        assert!(row.2.contains("[v1.0.0](/?tab=briefs&release=501)"));
        assert!(!row.2.contains("release=999"));
        assert!(row.2.contains("[@alice](https://github.com/alice)"));

        let memberships = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT release_id
            FROM brief_release_memberships
            WHERE brief_id = ?
            ORDER BY ordinal ASC
            "#,
        )
        .bind("brief-refresh-existing")
        .fetch_all(&pool)
        .await
        .expect("load refreshed memberships");
        assert_eq!(memberships, vec![501]);
    }

    #[tokio::test]
    async fn execute_brief_refresh_content_task_keeps_failed_candidates_retryable() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let now = "2026-03-07T00:00:00Z";

        seed_user(&pool, 1015, "brief-refresh-failed").await;
        sqlx::query(
            r#"
            INSERT INTO briefs (
              id, user_id, date,
              window_start_utc, window_end_utc,
              effective_time_zone, effective_local_boundary,
              generation_source, content_markdown, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("brief-refresh-invalid-window")
        .bind("1015")
        .bind("2026-03-07")
        .bind("not-a-rfc3339")
        .bind("2026-03-07T00:00:00Z")
        .bind("Asia/Shanghai")
        .bind("08:00")
        .bind("scheduled")
        .bind("## 概览\n\n- 旧格式\n")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert invalid refresh candidate");

        let task_id = enqueue_brief_refresh_content_if_needed(state.as_ref())
            .await
            .expect("enqueue refresh")
            .expect("task id");

        let err = execute_brief_refresh_content_task(state.as_ref(), &task_id)
            .await
            .expect_err("refresh should fail for invalid stored window");
        assert!(
            err.to_string()
                .contains("brief content refresh failed for all candidate briefs")
        );

        let generation_source = sqlx::query_scalar::<_, String>(
            r#"
            SELECT generation_source
            FROM briefs
            WHERE id = ?
            "#,
        )
        .bind("brief-refresh-invalid-window")
        .fetch_one(&pool)
        .await
        .expect("load failed refresh marker");
        assert_eq!(generation_source, "content_refresh_failed");

        let retry_task_id = enqueue_brief_refresh_content_if_needed(state.as_ref())
            .await
            .expect("re-enqueue refresh")
            .expect("retry task id");
        assert!(!retry_task_id.is_empty());
    }
}
