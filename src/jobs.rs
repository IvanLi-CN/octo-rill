use std::{convert::Infallible, sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow};
use async_stream::stream;
use axum::response::{
    IntoResponse, Response,
    sse::{Event, KeepAlive, Sse},
};
use chrono::{DateTime, NaiveTime, Timelike, Utc};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{ai, api, state::AppState, sync};

pub const STATUS_QUEUED: &str = "queued";
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_SUCCEEDED: &str = "succeeded";
pub const STATUS_FAILED: &str = "failed";
pub const STATUS_CANCELED: &str = "canceled";

pub const TASK_SYNC_STARRED: &str = "sync.starred";
pub const TASK_SYNC_RELEASES: &str = "sync.releases";
pub const TASK_SYNC_NOTIFICATIONS: &str = "sync.notifications";
pub const TASK_SYNC_ALL: &str = "sync.all";
pub const TASK_BRIEF_GENERATE: &str = "brief.generate";
pub const TASK_BRIEF_DAILY_SLOT: &str = "brief.daily_slot";
pub const TASK_TRANSLATE_RELEASE: &str = "translate.release";
pub const TASK_TRANSLATE_RELEASE_BATCH: &str = "translate.release.batch";
pub const TASK_TRANSLATE_RELEASE_DETAIL: &str = "translate.release_detail";
pub const TASK_TRANSLATE_NOTIFICATION: &str = "translate.notification";

#[derive(Debug, Clone)]
pub struct NewTask {
    pub task_type: String,
    pub payload: Value,
    pub source: String,
    pub requested_by: Option<i64>,
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
    payload_json: String,
    cancel_requested: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct SlotRow {
    enabled: i64,
    last_dispatch_at: Option<String>,
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

pub async fn enqueue_task(state: &AppState, new_task: NewTask) -> Result<EnqueuedTask> {
    let task_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let payload_json = serde_json::to_string(&new_task.payload).context("serialize payload")?;

    sqlx::query(
        r#"
        INSERT INTO job_tasks (
          id, task_type, status, source, requested_by, parent_task_id,
          payload_json, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&task_id)
    .bind(&new_task.task_type)
    .bind(STATUS_QUEUED)
    .bind(&new_task.source)
    .bind(new_task.requested_by)
    .bind(new_task.parent_task_id.as_deref())
    .bind(payload_json)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&state.pool)
    .await
    .context("failed to insert job task")?;

    append_task_event(
        state,
        &task_id,
        "task.created",
        json!({
            "task_id": task_id,
            "task_type": new_task.task_type,
            "status": STATUS_RUNNING,
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
    let task_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let payload_json = serde_json::to_string(&new_task.payload).context("serialize payload")?;

    sqlx::query(
        r#"
        INSERT INTO job_tasks (
          id, task_type, status, source, requested_by, parent_task_id,
          payload_json, created_at, started_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&task_id)
    .bind(&new_task.task_type)
    .bind(STATUS_RUNNING)
    .bind(&new_task.source)
    .bind(new_task.requested_by)
    .bind(new_task.parent_task_id.as_deref())
    .bind(payload_json)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&state.pool)
    .await
    .context("failed to insert inline task")?;

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
    requested_by: i64,
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
        INSERT INTO job_task_events (task_id, event_type, payload_json, created_at)
        VALUES (?, ?, ?, ?)
        "#,
    )
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
        let mut last_id = 0_i64;
        loop {
            #[derive(Debug, sqlx::FromRow)]
            struct EventRow {
                id: i64,
                event_type: String,
                payload_json: String,
            }

            let rows = sqlx::query_as::<_, EventRow>(
                r#"
                SELECT id, event_type, payload_json
                FROM job_task_events
                WHERE task_id = ? AND id > ?
                ORDER BY id ASC
                LIMIT 100
                "#,
            )
            .bind(&task_id)
            .bind(last_id)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

            for row in rows {
                last_id = row.id;
                yield Ok::<Event, Infallible>(
                    Event::default().event(row.event_type).data(row.payload_json),
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
                    SELECT id, event_type, payload_json
                    FROM job_task_events
                    WHERE task_id = ? AND id > ?
                    ORDER BY id ASC
                    LIMIT 100
                    "#,
                )
                .bind(&task_id)
                .bind(last_id)
                .fetch_all(&state.pool)
                .await
                .unwrap_or_default();

                for row in rows {
                    yield Ok::<Event, Infallible>(
                        Event::default().event(row.event_type).data(row.payload_json),
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
    event_id: i64,
    task_id: String,
    task_type: String,
    status: String,
    event_type: String,
    created_at: String,
}

pub fn admin_jobs_sse_response(state: Arc<AppState>) -> Response {
    let events = stream! {
        #[derive(Debug, sqlx::FromRow)]
        struct EventRow {
            id: i64,
            task_id: String,
            task_type: String,
            status: String,
            event_type: String,
            created_at: String,
        }

        let mut last_id = sqlx::query_scalar::<_, i64>(
            r#"SELECT COALESCE(MAX(id), 0) FROM job_task_events"#,
        )
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

        // Emit one lightweight frame immediately so proxies/browsers can
        // complete SSE handshake and update client connection state promptly.
        yield Ok::<Event, Infallible>(Event::default().comment("stream-ready"));

        loop {
            let rows = sqlx::query_as::<_, EventRow>(
                r#"
                SELECT
                  e.id,
                  e.task_id,
                  t.task_type,
                  t.status,
                  e.event_type,
                  e.created_at
                FROM job_task_events e
                JOIN job_tasks t ON t.id = e.task_id
                WHERE e.id > ?
                ORDER BY e.id ASC
                LIMIT 200
                "#,
            )
            .bind(last_id)
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

            for row in rows {
                last_id = row.id;
                let payload = AdminJobEventStreamItem {
                    event_id: row.id,
                    task_id: row.task_id,
                    task_type: row.task_type,
                    status: row.status,
                    event_type: row.event_type,
                    created_at: row.created_at,
                };
                let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_owned());
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .id(last_id.to_string())
                        .event("job.event")
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
        ORDER BY created_at ASC
        LIMIT 1
        "#,
    )
    .bind(STATUS_QUEUED)
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
        SET status = ?, started_at = ?, updated_at = ?
        WHERE id = ? AND status = ?
        "#,
    )
    .bind(STATUS_RUNNING)
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
        SELECT id, task_type, payload_json, cancel_requested
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

    let result = execute_task(state.as_ref(), &task.id, &task.task_type, &payload).await;

    if is_task_cancel_requested(state.as_ref(), &task.id)
        .await
        .unwrap_or(false)
    {
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
            let user_id = payload_i64(payload, "user_id")?;
            let res = sync::sync_starred(state, user_id).await?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_SYNC_RELEASES => {
            let user_id = payload_i64(payload, "user_id")?;
            let res = sync::sync_releases(state, user_id).await?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_SYNC_NOTIFICATIONS => {
            let user_id = payload_i64(payload, "user_id")?;
            let res = sync::sync_notifications(state, user_id).await?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_SYNC_ALL => {
            let user_id = payload_i64(payload, "user_id")?;
            let starred = sync::sync_starred(state, user_id).await?;
            if is_task_cancel_requested(state, task_id)
                .await
                .unwrap_or(false)
            {
                return Ok(json!({"canceled": true}));
            }
            let releases = sync::sync_releases(state, user_id).await?;
            if is_task_cancel_requested(state, task_id)
                .await
                .unwrap_or(false)
            {
                return Ok(json!({"canceled": true}));
            }
            let notifications = sync::sync_notifications(state, user_id).await?;
            Ok(json!({
                "starred": starred,
                "releases": releases,
                "notifications": notifications,
            }))
        }
        TASK_BRIEF_GENERATE => {
            let user_id = payload_i64(payload, "user_id")?;
            let content = ai::generate_daily_brief(state, user_id).await?;
            Ok(json!({"content_length": content.chars().count()}))
        }
        TASK_BRIEF_DAILY_SLOT => execute_daily_slot_task(state, task_id, payload).await,
        TASK_TRANSLATE_RELEASE => {
            let user_id = payload_i64(payload, "user_id")?;
            let release_id = payload_string(payload, "release_id")?;
            let res = api::translate_release_for_user(state, user_id, &release_id)
                .await
                .map_err(|err| anyhow!("translate_release failed: {}", err.code()))?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_TRANSLATE_RELEASE_BATCH => {
            let user_id = payload_i64(payload, "user_id")?;
            let release_ids = payload_i64_array(payload, "release_ids")?;
            let res = api::translate_releases_batch_for_user(state, user_id, &release_ids)
                .await
                .map_err(|err| anyhow!("translate_releases_batch failed: {}", err.code()))?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_TRANSLATE_RELEASE_DETAIL => {
            let user_id = payload_i64(payload, "user_id")?;
            let release_id = payload_string(payload, "release_id")?;
            let res = api::translate_release_detail_for_user(state, user_id, &release_id)
                .await
                .map_err(|err| anyhow!("translate_release_detail failed: {}", err.code()))?;
            Ok(serde_json::to_value(res).unwrap_or_else(|_| json!({"ok": true})))
        }
        TASK_TRANSLATE_NOTIFICATION => {
            let user_id = payload_i64(payload, "user_id")?;
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
        id: i64,
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

        match ai::generate_daily_brief_for_key_date_at(state, user.id, key_date, at).await {
            Ok(_) => {
                succeeded += 1;
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
        SET status = ?, result_json = ?, error_message = ?, finished_at = ?, updated_at = ?
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
