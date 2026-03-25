use std::{collections::HashMap, convert::Infallible, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow};
use async_stream::stream;
use axum::response::{
    IntoResponse, Response,
    sse::{Event, KeepAlive, Sse},
};
use chrono::{DateTime, NaiveTime, Timelike, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::{ai, api, local_id, runtime, state::AppState, sync, translations};

pub const STATUS_QUEUED: &str = "queued";
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_SUCCEEDED: &str = "succeeded";
pub const STATUS_FAILED: &str = "failed";
pub const STATUS_CANCELED: &str = "canceled";

pub const TASK_SYNC_STARRED: &str = "sync.starred";
pub const TASK_SYNC_RELEASES: &str = "sync.releases";
pub const TASK_SYNC_NOTIFICATIONS: &str = "sync.notifications";
pub const TASK_SYNC_ALL: &str = "sync.all";
pub const TASK_SYNC_SUBSCRIPTIONS: &str = "sync.subscriptions";
pub const TASK_BRIEF_GENERATE: &str = "brief.generate";
pub const TASK_BRIEF_DAILY_SLOT: &str = "brief.daily_slot";
pub const TASK_TRANSLATE_RELEASE: &str = "translate.release";
pub const TASK_TRANSLATE_RELEASE_BATCH: &str = "translate.release.batch";
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

const SUBSCRIPTION_SCHEDULE_NAME: &str = "sync.subscriptions";

pub fn is_scheduled_task_type(task_type: &str) -> bool {
    SCHEDULED_TASK_TYPES.contains(&task_type)
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

    let hour_key = now.format("%Y-%m-%dT%H").to_string();
    let already_dispatched = slot
        .last_dispatch_at
        .as_deref()
        .is_some_and(|value| value.starts_with(&hour_key));
    if already_dispatched {
        return Ok(None);
    }

    let task = enqueue_task(
        state,
        NewTask {
            task_type: TASK_BRIEF_DAILY_SLOT.to_owned(),
            payload: json!({ "hour_utc": hour_utc }),
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

            for worker in translations::translation_worker_runtime_statuses().await {
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
        TASK_SYNC_NOTIFICATIONS => {
            let user_id = payload_local_id(payload, "user_id")?;
            let res = sync::sync_notifications(state, user_id.as_str()).await?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_SYNC_ALL => {
            let user_id = payload_local_id(payload, "user_id")?;
            let starred = sync::sync_starred(state, user_id.as_str()).await?;
            if is_task_cancel_requested(state, task_id)
                .await
                .unwrap_or(false)
            {
                return Ok(json!({"canceled": true}));
            }
            let releases = sync::sync_releases(state, user_id.as_str()).await?;
            if is_task_cancel_requested(state, task_id)
                .await
                .unwrap_or(false)
            {
                return Ok(json!({"canceled": true}));
            }
            let notifications = sync::sync_notifications(state, user_id.as_str()).await?;
            Ok(json!({
                "starred": starred,
                "releases": releases,
                "notifications": notifications,
            }))
        }
        TASK_SYNC_SUBSCRIPTIONS => {
            let res = sync::sync_subscriptions(state, task_id, payload).await?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_BRIEF_GENERATE => {
            let user_id = payload_local_id(payload, "user_id")?;
            let content = ai::generate_daily_brief(state, user_id.as_str()).await?;
            Ok(json!({"content_length": content.chars().count()}))
        }
        TASK_BRIEF_DAILY_SLOT => execute_daily_slot_task(state, task_id, payload).await,
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

async fn execute_daily_slot_task(
    state: &AppState,
    task_id: &str,
    payload: &Value,
) -> Result<Value> {
    #[derive(Debug, sqlx::FromRow)]
    struct UserSlotRow {
        id: String,
        daily_brief_utc_time: String,
        last_active_at: Option<String>,
    }

    let hour_utc = payload_i64(payload, "hour_utc")?;

    let users = sqlx::query_as::<_, UserSlotRow>(
        r#"
        SELECT id, daily_brief_utc_time, last_active_at
        FROM users
        WHERE is_disabled = 0
          AND CAST(substr(daily_brief_utc_time, 1, 2) AS INTEGER) = ?
        ORDER BY
          CASE WHEN last_active_at IS NULL THEN 1 ELSE 0 END ASC,
          last_active_at DESC,
          id ASC
        "#,
    )
    .bind(hour_utc)
    .fetch_all(&state.pool)
    .await
    .context("failed to query users for daily slot")?;

    append_task_event(
        state,
        task_id,
        "task.progress",
        json!({
            "task_id": task_id,
            "stage": "collect",
            "hour_utc": hour_utc,
            "total_users": users.len(),
        }),
    )
    .await?;

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut canceled = false;

    for (index, user) in users.iter().enumerate() {
        if is_task_cancel_requested(state, task_id)
            .await
            .unwrap_or(false)
        {
            canceled = true;
            break;
        }

        let at =
            NaiveTime::parse_from_str(&user.daily_brief_utc_time, "%H:%M").unwrap_or_else(|_| {
                NaiveTime::from_hms_opt(hour_utc as u32, 0, 0)
                    .unwrap_or_else(|| NaiveTime::from_hms_opt(0, 0, 0).expect("00:00 valid"))
            });
        let key_date = key_date_for_boundary(Utc::now(), at);

        append_task_event(
            state,
            task_id,
            "task.progress",
            json!({
                "task_id": task_id,
                "stage": "generate",
                "index": index + 1,
                "total": users.len(),
                "user_id": user.id,
                "last_active_at": user.last_active_at,
                "key_date": key_date,
            }),
        )
        .await?;

        match ai::generate_daily_brief_for_key_date_at(state, user.id.as_str(), key_date, at).await
        {
            Ok(content) => {
                succeeded += 1;
                append_task_event(
                    state,
                    task_id,
                    "task.progress",
                    json!({
                        "task_id": task_id,
                        "stage": "user_succeeded",
                        "user_id": user.id,
                        "key_date": key_date,
                        "content_length": content.chars().count(),
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
                        "user_id": user.id,
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
            "total": users.len(),
            "succeeded": succeeded,
            "failed": failed,
            "canceled": canceled,
        }),
    )
    .await?;

    if canceled {
        return Ok(json!({
            "hour_utc": hour_utc,
            "total": users.len(),
            "succeeded": succeeded,
            "failed": failed,
            "canceled": true,
        }));
    }

    if failed > 0 && succeeded == 0 {
        return Err(anyhow!(
            "daily slot {hour_utc:02} failed for all users (failed={failed}, total={})",
            users.len()
        ));
    }

    Ok(json!({
        "hour_utc": hour_utc,
        "total": users.len(),
        "succeeded": succeeded,
        "failed": failed,
    }))
}

fn key_date_for_boundary(now: DateTime<Utc>, at: NaiveTime) -> chrono::NaiveDate {
    let today = now.date_naive();
    if now.time() >= at {
        today
    } else {
        today - chrono::Duration::days(1)
    }
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
                    OR runtime_owner_id != ?
                    OR lease_heartbeat_at IS NULL
                    OR julianday(lease_heartbeat_at) <= julianday(?)
                  )
                ORDER BY created_at ASC, id ASC
                "#,
            )
            .bind(STATUS_RUNNING)
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
        STATUS_FAILED, STATUS_QUEUED, STATUS_RUNNING, TASK_BRIEF_DAILY_SLOT, TASK_SYNC_RELEASES,
        TASK_SYNC_SUBSCRIPTIONS, TranslationStreamCursor, claim_next_queued_task,
        current_subscription_schedule_key, is_scheduled_task_type, load_translation_stream_cursor,
        load_translation_stream_rows, recover_runtime_state, recover_runtime_state_on_startup,
    };
    use chrono::{TimeZone, Utc};
    use sqlx::{
        Row, SqlitePool,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };
    use url::Url;

    use crate::{
        config::{AppConfig, GitHubOAuthConfig},
        crypto::EncryptionKey,
        state::{AppState, build_oauth_client},
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
    fn scheduled_task_types_include_subscription_sync() {
        assert!(is_scheduled_task_type(TASK_BRIEF_DAILY_SLOT));
        assert!(is_scheduled_task_type(TASK_SYNC_SUBSCRIPTIONS));
        assert!(!is_scheduled_task_type("translate.release"));
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
    async fn recover_runtime_state_on_startup_reclaims_live_foreign_owner_tasks() {
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
            ai_model_context_limit: None,
            ai_daily_at_local: None,
        };
        let oauth = build_oauth_client(&config).expect("build oauth client");
        Arc::new(AppState {
            llm_scheduler: Arc::new(crate::ai::LlmScheduler::new(config.ai_max_concurrency)),
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
}
