//! PostgreSQL administration mutations.
//!
//! These handlers are the PostgreSQL port of the legacy SQLite admin writes
//! (`src/api.rs`, `src/online_migrations.rs`, `src/content_processing.rs`,
//! `src/content_identity_upgrade.rs`, `src/search_index.rs`,
//! `src/translations.rs`). Every mutation keeps the legacy transaction
//! boundary and validation messages, but persists to the PostgreSQL tables
//! that the matching read handlers already serve.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, NaiveTime, Utc};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
use tower_sessions::Session;
use tracing::warn;

use crate::{
    briefs, local_id,
    postgres_server::{
        PostgresServerState, admin_identity_upgrade_status, admin_search_index_status,
        admin_translation_status, admin_user_profile, public_release_repos_body, require_admin,
    },
};

/// Legacy migration window for the recent-release reading backfill.
const RELEASE_READING_BACKFILL_WINDOW_DAYS: i64 = 30;
/// Task type and batch size of the release reading backfill worker.
const RELEASE_READING_BACKFILL_TASK: &str = "release.reading_backfill";
const RELEASE_READING_BACKFILL_BATCH_SIZE: i64 = 100;
/// Run states the backfill bootstrap treats as still resumable.
const RELEASE_READING_BACKFILL_ACTIVE_STATES: [&str; 5] =
    ["queued", "waiting", "paused", "running", "failed"];

/// Upper bound mirrored from `src/translations.rs`; the legacy module keeps
/// its copy private.
const MAX_TRANSLATION_WORKER_CONCURRENCY: usize = 64;

fn admin_required(status: StatusCode) -> Response {
    (status, Json(json!({"code": "admin_required"}))).into_response()
}

fn bad_request(message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"code": "bad_request", "message": message.into()})),
    )
        .into_response()
}

fn not_found(message: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"code": "not_found", "message": message})),
    )
        .into_response()
}

fn conflict(code: &str, message: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(json!({"code": code, "message": message})),
    )
        .into_response()
}

fn database_error(operation: &'static str, error: sqlx::Error) -> Response {
    warn!(%error, operation, "PostgreSQL admin mutation failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"code": "database_error"})),
    )
        .into_response()
}

/// Run `SELECT pg_advisory_xact_lock` so mode transitions serialize across
/// runtime processes; PostgreSQL has no legacy content-mode mutex.
async fn lock_content_mode(tx: &mut Transaction<'_, Postgres>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('octo-rill:content-processing-mode', 0))",
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn table_exists(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, bool>("SELECT to_regclass($1) IS NOT NULL")
        .bind(format!("public.{table}"))
        .fetch_one(&mut **tx)
        .await
}

// ---------------------------------------------------------------------------
// Public release repositories
// ---------------------------------------------------------------------------

/// Delete one public endpoint registration plus its now-unused release cache.
pub(crate) async fn admin_delete_public_release_repo(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(usage_id): Path<String>,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return admin_required(status);
    }
    let Some(usage_id) = local_id::normalize_local_id(&usage_id) else {
        return bad_request("invalid public_repo_usage_id");
    };

    let mut tx = match state.runtime.pool.begin().await {
        Ok(tx) => tx,
        Err(error) => return database_error("public_repo_delete_begin", error),
    };
    let usage = sqlx::query_as::<_, (Option<i64>, String)>(
        "SELECT repo_id, full_name FROM public.public_repo_release_usage WHERE id = $1",
    )
    .bind(&usage_id)
    .fetch_optional(&mut *tx)
    .await;
    let usage = match usage {
        Ok(Some(usage)) => usage,
        Ok(None) => return not_found("public repo usage not found"),
        Err(error) => return database_error("public_repo_usage_read", error),
    };
    let deleted = sqlx::query("DELETE FROM public.public_repo_release_usage WHERE id = $1")
        .bind(&usage_id)
        .execute(&mut *tx)
        .await;
    match deleted {
        Ok(result) if result.rows_affected() == 1 => {}
        Ok(_) => return not_found("public repo usage not found"),
        Err(error) => return database_error("public_repo_usage_delete", error),
    }
    let cleanup = match cleanup_public_release_repo_cache(&mut tx, usage.0, usage.1).await {
        Ok(cleanup) => cleanup,
        Err(error) => return database_error("public_repo_cache_cleanup", error),
    };
    if let Err(error) = tx.commit().await {
        return database_error("public_repo_delete_commit", error);
    }

    match public_release_repos_body(&state, 1, 20, "").await {
        Ok(mut body) => {
            body["cache_cleanup"] = cleanup;
            Json(body).into_response()
        }
        Err(error) => database_error("public_repo_list_after_delete", error),
    }
}

/// Drop cached releases for a repository that no endpoint needs anymore.
///
/// Mirrors `cleanup_public_release_repo_cache_if_unused_in_transaction`: any
/// remaining consumer keeps the cache intact and reports why.
async fn cleanup_public_release_repo_cache(
    tx: &mut Transaction<'_, Postgres>,
    repo_id: Option<i64>,
    full_name: String,
) -> Result<Value, sqlx::Error> {
    let skipped = |reason: &str| {
        json!({
            "repo_id": repo_id,
            "full_name": full_name,
            "deleted_release_count": 0_i64,
            "deleted_ai_cache_count": 0_u64,
            "skipped_reason": reason,
        })
    };
    let Some(repo_id) = repo_id else {
        return Ok(skipped("repo_not_resolved"));
    };

    let mode = sqlx::query_scalar::<_, String>(
        "SELECT mode FROM public.content_processing_control WHERE id = 1",
    )
    .fetch_optional(&mut **tx)
    .await?;
    if mode.as_deref().is_some_and(|mode| mode != "legacy") {
        return Ok(skipped("content_processing_transition"));
    }

    let remaining_public_usage = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.public_repo_release_usage WHERE repo_id = $1",
    )
    .bind(repo_id)
    .fetch_one(&mut **tx)
    .await?;
    if remaining_public_usage > 0 {
        return Ok(skipped("still_used_by_public_endpoint"));
    }

    let user_visible_usage = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.user_release_visible_repos WHERE repo_id = $1",
    )
    .bind(repo_id)
    .fetch_one(&mut **tx)
    .await?;
    if user_visible_usage > 0 {
        return Ok(skipped("still_used_by_user_release_visibility"));
    }

    let brief_usage = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM public.brief_release_memberships m JOIN public.repo_releases r ON r.release_id = m.release_id WHERE r.repo_id = $1",
    )
    .bind(repo_id)
    .fetch_one(&mut **tx)
    .await?;
    if brief_usage > 0 {
        return Ok(skipped("still_used_by_brief_membership"));
    }

    let release_ids = sqlx::query_scalar::<_, i64>(
        "SELECT release_id FROM public.repo_releases WHERE repo_id = $1",
    )
    .bind(repo_id)
    .fetch_all(&mut **tx)
    .await?;

    let mut deleted_ai_cache_count = 0_u64;
    let mut deleted_release_count = 0_i64;
    for release_id in release_ids {
        deleted_ai_cache_count += sqlx::query(
            "DELETE FROM public.ai_translations WHERE entity_type IN ('release_detail', 'release_smart') AND entity_id = $1",
        )
        .bind(release_id.to_string())
        .execute(&mut **tx)
        .await?
        .rows_affected();
        deleted_release_count +=
            sqlx::query("DELETE FROM public.repo_releases WHERE release_id = $1")
                .bind(release_id)
                .execute(&mut **tx)
                .await?
                .rows_affected() as i64;
    }
    sqlx::query("DELETE FROM public.repo_release_work_items WHERE repo_id = $1")
        .bind(repo_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM public.repo_release_sync_state WHERE repo_id = $1")
        .bind(repo_id)
        .execute(&mut **tx)
        .await?;

    Ok(json!({
        "repo_id": repo_id,
        "full_name": full_name,
        "deleted_release_count": deleted_release_count,
        "deleted_ai_cache_count": deleted_ai_cache_count,
        "skipped_reason": Value::Null,
    }))
}

// ---------------------------------------------------------------------------
// Release reading backfill
// ---------------------------------------------------------------------------

/// Start or resume the recent Release reading backfill through an explicit
/// admin action. Startup deliberately does not enqueue this job.
pub(crate) async fn admin_enqueue_release_reading_backfill(
    State(state): State<PostgresServerState>,
    session: Session,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return admin_required(status);
    }
    let task_id = match enqueue_release_reading_backfill_if_needed(&state.runtime.pool).await {
        Ok(task_id) => task_id,
        Err(error) => return database_error("release_reading_backfill_enqueue", error),
    };
    let status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM public.release_reading_backfill_runs ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .fetch_optional(&state.runtime.pool)
    .await;
    match status {
        Ok(status) => Json(json!({
            "task_id": task_id,
            "status": status.unwrap_or_else(|| {
                if task_id.is_some() {
                    "queued".to_owned()
                } else {
                    "idle".to_owned()
                }
            }),
        }))
        .into_response(),
        Err(error) => database_error("release_reading_backfill_status", error),
    }
}

/// Queue one page of the polishing audience-variant repair sweep.
///
/// Single-flight like the scheduled lane: while a `polish.backfill` task is
/// queued or running this returns that task instead of creating a second one, so
/// an operator clicking twice cannot double-advance the cursor. The executor
/// itself decides what the page is; this handler only enqueues.
pub(crate) async fn admin_enqueue_polish_backfill(
    State(state): State<PostgresServerState>,
    session: Session,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return admin_required(status);
    }
    let pool = &state.runtime.pool;
    let inflight = sqlx::query_scalar::<_, String>(
        "SELECT id FROM public.job_tasks WHERE task_type = $1 AND status IN ('queued', 'running') ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(crate::postgres_tasks::TASK_POLISH_BACKFILL)
    .fetch_optional(pool)
    .await;
    let task_id = match inflight {
        Ok(Some(task_id)) => Some(task_id),
        Ok(None) => {
            let enqueued = sqlx::query_scalar::<_, String>(
                r#"
                INSERT INTO public.job_tasks (
                  id, task_type, status, source, requested_by, payload_json, created_at, updated_at
                )
                SELECT $1, $2, 'queued', 'admin.polish_backfill', $3, $4, $5, $5
                WHERE NOT EXISTS (
                  SELECT 1
                  FROM public.job_tasks pending
                  WHERE pending.task_type = $2
                    AND pending.status IN ('queued', 'running')
                )
                RETURNING id
                "#,
            )
            .bind(crate::local_id::generate_local_id())
            .bind(crate::postgres_tasks::TASK_POLISH_BACKFILL)
            .bind(Option::<String>::None)
            .bind(json!({"trigger": "admin.polish_backfill"}).to_string())
            .bind(Utc::now())
            .fetch_optional(pool)
            .await;
            match enqueued {
                Ok(task_id) => task_id,
                Err(error) => return database_error("polish_backfill_enqueue", error),
            }
        }
        Err(error) => return database_error("polish_backfill_inflight", error),
    };
    let cursor = sqlx::query_scalar::<_, i64>(
        "SELECT last_release_id FROM public.polish_backfill_cursor WHERE id = 1",
    )
    .fetch_optional(pool)
    .await;
    match cursor {
        Ok(cursor) => Json(json!({
            "task_id": task_id,
            "status": if task_id.is_some() { "queued" } else { "idle" },
            "cursor": cursor.unwrap_or(0),
        }))
        .into_response(),
        Err(error) => database_error("polish_backfill_cursor", error),
    }
}

#[derive(Debug, sqlx::FromRow)]
struct BackfillRunRow {
    id: String,
    window_start_utc: String,
    window_end_utc: String,
    status: String,
    lease_owner: Option<String>,
    lease_task_id: Option<String>,
    lease_expires_at: Option<String>,
}

const BACKFILL_RUN_COLUMNS: &str =
    "id, window_start_utc, window_end_utc, status, lease_owner, lease_task_id, lease_expires_at";

async fn load_latest_backfill_run(pool: &PgPool) -> Result<Option<BackfillRunRow>, sqlx::Error> {
    sqlx::query_as::<_, BackfillRunRow>(&format!(
        "SELECT {BACKFILL_RUN_COLUMNS} FROM public.release_reading_backfill_runs ORDER BY created_at DESC, id DESC LIMIT 1"
    ))
    .fetch_optional(pool)
    .await
}

async fn inflight_backfill_task(pool: &PgPool) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM public.job_tasks WHERE task_type = $1 AND status IN ('queued', 'running') ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(RELEASE_READING_BACKFILL_TASK)
    .fetch_optional(pool)
    .await
}

/// Freeze the first recent 30-day release set once, then let the task worker
/// consume it. The run table is separate from `job_tasks` so a restart can
/// recover the window and counters without moving the boundary.
///
/// PostgreSQL has no `release.reading_backfill` executor registered, so the
/// queued task stays queued until one exists; the run row and its frozen
/// window are persisted exactly like the legacy runtime.
async fn enqueue_release_reading_backfill_if_needed(
    pool: &PgPool,
) -> Result<Option<String>, sqlx::Error> {
    for _ in 0..3 {
        let Some(mut run) = load_latest_backfill_run(pool).await? else {
            return seed_backfill_run(pool).await;
        };
        if !RELEASE_READING_BACKFILL_ACTIVE_STATES.contains(&run.status.as_str()) {
            return Ok(None);
        }
        match run.status.as_str() {
            "waiting" | "paused" => {
                resume_waiting_backfill_run(pool, &run).await?;
                run.status = "queued".to_owned();
            }
            "failed" => {
                if !retry_failed_backfill_run(pool, &run).await? {
                    continue;
                }
                run.status = "queued".to_owned();
                run.lease_owner = None;
                run.lease_task_id = None;
                run.lease_expires_at = None;
            }
            "running" => {
                if let Some(task_id) = inflight_backfill_task(pool).await? {
                    return Ok(Some(task_id));
                }
                let lease_active = run
                    .lease_expires_at
                    .as_deref()
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                    .is_some_and(|expires_at| expires_at.with_timezone(&Utc) > Utc::now());
                if lease_active {
                    return Ok(None);
                }
                if !recover_stale_backfill_lease(pool, &run).await? {
                    continue;
                }
                run.status = "queued".to_owned();
            }
            _ => {}
        }
        return ensure_backfill_task(pool, &run).await;
    }
    Ok(None)
}

async fn seed_backfill_run(pool: &PgPool) -> Result<Option<String>, sqlx::Error> {
    let now = Utc::now();
    let started_at = now.to_rfc3339();
    let window_start_utc =
        (now - chrono::Duration::days(RELEASE_READING_BACKFILL_WINDOW_DAYS)).to_rfc3339();
    let window_end_utc = started_at.clone();
    let run_id = local_id::generate_local_id();

    let mut tx = pool.begin().await?;
    let concurrent =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM public.release_reading_backfill_runs")
            .fetch_one(&mut *tx)
            .await?;
    if concurrent > 0 {
        tx.rollback().await?;
        return Box::pin(enqueue_release_reading_backfill_if_needed(pool)).await;
    }
    sqlx::query(
        r#"
        INSERT INTO public.release_reading_backfill_runs (
          id, started_at, window_start_utc, window_end_utc, status,
          total, succeeded, failed, waiting, skipped, created_at, updated_at
        ) VALUES ($1, $2, $3, $4, 'queued', 0, 0, 0, 0, 0, $2, $2)
        "#,
    )
    .bind(&run_id)
    .bind(&started_at)
    .bind(&window_start_utc)
    .bind(&window_end_utc)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO public.release_reading_backfill_items (
          run_id, release_id, authorized_user_id, published_at, status, attempt_count, updated_at
        )
        SELECT
          $1, r.release_id,
          (
            SELECT v.user_id
            FROM public.user_release_visible_repos v
            JOIN public.users u ON u.id = v.user_id
            WHERE v.repo_id = r.repo_id
              AND u.is_disabled = 0
              AND u.paused_at IS NULL
            ORDER BY v.user_id ASC
            LIMIT 1
          ),
          COALESCE(r.published_at, r.created_at, r.detected_at),
          'pending', 0, $2
        FROM public.repo_releases r
        WHERE COALESCE(r.is_draft, 0) = 0
          AND COALESCE(r.published_at, r.created_at, r.detected_at) IS NOT NULL
          AND COALESCE(r.published_at, r.created_at, r.detected_at) >= $3
          AND COALESCE(r.published_at, r.created_at, r.detected_at) < $4
          AND EXISTS (
            SELECT 1
            FROM public.user_release_visible_repos v
            WHERE v.repo_id = r.repo_id
          )
        ORDER BY COALESCE(r.published_at, r.created_at, r.detected_at) ASC,
                 r.release_id ASC
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(&run_id)
    .bind(&started_at)
    .bind(&window_start_utc)
    .bind(&window_end_utc)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"
        UPDATE public.release_reading_backfill_runs
        SET total = (SELECT COUNT(*) FROM public.release_reading_backfill_items WHERE run_id = $1),
            updated_at = $2
        WHERE id = $1
        "#,
    )
    .bind(&run_id)
    .bind(&started_at)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    ensure_backfill_task(
        pool,
        &BackfillRunRow {
            id: run_id,
            window_start_utc,
            window_end_utc,
            status: "queued".to_owned(),
            lease_owner: None,
            lease_task_id: None,
            lease_expires_at: None,
        },
    )
    .await
}

async fn resume_waiting_backfill_run(
    pool: &PgPool,
    run: &BackfillRunRow,
) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE public.release_reading_backfill_items SET status = 'pending', error_text = NULL, updated_at = $1 WHERE run_id = $2 AND status = 'waiting'",
    )
    .bind(&now)
    .bind(&run.id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        if run.status == "paused" {
            r#"
            UPDATE public.release_reading_backfill_runs
            SET status = 'queued',
                waiting = (SELECT COUNT(*) FROM public.release_reading_backfill_items WHERE run_id = $1 AND status = 'waiting'),
                error_text = NULL,
                updated_at = $2
            WHERE id = $1 AND status = 'paused'
            "#
        } else {
            r#"
            UPDATE public.release_reading_backfill_runs
            SET status = 'queued',
                waiting = (SELECT COUNT(*) FROM public.release_reading_backfill_items WHERE run_id = $1 AND status = 'waiting'),
                updated_at = $2
            WHERE id = $1 AND status = 'waiting'
            "#
        },
    )
    .bind(&run.id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

async fn retry_failed_backfill_run(
    pool: &PgPool,
    run: &BackfillRunRow,
) -> Result<bool, sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE public.release_reading_backfill_items SET status = 'pending', error_text = NULL, updated_at = $1 WHERE run_id = $2 AND status IN ('failed', 'waiting')",
    )
    .bind(&now)
    .bind(&run.id)
    .execute(&mut *tx)
    .await?;
    let updated = sqlx::query(
        r#"
        UPDATE public.release_reading_backfill_runs
        SET status = 'queued',
            failed = (SELECT COUNT(*) FROM public.release_reading_backfill_items WHERE run_id = $1 AND status = 'failed'),
            waiting = (SELECT COUNT(*) FROM public.release_reading_backfill_items WHERE run_id = $1 AND status = 'waiting'),
            error_text = NULL,
            lease_owner = NULL,
            lease_task_id = NULL,
            lease_expires_at = NULL,
            updated_at = $2
        WHERE id = $1 AND status = 'failed'
        "#,
    )
    .bind(&run.id)
    .bind(&now)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if updated == 0 {
        tx.rollback().await?;
        return Ok(false);
    }
    tx.commit().await?;
    Ok(true)
}

async fn recover_stale_backfill_lease(
    pool: &PgPool,
    run: &BackfillRunRow,
) -> Result<bool, sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    let mut tx = pool.begin().await?;
    let recovered = sqlx::query(
        "UPDATE public.release_reading_backfill_runs SET status = 'queued', lease_owner = NULL, lease_task_id = NULL, lease_expires_at = NULL, error_text = NULL, updated_at = $1 WHERE id = $2 AND status = 'running' AND lease_owner IS NOT DISTINCT FROM $3 AND lease_task_id IS NOT DISTINCT FROM $4 AND lease_expires_at IS NOT DISTINCT FROM $5",
    )
    .bind(&now)
    .bind(&run.id)
    .bind(run.lease_owner.as_deref())
    .bind(run.lease_task_id.as_deref())
    .bind(run.lease_expires_at.as_deref())
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if recovered == 0 {
        tx.rollback().await?;
        return Ok(false);
    }
    tx.commit().await?;
    Ok(true)
}

async fn ensure_backfill_task(
    pool: &PgPool,
    run: &BackfillRunRow,
) -> Result<Option<String>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    // Legacy serializes this enqueue with `task_singleton_enqueue_lock`; the
    // advisory lock gives the same single-flight guarantee across processes.
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('octo-rill:release-reading-backfill-task', 0))",
    )
    .execute(&mut *tx)
    .await?;
    if let Some(task_id) = sqlx::query_scalar::<_, String>(
        "SELECT id FROM public.job_tasks WHERE task_type = $1 AND status IN ('queued', 'running') ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(RELEASE_READING_BACKFILL_TASK)
    .fetch_optional(&mut *tx)
    .await?
    {
        tx.commit().await?;
        return Ok(Some(task_id));
    }
    let task_id = local_id::generate_local_id();
    let now = Utc::now();
    let payload = json!({
        "run_id": run.id,
        "window_start_utc": run.window_start_utc,
        "window_end_utc": run.window_end_utc,
        "batch_size": RELEASE_READING_BACKFILL_BATCH_SIZE,
    });
    sqlx::query(
        "INSERT INTO public.job_tasks (id, task_type, status, source, requested_by, parent_task_id, payload_json, created_at, updated_at) VALUES ($1, $2, 'queued', 'migration.bootstrap', NULL, NULL, $3, $4, $4)",
    )
    .bind(&task_id)
    .bind(RELEASE_READING_BACKFILL_TASK)
    .bind(payload.to_string())
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some(task_id))
}

// ---------------------------------------------------------------------------
// LLM scheduler runtime configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct LlmRuntimeConfigUpdateRequest {
    max_concurrency: i64,
    #[serde(default, deserialize_with = "deserialize_optional_nullable_i64")]
    ai_model_context_limit: Option<Option<i64>>,
    #[serde(default)]
    llm_models: Option<Vec<String>>,
    #[serde(default)]
    llm_recovery_enabled: Option<bool>,
    #[serde(default)]
    llm_recovery_rollout_percent: Option<i64>,
}

fn deserialize_optional_nullable_i64<'de, D>(
    deserializer: D,
) -> Result<Option<Option<i64>>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Some(Option::<i64>::deserialize(deserializer)?))
}

type ValidationError = String;

fn parse_positive_admin_concurrency(value: i64) -> Result<usize, ValidationError> {
    let parsed = usize::try_from(value)
        .map_err(|_| "max_concurrency must be a positive integer".to_owned())?;
    if parsed == 0 {
        return Err("max_concurrency must be a positive integer".to_owned());
    }
    if parsed > tokio::sync::Semaphore::MAX_PERMITS {
        return Err(format!(
            "max_concurrency must be a positive integer <= {}",
            tokio::sync::Semaphore::MAX_PERMITS
        ));
    }
    Ok(parsed)
}

fn parse_positive_runtime_limit(value: i64) -> Result<u32, ValidationError> {
    let parsed = u32::try_from(value)
        .map_err(|_| "ai_model_context_limit must be a positive integer".to_owned())?;
    if parsed == 0 {
        return Err("ai_model_context_limit must be a positive integer".to_owned());
    }
    Ok(parsed)
}

fn parse_llm_models(models: Vec<String>) -> Result<Vec<String>, ValidationError> {
    let nonblank_count = models
        .iter()
        .filter(|model| !model.trim().is_empty())
        .count();
    let normalized = crate::admin_runtime::normalize_llm_models(models);
    if normalized.is_empty() {
        return Err("llm_models must contain at least one non-empty model".to_owned());
    }
    if normalized.len() != nonblank_count {
        return Err("llm_models must not contain blank or duplicate models".to_owned());
    }
    Ok(normalized)
}

/// Persist the LLM scheduler configuration the readers use.
///
/// The legacy handler also pushed the new route into the in-process
/// `LlmScheduler`; the PostgreSQL runtime has no such scheduler, so the
/// persisted `admin_runtime_settings` and `llm_recovery_flags` rows are the
/// only source of truth and the status response reflects them on the next
/// read.
pub(crate) async fn admin_patch_llm_runtime_config(
    State(state): State<PostgresServerState>,
    session: Session,
    Json(request): Json<LlmRuntimeConfigUpdateRequest>,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return admin_required(status);
    }
    let max_concurrency = match parse_positive_admin_concurrency(request.max_concurrency) {
        Ok(value) => value,
        Err(message) => return bad_request(message),
    };

    let current = sqlx::query(
        "SELECT ai_model_context_limit, llm_models_json FROM public.admin_runtime_settings WHERE id = 1",
    )
    .fetch_optional(&state.runtime.pool)
    .await;
    let (current_context_limit, current_models) = match current {
        Ok(Some(row)) => (
            row.try_get::<Option<i64>, _>("ai_model_context_limit")
                .unwrap_or(None),
            row.try_get::<String, _>("llm_models_json")
                .ok()
                .map(|raw| {
                    serde_json::from_str::<Vec<String>>(&raw)
                        .map(crate::admin_runtime::normalize_llm_models)
                        .unwrap_or_default()
                })
                .unwrap_or_default(),
        ),
        Ok(None) => (None, Vec::new()),
        Err(error) => return database_error("llm_runtime_config_read", error),
    };

    let ai_model_context_limit = match request.ai_model_context_limit {
        Some(Some(value)) => match parse_positive_runtime_limit(value) {
            Ok(value) => Some(i64::from(value)),
            Err(message) => return bad_request(message),
        },
        Some(None) => None,
        None => current_context_limit,
    };
    let llm_models = match request.llm_models {
        Some(models) => match parse_llm_models(models) {
            Ok(models) => models,
            Err(message) => return bad_request(message),
        },
        None => current_models,
    };

    // Flag columns are BIGINT 0/1 (migration 0016 normalizes the baseline's
    // earlier BOOLEAN declarations), matching the rest of the runtime.
    let current_recovery = sqlx::query_as::<_, (i64, i32)>(
        "SELECT llm_recovery_enabled::bigint, llm_recovery_rollout_percent::int FROM public.llm_recovery_flags WHERE id = 1",
    )
    .fetch_optional(&state.runtime.pool)
    .await;
    let (current_enabled, current_rollout) = match current_recovery {
        Ok(Some(recovery)) => recovery,
        Ok(None) => (0, 0),
        Err(error) => return database_error("llm_recovery_config_read", error),
    };
    let recovery_enabled = request
        .llm_recovery_enabled
        .map(i64::from)
        .unwrap_or(current_enabled);
    let recovery_rollout_percent = match request.llm_recovery_rollout_percent {
        Some(value) if !(0..=100).contains(&value) => {
            return bad_request("llm_recovery_rollout_percent must be between 0 and 100");
        }
        Some(value) => value,
        None => i64::from(current_rollout),
    };

    let now_at = Utc::now();
    let now = now_at.to_rfc3339();
    let serialized_models = serde_json::to_string(&llm_models).unwrap_or_else(|_| "[]".to_owned());
    let applied = async {
        let mut tx = state.runtime.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO public.admin_runtime_settings (
              id, llm_max_concurrency, translation_general_worker_concurrency,
              translation_dedicated_worker_concurrency, ai_model_context_limit,
              ai_model_context_limit_migrated_at, llm_models_json, created_at, updated_at
            ) VALUES (1, $1, $2, $3, $4, $5, $6, $5, $5)
            ON CONFLICT (id) DO UPDATE SET
              llm_max_concurrency = excluded.llm_max_concurrency,
              ai_model_context_limit = excluded.ai_model_context_limit,
              ai_model_context_limit_migrated_at = COALESCE(admin_runtime_settings.ai_model_context_limit_migrated_at, excluded.ai_model_context_limit_migrated_at),
              llm_models_json = excluded.llm_models_json,
              updated_at = excluded.updated_at
            "#,
        )
        .bind(i64::try_from(max_concurrency).unwrap_or(i64::MAX))
        .bind(i64::try_from(
            crate::translation_runtime::DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY,
        )
        .unwrap_or(1))
        .bind(i64::try_from(
            crate::translation_runtime::DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY,
        )
        .unwrap_or(1))
        .bind(ai_model_context_limit)
        .bind(&now)
        .bind(&serialized_models)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO public.llm_recovery_flags (
              id, llm_recovery_enabled, llm_recovery_rollout_percent, created_at, updated_at
            ) VALUES (1, $1, $2, $3, $3)
            ON CONFLICT (id) DO UPDATE SET
              llm_recovery_enabled = excluded.llm_recovery_enabled,
              llm_recovery_rollout_percent = excluded.llm_recovery_rollout_percent,
              updated_at = excluded.updated_at
            "#,
        )
        .bind(recovery_enabled)
        .bind(i32::try_from(recovery_rollout_percent).unwrap_or(0))
        .bind(now_at)
        .execute(&mut *tx)
        .await?;
        clear_credential_park_cooldowns(&mut *tx, now_at).await?;
        tx.commit().await
    }
    .await;
    if let Err(error) = applied {
        return database_error("llm_runtime_config_apply", error);
    }

    crate::postgres_admin_llm::status(State(state), session).await
}

async fn clear_credential_park_cooldowns<'c, E>(
    executor: E,
    now: DateTime<Utc>,
) -> Result<u64, sqlx::Error>
where
    E: sqlx::Executor<'c, Database = Postgres>,
{
    sqlx::query(
        "UPDATE public.llm_model_health SET cooldown_until = NULL, updated_at = $1 WHERE last_failure_class = 'configuration' AND cooldown_until IS NOT NULL",
    )
    .bind(now)
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

// ---------------------------------------------------------------------------
// LLM diagnostic access audit
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct LlmDiagnosticAccessRequest {
    action: String,
}

/// Record that an administrator revealed or copied one call's diagnostics.
pub(crate) async fn admin_audit_llm_diagnostic_access(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(call_id): Path<String>,
    Json(request): Json<LlmDiagnosticAccessRequest>,
) -> Response {
    let actor_user_id = match require_admin(&state, &session).await {
        Ok(actor_user_id) => actor_user_id,
        Err(status) => return admin_required(status),
    };
    let Some(call_id) = local_id::normalize_local_id(&call_id) else {
        return bad_request("invalid call_id");
    };
    if !matches!(request.action.as_str(), "reveal" | "copy") {
        return bad_request("action must be reveal or copy");
    }

    let exists = async {
        let legacy = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM public.llm_calls WHERE id = $1)",
        )
        .bind(&call_id)
        .fetch_one(&state.runtime.pool)
        .await?;
        if legacy {
            return Ok::<_, sqlx::Error>(true);
        }
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM public.content_attempt_llm_calls WHERE id = $1)",
        )
        .bind(&call_id)
        .fetch_one(&state.runtime.pool)
        .await
    }
    .await;
    match exists {
        Ok(true) => {}
        Ok(false) => return not_found("llm call not found or diagnostic evidence expired"),
        Err(error) => return database_error("llm_diagnostic_access_lookup", error),
    }

    let inserted = sqlx::query(
        "INSERT INTO public.llm_diagnostic_access_audit (id, call_id, actor_user_id, action, created_at) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(local_id::generate_local_id())
    .bind(&call_id)
    .bind(&actor_user_id)
    .bind(&request.action)
    .bind(Utc::now())
    .execute(&state.runtime.pool)
    .await;
    match inserted {
        Ok(_) => Json(json!({"ok": true})).into_response(),
        Err(error) => database_error("llm_diagnostic_access_audit_insert", error),
    }
}

// ---------------------------------------------------------------------------
// Content processing mode transitions
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct ContentModeSwitchRequest {
    switch_token: String,
}

pub(crate) async fn admin_content_processing_cutover(
    State(state): State<PostgresServerState>,
    session: Session,
    Json(request): Json<ContentModeSwitchRequest>,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return admin_required(status);
    }
    let switch_token = request.switch_token.trim().to_owned();
    if switch_token.is_empty() {
        return bad_request("switch_token is required");
    }
    let result = async {
        let mut tx = state.runtime.pool.begin().await?;
        lock_content_mode(&mut tx).await?;
        let mode = sqlx::query_scalar::<_, String>(
            "SELECT mode FROM public.content_processing_control WHERE id = 1",
        )
        .fetch_optional(&mut *tx)
        .await?;
        if mode.as_deref() != Some("rollback_freeze") {
            tx.rollback().await?;
            return Ok::<_, sqlx::Error>(false);
        }
        if active_legacy_content_batches(&mut tx).await? {
            tx.rollback().await?;
            return Ok(false);
        }
        record_legacy_observations(&mut tx).await?;
        let changed = sqlx::query(
            "UPDATE public.content_processing_control SET mode = 'global', switch_token = $1, updated_at = CURRENT_TIMESTAMP WHERE id = 1 AND mode = 'rollback_freeze'",
        )
        .bind(&switch_token)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if !changed {
            tx.rollback().await?;
            return Ok(false);
        }
        tx.commit().await?;
        Ok(true)
    }
    .await;
    match result {
        Ok(true) => Json(json!({"mode": "global"})).into_response(),
        Ok(false) => conflict(
            "content_processing_cutover_not_ready",
            "content processing must be in rollback_freeze before cutover",
        ),
        Err(error) => database_error("content_processing_cutover", error),
    }
}

pub(crate) async fn admin_content_processing_freeze(
    State(state): State<PostgresServerState>,
    session: Session,
    Json(request): Json<ContentModeSwitchRequest>,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return admin_required(status);
    }
    let switch_token = request.switch_token.trim().to_owned();
    if switch_token.is_empty() {
        return bad_request("switch_token is required");
    }
    let result = async {
        let mut tx = state.runtime.pool.begin().await?;
        lock_content_mode(&mut tx).await?;
        let mode = sqlx::query_scalar::<_, String>(
            "SELECT mode FROM public.content_processing_control WHERE id = 1",
        )
        .fetch_optional(&mut *tx)
        .await?;
        if mode.as_deref() != Some("legacy") {
            tx.rollback().await?;
            return Ok::<_, sqlx::Error>(false);
        }
        if active_legacy_content_batches(&mut tx).await? {
            tx.rollback().await?;
            return Ok(false);
        }
        let changed = sqlx::query(
            "UPDATE public.content_processing_control SET mode = 'rollback_freeze', switch_token = $1, updated_at = CURRENT_TIMESTAMP WHERE id = 1 AND mode = 'legacy'",
        )
        .bind(&switch_token)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if !changed {
            tx.rollback().await?;
            return Ok(false);
        }
        tx.commit().await?;
        Ok(true)
    }
    .await;
    match result {
        Ok(true) => Json(json!({"mode": "rollback_freeze"})).into_response(),
        Ok(false) => conflict(
            "content_processing_freeze_not_ready",
            "content processing must be in legacy mode with no active legacy batches before freeze",
        ),
        Err(error) => database_error("content_processing_freeze", error),
    }
}

/// Whether any legacy translation batch or work item is still in flight.
async fn active_legacy_content_batches(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<bool, sqlx::Error> {
    for table in ["translation_batches", "translation_work_items"] {
        if !table_exists(tx, table).await? {
            continue;
        }
        let active = sqlx::query_scalar::<_, i64>(&format!(
            "SELECT COUNT(*) FROM public.{table} WHERE status NOT IN ('completed', 'failed')"
        ))
        .fetch_one(&mut **tx)
        .await?;
        if active > 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Observe the legacy translation cache before switching admission to the
/// global pipeline, so later reconciliation can classify every legacy row.
async fn record_legacy_observations(tx: &mut Transaction<'_, Postgres>) -> Result<(), sqlx::Error> {
    // Legacy creates the observation table on demand and repairs it in place,
    // adding a `legacy_source_hash` column only after an earlier cutover. Make
    // sure the table exists, then write through whichever shape is present.
    if !table_exists(tx, "content_legacy_observations").await? {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS public.content_legacy_observations (
              id text PRIMARY KEY,
              legacy_table text NOT NULL,
              legacy_primary_key text NOT NULL,
              canonical_resource_type text,
              canonical_resource_id text,
              pipeline text CHECK (pipeline IS NULL OR pipeline IN ('translation', 'polishing')),
              classification text NOT NULL CHECK (classification IN ('legacy_cached', 'legacy_conflict')),
              observation_basis_json text NOT NULL,
              observed_at text NOT NULL,
              UNIQUE (legacy_table, legacy_primary_key)
            )
            "#,
        )
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_content_legacy_observations_resource ON public.content_legacy_observations(canonical_resource_type, canonical_resource_id, pipeline)",
        )
        .execute(&mut **tx)
        .await?;
    }
    let has_source_hash = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'content_legacy_observations' AND column_name = 'legacy_source_hash')",
    )
    .fetch_one(&mut **tx)
    .await?;
    let (hash_column, cache_hash_value, work_hash_value) = if has_source_hash {
        ("legacy_source_hash,", "source_hash,", "w.source_hash,")
    } else {
        ("", "", "")
    };

    let has_ai_translations = table_exists(tx, "ai_translations").await?;
    if has_ai_translations {
        sqlx::query(&format!(
            r#"
            INSERT INTO public.content_legacy_observations (
              id, legacy_table, legacy_primary_key, {hash_column}
              canonical_resource_type, canonical_resource_id, pipeline,
              classification, observation_basis_json, observed_at
            )
            SELECT
              'legacy-cache-' || id || '-' || source_hash,
              'ai_translations', id, {cache_hash_value}
              CASE
                WHEN entity_type LIKE 'release%' THEN 'release'
                WHEN entity_type LIKE 'announcement%' THEN 'announcement'
                WHEN entity_type IN ('notification', 'notification_smart') THEN 'notification'
                ELSE NULL
              END,
              entity_id,
              CASE WHEN entity_type LIKE '%smart' THEN 'polishing' ELSE 'translation' END,
              CASE
                WHEN status = 'ready'
                  AND (NULLIF(btrim(title), '') IS NOT NULL OR NULLIF(btrim(summary), '') IS NOT NULL)
                THEN 'legacy_cached'
                ELSE 'legacy_conflict'
              END,
              '{{"source":"ai_translations","status":"' || replace(status, '"', '')
                || '","source_hash":"' || replace(source_hash, '"', '') || '"}}',
              CURRENT_TIMESTAMP::text
            FROM public.ai_translations
            ON CONFLICT DO NOTHING
            "#
        ))
        .execute(&mut **tx)
        .await?;
    }

    if table_exists(tx, "translation_work_items").await? {
        let cached_condition = if has_ai_translations {
            "w.status = 'completed' AND COALESCE(w.result_status, '') = 'ready' AND EXISTS (SELECT 1 FROM public.ai_translations c WHERE c.user_id = w.scope_user_id AND c.entity_id = w.entity_id AND c.lang = w.target_lang AND c.source_hash = w.source_hash AND c.status = 'ready' AND (NULLIF(btrim(c.title), '') IS NOT NULL OR NULLIF(btrim(c.summary), '') IS NOT NULL))"
        } else {
            "false"
        };
        sqlx::query(&format!(
            r#"
            INSERT INTO public.content_legacy_observations (
              id, legacy_table, legacy_primary_key, {hash_column}
              canonical_resource_type, canonical_resource_id, pipeline,
              classification, observation_basis_json, observed_at
            )
            SELECT
              'legacy-work-' || w.id || '-' || w.source_hash,
              'translation_work_items', w.id, {work_hash_value}
              CASE
                WHEN w.kind LIKE 'release%' THEN 'release'
                WHEN w.kind LIKE 'announcement%' THEN 'announcement'
                WHEN w.kind IN ('notification', 'notification_smart') THEN 'notification'
                ELSE NULL
              END,
              w.entity_id,
              CASE WHEN w.kind LIKE '%smart' THEN 'polishing' ELSE 'translation' END,
              CASE WHEN {cached_condition} THEN 'legacy_cached' ELSE 'legacy_conflict' END,
              '{{"source":"translation_work_items","status":"'
                || replace(COALESCE(w.status, ''), '"', '')
                || '","source_hash":"' || replace(COALESCE(w.source_hash, ''), '"', '') || '"}}',
              CURRENT_TIMESTAMP::text
            FROM public.translation_work_items w
            ON CONFLICT DO NOTHING
            "#
        ))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Content identity upgrade control
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum IdentityUpgradeAction {
    Pause,
    Resume,
}

#[derive(Debug, Deserialize)]
pub(crate) struct IdentityUpgradeControlRequest {
    action: IdentityUpgradeAction,
}

pub(crate) async fn admin_content_identity_upgrade_control(
    State(state): State<PostgresServerState>,
    session: Session,
    Json(request): Json<IdentityUpgradeControlRequest>,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return admin_required(status);
    }
    let now = Utc::now().to_rfc3339();
    let applied = match request.action {
        IdentityUpgradeAction::Pause => sqlx::query(
            "UPDATE public.content_identity_upgrade_control SET status = 'paused', updated_at = $1 WHERE id = 1 AND status IN ('pending', 'running')",
        )
        .bind(&now)
        .execute(&state.runtime.pool)
        .await
        .map(|result| result.rows_affected()),
        IdentityUpgradeAction::Resume => sqlx::query(
            "UPDATE public.content_identity_upgrade_control SET status = 'running', last_error_code = NULL, started_at = COALESCE(started_at, $1), updated_at = $1 WHERE id = 1 AND status IN ('paused', 'failed')",
        )
        .bind(&now)
        .execute(&state.runtime.pool)
        .await
        .map(|result| result.rows_affected()),
    };
    match applied {
        Ok(0) => conflict(
            "content_identity_upgrade_state_conflict",
            "identity upgrade cannot perform that action in its current state",
        ),
        Ok(_) => admin_identity_upgrade_status(State(state), session).await,
        Err(error) => database_error("content_identity_upgrade_control", error),
    }
}

// ---------------------------------------------------------------------------
// Search index resume
// ---------------------------------------------------------------------------

pub(crate) async fn admin_resume_search_index(
    State(state): State<PostgresServerState>,
    session: Session,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return admin_required(status);
    }
    let now = Utc::now().to_rfc3339();
    let applied = async {
        let mut tx = state.runtime.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE public.search_projection_backfill_state SET status = 'building', last_error = NULL, updated_at = $1 WHERE id = 1 AND status = 'failed'",
        )
        .bind(&now)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if updated == 0 {
            tx.rollback().await?;
            return Ok::<_, sqlx::Error>(false);
        }
        tx.commit().await?;
        Ok(true)
    }
    .await;
    match applied {
        Ok(true) => admin_search_index_status(State(state), session).await,
        Ok(false) => conflict(
            "search_projection_backfill_state_conflict",
            "search projection backfill is not in a failed state",
        ),
        Err(error) => database_error("search_index_resume", error),
    }
}

// ---------------------------------------------------------------------------
// Admin user profile
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct AdminProfilePatchRequest {
    daily_brief_time_zone: String,
    #[serde(default)]
    include_own_releases: Option<bool>,
}

/// Outcome of the serialized profile write.
enum ProfileApply {
    Applied,
    Stale,
    WebhookBusy(String),
}

/// Update one user's daily-brief profile and webhook eligibility.
///
/// Faithful port of `persist_daily_brief_profile`: the same scheduler-slot
/// validation, the same optimistic conflict guard, and the same webhook
/// reconcile enqueue. The reconcile demand/lease tables are only written by the
/// legacy SQLite runtime, so this port serializes webhook changes per user with
/// an advisory lock and guards on in-flight `job_tasks` instead.
pub(crate) async fn admin_patch_user_profile(
    State(state): State<PostgresServerState>,
    session: Session,
    Path(user_id): Path<String>,
    Json(request): Json<AdminProfilePatchRequest>,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return admin_required(status);
    }
    let Some(user_id) = local_id::normalize_local_id(&user_id) else {
        return bad_request("invalid user_id");
    };

    let time_zone = match briefs::parse_daily_brief_time_zone(&request.daily_brief_time_zone) {
        Ok(time_zone) => time_zone,
        Err(error) => return bad_request(error.to_string()),
    };
    if let Err(error) = briefs::validate_hour_aligned_time_zone(&time_zone, Utc::now()) {
        return bad_request(error.to_string());
    }
    let schedule_local_time = match load_daily_brief_schedule_local_time(&state.runtime.pool).await
    {
        Ok(local_time) => local_time,
        Err(error) => return database_error("daily_brief_schedule_read", error),
    };
    let enabled_hours = match load_enabled_scheduler_hours(&state.runtime.pool).await {
        Ok(hours) => hours,
        Err(error) => return database_error("daily_brief_slots_read", error),
    };
    let missing_hours = match briefs::missing_daily_brief_scheduler_hours(
        schedule_local_time,
        &time_zone,
        &enabled_hours,
    ) {
        Ok(hours) => hours,
        Err(error) => return bad_request(error.to_string()),
    };
    if !missing_hours.is_empty() {
        let missing_hours = missing_hours
            .into_iter()
            .map(|hour| format!("{hour:02}:00Z"))
            .collect::<Vec<_>>()
            .join(", ");
        return bad_request(format!(
            "invalid daily brief schedule for current scheduler configuration (missing enabled UTC slots: {missing_hours})"
        ));
    }

    let current = sqlx::query_as::<_, (i64, String)>(
        "SELECT include_own_releases, webhook_push_desired_state FROM public.users WHERE id = $1",
    )
    .bind(&user_id)
    .fetch_optional(&state.runtime.pool)
    .await;
    let current = match current {
        Ok(Some(current)) => current,
        Ok(None) => return not_found("user not found"),
        Err(error) => return database_error("user_profile_read", error),
    };
    let next_include_own_releases = request
        .include_own_releases
        .map(i64::from)
        .unwrap_or(current.0)
        != 0;
    let next_desired_state =
        if request.include_own_releases == Some(false) && current.1 == "enabled" {
            "paused".to_owned()
        } else {
            current.1.clone()
        };
    if !matches!(
        next_desired_state.as_str(),
        "enabled" | "paused" | "deleted"
    ) {
        return bad_request("webhook_push_desired_state must be enabled, paused, or deleted");
    }
    if !next_include_own_releases && next_desired_state == "enabled" {
        return conflict(
            "my_releases_required",
            "关闭“我的发布”时不能保持启用 Webhook 推送。",
        );
    }
    let webhook_state_changed = next_desired_state != current.1;
    if webhook_state_changed {
        match inflight_webhook_task(&state.runtime.pool, &user_id).await {
            Ok(Some(task_id)) => {
                return conflict(
                    "webhook_push_operation_in_progress",
                    &format!("Webhook 对齐操作 {task_id} 尚未完成，请等待本轮结束。"),
                );
            }
            Ok(None) => {}
            Err(error) => return database_error("webhook_inflight_check", error),
        }
    }

    let now_at = Utc::now();
    let now = now_at.to_rfc3339();
    let applied = async {
        let mut tx = state.runtime.pool.begin().await?;
        // Legacy funnels webhook changes through a per-user operation lock;
        // serialize the PostgreSQL write so one user cannot enqueue two
        // reconcile tasks at once.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('octo-rill:webhook-push:' || $1, 0))")
            .bind(&user_id)
            .execute(&mut *tx)
            .await?;
        if webhook_state_changed
            && let Some(task_id) = sqlx::query_scalar::<_, String>(
                "SELECT id FROM public.job_tasks WHERE task_type = 'webhook.push.manage' AND requested_by = $1 AND status IN ('queued', 'running') ORDER BY created_at DESC, id DESC LIMIT 1",
            )
            .bind(&user_id)
            .fetch_optional(&mut *tx)
            .await?
        {
            tx.rollback().await?;
            return Ok::<_, sqlx::Error>(ProfileApply::WebhookBusy(task_id));
        }
        let updated = sqlx::query(
            r#"
            UPDATE public.users
            SET daily_brief_time_zone = $1,
                include_own_releases = COALESCE($2, include_own_releases),
                webhook_push_desired_state = $3,
                webhook_push_enabled = $4,
                updated_at = $5
            WHERE id = $6
              AND include_own_releases = $7
              AND webhook_push_desired_state = $8
            "#,
        )
        .bind(&time_zone)
        .bind(
            request
                .include_own_releases
                .map(|value| if value { 1_i64 } else { 0_i64 }),
        )
        .bind(&next_desired_state)
        .bind(i64::from(
            next_include_own_releases && next_desired_state == "enabled",
        ))
        .bind(&now)
        .bind(&user_id)
        .bind(current.0)
        .bind(&current.1)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if updated == 0 {
            tx.rollback().await?;
            return Ok(ProfileApply::Stale);
        }
        if webhook_state_changed {
            let payload = json!({
                "user_id": &user_id,
                "operation": "reconcile",
                "repo_id": Value::Null,
            });
            sqlx::query(
                "INSERT INTO public.job_tasks (id, task_type, status, source, requested_by, parent_task_id, payload_json, created_at, updated_at) VALUES ($1, 'webhook.push.manage', 'queued', 'profile', $2, NULL, $3, $4, $4)",
            )
            .bind(local_id::generate_local_id())
            .bind(&user_id)
            .bind(payload.to_string())
            .bind(now_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(ProfileApply::Applied)
    }
    .await;
    match applied {
        Ok(ProfileApply::Applied) => {}
        Ok(ProfileApply::Stale) => {
            return conflict(
                "webhook_push_operation_in_progress",
                "用户设置刚刚发生变化，请刷新后重试。",
            );
        }
        Ok(ProfileApply::WebhookBusy(task_id)) => {
            return conflict(
                "webhook_push_operation_in_progress",
                &format!("Webhook 对齐操作 {task_id} 尚未完成，请等待本轮结束。"),
            );
        }
        Err(error) => return database_error("user_profile_update", error),
    }

    admin_user_profile(State(state), session, Path(user_id))
        .await
        .into_response()
}

/// Effective global daily-brief local time, defaulting to the PostgreSQL
/// entry's `06:00` when the settings row or value is unusable.
async fn load_daily_brief_schedule_local_time(pool: &PgPool) -> Result<NaiveTime, sqlx::Error> {
    let raw = sqlx::query_scalar::<_, String>(
        "SELECT daily_brief_schedule_local_time FROM public.admin_runtime_settings WHERE id = 1 LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(raw
        .as_deref()
        .and_then(|value| briefs::parse_daily_brief_local_time(value).ok())
        .unwrap_or_else(|| NaiveTime::from_hms_opt(6, 0, 0).expect("06:00 is valid")))
}

async fn load_enabled_scheduler_hours(
    pool: &PgPool,
) -> Result<std::collections::BTreeSet<u8>, sqlx::Error> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT hour_utc FROM public.daily_brief_hour_slots WHERE enabled = 1",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .filter_map(|hour| u8::try_from(hour).ok())
    .filter(|hour| *hour <= 23)
    .collect())
}

async fn inflight_webhook_task(
    pool: &PgPool,
    user_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM public.job_tasks WHERE task_type = 'webhook.push.manage' AND requested_by = $1 AND status IN ('queued', 'running') ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

// ---------------------------------------------------------------------------
// Translation runtime configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct TranslationRuntimeConfigUpdateRequest {
    general_worker_concurrency: i64,
    dedicated_worker_concurrency: i64,
}

fn parse_worker_concurrency(value: i64, field: &str) -> Result<usize, ValidationError> {
    let parsed =
        usize::try_from(value).map_err(|_| format!("{field} must be a positive integer"))?;
    if parsed == 0 {
        return Err(format!("{field} must be a positive integer"));
    }
    if parsed > MAX_TRANSLATION_WORKER_CONCURRENCY {
        return Err(format!(
            "{field} must be a positive integer <= {MAX_TRANSLATION_WORKER_CONCURRENCY}"
        ));
    }
    Ok(parsed)
}

fn validate_worker_concurrency_total(
    general_worker_concurrency: usize,
    dedicated_worker_concurrency: usize,
) -> Result<(), ValidationError> {
    let total = general_worker_concurrency
        .checked_add(dedicated_worker_concurrency)
        .ok_or_else(|| {
            format!(
                "general_worker_concurrency + dedicated_worker_concurrency must be <= {MAX_TRANSLATION_WORKER_CONCURRENCY}"
            )
        })?;
    if total > MAX_TRANSLATION_WORKER_CONCURRENCY {
        return Err(format!(
            "general_worker_concurrency + dedicated_worker_concurrency must be <= {MAX_TRANSLATION_WORKER_CONCURRENCY}"
        ));
    }
    Ok(())
}

/// Persist the desired translation worker concurrency.
///
/// The PostgreSQL runtime runs one sequential worker, so the stored target
/// values drive the translation status response only through the settings row;
/// the legacy in-process scheduler apply has no PostgreSQL counterpart.
pub(crate) async fn admin_patch_translation_runtime_config(
    State(state): State<PostgresServerState>,
    session: Session,
    Json(request): Json<TranslationRuntimeConfigUpdateRequest>,
) -> Response {
    if let Err(status) = require_admin(&state, &session).await {
        return admin_required(status);
    }
    let general_worker_concurrency = match parse_worker_concurrency(
        request.general_worker_concurrency,
        "general_worker_concurrency",
    ) {
        Ok(value) => value,
        Err(message) => return bad_request(message),
    };
    let dedicated_worker_concurrency = match parse_worker_concurrency(
        request.dedicated_worker_concurrency,
        "dedicated_worker_concurrency",
    ) {
        Ok(value) => value,
        Err(message) => return bad_request(message),
    };
    if let Err(message) =
        validate_worker_concurrency_total(general_worker_concurrency, dedicated_worker_concurrency)
    {
        return bad_request(message);
    }

    let now = Utc::now().to_rfc3339();
    let applied = sqlx::query(
        r#"
        INSERT INTO public.admin_runtime_settings (
          id, llm_max_concurrency, translation_general_worker_concurrency,
          translation_dedicated_worker_concurrency, created_at, updated_at
        ) VALUES (1, 1, $1, $2, $3, $3)
        ON CONFLICT (id) DO UPDATE SET
          translation_general_worker_concurrency = excluded.translation_general_worker_concurrency,
          translation_dedicated_worker_concurrency = excluded.translation_dedicated_worker_concurrency,
          updated_at = excluded.updated_at
        "#,
    )
    .bind(i64::try_from(general_worker_concurrency).unwrap_or(i64::MAX))
    .bind(i64::try_from(dedicated_worker_concurrency).unwrap_or(i64::MAX))
    .bind(&now)
    .execute(&state.runtime.pool)
    .await;
    if let Err(error) = applied {
        return database_error("translation_runtime_config_apply", error);
    }

    admin_translation_status(State(state), session).await
}

#[cfg(test)]
mod tests {
    use super::clear_credential_park_cooldowns;
    use chrono::{Duration, Utc};
    use sqlx::PgPool;

    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn clear_credential_park_cooldowns_clears_only_configuration_rows() -> anyhow::Result<()>
    {
        let admin_url = std::env::var("TEST_PG_TRANSLATE_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("admin_health_{}", crate::local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;
        let result = async {
            sqlx::raw_sql(
                r#"
                CREATE TABLE public.llm_model_health(model text PRIMARY KEY, relevant_failure_count bigint NOT NULL DEFAULT 0, window_started_at timestamptz, cooldown_until timestamptz, last_failure_class text, last_failure_at timestamptz, created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL);
                "#,
            )
            .execute(&pool)
            .await?;
            let now = Utc::now();
            sqlx::query(
                "INSERT INTO public.llm_model_health(model, relevant_failure_count, cooldown_until, last_failure_class, created_at, updated_at) VALUES ('parked', 2, $1, 'configuration', $2, $2), ('cooling', 2, $3, 'rate_limited', $2, $2)",
            )
            .bind(now + Duration::hours(24))
            .bind(now)
            .bind(now + Duration::minutes(5))
            .execute(&pool)
            .await?;
            let updated = clear_credential_park_cooldowns(&pool, now).await?;
            assert_eq!(updated, 1);
            let parked: (Option<chrono::DateTime<Utc>>, Option<String>) = sqlx::query_as(
                "SELECT cooldown_until, last_failure_class FROM public.llm_model_health WHERE model = 'parked'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(parked.0, None);
            assert_eq!(parked.1.as_deref(), Some("configuration"));
            let cooling: Option<chrono::DateTime<Utc>> = sqlx::query_scalar(
                "SELECT cooldown_until FROM public.llm_model_health WHERE model = 'cooling'",
            )
            .fetch_one(&pool)
            .await?;
            assert!(cooling.is_some_and(|until| until > now));
            Ok::<_, anyhow::Error>(())
        }
        .await;
        pool.close().await;
        sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
            .execute(&admin)
            .await?;
        admin.close().await;
        result
    }
}
