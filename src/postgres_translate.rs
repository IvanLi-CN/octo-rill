//! PostgreSQL translation endpoints for releases and inbox notifications.
//!
//! Translation state lives in the native PostgreSQL translation tables
//! (`translation_requests`, `translation_work_items`, `translation_attempt_*`)
//! and every model call is persisted into `public.llm_calls` before the
//! response is written, so the administration LLM audit observes this path.
//! The `task_id` worker entry ([`translate_work_item_for_task`]) drives the
//! same recorder for work items enqueued as `translation.generate` tasks.
//!
//! Request validation, return modes (`sync` / `task_id` / `sse`), batch stream
//! framing, disabled-AI payloads, and failure classification mirror the legacy
//! SQLite implementation.

// These handlers return `Response` as their error type so a failure keeps its
// status and body; boxing it would only add an allocation on the error path.
#![allow(clippy::result_large_err)]

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    time::Duration,
};

use anyhow::{Context, anyhow};
use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
};
use chrono::{DateTime, Utc};
use futures_util::stream;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tower_sessions::Session;
use tracing::{info, warn};

use crate::{
    api::{TranslateBatchItem, TranslateBatchResponse, TranslateResponse},
    config::AiConfig,
    local_id,
    postgres_server::PostgresServerState,
    translation_error::translation_error_summary,
    translations::{TranslationRequestItemInput, TranslationSourceBlock},
};

const TARGET_LANG: &str = "zh-CN";
const RELEASE_BATCH_MAX_ITEMS: usize = 60;
const RELEASE_DETAIL_BATCH_MAX_ITEMS: usize = 20;
const NOTIFICATION_BATCH_MAX_ITEMS: usize = 60;
const TRANSLATION_MAX_WAIT_MS: i64 = 60_000;
const RELEASE_TITLE_MAX_TOKENS: u32 = 120;
const RELEASE_BODY_MAX_TOKENS: u32 = 4_000;
const NOTIFICATION_MAX_TOKENS: u32 = 1_100;
const LLM_CALL_TIMEOUT: Duration = Duration::from_secs(120);
const TASK_EVENT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const TASK_EVENT_POLLS: u32 = 600;
const RELEASE_TITLE_SYSTEM: &str =
    "你是一个翻译助手，只把 GitHub Release 标题翻译成自然中文。输出纯文本，不要解释。";
const RELEASE_BODY_SYSTEM: &str =
    "你是一个严谨的技术文档翻译助手，负责把 GitHub Release notes 翻译成中文并保持 Markdown 结构。";
const NOTIFICATION_SYSTEM: &str = "你是一个助理，负责把 GitHub Notifications 条目转写为中文标题与简短建议（Markdown）。不要包含任何 URL。";

// ---------------------------------------------------------------------------
// Request and response shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(super) struct ReturnModeQuery {
    return_mode: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReturnMode {
    Sync,
    TaskId,
    Sse,
}

#[derive(Debug, Deserialize)]
pub(super) struct TranslateReleaseRequest {
    release_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TranslateReleasesBatchRequest {
    release_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TranslateReleaseDetailRequest {
    release_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TranslateReleaseDetailBatchRequest {
    release_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TranslateNotificationRequest {
    thread_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TranslateNotificationsBatchRequest {
    thread_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetKind {
    ReleaseSummary,
    ReleaseDetail,
    Notification,
}

impl TargetKind {
    /// Shared work item identity. Release summaries reuse the release detail
    /// work item so a cached detail translation serves both routes, matching
    /// the legacy shared cache.
    fn work_kind(self) -> &'static str {
        match self {
            Self::Notification => "notification",
            Self::ReleaseSummary | Self::ReleaseDetail => "release_detail",
        }
    }

    fn work_variant(self) -> &'static str {
        match self {
            Self::Notification => "shared",
            Self::ReleaseSummary | Self::ReleaseDetail => "detail",
        }
    }

    fn request_kind(self) -> &'static str {
        match self {
            Self::ReleaseSummary => "release_summary",
            Self::ReleaseDetail => "release_detail",
            Self::Notification => "notification",
        }
    }

    fn request_variant(self) -> &'static str {
        match self {
            Self::ReleaseSummary => "summary",
            Self::ReleaseDetail => "detail",
            Self::Notification => "shared",
        }
    }

    fn missing_message(self) -> &'static str {
        match self {
            Self::Notification => "notification not found",
            Self::ReleaseSummary | Self::ReleaseDetail => "release not found",
        }
    }

    fn allows_brief_access(self) -> bool {
        matches!(self, Self::ReleaseDetail)
    }

    fn audit_stage(self) -> &'static str {
        match self {
            Self::Notification => "notification",
            Self::ReleaseSummary | Self::ReleaseDetail => "release",
        }
    }
}

#[derive(Debug, Clone)]
struct Target {
    entity_id: String,
    kind: TargetKind,
    variant: String,
    source: String,
    title: String,
    body: String,
    repo_full_name: String,
    reason: String,
    subject_type: String,
    source_blocks: Vec<TranslationSourceBlock>,
    target_slots: Vec<String>,
    source_hash: String,
}

#[derive(Debug)]
enum Outcome {
    Ready {
        title: Option<String>,
        summary: Option<String>,
    },
    Disabled,
    Missing {
        message: String,
    },
    Failed {
        message: String,
        failure_class: Option<&'static str>,
    },
}

impl Outcome {
    fn batch_item(&self, id: &str) -> TranslateBatchItem {
        match self {
            Self::Ready { title, summary } => TranslateBatchItem {
                id: id.to_owned(),
                lang: TARGET_LANG.to_owned(),
                status: "ready".to_owned(),
                title: title.clone(),
                summary: summary.clone(),
                error: None,
                failure_class: None,
            },
            Self::Disabled => disabled_item(id),
            Self::Missing { message } => TranslateBatchItem {
                id: id.to_owned(),
                lang: TARGET_LANG.to_owned(),
                status: "missing".to_owned(),
                title: None,
                summary: None,
                error: Some(message.clone()),
                failure_class: None,
            },
            Self::Failed {
                message,
                failure_class,
            } => {
                let public =
                    translation_error_summary(Some(message)).or_else(|| Some(message.clone()));
                TranslateBatchItem {
                    id: id.to_owned(),
                    lang: TARGET_LANG.to_owned(),
                    status: "error".to_owned(),
                    title: None,
                    summary: None,
                    error: public,
                    failure_class: failure_class.map(str::to_owned),
                }
            }
        }
    }

    fn into_single(self) -> Result<TranslateResponse, Response> {
        match self {
            Self::Ready { title, summary } => Ok(TranslateResponse {
                lang: TARGET_LANG.to_owned(),
                status: "ready".to_owned(),
                title,
                summary,
            }),
            Self::Disabled => Ok(TranslateResponse {
                lang: TARGET_LANG.to_owned(),
                status: "disabled".to_owned(),
                title: None,
                summary: None,
            }),
            Self::Missing { message } => {
                Err(error_response(StatusCode::NOT_FOUND, "not_found", message))
            }
            Self::Failed {
                message,
                failure_class,
            } => {
                let public = translation_error_summary(Some(&message)).unwrap_or(message);
                let mut body = json!({"code": "llm_error", "message": public});
                if let Some(class) = failure_class {
                    body["failure_class"] = json!(class);
                }
                Err((StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response())
            }
        }
    }
}

fn disabled_item(id: &str) -> TranslateBatchItem {
    TranslateBatchItem {
        id: id.to_owned(),
        lang: TARGET_LANG.to_owned(),
        status: "disabled".to_owned(),
        title: None,
        summary: None,
        error: None,
        failure_class: None,
    }
}

#[derive(Debug, Clone)]
struct TrackingTask {
    id: String,
    task_type: &'static str,
}

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

pub(super) async fn translate_releases_batch(
    State(state): State<PostgresServerState>,
    session: Session,
    Json(request): Json<TranslateReleasesBatchRequest>,
) -> Response {
    let user_id = match session_user(&session).await {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };
    let release_ids = match parse_release_ids(&request.release_ids, RELEASE_BATCH_MAX_ITEMS) {
        Ok(ids) => ids,
        Err(response) => return response,
    };
    let entities = release_ids.iter().map(i64::to_string).collect::<Vec<_>>();
    match batch_items(
        &state,
        &user_id,
        TargetKind::ReleaseSummary,
        "api.translate_releases_batch",
        &entities,
    )
    .await
    {
        Ok(items) => Json(TranslateBatchResponse { items }).into_response(),
        Err(error) => db_failure(error),
    }
}

pub(super) async fn translate_releases_batch_stream(
    State(state): State<PostgresServerState>,
    session: Session,
    Json(request): Json<TranslateReleasesBatchRequest>,
) -> Response {
    let user_id = match session_user(&session).await {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };
    let release_ids = match parse_release_ids(&request.release_ids, RELEASE_BATCH_MAX_ITEMS) {
        Ok(ids) => ids,
        Err(response) => return response,
    };
    let task = TrackingTask {
        id: local_id::generate_local_id(),
        task_type: "translate.release.batch",
    };
    let payload = json!({"user_id": user_id.clone(), "release_ids": release_ids.clone()});
    if let Err(error) = start_tracking_task(
        &state,
        &task,
        "api.translate_releases_batch_stream",
        &user_id,
        &payload,
        Utc::now(),
    )
    .await
    {
        return db_failure(error);
    }
    let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(64);
    let worker_state = state.clone();
    let worker_user = user_id.clone();
    let worker_task = task.clone();
    tokio::spawn(async move {
        run_release_batch_stream(worker_state, worker_user, release_ids, worker_task, tx).await;
    });
    let body = Body::from_stream(ReceiverStream::new(rx));
    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

pub(super) async fn translate_release(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<ReturnModeQuery>,
    Json(request): Json<TranslateReleaseRequest>,
) -> Response {
    let user_id = match session_user(&session).await {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };
    let mode = match parse_return_mode(&query, ReturnMode::Sync) {
        Ok(mode) => mode,
        Err(response) => return response,
    };
    let release_id = match parse_release_id(&request.release_id) {
        Ok(id) => id,
        Err(response) => return response,
    };
    single_translation(
        state,
        user_id,
        release_id.to_string(),
        TargetKind::ReleaseSummary,
        "api.translate_release",
        mode,
    )
    .await
}

/// Streaming alias for a single release. The path itself selects SSE.
pub(super) async fn translate_release_stream(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<ReturnModeQuery>,
    Json(request): Json<TranslateReleaseRequest>,
) -> Response {
    let user_id = match session_user(&session).await {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };
    let mode = match parse_return_mode(&query, ReturnMode::Sse) {
        Ok(mode) => mode,
        Err(response) => return response,
    };
    let release_id = match parse_release_id(&request.release_id) {
        Ok(id) => id,
        Err(response) => return response,
    };
    single_translation(
        state,
        user_id,
        release_id.to_string(),
        TargetKind::ReleaseSummary,
        "api.translate_release.stream",
        mode,
    )
    .await
}

pub(super) async fn translate_release_detail(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<ReturnModeQuery>,
    Json(request): Json<TranslateReleaseDetailRequest>,
) -> Response {
    let user_id = match session_user(&session).await {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };
    let mode = match parse_return_mode(&query, ReturnMode::Sync) {
        Ok(mode) => mode,
        Err(response) => return response,
    };
    let release_id = match parse_release_id(&request.release_id) {
        Ok(id) => id,
        Err(response) => return response,
    };
    single_translation(
        state,
        user_id,
        release_id.to_string(),
        TargetKind::ReleaseDetail,
        "api.translate_release_detail",
        mode,
    )
    .await
}

pub(super) async fn translate_release_detail_batch(
    State(state): State<PostgresServerState>,
    session: Session,
    Json(request): Json<TranslateReleaseDetailBatchRequest>,
) -> Response {
    let user_id = match session_user(&session).await {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };
    let release_ids = match parse_release_ids(&request.release_ids, RELEASE_DETAIL_BATCH_MAX_ITEMS)
    {
        Ok(ids) => ids,
        Err(response) => return response,
    };
    let entities = release_ids.iter().map(i64::to_string).collect::<Vec<_>>();
    match batch_items(
        &state,
        &user_id,
        TargetKind::ReleaseDetail,
        "api.translate_release_detail_batch",
        &entities,
    )
    .await
    {
        Ok(items) => Json(TranslateBatchResponse { items }).into_response(),
        Err(error) => db_failure(error),
    }
}

pub(super) async fn translate_notifications_batch(
    State(state): State<PostgresServerState>,
    session: Session,
    Json(request): Json<TranslateNotificationsBatchRequest>,
) -> Response {
    let user_id = match session_user(&session).await {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };
    let thread_ids = match parse_thread_ids(&request.thread_ids, NOTIFICATION_BATCH_MAX_ITEMS) {
        Ok(ids) => ids,
        Err(response) => return response,
    };
    match batch_items(
        &state,
        &user_id,
        TargetKind::Notification,
        "api.translate_notifications_batch",
        &thread_ids,
    )
    .await
    {
        Ok(items) => Json(TranslateBatchResponse { items }).into_response(),
        Err(error) => db_failure(error),
    }
}

pub(super) async fn translate_notification(
    State(state): State<PostgresServerState>,
    session: Session,
    Query(query): Query<ReturnModeQuery>,
    Json(request): Json<TranslateNotificationRequest>,
) -> Response {
    let user_id = match session_user(&session).await {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };
    let mode = match parse_return_mode(&query, ReturnMode::Sync) {
        Ok(mode) => mode,
        Err(response) => return response,
    };
    let thread_id = request.thread_id.trim().to_owned();
    if thread_id.is_empty() {
        return bad_request("thread_id is required");
    }
    single_translation(
        state,
        user_id,
        thread_id,
        TargetKind::Notification,
        "api.translate_notification",
        mode,
    )
    .await
}

// ---------------------------------------------------------------------------
// Shared request flow
// ---------------------------------------------------------------------------

async fn single_translation(
    state: PostgresServerState,
    user_id: String,
    entity_id: String,
    kind: TargetKind,
    source: &'static str,
    mode: ReturnMode,
) -> Response {
    match mode {
        ReturnMode::Sync => {
            let Some(ai) = state.ai.as_ref() else {
                return Json(TranslateResponse {
                    lang: TARGET_LANG.to_owned(),
                    status: "disabled".to_owned(),
                    title: None,
                    summary: None,
                })
                .into_response();
            };
            let targets = match load_targets(
                &state.runtime.pool,
                &user_id,
                kind,
                source,
                std::slice::from_ref(&entity_id),
            )
            .await
            {
                Ok(targets) => targets,
                Err(error) => return db_failure(error),
            };
            let outcome = match targets.get(&entity_id) {
                Some(target) => {
                    match translate_target(
                        &state.runtime.pool,
                        &state.http,
                        ai,
                        &user_id,
                        target,
                        "wait",
                        None,
                    )
                    .await
                    {
                        Ok(outcome) => outcome,
                        Err(error) => return db_failure(error),
                    }
                }
                None => Outcome::Missing {
                    message: kind.missing_message().to_owned(),
                },
            };
            match outcome.into_single() {
                Ok(body) => Json(body).into_response(),
                Err(response) => response,
            }
        }
        ReturnMode::TaskId => {
            match enqueue_translation_task(&state, &user_id, &entity_id, kind, source).await {
                Ok((task_id, status)) => Json(json!({
                    "mode": "task_id",
                    "task_id": task_id,
                    "task_type": "translation.generate",
                    "status": status,
                }))
                .into_response(),
                Err(error) => db_failure(error),
            }
        }
        ReturnMode::Sse => {
            let task = TrackingTask {
                id: local_id::generate_local_id(),
                task_type: tracking_task_type(kind),
            };
            let payload = json!({"user_id": user_id.clone(), "entity_id": entity_id.clone(), "kind": kind.request_kind()});
            if let Err(error) =
                start_tracking_task(&state, &task, source, &user_id, &payload, Utc::now()).await
            {
                return db_failure(error);
            }
            run_inline_tracked_translation(&state, &user_id, &entity_id, kind, source, &task).await;
            task_events_sse(state, task.id)
        }
    }
}

fn tracking_task_type(kind: TargetKind) -> &'static str {
    match kind {
        TargetKind::ReleaseSummary => "translate.release",
        TargetKind::ReleaseDetail => "translate.release_detail",
        TargetKind::Notification => "translate.notification",
    }
}

/// Persist the request rows, enqueue one PostgreSQL task, and report the id.
///
/// A target that is not resolvable still enqueues a task, mirroring the legacy
/// enqueue-first contract: the task fails visibly instead of the caller seeing
/// a synchronous 404 on an asynchronous return mode. Without an AI provider the
/// task is finalized with the legacy `disabled` payload.
async fn enqueue_translation_task(
    state: &PostgresServerState,
    user_id: &str,
    entity_id: &str,
    kind: TargetKind,
    source: &'static str,
) -> Result<(String, &'static str), sqlx::Error> {
    let task_id = local_id::generate_local_id();
    let now = Utc::now();
    let mut payload = json!({
        "user_id": user_id,
        "kind": kind.request_kind(),
        "entity_id": entity_id,
    });
    if state.ai.is_none() {
        // Without a provider the legacy worker still finalizes the task with a
        // disabled payload instead of leaving it queued forever.
        let result =
            json!({"lang": TARGET_LANG, "status": "disabled", "title": null, "summary": null});
        sqlx::query(
            "INSERT INTO public.job_tasks (id, task_type, status, source, requested_by, payload_json, result_json, created_at, updated_at, started_at, finished_at) VALUES ($1, 'translation.generate', 'succeeded', $2, $3, $4, $5, $6, $6, $6, $6)",
        )
        .bind(&task_id)
        .bind(source)
        .bind(user_id)
        .bind(payload.to_string())
        .bind(result.to_string())
        .bind(now)
        .execute(&state.runtime.pool)
        .await?;
        append_task_event(
            &state.runtime.pool,
            &task_id,
            "task.completed",
            json!({"task_id": task_id, "status": "succeeded", "summary": result}),
            now,
        )
        .await?;
        return Ok((task_id, "succeeded"));
    }
    let targets = load_targets(
        &state.runtime.pool,
        user_id,
        kind,
        source,
        std::slice::from_ref(&entity_id.to_owned()),
    )
    .await?;
    if let Some(target) = targets.get(entity_id) {
        let scope = begin_request(&state.runtime.pool, user_id, target, "async", false).await?;
        payload["work_item_id"] = json!(scope.work_item_id);
        if let Some(request_id) = scope.request_id.as_deref() {
            payload["request_id"] = json!(request_id);
        }
        payload["release_id"] = json!(entity_id);
    }
    sqlx::query(
        "INSERT INTO public.job_tasks (id, task_type, status, source, requested_by, payload_json, created_at, updated_at) VALUES ($1, 'translation.generate', 'queued', $2, $3, $4, $5, $5)",
    )
    .bind(&task_id)
    .bind(source)
    .bind(user_id)
    .bind(payload.to_string())
    .bind(now)
    .execute(&state.runtime.pool)
    .await?;
    Ok((task_id, "queued"))
}

async fn batch_items(
    state: &PostgresServerState,
    user_id: &str,
    kind: TargetKind,
    source: &'static str,
    entities: &[String],
) -> Result<Vec<TranslateBatchItem>, sqlx::Error> {
    let Some(ai) = state.ai.as_ref() else {
        return Ok(entities.iter().map(|id| disabled_item(id)).collect());
    };
    let targets = load_targets(&state.runtime.pool, user_id, kind, source, entities).await?;
    let mut items = Vec::with_capacity(entities.len());
    for entity_id in entities {
        let outcome = match targets.get(entity_id) {
            Some(target) => {
                translate_target(
                    &state.runtime.pool,
                    &state.http,
                    ai,
                    user_id,
                    target,
                    "wait",
                    None,
                )
                .await?
            }
            None => Outcome::Missing {
                message: kind.missing_message().to_owned(),
            },
        };
        items.push(outcome.batch_item(entity_id));
    }
    Ok(items)
}

async fn run_inline_tracked_translation(
    state: &PostgresServerState,
    user_id: &str,
    entity_id: &str,
    kind: TargetKind,
    source: &'static str,
    task: &TrackingTask,
) {
    let now = Utc::now();
    let outcome = match state.ai.as_ref() {
        None => Outcome::Disabled,
        Some(ai) => match load_targets(
            &state.runtime.pool,
            user_id,
            kind,
            source,
            std::slice::from_ref(&entity_id.to_owned()),
        )
        .await
        {
            Ok(targets) => match targets.get(entity_id) {
                Some(target) => {
                    match translate_target(
                        &state.runtime.pool,
                        &state.http,
                        ai,
                        user_id,
                        target,
                        "stream",
                        Some(task),
                    )
                    .await
                    {
                        Ok(outcome) => outcome,
                        Err(error) => {
                            let _ = complete_tracking_task(
                                state,
                                task,
                                "failed",
                                None,
                                Some("database_error"),
                                Utc::now(),
                            )
                            .await;
                            warn!(%error, task_id = %task.id, source, "PostgreSQL tracked translation failed");
                            return;
                        }
                    }
                }
                None => Outcome::Missing {
                    message: kind.missing_message().to_owned(),
                },
            },
            Err(error) => {
                let _ = complete_tracking_task(
                    state,
                    task,
                    "failed",
                    None,
                    Some("database_error"),
                    Utc::now(),
                )
                .await;
                warn!(%error, task_id = %task.id, source, "PostgreSQL tracked translation failed");
                return;
            }
        },
    };
    let item = outcome.batch_item(entity_id);
    let progress = json!({
        "task_id": task.id,
        "stage": kind.audit_stage(),
        "entity_id": entity_id,
        "release_id": entity_id,
        "item_status": item.status,
        "item_error": item.error.clone(),
    });
    let _ = append_task_event(
        &state.runtime.pool,
        &task.id,
        "task.progress",
        progress,
        now,
    )
    .await;
    let summary = json!({
        "total": 1,
        "ready": usize::from(item.status == "ready"),
        "disabled": usize::from(item.status == "disabled"),
        "missing": usize::from(item.status == "missing"),
        "error": usize::from(item.status == "error"),
    });
    let (status, error) = match item.status.as_str() {
        "error" | "missing" => ("failed", item.error.clone()),
        _ => ("succeeded", None),
    };
    let _ = complete_tracking_task(
        state,
        task,
        status,
        Some(&summary),
        error.as_deref(),
        Utc::now(),
    )
    .await;
}

async fn run_release_batch_stream(
    state: PostgresServerState,
    user_id: String,
    release_ids: Vec<i64>,
    task: TrackingTask,
    tx: mpsc::Sender<Result<Bytes, Infallible>>,
) {
    let started_at = Utc::now();
    let mut ready = 0usize;
    let mut disabled = 0usize;
    let mut missing = 0usize;
    let mut error_count = 0usize;
    let collect = json!({
        "task_id": task.id,
        "stage": "collect",
        "total_releases": release_ids.len(),
    });
    let _ = append_task_event(
        &state.runtime.pool,
        &task.id,
        "task.progress",
        collect,
        started_at,
    )
    .await;

    let Some(ai) = state.ai.as_ref() else {
        for release_id in &release_ids {
            let item = disabled_item(&release_id.to_string());
            if !send_batch_event(&tx, json!({"event": "item", "item": item})).await {
                let _ = complete_tracking_task(
                    &state,
                    &task,
                    "failed",
                    None,
                    Some("stream client disconnected"),
                    Utc::now(),
                )
                .await;
                return;
            }
            disabled += 1;
            let progress = item_progress_payload(&task.id, &item, release_id);
            let _ = append_task_event(
                &state.runtime.pool,
                &task.id,
                "task.progress",
                progress,
                Utc::now(),
            )
            .await;
        }
        finish_release_stream(
            &state,
            &task,
            (release_ids.len(), ready, disabled, missing, error_count),
            None,
            &tx,
        )
        .await;
        return;
    };

    let entities = release_ids.iter().map(i64::to_string).collect::<Vec<_>>();
    let targets = match load_targets(
        &state.runtime.pool,
        &user_id,
        TargetKind::ReleaseSummary,
        "api.translate_releases_batch_stream",
        &entities,
    )
    .await
    {
        Ok(targets) => targets,
        Err(error) => {
            clean_failed_stream(&state, &task, "database_error: stream worker failed", &tx).await;
            warn!(%error, task_id = %task.id, "PostgreSQL translation batch stream load failed");
            return;
        }
    };

    for release_id in &release_ids {
        let entity_id = release_id.to_string();
        let outcome = match targets.get(&entity_id) {
            Some(target) => {
                match translate_target(
                    &state.runtime.pool,
                    &state.http,
                    ai,
                    &user_id,
                    target,
                    "stream",
                    Some(&task),
                )
                .await
                {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        clean_failed_stream(
                            &state,
                            &task,
                            "database_error: stream worker failed",
                            &tx,
                        )
                        .await;
                        warn!(%error, task_id = %task.id, "PostgreSQL translation batch stream item failed");
                        return;
                    }
                }
            }
            None => Outcome::Missing {
                message: TargetKind::ReleaseSummary.missing_message().to_owned(),
            },
        };
        let item = outcome.batch_item(&entity_id);
        match item.status.as_str() {
            "ready" => ready += 1,
            "disabled" => disabled += 1,
            "missing" => missing += 1,
            _ => error_count += 1,
        }
        if !send_batch_event(&tx, json!({"event": "item", "item": item})).await {
            let _ = complete_tracking_task(
                &state,
                &task,
                "failed",
                None,
                Some("stream client disconnected"),
                Utc::now(),
            )
            .await;
            return;
        }
        let progress = item_progress_payload(&task.id, &item, release_id);
        let _ = append_task_event(
            &state.runtime.pool,
            &task.id,
            "task.progress",
            progress,
            Utc::now(),
        )
        .await;
    }

    finish_release_stream(
        &state,
        &task,
        (release_ids.len(), ready, disabled, missing, error_count),
        None,
        &tx,
    )
    .await;
}

fn item_progress_payload(task_id: &str, item: &TranslateBatchItem, release_id: &i64) -> Value {
    json!({
        "task_id": task_id,
        "stage": "release",
        "release_id": release_id,
        "item_status": item.status,
        "item_error": item.error.clone(),
    })
}

async fn finish_release_stream(
    state: &PostgresServerState,
    task: &TrackingTask,
    counts: (usize, usize, usize, usize, usize),
    error: Option<&str>,
    tx: &mpsc::Sender<Result<Bytes, Infallible>>,
) {
    let (total, ready, disabled, missing, error_count) = counts;
    let summary = json!({
        "total": total,
        "ready": ready,
        "disabled": disabled,
        "missing": missing,
        "error": error_count,
    });
    let status = if error.is_some() {
        "failed"
    } else {
        "succeeded"
    };
    let completed =
        complete_tracking_task(state, task, status, Some(&summary), error, Utc::now()).await;
    match completed {
        Ok(true) => {
            let _ = send_batch_event(tx, json!({"event": "done"})).await;
        }
        Ok(false) => {
            let _ = send_batch_event(
                tx,
                json!({"event": "error", "error": "translation stream task was not finalized"}),
            )
            .await;
        }
        Err(db_error) => {
            warn!(%db_error, task_id = %task.id, "PostgreSQL translation stream finalize failed");
            let _ = send_batch_event(
                tx,
                json!({"event": "error", "error": "translation stream task was not finalized"}),
            )
            .await;
        }
    }
}

async fn clean_failed_stream(
    state: &PostgresServerState,
    task: &TrackingTask,
    message: &str,
    tx: &mpsc::Sender<Result<Bytes, Infallible>>,
) {
    let _ = complete_tracking_task(state, task, "failed", None, Some(message), Utc::now()).await;
    let _ = send_batch_event(tx, json!({"event": "error", "error": message})).await;
}

async fn send_batch_event(tx: &mpsc::Sender<Result<Bytes, Infallible>>, event: Value) -> bool {
    let mut line = event.to_string();
    line.push('\n');
    tx.send(Ok(Bytes::from(line))).await.is_ok()
}

// ---------------------------------------------------------------------------
// Task lifecycle and streaming
// ---------------------------------------------------------------------------

async fn start_tracking_task(
    state: &PostgresServerState,
    task: &TrackingTask,
    source: &'static str,
    user_id: &str,
    payload: &Value,
    now: chrono::DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO public.job_tasks (id, task_type, status, source, requested_by, payload_json, created_at, updated_at, started_at, runtime_owner_id) VALUES ($1, $2, 'running', $3, $4, $5, $6, $6, $6, $7)",
    )
    .bind(&task.id)
    .bind(task.task_type)
    .bind(source)
    .bind(user_id)
    .bind(payload.to_string())
    .bind(now)
    .bind(state.owner_id.as_ref())
    .execute(&state.runtime.pool)
    .await?;
    Ok(())
}

async fn append_task_event(
    pool: &PgPool,
    task_id: &str,
    event_type: &str,
    payload: Value,
    now: chrono::DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO public.job_task_events (id, task_id, event_type, payload_json, created_at) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(local_id::generate_local_id())
    .bind(task_id)
    .bind(event_type)
    .bind(payload.to_string())
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

async fn complete_tracking_task(
    state: &PostgresServerState,
    task: &TrackingTask,
    status: &'static str,
    summary: Option<&Value>,
    error: Option<&str>,
    now: chrono::DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let result_json = summary.map(Value::to_string);
    let updated = sqlx::query(
        "UPDATE public.job_tasks SET status = $1, result_json = $2, error_message = $3, finished_at = $4, updated_at = $4 WHERE id = $5 AND runtime_owner_id = $6 AND status = 'running'",
    )
    .bind(status)
    .bind(result_json)
    .bind(error)
    .bind(now)
    .bind(&task.id)
    .bind(state.owner_id.as_ref())
    .execute(&state.runtime.pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Ok(false);
    }
    let payload = match summary {
        Some(summary) => json!({"task_id": task.id, "status": status, "summary": summary}),
        None => json!({"task_id": task.id, "status": status, "error": error}),
    };
    append_task_event(
        &state.runtime.pool,
        &task.id,
        "task.completed",
        payload,
        now,
    )
    .await?;
    Ok(true)
}

/// Stream the task event log with the legacy SSE framing: one `event:` name and
/// one JSON `data:` payload per persisted `job_task_events` row, ending after
/// the terminal `task.completed` event.
fn task_events_sse(state: PostgresServerState, task_id: String) -> Response {
    struct Cursor {
        pool: PgPool,
        task_id: String,
        offset: i64,
        polls: u32,
    }
    let cursor = Cursor {
        pool: state.runtime.pool.clone(),
        task_id,
        offset: 0,
        polls: 0,
    };
    let events = stream::unfold(cursor, |mut cursor| async move {
        loop {
            let rows = sqlx::query(
                "SELECT id, event_type, payload_json FROM public.job_task_events WHERE task_id = $1 ORDER BY created_at ASC, id ASC LIMIT 100 OFFSET $2",
            )
            .bind(&cursor.task_id)
            .bind(cursor.offset)
            .fetch_all(&cursor.pool)
            .await;
            let rows = match rows {
                Ok(rows) => rows,
                Err(error) => {
                    warn!(%error, task_id = %cursor.task_id, "PostgreSQL task event stream read failed");
                    return None;
                }
            };
            if let Some(row) = rows.first() {
                let id: String = match row.try_get("id") {
                    Ok(id) => id,
                    Err(_) => return None,
                };
                let event_type: String = row.try_get("event_type").ok()?;
                let payload_json: String = row.try_get("payload_json").ok()?;
                cursor.offset += 1;
                let event = Event::default().id(id).event(event_type).data(payload_json);
                return Some((Ok::<Event, Infallible>(event), cursor));
            }
            let status = sqlx::query_scalar::<_, String>(
                "SELECT status FROM public.job_tasks WHERE id = $1 LIMIT 1",
            )
            .bind(&cursor.task_id)
            .fetch_optional(&cursor.pool)
            .await;
            match status {
                Ok(Some(status))
                    if matches!(status.as_str(), "succeeded" | "failed" | "cancelled") =>
                {
                    return None;
                }
                Ok(None) => return None,
                Ok(Some(_)) => {}
                Err(error) => {
                    warn!(%error, task_id = %cursor.task_id, "PostgreSQL task status read failed");
                    return None;
                }
            }
            if cursor.polls >= TASK_EVENT_POLLS {
                return None;
            }
            cursor.polls += 1;
            tokio::time::sleep(TASK_EVENT_POLL_INTERVAL).await;
        }
    });
    Sse::new(events)
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        )
        .into_response()
}

// ---------------------------------------------------------------------------
// Source loading
// ---------------------------------------------------------------------------

async fn load_targets(
    pool: &PgPool,
    user_id: &str,
    kind: TargetKind,
    source: &str,
    entities: &[String],
) -> Result<HashMap<String, Target>, sqlx::Error> {
    match kind {
        TargetKind::ReleaseSummary | TargetKind::ReleaseDetail => {
            let release_ids = entities
                .iter()
                .filter_map(|value| value.parse::<i64>().ok())
                .collect::<Vec<_>>();
            if release_ids.is_empty() {
                return Ok(HashMap::new());
            }
            load_release_targets(pool, user_id, kind, source, &release_ids).await
        }
        TargetKind::Notification => {
            if entities.is_empty() {
                return Ok(HashMap::new());
            }
            load_notification_targets(pool, user_id, source, entities).await
        }
    }
}

async fn load_release_targets(
    pool: &PgPool,
    user_id: &str,
    kind: TargetKind,
    source: &str,
    release_ids: &[i64],
) -> Result<HashMap<String, Target>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT r.release_id, r.repo_id, sr.full_name, r.html_url, r.tag_name, r.name, r.body,
               r.updated_at, r.detected_at,
               EXISTS(
                 SELECT 1
                 FROM public.brief_release_memberships m
                 JOIN public.briefs b ON b.id = m.brief_id
                 WHERE b.user_id = $1 AND m.release_id = r.release_id
               ) AS brief_visible
        FROM public.repo_releases r
        LEFT JOIN public.user_release_visible_repos sr
          ON sr.user_id = $1 AND sr.repo_id = r.repo_id
        WHERE r.release_id = ANY($2)
        "#,
    )
    .bind(user_id)
    .bind(release_ids)
    .fetch_all(pool)
    .await?;

    let mut targets = HashMap::with_capacity(rows.len());
    for row in rows {
        let release_id: i64 = row.try_get("release_id")?;
        let repo_id: i64 = row.try_get("repo_id")?;
        let visible_full_name: Option<String> = row.try_get("full_name")?;
        let brief_visible: bool = row.try_get("brief_visible")?;
        if visible_full_name.is_none() && !(kind.allows_brief_access() && brief_visible) {
            continue;
        }
        let html_url: String = row.try_get("html_url")?;
        let tag_name: String = row.try_get("tag_name")?;
        let name: Option<String> = row.try_get("name")?;
        let body: Option<String> = row.try_get("body")?;
        let updated_at: Option<String> = row.try_get("updated_at")?;
        let detected_at: Option<String> = row.try_get("detected_at")?;

        let repo_full_name = visible_full_name
            .filter(|value| !value.trim().is_empty())
            .or_else(|| crate::release_links::parse_repo_full_name_from_release_url(&html_url))
            .unwrap_or_else(|| format!("unknown/{repo_id}"));
        let title = name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| tag_name.trim())
            .to_owned();
        let body = body
            .map(|value| value.replace("\r\n", "\n"))
            .map(|value| value.trim().to_owned())
            .unwrap_or_default();

        let tiebreak = crate::content_source_identity::release_source_revision_content_tiebreak(&[
            html_url.as_str(),
            tag_name.as_str(),
            title.as_str(),
            body.as_str(),
        ]);
        let observed_at = crate::content_source_identity::authoritative_release_revision(
            updated_at.as_deref().unwrap_or_default(),
            detected_at.as_deref(),
        );
        let source_hash = crate::content_source_identity::release_detail_source_hash(
            &repo_full_name,
            &title,
            &body,
        );
        let mut source_blocks = vec![
            TranslationSourceBlock {
                slot: "metadata".to_owned(),
                text: format!("{repo_full_name}\n{}", tag_name.trim()),
            },
            TranslationSourceBlock {
                slot: "title".to_owned(),
                text: title.clone(),
            },
        ];
        if !body.is_empty() {
            source_blocks.push(TranslationSourceBlock {
                slot: "body_markdown".to_owned(),
                text: body.clone(),
            });
        }
        let source_blocks =
            with_source_revision(source_blocks, observed_at, Some(tiebreak.as_str()));
        let target_slots = match (kind, body.is_empty()) {
            (TargetKind::ReleaseDetail, false) => vec!["title_zh".to_owned(), "body_md".to_owned()],
            (TargetKind::ReleaseDetail, true) => vec!["title_zh".to_owned()],
            _ => vec!["title_zh".to_owned(), "summary_md".to_owned()],
        };
        targets.insert(
            release_id.to_string(),
            Target {
                entity_id: release_id.to_string(),
                kind,
                variant: kind.work_variant().to_owned(),
                source: source.to_owned(),
                title,
                body,
                repo_full_name,
                reason: String::new(),
                subject_type: String::new(),
                source_blocks,
                target_slots,
                source_hash,
            },
        );
    }
    Ok(targets)
}

async fn load_notification_targets(
    pool: &PgPool,
    user_id: &str,
    source: &str,
    thread_ids: &[String],
) -> Result<HashMap<String, Target>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT thread_id, repo_full_name, subject_title, reason, subject_type, updated_at
        FROM public.notifications
        WHERE user_id = $1 AND thread_id = ANY($2)
        ORDER BY updated_at DESC NULLS LAST, id DESC
        "#,
    )
    .bind(user_id)
    .bind(thread_ids)
    .fetch_all(pool)
    .await?;

    let mut targets = HashMap::with_capacity(rows.len());
    for row in rows {
        let thread_id: String = row.try_get("thread_id")?;
        if targets.contains_key(&thread_id) {
            continue;
        }
        let repo_full_name: Option<String> = row.try_get("repo_full_name")?;
        let subject_title: Option<String> = row.try_get("subject_title")?;
        let reason: Option<String> = row.try_get("reason")?;
        let subject_type: Option<String> = row.try_get("subject_type")?;
        let updated_at: Option<String> = row.try_get("updated_at")?;

        let repo_full_name = repo_full_name
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "(unknown repo)".to_owned());
        let title = subject_title
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "(no title)".to_owned());
        let reason = reason.unwrap_or_default();
        let subject_type = subject_type.unwrap_or_default();

        let tiebreak = crate::content_source_identity::notification_source_revision_tiebreak(
            thread_id.as_str(),
            Some(repo_full_name.as_str()),
            Some(title.as_str()),
            Some(reason.as_str()),
            Some(subject_type.as_str()),
        );
        let source_hash = crate::content_source_identity::sha256_hex(&format!(
            "kind=notification\nrepo={repo_full_name}\ntitle={title}\nreason={reason}\nsubject_type={subject_type}\n"
        ));
        let source_blocks = with_source_revision(
            vec![
                TranslationSourceBlock {
                    slot: "metadata".to_owned(),
                    text: format!("repo={repo_full_name}\nsubject_type={subject_type}"),
                },
                TranslationSourceBlock {
                    slot: "title".to_owned(),
                    text: title.clone(),
                },
            ],
            updated_at.as_deref(),
            Some(tiebreak.as_str()),
        );
        targets.insert(
            thread_id.clone(),
            Target {
                entity_id: thread_id,
                kind: TargetKind::Notification,
                variant: TargetKind::Notification.work_variant().to_owned(),
                source: source.to_owned(),
                title,
                body: String::new(),
                repo_full_name,
                reason,
                subject_type,
                source_blocks,
                target_slots: vec!["title_zh".to_owned(), "summary_md".to_owned()],
                source_hash,
            },
        );
    }
    Ok(targets)
}

/// Publish the projection for a finished translation attempt.
///
/// The projection identity is rebuilt twice with the same construction the
/// global read path uses (`postgres_content_overlay::identity_item`):
///
/// * from the attempt's persisted source blocks — the text that was actually
///   translated;
/// * from the stored release row — the text readers will look up.
///
/// Equal identities publish; a mismatch means the release changed after the
/// work item was enqueued, so the output describes text no reader can resolve
/// and publishing it would strand an unreachable row. A mismatch therefore
/// logs and skips instead of writing a projection.
///
/// Results the shared output contract rejects are also skipped. Neither skip
/// rewrites the attempt state, which stays the honest record of what ran.
async fn publish_finished_translation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    target: &Target,
    title: Option<&str>,
    summary: Option<&str>,
) -> anyhow::Result<()> {
    // Release detail is the only lane kind whose read path consumes a release
    // content projection. Notifications read through their translation request,
    // and the release feed body has its own truncated identity, so neither may
    // publish under the detail identity.
    if target.kind != TargetKind::ReleaseDetail {
        return Ok(());
    }
    let Some(attempted) = attempted_identity_item(target) else {
        return Ok(());
    };
    let Some(current) = release_publication_item(tx, target).await? else {
        return Ok(());
    };
    let attempted_hash = crate::content_source_identity::source_hash_for_item(&attempted);
    let current_hash = crate::content_source_identity::source_hash_for_item(&current);
    if attempted_hash != current_hash {
        warn!(
            release_id = %target.entity_id,
            attempted_source_hash = %attempted_hash,
            canonical_source_hash = %current_hash,
            "release content changed after the attempt was enqueued; projection not published"
        );
        return Ok(());
    }
    let raw = translations_payload_json(title, summary, &current.target_slots);
    let output = match crate::content_processing::validate_output(
        &raw,
        &current.target_slots,
        &current.source_blocks,
    ) {
        Ok(output) => output,
        Err(error) => {
            warn!(
                release_id = %target.entity_id,
                variant = %current.variant,
                error = %error,
                "translation result failed the global output contract; projection not published"
            );
            return Ok(());
        }
    };
    crate::postgres_content::publish_translation_result(
        tx,
        crate::postgres_content::TranslationPublication {
            canonical_resource_type: "release",
            pipeline: "translation",
            item: &current,
            source_hash: &current_hash,
            payload: &output,
        },
    )
    .await
    .context("publish translation projection")?;
    Ok(())
}

/// Projection identity rebuilt from the attempt's persisted source blocks.
///
/// The metadata block carries `repository\ntag`; the reader builds the same
/// metadata text from the release row, so this reproduces its identity for the
/// text that was actually translated.
fn attempted_identity_item(target: &Target) -> Option<TranslationRequestItemInput> {
    let metadata = source_block_text(&target.source_blocks, "metadata")?.to_owned();
    let title = source_block_text(&target.source_blocks, "title")?.to_owned();
    let body = source_block_text(&target.source_blocks, "body_markdown").map(str::to_owned);
    let mut item = crate::postgres_content_overlay::identity_item(
        "release_detail",
        "detail",
        &target.entity_id,
        metadata,
        title,
        body,
    );
    item.producer_ref = "postgres.translate".to_owned();
    Some(item)
}

/// Rebuild the projection identity item for a release from its stored row.
///
/// Mirrors `postgres_content_overlay::ReleaseSource::identity_hash` for the
/// release-detail identity: repository identity from the release URL (with the
/// `unknown/<repo_id>` fallback), the tag line in metadata, the display title,
/// and the body with line endings normalized but not trimmed.
async fn release_publication_item(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    target: &Target,
) -> anyhow::Result<Option<TranslationRequestItemInput>> {
    let entity_id = target.entity_id.as_str();
    let Some(release_id) = entity_id.parse::<i64>().ok() else {
        return Ok(None);
    };
    let Some(row) = sqlx::query(
        "SELECT repo_id, html_url, tag_name, name, body FROM public.repo_releases WHERE release_id = $1",
    )
    .bind(release_id)
    .fetch_optional(&mut **tx)
    .await
    .context("load release row for projection publication")?
    else {
        return Ok(None);
    };
    let repo_id: i64 = row.try_get("repo_id")?;
    let html_url: String = row.try_get("html_url")?;
    let tag_name: String = row.try_get("tag_name")?;
    let name: Option<String> = row.try_get("name")?;
    let body: Option<String> = row.try_get("body")?;
    let repo_identity = crate::postgres_content_overlay::release_repo_identity(&html_url, repo_id);
    let title = name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(tag_name.as_str())
        .to_owned();
    let mut item = crate::postgres_content_overlay::identity_item(
        "release_detail",
        "detail",
        entity_id,
        format!("{repo_identity}\n{tag_name}"),
        title,
        body,
    );
    item.producer_ref = "postgres.translate".to_owned();
    Ok(Some(item))
}

/// Render stored translation fields as the JSON object the output contract
/// validates.
///
/// `validate_output` reads a model response; the lane stores the parsed slots in
/// columns, so this rebuilds the same object for contract validation instead of
/// duplicating the slot rules.
fn translations_payload_json(
    title: Option<&str>,
    summary: Option<&str>,
    target_slots: &[String],
) -> String {
    let mut object = serde_json::Map::new();
    for slot in target_slots {
        let value = match slot.as_str() {
            "title_zh" => title.map(str::to_owned).map(Value::String),
            "body_md" | "summary_md" => summary.map(str::to_owned).map(Value::String),
            _ => continue,
        };
        object.insert(slot.clone(), value.unwrap_or(Value::Null));
    }
    Value::Object(object).to_string()
}

fn with_source_revision(
    mut source_blocks: Vec<TranslationSourceBlock>,
    revision: Option<&str>,
    revision_tiebreak: Option<&str>,
) -> Vec<TranslationSourceBlock> {
    if let Some(revision) = revision.filter(|value| !value.trim().is_empty()) {
        if let Some(revision_tiebreak) = revision_tiebreak {
            source_blocks.insert(
                0,
                TranslationSourceBlock {
                    slot: "source_revision_tiebreak".to_owned(),
                    text: revision_tiebreak.to_owned(),
                },
            );
        }
        source_blocks.insert(
            0,
            TranslationSourceBlock {
                slot: "source_observed_at".to_owned(),
                text: revision.to_owned(),
            },
        );
    }
    source_blocks
}

// ---------------------------------------------------------------------------
// Persisted translation attempt
// ---------------------------------------------------------------------------

struct RequestScope {
    /// Request that owns this attempt; absent only for work items without a
    /// persisted request row.
    request_id: Option<String>,
    work_item_id: String,
    variant: String,
    attempt_no: i32,
}

fn dedupe_key(user_id: &str, target: &Target) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}",
        user_id,
        target.kind.work_kind(),
        target.kind.work_variant(),
        target.entity_id,
        TARGET_LANG,
        target.source_hash
    )
}

async fn begin_request(
    pool: &PgPool,
    user_id: &str,
    target: &Target,
    request_mode: &'static str,
    start_running: bool,
) -> Result<RequestScope, sqlx::Error> {
    let now = Utc::now();
    let key = dedupe_key(user_id, target);
    let source_blocks_json =
        serde_json::to_string(&target.source_blocks).unwrap_or_else(|_| "[]".to_owned());
    let target_slots_json =
        serde_json::to_string(&target.target_slots).unwrap_or_else(|_| "[]".to_owned());
    let request_id = local_id::generate_local_id();
    let deadline_at = now + chrono::Duration::milliseconds(TRANSLATION_MAX_WAIT_MS);

    let mut tx = pool.begin().await?;
    let work_item_id = sqlx::query_scalar::<_, String>(
        "INSERT INTO public.translation_work_items (id, dedupe_key, scope_user_id, kind, variant, entity_id, target_lang, protocol_version, model_profile, source_hash, source_blocks_json, target_slots_json, token_estimate, deadline_at, status, created_at, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,'translation-request.v1','postgres-default',$8,$9,$10,0,$11,'queued',$12,$12) ON CONFLICT (dedupe_key) DO UPDATE SET updated_at = EXCLUDED.updated_at RETURNING id",
    )
    .bind(local_id::generate_local_id())
    .bind(&key)
    .bind(user_id)
    .bind(target.kind.work_kind())
    .bind(target.kind.work_variant())
    .bind(&target.entity_id)
    .bind(TARGET_LANG)
    .bind(&target.source_hash)
    .bind(&source_blocks_json)
    .bind(&target_slots_json)
    .bind(deadline_at)
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO public.translation_requests (id, mode, source, request_origin, requested_by, scope_user_id, producer_ref, kind, variant, entity_id, target_lang, max_wait_ms, source_hash, source_blocks_json, target_slots_json, work_item_id, status, created_at, updated_at) VALUES ($1,$2,'postgres','user',$3,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,'queued',$14,$14)",
    )
    .bind(&request_id)
    .bind(request_mode)
    .bind(user_id)
    .bind(&target.source)
    .bind(target.kind.request_kind())
    .bind(target.kind.request_variant())
    .bind(&target.entity_id)
    .bind(TARGET_LANG)
    .bind(TRANSLATION_MAX_WAIT_MS)
    .bind(&target.source_hash)
    .bind(&source_blocks_json)
    .bind(&target_slots_json)
    .bind(&work_item_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    let mut attempt_no = 0;
    if start_running {
        attempt_no = start_attempt(
            &mut tx,
            &work_item_id,
            Some(&request_id),
            user_id,
            target.kind,
            &target.variant,
            &target.entity_id,
            now,
        )
        .await?;
    }
    tx.commit().await?;

    Ok(RequestScope {
        request_id: Some(request_id),
        work_item_id,
        variant: target.variant.clone(),
        attempt_no,
    })
}

/// Admit Release detail translation for the Releases one sync inserted.
///
/// This is the Release producer of the global content model: a sync no longer
/// writes per-user `translation_work_items`/`translation_requests` rows or
/// `translation.generate` tasks, it hands each new Release to
/// [`crate::postgres_content::admit_release_detail_translation`], and the single
/// global content worker owns the attempt and the projection (REQ-PGCP-006).
///
/// Admission is keyed by the canonical identity, so a second sync of the same
/// source revision creates nothing and the count only reports work this call
/// actually created. The sync already committed its Release writes, so a failure
/// for one Release is logged and swallowed: the Release stays stored and the
/// next sync or explicit request re-admits it.
///
/// Returns how many Releases got new global translation work.
pub(crate) async fn admit_release_detail_translations(
    pool: &PgPool,
    release_ids: &[i64],
    fallback_model: &str,
    producer_ref: &str,
) -> usize {
    let release_ids = deduped_release_ids(release_ids);
    if release_ids.is_empty() {
        return 0;
    }
    let Ok(rows) = sqlx::query(
        "SELECT release_id, name, body FROM public.repo_releases WHERE release_id = ANY($1)",
    )
    .bind(&release_ids)
    .fetch_all(pool)
    .await
    else {
        warn!(
            event = "content.admission",
            producer_ref,
            releases = release_ids.len(),
            "could not load Release content for global translation admission"
        );
        return 0;
    };
    let mut translatable = HashSet::with_capacity(rows.len());
    for row in rows {
        let release_id: i64 = match row.try_get("release_id") {
            Ok(value) => value,
            Err(_) => continue,
        };
        let name: Option<String> = row.try_get("name").ok().flatten();
        let body: Option<String> = row.try_get("body").ok().flatten();
        if release_has_translatable_content(name.as_deref(), body.as_deref()) {
            translatable.insert(release_id);
        }
    }
    let mut admitted = 0usize;
    for release_id in release_ids {
        if !translatable.contains(&release_id) {
            continue;
        }
        match crate::postgres_content::admit_release_detail_translation(
            pool,
            release_id,
            fallback_model,
            producer_ref,
        )
        .await
        {
            Ok(work) => {
                if work.outcome == crate::postgres_content::DetailAdmission::Created {
                    admitted += 1;
                }
            }
            Err(error) => warn!(
                event = "content.admission",
                producer_ref,
                release_id,
                error = %error,
                "global Release detail translation admission failed"
            ),
        }
    }
    if admitted > 0 {
        info!(
            event = "content.admission",
            producer_ref, admitted, "global Release detail translation work admitted"
        );
    }
    admitted
}

/// Deduplicate the caller's release ids while keeping the caller's order.
fn deduped_release_ids(release_ids: &[i64]) -> Vec<i64> {
    let mut seen = HashSet::with_capacity(release_ids.len());
    release_ids
        .iter()
        .copied()
        .filter(|release_id| seen.insert(*release_id))
        .collect()
}

/// A release with neither a name nor a body has nothing to translate. The
/// title falls back to the tag name at read time, which would name a target but
/// still send an empty body to the provider.
fn release_has_translatable_content(name: Option<&str>, body: Option<&str>) -> bool {
    let filled = |value: Option<&str>| value.is_some_and(|value| !value.trim().is_empty());
    filled(name) || filled(body)
}

/// Open the next attempt on an existing work item.
///
/// Shared by [`begin_request`] (new work item) and
/// [`translate_work_item_for_task`] (work item already persisted by the
/// `task_id` enqueue) so both paths write identical attempt events.
#[allow(clippy::too_many_arguments)]
async fn start_attempt(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    work_item_id: &str,
    request_id: Option<&str>,
    user_id: &str,
    kind: TargetKind,
    variant: &str,
    entity_id: &str,
    now: chrono::DateTime<Utc>,
) -> Result<i32, sqlx::Error> {
    let attempt_no = sqlx::query_scalar::<_, i32>(
        "SELECT (COALESCE(MAX(attempt_no), 0) + 1)::integer FROM public.translation_attempt_events WHERE work_item_id = $1",
    )
    .bind(work_item_id)
    .fetch_one(&mut **tx)
    .await?;
    insert_attempt_event(
        tx,
        work_item_id,
        request_id,
        user_id,
        kind,
        variant,
        entity_id,
        attempt_no,
        "attempt_queued",
        None,
        None,
        None,
        false,
        "[]",
        now,
    )
    .await?;
    sqlx::query(
        "UPDATE public.translation_work_items SET status = 'running', started_at = $1, updated_at = $1 WHERE id = $2",
    )
    .bind(now)
    .bind(work_item_id)
    .execute(&mut **tx)
    .await?;
    if let Some(request_id) = request_id {
        sqlx::query(
            "UPDATE public.translation_requests SET status = 'running', started_at = $1, updated_at = $1 WHERE id = $2",
        )
        .bind(now)
        .bind(request_id)
        .execute(&mut **tx)
        .await?;
    }
    insert_attempt_event(
        tx,
        work_item_id,
        request_id,
        user_id,
        kind,
        variant,
        entity_id,
        attempt_no,
        "attempt_started",
        None,
        None,
        None,
        false,
        "[]",
        now,
    )
    .await?;
    Ok(attempt_no)
}

#[allow(clippy::too_many_arguments)]
async fn insert_attempt_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    work_item_id: &str,
    request_id: Option<&str>,
    user_id: &str,
    kind: TargetKind,
    variant: &str,
    entity_id: &str,
    attempt_no: i32,
    event_type: &str,
    result_status: Option<&str>,
    error_code: Option<&str>,
    error_summary: Option<&str>,
    retry_eligible: bool,
    llm_call_ids_json: &str,
    now: chrono::DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let failure_class = if event_type == "attempt_completed" {
        error_code
    } else {
        None
    };
    sqlx::query(
        "INSERT INTO public.translation_attempt_events (id, work_item_id, request_id, scope_user_id, kind, variant, entity_id, target_lang, attempt_no, trigger, event_type, result_status, error_code, error_summary, failure_class, retry_eligible, llm_call_ids_json, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,'initial',$10,$11,$12,$13,$14,$15,$16,$17) ON CONFLICT (work_item_id, attempt_no, event_type) DO NOTHING",
    )
    .bind(local_id::generate_local_id())
    .bind(work_item_id)
    .bind(request_id)
    .bind(user_id)
    .bind(kind.work_kind())
    .bind(variant)
    .bind(entity_id)
    .bind(TARGET_LANG)
    .bind(attempt_no)
    .bind(event_type)
    .bind(result_status)
    .bind(error_code)
    .bind(error_summary)
    .bind(failure_class)
    .bind(i64::from(retry_eligible))
    .bind(llm_call_ids_json)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn cached_outcome(
    pool: &PgPool,
    key: &str,
    target: &Target,
) -> Result<Option<Outcome>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT status, result_status, title_zh, summary_md, body_md, error_text FROM public.translation_work_items WHERE dedupe_key = $1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let status: String = row.try_get("status")?;
    if status != "completed" {
        return Ok(None);
    }
    let result_status: Option<String> = row.try_get("result_status")?;
    let title: Option<String> = row.try_get("title_zh")?;
    let summary: Option<String> = row.try_get("summary_md")?;
    let body: Option<String> = row.try_get("body_md")?;
    let error_text: Option<String> = row.try_get("error_text")?;
    let outcome = match result_status.as_deref() {
        Some("ready") => {
            let summary = summary.or_else(|| body.clone());
            if !translation_ready(&target.body, summary.as_deref()) {
                return Ok(None);
            }
            Outcome::Ready { title, summary }
        }
        Some("disabled") => Outcome::Disabled,
        Some("missing") => Outcome::Missing {
            message: error_text.unwrap_or_else(|| target.kind.missing_message().to_owned()),
        },
        Some("error") => Outcome::Failed {
            message: error_text.unwrap_or_else(|| "translation failed".to_owned()),
            failure_class: None,
        },
        _ => return Ok(None),
    };
    Ok(Some(outcome))
}

fn translation_ready(body: &str, summary: Option<&str>) -> bool {
    if body.trim().is_empty() {
        return true;
    }
    summary.is_some_and(|value| !value.trim().is_empty())
}

async fn translate_target(
    pool: &PgPool,
    http: &Client,
    ai: &AiConfig,
    user_id: &str,
    target: &Target,
    request_mode: &'static str,
    tracking: Option<&TrackingTask>,
) -> Result<Outcome, sqlx::Error> {
    let key = dedupe_key(user_id, target);
    if let Some(cached) = cached_outcome(pool, &key, target).await? {
        return Ok(cached);
    }

    let scope = begin_request(pool, user_id, target, request_mode, true).await?;
    let parent = tracking.map(|task| (task.id.as_str(), task.task_type));
    let mut audit = audit_recorder(
        pool,
        http,
        ai,
        target.source.as_str(),
        user_id,
        &scope.work_item_id,
        scope.attempt_no,
        parent,
    );
    let result = run_translation(&mut audit, target).await;
    let call_ids_json = serde_json::to_string(&audit.call_ids).unwrap_or_else(|_| "[]".to_owned());
    let now = Utc::now();

    match result {
        Ok((title, summary)) => {
            finish_request(
                pool,
                user_id,
                target.kind,
                &target.entity_id,
                Some(target),
                &scope,
                AttemptOutcome {
                    result_status: "ready",
                    title: title.as_deref(),
                    summary: summary.as_deref(),
                    error_code: None,
                    error_summary: None,
                    retry_eligible: false,
                },
                &call_ids_json,
                now,
            )
            .await?;
            Ok(Outcome::Ready { title, summary })
        }
        Err(failure) => {
            finish_request(
                pool,
                user_id,
                target.kind,
                &target.entity_id,
                Some(target),
                &scope,
                AttemptOutcome {
                    result_status: "error",
                    title: None,
                    summary: None,
                    error_code: Some(failure.class.as_str()),
                    error_summary: Some(failure.message.as_str()),
                    retry_eligible: failure.class.is_recoverable(),
                },
                &call_ids_json,
                now,
            )
            .await?;
            Ok(Outcome::Failed {
                message: failure.message,
                failure_class: Some(failure.class.as_str()),
            })
        }
    }
}

/// Terminal state persisted for one translation attempt.
struct AttemptOutcome<'a> {
    result_status: &'a str,
    title: Option<&'a str>,
    summary: Option<&'a str>,
    error_code: Option<&'a str>,
    error_summary: Option<&'a str>,
    retry_eligible: bool,
}

/// Persist the terminal attempt state for the work item and its request.
#[allow(clippy::too_many_arguments)]
async fn finish_request(
    pool: &PgPool,
    user_id: &str,
    kind: TargetKind,
    entity_id: &str,
    publication: Option<&Target>,
    scope: &RequestScope,
    outcome: AttemptOutcome<'_>,
    call_ids_json: &str,
    now: chrono::DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let summary_slot = outcome.summary.filter(|value| !value.trim().is_empty());
    let mut tx = pool.begin().await?;
    if outcome.result_status == "ready" {
        // The projection commits with the terminal attempt state, so a stored
        // `ready` translation is always readable through the global projection.
        if let Some(target) = publication {
            publish_finished_translation(&mut tx, target, outcome.title, summary_slot)
                .await
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
        }
        sqlx::query(
            "UPDATE public.translation_work_items SET status = 'completed', result_status = 'ready', title_zh = $1, summary_md = $2, body_md = $2, error_text = NULL, finished_at = $3, updated_at = $3 WHERE id = $4",
        )
        .bind(outcome.title)
        .bind(summary_slot)
        .bind(now)
        .bind(&scope.work_item_id)
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query(
            "UPDATE public.translation_work_items SET status = 'failed', result_status = $1, error_text = $2, finished_at = $3, updated_at = $3 WHERE id = $4",
        )
        .bind(outcome.result_status)
        .bind(outcome.error_summary)
        .bind(now)
        .bind(&scope.work_item_id)
        .execute(&mut *tx)
        .await?;
    }
    if let Some(request_id) = scope.request_id.as_deref() {
        if outcome.result_status == "ready" {
            sqlx::query(
                "UPDATE public.translation_requests SET status = 'completed', result_status = 'ready', title_zh = $1, summary_md = $2, body_md = $2, error_text = NULL, finished_at = $3, updated_at = $3 WHERE id = $4",
            )
            .bind(outcome.title)
            .bind(summary_slot)
            .bind(now)
            .bind(request_id)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(
                "UPDATE public.translation_requests SET status = 'failed', result_status = $1, error_text = $2, finished_at = $3, updated_at = $3 WHERE id = $4",
            )
            .bind(outcome.result_status)
            .bind(outcome.error_summary)
            .bind(now)
            .bind(request_id)
            .execute(&mut *tx)
            .await?;
        }
    }
    insert_attempt_event(
        &mut tx,
        &scope.work_item_id,
        scope.request_id.as_deref(),
        user_id,
        kind,
        &scope.variant,
        entity_id,
        scope.attempt_no,
        "attempt_completed",
        Some(outcome.result_status),
        outcome.error_code,
        outcome.error_summary,
        outcome.retry_eligible,
        call_ids_json,
        now,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Close request rows whose work item already reached a terminal state.
///
/// A `translation_requests` row is only a view of its work item. An automatic
/// enqueue that re-created a request for a terminal work item, or a lost
/// finalization, leaves that view at `queued`/`running` after the work item is
/// `completed`/`failed`. Those rows can never be claimed, yet they inflate the
/// queue counters and the admin request list.
///
/// This repair is idempotent, performs no provider call, never rewrites attempt
/// or LLM history, and skips any work item that still has a queued or running
/// `translation.generate` task.
pub(crate) async fn reconcile_terminal_translation_requests(pool: &PgPool) -> anyhow::Result<u64> {
    let updated = sqlx::query(
        r#"
        UPDATE public.translation_requests AS request
        SET status = CASE
                WHEN work.status = 'completed' AND work.result_status = 'ready' THEN 'completed'
                ELSE 'failed'
            END,
            result_status = CASE
                WHEN work.status = 'completed' AND work.result_status = 'ready' THEN 'ready'
                WHEN work.result_status = 'ready' THEN 'error'
                ELSE COALESCE(work.result_status, 'error')
            END,
            title_zh = CASE
                WHEN work.status = 'completed' AND work.result_status = 'ready' THEN work.title_zh
                ELSE request.title_zh
            END,
            summary_md = CASE
                WHEN work.status = 'completed' AND work.result_status = 'ready' THEN work.summary_md
                ELSE request.summary_md
            END,
            body_md = CASE
                WHEN work.status = 'completed' AND work.result_status = 'ready' THEN work.body_md
                ELSE request.body_md
            END,
            error_text = CASE
                WHEN work.status = 'completed' AND work.result_status = 'ready' THEN NULL
                ELSE COALESCE(work.error_text, 'translation work reached a terminal state')
            END,
            finished_at = COALESCE(request.finished_at::timestamptz, now()),
            updated_at = now()
        FROM public.translation_work_items AS work
        WHERE request.work_item_id = work.id
          AND request.status IN ('queued', 'running')
          AND work.status IN ('completed', 'failed')
          AND NOT EXISTS (
            SELECT 1
            FROM public.job_tasks AS task
            WHERE task.task_type = 'translation.generate'
              AND task.status IN ('queued', 'running')
              AND pg_input_is_valid(task.payload_json, 'jsonb')
              AND task.payload_json::jsonb ->> 'work_item_id' = work.id
          )
        "#,
    )
    .execute(pool)
    .await
    .context("reconcile terminal PostgreSQL translation requests")?;
    Ok(updated.rows_affected())
}

/// Source blocks persisted on the work item when it was enqueued.
///
/// The persisted `source_blocks_json` is either the bare block array or a
/// snapshot object carrying a `source_blocks` array, the same shape the legacy
/// content-processing writer hashed into `source_hash`.
fn persisted_source_blocks(raw: &str) -> Option<Vec<TranslationSourceBlock>> {
    if let Ok(blocks) = serde_json::from_str::<Vec<TranslationSourceBlock>>(raw) {
        return (!blocks.is_empty()).then_some(blocks);
    }
    let value = serde_json::from_str::<Value>(raw).ok()?;
    let blocks =
        serde_json::from_value::<Vec<TranslationSourceBlock>>(value.get("source_blocks")?.clone())
            .ok()?;
    (!blocks.is_empty()).then_some(blocks)
}

/// Target slots persisted on the work item or request.
fn persisted_target_slots(raw: &str) -> Option<Vec<String>> {
    if let Ok(slots) = serde_json::from_str::<Vec<String>>(raw) {
        return (!slots.is_empty()).then_some(slots);
    }
    let value = serde_json::from_str::<Value>(raw).ok()?;
    let slots = serde_json::from_value::<Vec<String>>(value.get("target_slots")?.clone()).ok()?;
    (!slots.is_empty()).then_some(slots)
}

fn default_target_slots(kind: TargetKind) -> Vec<String> {
    let body_slot = match kind {
        TargetKind::ReleaseDetail => "body_md",
        TargetKind::ReleaseSummary | TargetKind::Notification => "summary_md",
    };
    vec!["title_zh".to_owned(), body_slot.to_owned()]
}

fn source_block_text<'a>(blocks: &'a [TranslationSourceBlock], slot: &str) -> Option<&'a str> {
    blocks
        .iter()
        .find(|block| block.slot == slot)
        .map(|block| block.text.as_str())
}

/// Warn once per process when a work item has no usable persisted snapshot.
fn warn_live_content_fallback(reason: &str) {
    static WARNED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    WARNED.get_or_init(|| {
        warn!(
            reason = reason,
            "translation work item has no usable persisted source snapshot; falling back to live content"
        );
    });
}

/// Replace the live release/notification fields with the work item's persisted
/// source blocks.
///
/// Blocks are read in the persisted metadata → title → body_markdown order: the
/// metadata block supplies the repository (and, for notifications, the subject
/// type), the title block the source title, and body_markdown the source body.
/// A persisted snapshot is authoritative, so a missing body block yields an
/// empty body instead of leaking the live text.
fn persisted_prompt_target(
    live: &Target,
    source_blocks_json: &str,
    target_slots_json: &str,
    kind: TargetKind,
) -> Target {
    let Some(blocks) = persisted_source_blocks(source_blocks_json) else {
        warn_live_content_fallback("persisted translation source blocks are absent or malformed");
        return live.clone();
    };
    let mut target = live.clone();
    target.target_slots =
        persisted_target_slots(target_slots_json).unwrap_or_else(|| default_target_slots(kind));
    if let Some(metadata) = source_block_text(&blocks, "metadata") {
        match kind {
            TargetKind::Notification => {
                for line in metadata.lines() {
                    if let Some(value) = line.strip_prefix("repo=") {
                        if !value.trim().is_empty() {
                            target.repo_full_name = value.trim().to_owned();
                        }
                    } else if let Some(value) = line.strip_prefix("subject_type=") {
                        target.subject_type = value.trim().to_owned();
                    }
                }
            }
            TargetKind::ReleaseSummary | TargetKind::ReleaseDetail => {
                if let Some(repo) = metadata
                    .lines()
                    .next()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    target.repo_full_name = repo.to_owned();
                }
            }
        }
    }
    if let Some(title) =
        source_block_text(&blocks, "title").filter(|value| !value.trim().is_empty())
    {
        target.title = title.trim().to_owned();
    }
    target.body = source_block_text(&blocks, "body_markdown")
        .or_else(|| source_block_text(&blocks, "excerpt"))
        .map(|body| body.replace("\r\n", "\n").trim().to_owned())
        .unwrap_or_default();
    target.source_blocks = blocks;
    target
}

/// Translate one `translation.generate` work item enqueued by
/// `return_mode=task_id`.
///
/// The durable request, work item and dedupe key are written when the task is
/// enqueued, so the worker opens a new attempt on that same work item instead
/// of creating a second request. The prompt is built from the work item's
/// persisted `source_blocks_json` (metadata, title, body_markdown), matching the
/// legacy content-processing pipeline, so a release edited after enqueue is
/// still translated from the snapshot its `source_hash` was computed for.
/// Model calls flow through [`AuditRecorder`],
/// which makes the asynchronous path write the same `llm_calls`,
/// `llm_call_events` and `translation_attempt_llm_calls` rows as the
/// synchronous and SSE paths. Audit inserts degrade to a warning on databases
/// that predate the audit tables, so the translation itself still completes.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn translate_work_item_for_task(
    pool: &PgPool,
    http: &Client,
    ai: &AiConfig,
    parent_task_id: &str,
    parent_task_type: &str,
    work_item_id: &str,
    request_id: Option<&str>,
    source: &str,
) -> anyhow::Result<TaskTranslationOutcome> {
    let row = sqlx::query(
        "SELECT scope_user_id, kind, variant, entity_id, status, source_blocks_json, target_slots_json FROM public.translation_work_items WHERE id = $1",
    )
    .bind(work_item_id)
    .fetch_optional(pool)
    .await
    .context("load PostgreSQL translation work item")?
    .ok_or_else(|| anyhow!("translation work item is missing or already finished"))?;
    let status: String = row.try_get("status")?;
    if status != "queued" && status != "running" {
        return Err(anyhow!(
            "translation work item is missing or already finished"
        ));
    }
    let user_id: String = row.try_get("scope_user_id")?;
    let work_kind: String = row.try_get("kind")?;
    let variant: String = row.try_get("variant")?;
    let entity_id: String = row.try_get("entity_id")?;
    let source_blocks_json: String = row.try_get("source_blocks_json")?;
    let target_slots_json: String = row.try_get("target_slots_json")?;

    // Prefer the request written by the enqueue, falling back to the newest
    // request of the work item when the payload carries no usable id.
    let request_row = sqlx::query(
        "SELECT id, kind FROM public.translation_requests WHERE work_item_id = $1 ORDER BY (id = $2) DESC NULLS LAST, created_at DESC, id DESC LIMIT 1",
    )
    .bind(work_item_id)
    .bind(request_id)
    .fetch_optional(pool)
    .await
    .context("load PostgreSQL translation request")?;
    let (request_id, request_kind) = match request_row {
        Some(row) => (
            Some(row.try_get::<String, _>("id")?),
            Some(row.try_get::<String, _>("kind")?),
        ),
        None => (None, None),
    };
    let kind = task_target_kind(&work_kind, request_kind.as_deref());
    let targets = load_targets(
        pool,
        &user_id,
        kind,
        source,
        std::slice::from_ref(&entity_id),
    )
    .await
    .context("load PostgreSQL translation target")?;
    let now = Utc::now();
    let scope = {
        let mut tx = pool.begin().await.context("begin PostgreSQL translation")?;
        let attempt_no = start_attempt(
            &mut tx,
            work_item_id,
            request_id.as_deref(),
            &user_id,
            kind,
            &variant,
            &entity_id,
            now,
        )
        .await
        .context("start PostgreSQL translation attempt")?;
        tx.commit().await.context("commit PostgreSQL translation")?;
        RequestScope {
            request_id,
            work_item_id: work_item_id.to_owned(),
            variant: variant.to_owned(),
            attempt_no,
        }
    };
    let Some(target) = targets.get(&entity_id) else {
        // The entity lost visibility between enqueue and execution; the
        // synchronous paths report this as a missing target.
        finish_request(
            pool,
            &user_id,
            kind,
            &entity_id,
            None,
            &scope,
            AttemptOutcome {
                result_status: "missing",
                title: None,
                summary: None,
                error_code: None,
                error_summary: Some(kind.missing_message()),
                retry_eligible: false,
            },
            "[]",
            Utc::now(),
        )
        .await
        .context("persist PostgreSQL translation miss")?;
        return Ok(TaskTranslationOutcome {
            status: "missing",
            error: Some(kind.missing_message().to_owned()),
        });
    };

    // The work item's persisted blocks describe the source version this work
    // item was enqueued for; live content may have changed since then.
    let target = persisted_prompt_target(target, &source_blocks_json, &target_slots_json, kind);

    let mut audit = audit_recorder(
        pool,
        http,
        ai,
        source,
        &user_id,
        work_item_id,
        scope.attempt_no,
        Some((parent_task_id, parent_task_type)),
    );
    let result = run_translation(&mut audit, &target).await;
    let call_ids_json = serde_json::to_string(&audit.call_ids).unwrap_or_else(|_| "[]".to_owned());
    let now = Utc::now();

    match result {
        Ok((title, summary)) => {
            finish_request(
                pool,
                &user_id,
                kind,
                &entity_id,
                Some(&target),
                &scope,
                AttemptOutcome {
                    result_status: "ready",
                    title: title.as_deref(),
                    summary: summary.as_deref(),
                    error_code: None,
                    error_summary: None,
                    retry_eligible: false,
                },
                &call_ids_json,
                now,
            )
            .await
            .context("persist PostgreSQL translation result")?;
            Ok(TaskTranslationOutcome {
                status: "ready",
                error: None,
            })
        }
        Err(failure) => {
            finish_request(
                pool,
                &user_id,
                kind,
                &entity_id,
                Some(&target),
                &scope,
                AttemptOutcome {
                    result_status: "error",
                    title: None,
                    summary: None,
                    error_code: Some(failure.class.as_str()),
                    error_summary: Some(failure.message.as_str()),
                    retry_eligible: failure.class.is_recoverable(),
                },
                &call_ids_json,
                now,
            )
            .await
            .context("persist PostgreSQL translation failure")?;
            Ok(TaskTranslationOutcome {
                status: "error",
                error: Some(failure.message),
            })
        }
    }
}

/// Terminal result of [`translate_work_item_for_task`].
#[derive(Debug, Clone)]
pub(crate) struct TaskTranslationOutcome {
    /// `ready` when the work item holds a translation, otherwise `error` or
    /// `missing`.
    pub status: &'static str,
    /// Failure text for any status other than `ready`.
    pub error: Option<String>,
}

/// Map a persisted work item back to the route kind that enqueued it.
///
/// Release summaries and details share one work item, so the request row is
/// the only place the summary/detail distinction survives.
fn task_target_kind(work_kind: &str, request_kind: Option<&str>) -> TargetKind {
    if work_kind == TargetKind::Notification.work_kind() {
        return TargetKind::Notification;
    }
    match request_kind {
        Some(kind) if kind == TargetKind::ReleaseSummary.request_kind() => {
            TargetKind::ReleaseSummary
        }
        _ => TargetKind::ReleaseDetail,
    }
}

// ---------------------------------------------------------------------------
// Translation prompts
// ---------------------------------------------------------------------------

async fn run_translation(
    audit: &mut AuditRecorder<'_>,
    target: &Target,
) -> Result<(Option<String>, Option<String>), LlmFailure> {
    match target.kind {
        TargetKind::Notification => {
            let prompt = format!(
                "Repo: {repo}\nOriginal title: {title}\nReason: {reason}\nType: {subject_type}\n\n请把这条 Inbox 通知翻译/解释为中文，输出严格 JSON（不要 markdown code block）：\n{{\"title_zh\": \"...\", \"summary_md\": \"- ...\"}}\n\n要求：summary_md 1-3 条；给出建议动作；不包含任何 URL。",
                repo = target.repo_full_name,
                title = target.title,
                reason = target.reason,
                subject_type = target.subject_type,
            );
            let raw = audit
                .complete(
                    "primary",
                    NOTIFICATION_SYSTEM,
                    &prompt,
                    NOTIFICATION_MAX_TOKENS,
                )
                .await?;
            let (title, summary) = parse_json_translation(&raw);
            let title = title
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            let summary = summary
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    let trimmed = raw.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_owned())
                });
            if title.is_none() && summary.is_none() {
                return Err(LlmFailure::empty_content());
            }
            Ok((title, summary))
        }
        TargetKind::ReleaseSummary | TargetKind::ReleaseDetail => {
            // The work item declares which slots the caller asked for; the
            // route defaults (title + body summary) apply when the persisted
            // slots are absent.
            let wants_title = target.target_slots.iter().any(|slot| slot == "title_zh");
            let wants_body = target
                .target_slots
                .iter()
                .any(|slot| slot == "body_md" || slot == "summary_md");
            let translated_title = if !wants_title || target.title.trim().is_empty() {
                None
            } else {
                let prompt = format!(
                    "Repo: {}\nOriginal title: {}\n\n输出中文标题：",
                    target.repo_full_name, target.title
                );
                let raw = audit
                    .complete(
                        "title",
                        RELEASE_TITLE_SYSTEM,
                        &prompt,
                        RELEASE_TITLE_MAX_TOKENS,
                    )
                    .await?;
                let trimmed = raw.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_owned())
            };

            if !wants_body || target.body.trim().is_empty() {
                return Ok((translated_title, None));
            }

            let chunk_chars =
                body_chunk_chars_for_route(audit.pool, audit.ai.model.as_str()).await?;
            let chunks = split_markdown_chunks(&target.body, chunk_chars);
            let total = chunks.len();
            let mut translated = String::with_capacity(target.body.len());
            for (index, chunk) in chunks.iter().enumerate() {
                let stage = if index == 0 {
                    "body".to_owned()
                } else {
                    format!("body.{}", index + 1)
                };
                let prompt = format!(
                    "Repo: {repo}\nTitle: {title}\nChunk: {current}/{total}\n\nRelease notes chunk (Markdown):\n{chunk}\n\n请把这段 GitHub Release notes 翻译成中文 Markdown，要求：\n1) 保留原有 Markdown 结构（标题/列表/表格/引用/代码块）；\n2) 保留链接 URL 与代码；\n3) 不新增、不删减信息；\n4) 只输出翻译后的 Markdown，不要解释。",
                    repo = target.repo_full_name,
                    title = target.title,
                    current = index + 1,
                    total = total,
                    chunk = chunk,
                );
                let raw = audit
                    .complete(
                        &stage,
                        RELEASE_BODY_SYSTEM,
                        &prompt,
                        RELEASE_BODY_MAX_TOKENS,
                    )
                    .await?;
                if raw.trim().is_empty() {
                    return Err(LlmFailure::empty_content());
                }
                translated.push_str(raw.trim());
                if index + 1 < total {
                    translated.push('\n');
                }
            }
            let summary = (!translated.trim().is_empty()).then(|| translated.trim().to_owned());
            if !translation_ready(&target.body, summary.as_deref()) {
                return Err(LlmFailure::empty_content());
            }
            Ok((translated_title, summary))
        }
    }
}

fn split_markdown_chunks(body: &str, max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.max(512);
    let mut chunks = Vec::new();
    let mut current = String::new();
    for line in body.lines() {
        if !current.is_empty() && current.chars().count() + line.chars().count() + 1 > max_chars {
            chunks.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
        while current.chars().count() > max_chars {
            let mut split_at = max_chars;
            while !current.is_char_boundary(split_at) {
                split_at -= 1;
            }
            let rest = current.split_off(split_at);
            chunks.push(std::mem::take(&mut current));
            current = rest;
        }
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    if chunks.is_empty() {
        chunks.push(body.to_owned());
    }
    chunks
}

/// Parse the model JSON contract, tolerating fenced code blocks and prose.
fn parse_json_translation(raw: &str) -> (Option<String>, Option<String>) {
    let trimmed = raw.trim();
    let candidate = if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        let after = after.trim_start_matches(|ch: char| ch.is_ascii_alphabetic());
        after.split("```").next().map(str::trim).unwrap_or(trimmed)
    } else {
        trimmed
    };
    let slice = match (candidate.find('{'), candidate.rfind('}')) {
        (Some(start), Some(end)) if end > start => &candidate[start..=end],
        _ => return (None, None),
    };
    let Ok(value) = serde_json::from_str::<Value>(slice) else {
        return (None, None);
    };
    let string_field = |names: &[&str]| -> Option<String> {
        names
            .iter()
            .find_map(|name| value.get(*name).and_then(Value::as_str))
            .map(|field| field.replace("\\n", "\n"))
    };
    (
        string_field(&["title_zh", "title", "title_cn"]),
        string_field(&["summary_md", "summary", "body_md", "body", "summary_zh"]),
    )
}

// ---------------------------------------------------------------------------
// Audited LLM calls
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) struct LlmFailure {
    pub(crate) class: crate::llm_contract::LlmFailureClass,
    pub(crate) message: String,
    /// Why the upstream refused, when the raw body says something the failure
    /// class cannot: `quota` and `credential` both land in the configuration
    /// class, and only the credential one parks a model. The worker records this
    /// on the attempt event so a later sweep can tell them apart; it is not part
    /// of the class mapping.
    pub(crate) disposition: Option<&'static str>,
}

impl LlmFailure {
    fn empty_content() -> Self {
        let class = crate::llm_contract::LlmFailureClass::EmptyContent;
        Self {
            class,
            message: class.safe_message().to_owned(),
            disposition: None,
        }
    }

    fn transient() -> Self {
        let class = crate::llm_contract::LlmFailureClass::Transient;
        Self {
            class,
            message: class.safe_message().to_owned(),
            disposition: None,
        }
    }

    fn configuration() -> Self {
        let class = crate::llm_contract::LlmFailureClass::Configuration;
        Self {
            class,
            message: class.safe_message().to_owned(),
            disposition: None,
        }
    }
}

impl std::fmt::Display for LlmFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for LlmFailure {}

/// Legacy routing bounds mirrored from `ai::LlmScheduler`: one logical call
/// tries at most eight upstream requests, each candidate twice for transient or
/// rate-limited failures, and a candidate that reaches two failures inside the
/// rolling window rests for ten minutes.
const ROUTE_MAX_UPSTREAM_ATTEMPTS: usize = 8;
const ROUTE_ATTEMPTS_PER_MODEL: usize = 2;
const MODEL_FAILURE_WINDOW_MINUTES: i64 = 10;
const MODEL_FAILURE_COOLDOWN_MINUTES: i64 = 10;
const MODEL_CREDENTIAL_COOLDOWN_HOURS: i64 = 24;
const MODEL_FAILURE_THRESHOLD: i64 = 2;

#[derive(Debug, Clone, Default)]
struct ModelRouteHealth {
    cooldown_until: Option<DateTime<Utc>>,
    last_failure_class: Option<String>,
}

/// Read the administrator-ordered model list. `AI_MODEL` is retained only as
/// the runtime fallback when the persisted list is empty.
async fn route_models(pool: &PgPool, fallback_model: &str) -> Result<Vec<String>, LlmFailure> {
    let raw = sqlx::query_scalar::<_, String>(
        "SELECT llm_models_json FROM public.admin_runtime_settings WHERE id = 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        tracing::warn!(%error, "failed to read PostgreSQL LLM routing settings");
        LlmFailure::transient()
    })?;
    let parsed = match raw {
        Some(raw) if !raw.trim().is_empty() => {
            serde_json::from_str::<Vec<String>>(&raw).map_err(|error| {
                tracing::warn!(%error, "failed to parse PostgreSQL LLM routing settings");
                LlmFailure::configuration()
            })?
        }
        Some(_) => Vec::new(),
        None => {
            tracing::warn!("PostgreSQL LLM routing settings row is missing");
            return Err(LlmFailure::configuration());
        }
    };
    let mut models = crate::admin_runtime::normalize_llm_models(parsed);
    if models.is_empty() {
        let fallback = fallback_model.trim();
        if !fallback.is_empty() {
            models.push(fallback.to_owned());
        }
    }
    Ok(models)
}

async fn load_route_health(pool: &PgPool) -> Result<HashMap<String, ModelRouteHealth>, LlmFailure> {
    let rows = sqlx::query(
        "SELECT model, cooldown_until, last_failure_class FROM public.llm_model_health",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| {
        tracing::warn!(%error, "failed to read PostgreSQL LLM model health");
        LlmFailure::transient()
    })?;
    let mut health = HashMap::new();
    for row in rows {
        let model = row.try_get::<String, _>("model").map_err(|error| {
            tracing::warn!(%error, "failed to decode PostgreSQL LLM model health");
            LlmFailure::transient()
        })?;
        let cooldown_until = row
            .try_get::<Option<DateTime<Utc>>, _>("cooldown_until")
            .map_err(|error| {
                tracing::warn!(%error, "failed to decode PostgreSQL LLM model health");
                LlmFailure::transient()
            })?;
        let last_failure_class = row
            .try_get::<Option<String>, _>("last_failure_class")
            .map_err(|error| {
                tracing::warn!(%error, "failed to decode PostgreSQL LLM model health");
                LlmFailure::transient()
            })?;
        health.insert(
            model,
            ModelRouteHealth {
                cooldown_until,
                last_failure_class,
            },
        );
    }
    Ok(health)
}

fn health_for_model<'a>(
    health: &'a HashMap<String, ModelRouteHealth>,
    model: &str,
) -> Option<&'a ModelRouteHealth> {
    let normalized = model.trim().to_ascii_lowercase();
    health
        .get(model)
        .or_else(|| health.get(normalized.as_str()))
}

/// Order candidates by administrator order. While any usable model is ready,
/// only ready models participate. When every usable model is cooling, probe
/// only the earliest expiry. Credential-parked models stay out entirely until
/// an operator saves the runtime configuration.
fn route_candidates(
    models: &[String],
    health: &HashMap<String, ModelRouteHealth>,
    now: DateTime<Utc>,
) -> Vec<String> {
    let mut available = Vec::new();
    let mut cooling = Vec::new();
    for model in models {
        let state = health_for_model(health, model);
        let parked = state.is_some_and(|state| {
            state.last_failure_class.as_deref() == Some("configuration")
                && state.cooldown_until.is_some_and(|until| until > now)
        });
        if parked {
            continue;
        }
        match state.and_then(|state| state.cooldown_until) {
            Some(until) if until > now => cooling.push((model.clone(), until)),
            _ => available.push(model.clone()),
        }
    }
    if !available.is_empty() {
        return available;
    }
    cooling.sort_by_key(|(_, until)| *until);
    cooling
        .into_iter()
        .take(1)
        .map(|(model, _)| model)
        .collect()
}

fn chunk_chars_for_input_limit(input_limit: u32) -> usize {
    ((input_limit as usize).saturating_mul(3)).clamp(2_000, 12_000)
}

/// Size chunks for the smallest context among the currently usable candidates
/// so a fallback model cannot receive a request larger than its input limit.
async fn body_chunk_chars_for_route(
    pool: &PgPool,
    fallback_model: &str,
) -> Result<usize, LlmFailure> {
    let models = route_models(pool, fallback_model).await?;
    let health = load_route_health(pool).await?;
    let candidates = route_candidates(&models, &health, Utc::now());
    let input_limit = candidates
        .iter()
        .map(|model| crate::llm_contract::resolve_model_input_limit_for_model(model).0)
        .min()
        .unwrap_or_else(|| {
            crate::llm_contract::resolve_model_input_limit_for_model(fallback_model.trim()).0
        });
    Ok(chunk_chars_for_input_limit(input_limit))
}

async fn record_model_failure(
    pool: &PgPool,
    model: &str,
    class: crate::llm_contract::LlmFailureClass,
) {
    record_model_health(pool, model, class, false).await;
}

async fn park_model(pool: &PgPool, model: &str) {
    record_model_health(
        pool,
        model,
        crate::llm_contract::LlmFailureClass::Configuration,
        true,
    )
    .await;
}

/// Read the administrator-ordered model list for a caller outside this module.
pub(crate) async fn route_models_for_runtime(
    pool: &PgPool,
    fallback_model: &str,
) -> Result<Vec<String>, LlmFailure> {
    route_models(pool, fallback_model).await
}

/// The route a fresh attempt would take, and the models that can serve it now.
///
/// The persisted route is what an attempt snapshots and fingerprints. The
/// callable subset applies the same health view the execution path uses
/// ([`route_candidates`]), so a caller can tell "the administrator configured
/// models" apart from "an attempt right now can reach a provider". Credential
/// parks and cooldowns live only in `llm_model_health`, so reading the persisted
/// list alone cannot answer the second question.
pub(crate) struct AttemptRoute {
    /// The administrator-ordered persisted route, including the runtime model
    /// fallback. This is the route an attempt is bound to.
    pub(crate) models: Vec<String>,
    /// The models a fresh attempt could call now: ready models first, or the
    /// earliest-expiring model as a probe when every usable model is cooling.
    /// Empty means every routed model is credential-parked, so an attempt would
    /// fail without issuing any provider request.
    pub(crate) callable: Vec<String>,
}

/// Read the route a fresh attempt would use together with its callable models.
pub(crate) async fn attempt_route_for_runtime(
    pool: &PgPool,
    fallback_model: &str,
) -> Result<AttemptRoute, LlmFailure> {
    let models = route_models(pool, fallback_model).await?;
    let health = load_route_health(pool).await?;
    let callable = route_candidates(&models, &health, Utc::now());
    Ok(AttemptRoute { models, callable })
}

async fn record_model_health(
    pool: &PgPool,
    model: &str,
    class: crate::llm_contract::LlmFailureClass,
    park: bool,
) {
    if model.trim().is_empty() {
        return;
    }
    let now = Utc::now();
    let result = async {
        let mut tx = pool.begin().await?;
        sqlx::query(
            "INSERT INTO public.llm_model_health (model, relevant_failure_count, created_at, updated_at) VALUES ($1, 0, $2, $2) ON CONFLICT (model) DO NOTHING",
        )
        .bind(model)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(
            "SELECT relevant_failure_count, window_started_at, cooldown_until FROM public.llm_model_health WHERE model = $1 FOR UPDATE",
        )
        .bind(model)
        .fetch_one(&mut *tx)
        .await?;
        let previous_count = row.try_get::<i64, _>("relevant_failure_count").unwrap_or(0);
        let previous_window = row
            .try_get::<Option<DateTime<Utc>>, _>("window_started_at")
            .ok()
            .flatten();
        let existing_cooldown = row
            .try_get::<Option<DateTime<Utc>>, _>("cooldown_until")
            .ok()
            .flatten()
            .filter(|until| *until > now);
        let within_window = previous_window.is_some_and(|started| {
            now.signed_duration_since(started) < chrono::Duration::minutes(MODEL_FAILURE_WINDOW_MINUTES)
        });
        let count = if within_window {
            previous_count.saturating_add(1)
        } else {
            1
        };
        let window_started = if within_window {
            previous_window.unwrap_or(now)
        } else {
            now
        };
        let requested_cooldown = if park {
            Some(now + chrono::Duration::hours(MODEL_CREDENTIAL_COOLDOWN_HOURS))
        } else if count >= MODEL_FAILURE_THRESHOLD {
            Some(now + chrono::Duration::minutes(MODEL_FAILURE_COOLDOWN_MINUTES))
        } else {
            None
        };
        let cooldown_until = match (existing_cooldown, requested_cooldown) {
            (Some(existing), Some(requested)) => Some(existing.max(requested)),
            (Some(existing), None) => Some(existing),
            (None, Some(requested)) => Some(requested),
            (None, None) => None,
        };
        sqlx::query(
            "UPDATE public.llm_model_health SET relevant_failure_count = $2, window_started_at = $3, cooldown_until = $4, last_failure_class = $5, last_failure_at = $6, updated_at = $6 WHERE model = $1",
        )
        .bind(model)
        .bind(count)
        .bind(window_started)
        .bind(cooldown_until)
        .bind(class.as_str())
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok::<_, sqlx::Error>(())
    }
    .await;
    if let Err(error) = result {
        tracing::warn!(%error, model, "failed to persist PostgreSQL model route health");
    }
}

struct AuditRecorder<'a> {
    pool: &'a PgPool,
    http: &'a Client,
    ai: &'a AiConfig,
    source: &'a str,
    requested_by: &'a str,
    work_item_id: &'a str,
    attempt_no: i32,
    parent_task_id: Option<String>,
    parent_task_type: Option<String>,
    call_ids: Vec<String>,
    ordinal: i64,
}

/// Build an audit recorder for one translation attempt.
#[allow(clippy::too_many_arguments)]
fn audit_recorder<'a>(
    pool: &'a PgPool,
    http: &'a Client,
    ai: &'a AiConfig,
    source: &'a str,
    requested_by: &'a str,
    work_item_id: &'a str,
    attempt_no: i32,
    parent: Option<(&str, &str)>,
) -> AuditRecorder<'a> {
    AuditRecorder {
        pool,
        http,
        ai,
        source,
        requested_by,
        work_item_id,
        attempt_no,
        parent_task_id: parent.map(|(task_id, _)| task_id.to_owned()),
        parent_task_type: parent.map(|(_, task_type)| task_type.to_owned()),
        call_ids: Vec::new(),
        ordinal: 0,
    }
}

struct CallAudit {
    status: &'static str,
    model: String,
    provider_request_id: Option<String>,
    provider_http_status: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    finish_reason: Option<String>,
    failure_class: Option<&'static str>,
    error_text: Option<String>,
    response_text: Option<String>,
    duration_ms: i64,
}

impl AuditRecorder<'_> {
    async fn complete(
        &mut self,
        stage: &str,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, LlmFailure> {
        let models = route_models(self.pool, self.ai.model.as_str()).await?;
        let health = load_route_health(self.pool).await?;
        let candidates = route_candidates(&models, &health, Utc::now());
        if candidates.is_empty() {
            return Err(LlmFailure::configuration());
        }
        let mut remaining = ROUTE_MAX_UPSTREAM_ATTEMPTS;
        let mut last_failure: Option<LlmFailure> = None;
        for (candidate_index, model) in candidates.into_iter().enumerate() {
            let fallback_count = i64::try_from(candidate_index).unwrap_or(i64::MAX);
            let mut retry_class: Option<crate::llm_contract::LlmFailureClass> = None;
            for model_attempt in 0..ROUTE_ATTEMPTS_PER_MODEL {
                if remaining == 0 {
                    break;
                }
                remaining -= 1;
                let call_id = local_id::generate_local_id();
                let ordinal = self.ordinal;
                self.ordinal += 1;
                let prompt_text = truncate_chars(&format!("{system}\n\n{user}"), 8_000);
                let inserted = insert_call(
                    self.pool,
                    &call_id,
                    self.source,
                    model.as_str(),
                    self.requested_by,
                    self.parent_task_id.as_deref(),
                    self.parent_task_type.as_deref(),
                    max_tokens,
                    &prompt_text,
                    &json!([
                        {"role": "system", "content": system},
                        {"role": "user", "content": user},
                    ])
                    .to_string(),
                )
                .await;
                if inserted {
                    self.call_ids.push(call_id.clone());
                    let _ = mark_call_running(self.pool, &call_id).await;
                }

                let started = std::time::Instant::now();
                let outcome = self.invoke(model.as_str(), system, user, max_tokens).await;
                let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);

                let audit = match &outcome {
                    Ok(response) => CallAudit {
                        status: "succeeded",
                        model: response.model.clone(),
                        provider_request_id: response.provider_request_id.clone(),
                        provider_http_status: response.provider_http_status,
                        input_tokens: response.input_tokens,
                        output_tokens: response.output_tokens,
                        total_tokens: response.total_tokens,
                        finish_reason: response.finish_reason.clone(),
                        failure_class: None,
                        error_text: None,
                        response_text: Some(truncate_chars(&response.content, 8_000)),
                        duration_ms,
                    },
                    Err(failure) => CallAudit {
                        status: "failed",
                        model: model.clone(),
                        provider_request_id: None,
                        provider_http_status: failure.http_status,
                        input_tokens: None,
                        output_tokens: None,
                        total_tokens: None,
                        finish_reason: None,
                        failure_class: Some(failure.error.class.as_str()),
                        error_text: Some(failure.detail.clone()),
                        response_text: None,
                        duration_ms,
                    },
                };
                if inserted {
                    let _ = finalize_call(self.pool, &call_id, &audit, fallback_count).await;
                    let _ = link_attempt_call(
                        self.pool,
                        self.work_item_id,
                        self.attempt_no,
                        &call_id,
                        stage,
                        ordinal,
                    )
                    .await;
                }

                match outcome {
                    Ok(response) => return Ok(response.content),
                    Err(failure) => {
                        let class = failure.error.class;
                        let http_status = failure.http_status;
                        last_failure = Some(failure.error);
                        if class == crate::llm_contract::LlmFailureClass::Configuration {
                            if is_credential_rejection(http_status) {
                                park_model(self.pool, model.as_str()).await;
                            }
                            break;
                        }
                        if class == crate::llm_contract::LlmFailureClass::EmptyContent {
                            retry_class = Some(class);
                            break;
                        }
                        if model_attempt + 1 == ROUTE_ATTEMPTS_PER_MODEL || remaining == 0 {
                            retry_class = Some(class);
                            break;
                        }
                    }
                }
            }
            if let Some(class) = retry_class {
                record_model_failure(self.pool, model.as_str(), class).await;
            }
            if remaining == 0 {
                break;
            }
        }
        Err(last_failure.unwrap_or_else(LlmFailure::transient))
    }

    async fn invoke(
        &self,
        model: &str,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<CallResponse, CallFailure> {
        let endpoint = self
            .ai
            .base_url
            .join("chat/completions")
            .map_err(|_| CallFailure {
                error: LlmFailure::configuration(),
                detail: "invalid AI base URL".to_owned(),
                http_status: None,
            })?;
        let response = self
            .http
            .post(endpoint)
            .bearer_auth(&self.ai.api_key)
            .timeout(LLM_CALL_TIMEOUT)
            .json(&json!({
                "model": model,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
                "max_tokens": max_tokens,
                "temperature": 0,
                "stream": false,
            }))
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                return Err(CallFailure {
                    error: LlmFailure::transient(),
                    detail: truncate_chars(&error.to_string(), 200),
                    http_status: None,
                });
            }
        };
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        let body: Value = serde_json::from_str(&body_text).unwrap_or(Value::Null);
        if !status.is_success() {
            let detail = body_text.trim();
            let detail = if detail.is_empty() {
                format!("upstream status {}", status.as_u16())
            } else {
                truncate_chars(detail, 200)
            };
            let (class, disposition) =
                classify_upstream_failure_with_reason(status.as_u16(), detail.as_str());
            return Err(CallFailure {
                error: LlmFailure {
                    class,
                    message: class.safe_message().to_owned(),
                    disposition,
                },
                detail,
                http_status: Some(i64::from(status.as_u16())),
            });
        }
        let content = body
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let content = match content {
            Some(content) if !content.trim().is_empty() => content,
            _ => {
                return Err(CallFailure {
                    error: LlmFailure::empty_content(),
                    detail: "ai response missing content".to_owned(),
                    http_status: Some(i64::from(status.as_u16())),
                });
            }
        };
        Ok(CallResponse {
            content,
            model: body
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or(model)
                .to_owned(),
            provider_request_id: body.get("id").and_then(Value::as_str).map(str::to_owned),
            provider_http_status: Some(i64::from(status.as_u16())),
            input_tokens: body.pointer("/usage/prompt_tokens").and_then(Value::as_i64),
            output_tokens: body
                .pointer("/usage/completion_tokens")
                .and_then(Value::as_i64),
            total_tokens: body.pointer("/usage/total_tokens").and_then(Value::as_i64),
            finish_reason: body
                .pointer("/choices/0/finish_reason")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    }
}

struct CallResponse {
    content: String,
    model: String,
    provider_request_id: Option<String>,
    provider_http_status: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    finish_reason: Option<String>,
}

struct CallFailure {
    error: LlmFailure,
    detail: String,
    http_status: Option<i64>,
}

/// Class-only view of the classifier, kept for the class-mapping tests.
#[cfg(test)]
fn classify_upstream_failure(status: u16, detail: &str) -> crate::llm_contract::LlmFailureClass {
    classify_upstream_failure_with_reason(status, detail).0
}

/// The failure class plus why the upstream refused, when the body names a cause
/// the class cannot express.
///
/// `quota` and `credential` both map to the configuration class, but they mean
/// opposite things to recovery: a credential rejection parks the model until an
/// operator saves configuration, while a quota rejection only waits for the
/// allowance to be restored. The class mapping is unchanged; the reason is
/// carried alongside it so the attempt event can record it.
pub(crate) fn classify_upstream_failure_with_reason(
    status: u16,
    detail: &str,
) -> (crate::llm_contract::LlmFailureClass, Option<&'static str>) {
    let lowered = detail.to_ascii_lowercase();
    let model_rejected = lowered.contains("model not found")
        || lowered.contains("invalid_model_error")
        || lowered.contains("insufficient_quota")
        || lowered.contains("model_not_found");
    // Several providers answer an exhausted allowance with 429 rather than 402
    // or a 4xx quota body, so the body is checked before the status.
    let quota = lowered.contains("insufficient_quota")
        || lowered.contains("exceeds tpm/rpm")
        || lowered.contains("quota");
    if status == 429 {
        return (
            crate::llm_contract::LlmFailureClass::RateLimited,
            quota.then_some("quota"),
        );
    }
    if model_rejected || matches!(status, 400..=499) {
        let disposition = if quota {
            Some("quota")
        } else if is_credential_rejection(Some(i64::from(status))) {
            Some("credential")
        } else {
            None
        };
        return (
            crate::llm_contract::LlmFailureClass::Configuration,
            disposition,
        );
    }
    (
        crate::llm_contract::LlmFailureClass::Transient,
        quota.then_some("quota"),
    )
}

fn is_credential_rejection(status: Option<i64>) -> bool {
    matches!(status, Some(401) | Some(403))
}

#[allow(clippy::too_many_arguments)]
async fn insert_call(
    pool: &PgPool,
    call_id: &str,
    source: &str,
    model: &str,
    requested_by: &str,
    parent_task_id: Option<&str>,
    parent_task_type: Option<&str>,
    max_tokens: u32,
    prompt_text: &str,
    input_messages_json: &str,
) -> bool {
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "INSERT INTO public.llm_calls (id, status, source, model, requested_by, parent_task_id, parent_task_type, max_tokens, attempt_count, scheduler_wait_ms, prompt_text, input_messages_json, created_at, updated_at) VALUES ($1, 'queued', $2, $3, $4, $5, $6, $7, 1, 0, $8, $9, $10, $10)",
    )
    .bind(call_id)
    .bind(source)
    .bind(model)
    .bind(requested_by)
    .bind(parent_task_id)
    .bind(parent_task_type)
    .bind(i64::from(max_tokens))
    .bind(prompt_text)
    .bind(input_messages_json)
    .bind(&now)
    .execute(pool)
    .await;
    match result {
        Ok(_) => {
            let _ =
                insert_call_event(pool, call_id, "llm.queued", "queued", model, None, &now).await;
            true
        }
        Err(error) => {
            warn!(%error, call_id, "PostgreSQL LLM call audit insert failed");
            false
        }
    }
}

async fn mark_call_running(pool: &PgPool, call_id: &str) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE public.llm_calls SET status = 'running', started_at = $1, updated_at = $1 WHERE id = $2")
        .bind(&now)
        .bind(call_id)
        .execute(pool)
        .await?;
    let model = sqlx::query_scalar::<_, String>("SELECT model FROM public.llm_calls WHERE id = $1")
        .bind(call_id)
        .fetch_optional(pool)
        .await?
        .unwrap_or_default();
    insert_call_event(pool, call_id, "llm.running", "running", &model, None, &now).await?;
    Ok(())
}

async fn finalize_call(
    pool: &PgPool,
    call_id: &str,
    audit: &CallAudit,
    fallback_count: i64,
) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE public.llm_calls SET status = $1, model = $2, final_model = $2, provider_request_id = $3, provider_http_status = $4, input_tokens = $5, output_tokens = $6, total_tokens = $7, finish_reason = $8, failure_class = $9, error_text = $10, response_text = $11, duration_ms = $12, fallback_count = $13, recovery_attempt_count = 0, finished_at = $14, updated_at = $14, runtime_owner_id = NULL, lease_heartbeat_at = NULL WHERE id = $15",
    )
    .bind(audit.status)
    .bind(&audit.model)
    .bind(audit.provider_request_id.as_deref())
    .bind(audit.provider_http_status)
    .bind(audit.input_tokens)
    .bind(audit.output_tokens)
    .bind(audit.total_tokens)
    .bind(audit.finish_reason.as_deref())
    .bind(audit.failure_class)
    .bind(audit.error_text.as_deref())
    .bind(audit.response_text.as_deref())
    .bind(audit.duration_ms)
    .bind(fallback_count)
    .bind(&now)
    .bind(call_id)
    .execute(pool)
    .await?;
    let event_type = if audit.status == "succeeded" {
        "llm.succeeded"
    } else {
        "llm.failed"
    };
    insert_call_event(
        pool,
        call_id,
        event_type,
        audit.status,
        &audit.model,
        audit.failure_class,
        &now,
    )
    .await?;
    Ok(())
}

async fn insert_call_event(
    pool: &PgPool,
    call_id: &str,
    event_type: &str,
    status: &str,
    model: &str,
    failure_class: Option<&str>,
    now: &str,
) -> Result<(), sqlx::Error> {
    // Project the audit context from the call row so the event keeps the
    // non-null source/requester fields the LLM audit read model expects.
    sqlx::query(
        "INSERT INTO public.llm_call_events (id, call_id, event_type, status, source, requested_by, parent_task_id, failure_class, model, attempt, payload_json, created_at) SELECT $1, id, $2, $3, source, requested_by, parent_task_id, $4, $5, 1, '{}', $6 FROM public.llm_calls WHERE id = $7",
    )
    .bind(local_id::generate_local_id())
    .bind(event_type)
    .bind(status)
    .bind(failure_class)
    .bind(model)
    .bind(now)
    .bind(call_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn link_attempt_call(
    pool: &PgPool,
    work_item_id: &str,
    attempt_no: i32,
    call_id: &str,
    stage: &str,
    ordinal: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO public.translation_attempt_llm_calls (id, work_item_id, attempt_no, llm_call_id, stage, ordinal, relation_role, evidence_availability, created_at) VALUES ($1, $2, $3, $4, $5, $6, 'primary', 'available', $7) ON CONFLICT DO NOTHING",
    )
    .bind(local_id::generate_local_id())
    .bind(work_item_id)
    .bind(attempt_no)
    .bind(call_id)
    .bind(stage)
    .bind(ordinal)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

/// One logical provider completion with its persisted audit identity.
///
/// Shared by the PostgreSQL translation path and the global content worker so
/// both route models, record `llm_calls` facts and honour model health through
/// the same code instead of a second drifting implementation.
pub(crate) struct AuditedCompletion {
    pub content: String,
    pub call_id: Option<String>,
    pub model: String,
    pub provider_request_id: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub duration_ms: Option<i64>,
    pub finish_reason: Option<String>,
    /// Provider requests issued by this call, so a follow-up call can continue
    /// the admission ordinal sequence instead of colliding with it.
    pub attempts_used: i64,
}

/// Why one audited completion did not produce content.
///
/// Keeping lease loss separate from a provider verdict prevents the worker from
/// recording a provider failure (and spending retry budget) on work another
/// runtime now owns.
#[derive(Debug)]
pub(crate) enum CompletionFailure {
    Provider(LlmFailure),
    LeaseLost,
}

impl CompletionFailure {
    fn from_admission(error: anyhow::Error) -> Self {
        Self::Provider(LlmFailure {
            class: crate::llm_contract::LlmFailureClass::Transient,
            message: format!("provider admission failed: {error}"),
            disposition: None,
        })
    }
}

impl std::fmt::Display for CompletionFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider(failure) => formatter.write_str(&failure.message),
            Self::LeaseLost => formatter.write_str("content work lease is no longer owned"),
        }
    }
}

impl std::error::Error for CompletionFailure {}

/// Provider admission boundary for a content work attempt.
///
/// Every provider request must renew the live lease and append an admission
/// fact before the upstream call, so an expired or stolen claim cannot issue a
/// new request and the audit graph stays attributable.
pub(crate) struct ContentProviderAdmission<'a> {
    pub pool: &'a PgPool,
    pub work_item_id: &'a str,
    pub attempt_event_id: &'a str,
    pub attempt_no: i64,
    pub lease_owner: &'a str,
    pub source_hash: &'a str,
    pub source_revision_json: &'a str,
    /// First ordinal this admission sequence may use. A second provider call
    /// inside the same attempt (length recovery) continues after the first.
    pub call_ordinal_base: i64,
}

impl ContentProviderAdmission<'_> {
    pub(crate) async fn admit(
        &self,
        call_ordinal: i64,
        relation_role: &str,
    ) -> anyhow::Result<bool> {
        let mut tx = self.pool.begin().await?;
        let renewed = sqlx::query_scalar::<_, String>(
            "UPDATE public.content_work_items SET lease_expires_at = now() + interval '300 seconds', updated_at = now() WHERE id = $1 AND status = 'running' AND attempt_count = $2 AND lease_owner = $3 AND lease_expires_at::timestamptz > now() RETURNING id",
        )
        .bind(self.work_item_id)
        .bind(i32::try_from(self.attempt_no).unwrap_or(i32::MAX))
        .bind(self.lease_owner)
        .fetch_optional(&mut *tx)
        .await?;
        if renewed.is_none() {
            tx.rollback().await?;
            return Ok(false);
        }
        sqlx::query(
            "INSERT INTO public.content_attempt_provider_admissions (id, attempt_event_id, work_item_id, attempt_no, call_ordinal, relation_role, source_hash, source_revision_json, admitted_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now()) ON CONFLICT (attempt_event_id, call_ordinal) DO NOTHING",
        )
        .bind(local_id::generate_local_id())
        .bind(self.attempt_event_id)
        .bind(self.work_item_id)
        .bind(self.attempt_no)
        .bind(call_ordinal)
        .bind(relation_role)
        .bind(self.source_hash)
        .bind(self.source_revision_json)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }
}

/// Run one routed completion and persist every attempt in `llm_calls`.
///
/// Unlike [`AuditRecorder::complete`] this does not link the call into
/// `translation_attempt_llm_calls`; the global content worker owns its own
/// `content_attempt_llm_calls` link so the two work models stay separate.
/// `content_attempt_llm_calls` link so the two work models stay separate.
///
/// Inputs for one audited completion request.
pub(crate) struct CompletionRequest<'a> {
    pub source: &'a str,
    pub requested_by: Option<&'a str>,
    pub parent_task: Option<(&'a str, &'a str)>,
    pub system: &'a str,
    pub user: &'a str,
    pub max_tokens: u32,
}

pub(crate) async fn complete_with_audit(
    pool: &PgPool,
    http: &Client,
    ai: &AiConfig,
    request: CompletionRequest<'_>,
    admission: Option<&ContentProviderAdmission<'_>>,
) -> Result<AuditedCompletion, CompletionFailure> {
    let CompletionRequest {
        source,
        requested_by,
        parent_task: parent,
        system,
        user,
        max_tokens,
    } = request;
    let models = route_models(pool, ai.model.as_str())
        .await
        .map_err(CompletionFailure::Provider)?;
    let health = load_route_health(pool)
        .await
        .map_err(CompletionFailure::Provider)?;
    let candidates = route_candidates(&models, &health, Utc::now());
    if candidates.is_empty() {
        return Err(CompletionFailure::Provider(LlmFailure::configuration()));
    }
    let parent_task_id = parent.map(|(task_id, _)| task_id);
    let parent_task_type = parent.map(|(_, task_type)| task_type);
    let mut remaining = ROUTE_MAX_UPSTREAM_ATTEMPTS;
    let mut last_failure: Option<LlmFailure> = None;
    let mut call_ordinal = 0_i64;
    for (candidate_index, model) in candidates.into_iter().enumerate() {
        // The configured primary keeps its role even when health filtering
        // promotes another model to the first candidate position.
        let role = if models.first() == Some(&model) {
            "primary"
        } else {
            "fallback"
        };
        let fallback_count = i64::try_from(candidate_index).unwrap_or(i64::MAX);
        let mut retry_class: Option<crate::llm_contract::LlmFailureClass> = None;
        for model_attempt in 0..ROUTE_ATTEMPTS_PER_MODEL {
            if remaining == 0 {
                break;
            }
            remaining -= 1;
            if let Some(admission) = admission {
                let ordinal = admission.call_ordinal_base + call_ordinal;
                if !admission
                    .admit(ordinal, role)
                    .await
                    .map_err(CompletionFailure::from_admission)?
                {
                    return Err(CompletionFailure::LeaseLost);
                }
                call_ordinal += 1;
            }
            let call_id = local_id::generate_local_id();
            let prompt_text = truncate_chars(&format!("{system}\n\n{user}"), 8_000);
            let inserted = insert_call(
                pool,
                &call_id,
                source,
                model.as_str(),
                requested_by.unwrap_or(""),
                parent_task_id,
                parent_task_type,
                max_tokens,
                &prompt_text,
                &json!([
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ])
                .to_string(),
            )
            .await;
            if inserted {
                let _ = mark_call_running(pool, &call_id).await;
            }
            let started = std::time::Instant::now();
            let outcome = invoke_model(http, ai, model.as_str(), system, user, max_tokens).await;
            let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
            let audit = match &outcome {
                Ok(response) => CallAudit {
                    status: "succeeded",
                    model: response.model.clone(),
                    provider_request_id: response.provider_request_id.clone(),
                    provider_http_status: response.provider_http_status,
                    input_tokens: response.input_tokens,
                    output_tokens: response.output_tokens,
                    total_tokens: response.total_tokens,
                    finish_reason: response.finish_reason.clone(),
                    failure_class: None,
                    error_text: None,
                    response_text: Some(truncate_chars(&response.content, 8_000)),
                    duration_ms,
                },
                Err(failure) => CallAudit {
                    status: "failed",
                    model: model.clone(),
                    provider_request_id: None,
                    provider_http_status: failure.http_status,
                    input_tokens: None,
                    output_tokens: None,
                    total_tokens: None,
                    finish_reason: None,
                    failure_class: Some(failure.error.class.as_str()),
                    error_text: Some(failure.detail.clone()),
                    response_text: None,
                    duration_ms,
                },
            };
            if inserted {
                let _ = finalize_call(pool, &call_id, &audit, fallback_count).await;
            }
            match outcome {
                Ok(response) => {
                    return Ok(AuditedCompletion {
                        content: response.content,
                        call_id: inserted.then_some(call_id),
                        model: response.model,
                        provider_request_id: response.provider_request_id,
                        input_tokens: response.input_tokens,
                        output_tokens: response.output_tokens,
                        duration_ms: Some(duration_ms),
                        finish_reason: response.finish_reason,
                        attempts_used: call_ordinal,
                    });
                }
                Err(failure) => {
                    let class = failure.error.class;
                    let http_status = failure.http_status;
                    last_failure = Some(failure.error);
                    if class == crate::llm_contract::LlmFailureClass::Configuration {
                        if is_credential_rejection(http_status) {
                            park_model(pool, model.as_str()).await;
                        }
                        break;
                    }
                    if class == crate::llm_contract::LlmFailureClass::EmptyContent {
                        retry_class = Some(class);
                        break;
                    }
                    if model_attempt + 1 == ROUTE_ATTEMPTS_PER_MODEL || remaining == 0 {
                        retry_class = Some(class);
                        break;
                    }
                }
            }
        }
        if let Some(class) = retry_class {
            record_model_failure(pool, model.as_str(), class).await;
        }
        if remaining == 0 {
            break;
        }
    }
    Err(CompletionFailure::Provider(
        last_failure.unwrap_or_else(LlmFailure::transient),
    ))
}

async fn invoke_model(
    http: &Client,
    ai: &AiConfig,
    model: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<CallResponse, CallFailure> {
    let endpoint = ai
        .base_url
        .join("chat/completions")
        .map_err(|_| CallFailure {
            error: LlmFailure::configuration(),
            detail: "invalid AI base URL".to_owned(),
            http_status: None,
        })?;
    let response = http
        .post(endpoint)
        .bearer_auth(&ai.api_key)
        .timeout(LLM_CALL_TIMEOUT)
        .json(&json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "max_tokens": max_tokens,
            "temperature": 0,
            "stream": false,
        }))
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            return Err(CallFailure {
                error: LlmFailure::transient(),
                detail: truncate_chars(&error.to_string(), 200),
                http_status: None,
            });
        }
    };
    let status = response.status();
    let body_text = response.text().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&body_text).unwrap_or(Value::Null);
    if !status.is_success() {
        let detail = body_text.trim();
        let detail = if detail.is_empty() {
            format!("upstream status {}", status.as_u16())
        } else {
            truncate_chars(detail, 200)
        };
        let (class, disposition) =
            classify_upstream_failure_with_reason(status.as_u16(), detail.as_str());
        return Err(CallFailure {
            error: LlmFailure {
                class,
                message: class.safe_message().to_owned(),
                disposition,
            },
            detail,
            http_status: Some(i64::from(status.as_u16())),
        });
    }
    let content = body
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let content = match content {
        Some(content) if !content.trim().is_empty() => content,
        _ => {
            return Err(CallFailure {
                error: LlmFailure::empty_content(),
                detail: "ai response missing content".to_owned(),
                http_status: Some(i64::from(status.as_u16())),
            });
        }
    };
    Ok(CallResponse {
        content,
        model: body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(model)
            .to_owned(),
        provider_request_id: body.get("id").and_then(Value::as_str).map(str::to_owned),
        provider_http_status: Some(i64::from(status.as_u16())),
        input_tokens: body.pointer("/usage/prompt_tokens").and_then(Value::as_i64),
        output_tokens: body
            .pointer("/usage/completion_tokens")
            .and_then(Value::as_i64),
        total_tokens: body.pointer("/usage/total_tokens").and_then(Value::as_i64),
        finish_reason: body
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn truncate_chars(raw: &str, max_chars: usize) -> String {
    if raw.chars().count() <= max_chars {
        return raw.to_owned();
    }
    raw.chars().take(max_chars).collect()
}

// ---------------------------------------------------------------------------
// Validation and responses
// ---------------------------------------------------------------------------

async fn session_user(session: &Session) -> Result<String, Response> {
    match session.get::<String>("user_id").await {
        Ok(Some(user_id)) if !user_id.trim().is_empty() => Ok(user_id),
        Ok(_) => Err(unauthorized()),
        Err(error) => {
            warn!(%error, "PostgreSQL translation session read failed");
            Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "session_error",
                "session read failed",
            ))
        }
    }
}

fn parse_return_mode(query: &ReturnModeQuery, default: ReturnMode) -> Result<ReturnMode, Response> {
    let raw = query
        .return_mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match raw {
        None => Ok(default),
        Some("sync") => Ok(ReturnMode::Sync),
        Some("task_id") => Ok(ReturnMode::TaskId),
        Some("sse") => Ok(ReturnMode::Sse),
        Some(_) => Err(bad_request(
            "invalid return_mode, expected sync|task_id|sse",
        )),
    }
}

fn parse_release_id(raw: &str) -> Result<i64, Response> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(bad_request("release_id is required"));
    }
    trimmed
        .parse::<i64>()
        .map_err(|_| bad_request("release_id must be an integer string"))
}

fn parse_release_ids(raw_ids: &[String], max_items: usize) -> Result<Vec<i64>, Response> {
    if raw_ids.is_empty() {
        return Err(bad_request("release_ids is required"));
    }
    if raw_ids.len() > max_items {
        return Err(bad_request(&format!(
            "release_ids supports at most {max_items} items"
        )));
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw in raw_ids {
        let release_id = parse_release_id(raw)?;
        if seen.insert(release_id) {
            out.push(release_id);
        }
    }
    if out.is_empty() {
        return Err(bad_request("release_ids is required"));
    }
    Ok(out)
}

fn parse_thread_ids(raw_ids: &[String], max_items: usize) -> Result<Vec<String>, Response> {
    if raw_ids.is_empty() {
        return Err(bad_request("thread_ids is required"));
    }
    if raw_ids.len() > max_items {
        return Err(bad_request(&format!(
            "thread_ids supports at most {max_items} items"
        )));
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw in raw_ids {
        let thread_id = raw.trim();
        if thread_id.is_empty() {
            continue;
        }
        if seen.insert(thread_id.to_owned()) {
            out.push(thread_id.to_owned());
        }
    }
    if out.is_empty() {
        return Err(bad_request("thread_ids is required"));
    }
    Ok(out)
}

fn bad_request(message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, "bad_request", message)
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"code": "unauthorized"})),
    )
        .into_response()
}

fn error_response(status: StatusCode, code: &str, message: impl AsRef<str>) -> Response {
    (
        status,
        Json(json!({"code": code, "message": message.as_ref()})),
    )
        .into_response()
}

fn db_failure(error: sqlx::Error) -> Response {
    warn!(%error, "PostgreSQL translation endpoint failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"code": "database_error"})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{
        ModelRouteHealth, Target, TargetKind, admit_release_detail_translations, audit_recorder,
        chunk_chars_for_input_limit, classify_upstream_failure, deduped_release_ids,
        is_credential_rejection, load_route_health, park_model, publish_finished_translation,
        record_model_failure, release_has_translatable_content, route_candidates, route_models,
        start_attempt,
    };
    use crate::translations::TranslationSourceBlock;

    type LlmCallAuditRow = (String, String, Option<String>, i64, Option<String>);

    #[test]
    fn release_admission_deduplicates_release_ids_in_caller_order() {
        assert_eq!(deduped_release_ids(&[9, 9, 7, 9]), vec![9, 7]);
        assert_eq!(deduped_release_ids(&[]), Vec::<i64>::new());
        let ids = (1..=40).collect::<Vec<i64>>();
        assert_eq!(deduped_release_ids(&ids), ids);
    }

    #[test]
    fn release_admission_skips_releases_without_translatable_content() {
        assert!(!release_has_translatable_content(None, None));
        assert!(!release_has_translatable_content(Some(""), Some("  \n")));
        assert!(release_has_translatable_content(Some("v1.2.0"), None));
        assert!(release_has_translatable_content(None, Some("## Notes")));
    }

    /// REQ-RTR-008 functional gate: a Release sync that inserts a new Release
    /// admits exactly one global translation work item and writes no legacy
    /// `translation_work_items`/`translation_requests`/`translation.generate`
    /// row for it. A second pass for the same source revision creates nothing,
    /// which is what makes the counter meaningful.
    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_release_sync_admits_global_translation_without_legacy_rows()
    -> anyhow::Result<()> {
        use sqlx::PgPool;
        let admin_url = std::env::var("TEST_PG_TRANSLATE_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("translate_admit_{}", crate::local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;
        let result = async {
            sqlx::raw_sql(
                r#"
                CREATE TABLE public.repo_releases(release_id bigint PRIMARY KEY, repo_id bigint NOT NULL, html_url text NOT NULL, tag_name text NOT NULL, name text, body text, updated_at text, detected_at text);
                CREATE TABLE public.content_work_items(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, model_profile text NOT NULL, source_snapshot_json text NOT NULL, configuration_fingerprint text NOT NULL, status text NOT NULL, priority integer NOT NULL DEFAULT 0, cache_hit boolean NOT NULL DEFAULT false, token_estimate integer NOT NULL DEFAULT 0, attempt_count integer NOT NULL DEFAULT 0, evidence_status text NOT NULL DEFAULT 'not_required', next_retry_at text, retry_expires_at text, retry_after_at text, failure_class text, lease_owner text, lease_expires_at text, started_at text, finished_at text, cancelled_at text, supersedes_work_item_id text, batch_id text, created_at text NOT NULL, updated_at text NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile));
                CREATE TABLE public.content_work_identities(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, created_at text NOT NULL, updated_at text NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version));
                CREATE TABLE public.content_work_identity_members(work_item_id text PRIMARY KEY, identity_id text NOT NULL, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, linked_at text NOT NULL, UNIQUE (identity_id, work_item_id));
                CREATE TABLE public.content_result_projections(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, protocol_version text NOT NULL, model_profile text NOT NULL, source_hash text NOT NULL, work_item_id text NOT NULL, active_work_item_id text, payload_json text NOT NULL, published_at text NOT NULL, updated_at text NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash));
                CREATE TABLE public.content_current_result_projections(identity_id text PRIMARY KEY, work_item_id text NOT NULL, active_work_item_id text, source_projection_id text, payload_json text NOT NULL, published_at text NOT NULL, updated_at text NOT NULL);
                CREATE TABLE public.content_work_admission_events(id text PRIMARY KEY, work_item_id text NOT NULL, event_type text NOT NULL, replaced_by_work_item_id text, source_hash text NOT NULL, source_revision_json text NOT NULL DEFAULT '{}', producer_ref text NOT NULL DEFAULT '', requester_id text, reason_code text NOT NULL DEFAULT '', created_at text NOT NULL, UNIQUE (work_item_id, event_type, source_hash));
                CREATE TABLE public.content_attempt_events(id text PRIMARY KEY, work_item_id text NOT NULL, attempt_no bigint NOT NULL, trigger text NOT NULL, event_type text NOT NULL, result_status text, error_code text, error_summary text, failure_class text, retry_eligible bigint NOT NULL DEFAULT 0, next_retry_at text, duration_ms bigint, token_count bigint, cost_microunits bigint, created_at text NOT NULL, configuration_snapshot_json text, route_snapshot_json text, configuration_fingerprint text, UNIQUE (work_item_id, attempt_no, event_type));
                CREATE TABLE public.content_request_links(id text PRIMARY KEY, request_id text NOT NULL UNIQUE, work_item_id text NOT NULL, requester_type text NOT NULL, requester_id text, authorization_snapshot_json text NOT NULL, producer_ref text NOT NULL, request_source text NOT NULL, delivery_mode text NOT NULL, response_status text, created_at text NOT NULL, updated_at text NOT NULL);
                CREATE TABLE public.content_processing_control(id integer PRIMARY KEY, mode text NOT NULL, switch_token text, updated_at text NOT NULL);
                INSERT INTO public.content_processing_control(id, mode, updated_at) VALUES (1, 'global', now()::text);
                CREATE TABLE public.content_identity_upgrade_control(id bigint PRIMARY KEY, status text NOT NULL, phase text NOT NULL);
                INSERT INTO public.content_identity_upgrade_control(id, status, phase) VALUES (1, 'completed', 'complete');
                CREATE TABLE public.admin_runtime_settings(id integer PRIMARY KEY, llm_models_json text NOT NULL);
                INSERT INTO public.admin_runtime_settings(id, llm_models_json) VALUES (1, '["m1"]');
                CREATE TABLE public.llm_model_health(model text PRIMARY KEY, relevant_failure_count bigint NOT NULL DEFAULT 0, window_started_at timestamptz, cooldown_until timestamptz, last_failure_class text, last_failure_at timestamptz, created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL);
                CREATE TABLE public.translation_work_items(id text PRIMARY KEY, dedupe_key text UNIQUE NOT NULL, scope_user_id text NOT NULL, kind text NOT NULL, variant text NOT NULL, entity_id text NOT NULL, target_lang text NOT NULL, protocol_version text NOT NULL, model_profile text NOT NULL, source_hash text NOT NULL, source_blocks_json text NOT NULL, target_slots_json text NOT NULL, token_estimate integer NOT NULL, deadline_at text NOT NULL, status text NOT NULL, result_status text, error_text text, created_at text NOT NULL, finished_at text, updated_at text NOT NULL);
                CREATE TABLE public.translation_requests(id text PRIMARY KEY, mode text NOT NULL, source text NOT NULL, request_origin text NOT NULL, requested_by text, scope_user_id text NOT NULL, producer_ref text NOT NULL, kind text NOT NULL, variant text NOT NULL, entity_id text NOT NULL, target_lang text NOT NULL, max_wait_ms integer NOT NULL, source_hash text NOT NULL, source_blocks_json text NOT NULL, target_slots_json text NOT NULL, work_item_id text, status text NOT NULL, result_status text, error_text text, created_at text NOT NULL, started_at text, finished_at text, updated_at text NOT NULL);
                CREATE TABLE public.job_tasks(id text PRIMARY KEY, task_type text NOT NULL, status text NOT NULL, source text NOT NULL, requested_by text, payload_json text NOT NULL, created_at text NOT NULL, updated_at text NOT NULL);
                INSERT INTO public.repo_releases(release_id, repo_id, html_url, tag_name, name, body, updated_at)
                  VALUES (9001, 7, 'https://github.com/acme/demo/releases/tag/v1.0.0', 'v1.0.0', 'v1.0.0', 'Release notes body', '2026-11-24T00:00:00Z');
                "#,
            )
            .execute(&pool)
            .await?;

            let admitted =
                admit_release_detail_translations(&pool, &[9001], "m1", "sync.releases.auto_translate")
                    .await;
            assert_eq!(admitted, 1, "one new Release becomes one global work item");
            let work_items: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.content_work_items WHERE canonical_resource_type = 'release' AND canonical_resource_id = '9001' AND pipeline = 'translation' AND variant = 'detail'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(work_items, 1, "exactly one global translation work item");
            let status: String = sqlx::query_scalar(
                "SELECT status FROM public.content_work_items WHERE canonical_resource_id = '9001'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(status, "queued", "new work is queued for the global worker");

            let legacy_work: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM public.translation_work_items")
                    .fetch_one(&pool)
                    .await?;
            assert_eq!(legacy_work, 0, "no per-user work item is written");
            let legacy_requests: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM public.translation_requests")
                    .fetch_one(&pool)
                    .await?;
            assert_eq!(legacy_requests, 0, "no per-user request row is written");
            let legacy_tasks: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.job_tasks WHERE task_type = 'translation.generate'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(legacy_tasks, 0, "no legacy translation.generate task is queued");

            let again =
                admit_release_detail_translations(&pool, &[9001], "m1", "sync.releases.auto_translate")
                    .await;
            assert_eq!(again, 0, "the same source revision admits nothing twice");
            let work_items: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.content_work_items WHERE canonical_resource_id = '9001'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(work_items, 1);
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

    fn release_target_for_test(release_id: i64, body: &str) -> Target {
        let source_blocks = vec![
            TranslationSourceBlock {
                slot: "metadata".to_owned(),
                text: "acme/demo\nv1".to_owned(),
            },
            TranslationSourceBlock {
                slot: "title".to_owned(),
                text: "v1".to_owned(),
            },
            TranslationSourceBlock {
                slot: "body_markdown".to_owned(),
                text: body.to_owned(),
            },
        ];
        Target {
            entity_id: release_id.to_string(),
            kind: TargetKind::ReleaseDetail,
            variant: TargetKind::ReleaseDetail.work_variant().to_owned(),
            source: "test".to_owned(),
            title: "v1".to_owned(),
            body: body.to_owned(),
            repo_full_name: "acme/demo".to_owned(),
            reason: String::new(),
            subject_type: String::new(),
            source_blocks,
            target_slots: vec!["title_zh".to_owned(), "body_md".to_owned()],
            source_hash: crate::content_source_identity::release_detail_source_hash(
                "acme/demo",
                "v1",
                body,
            ),
        }
    }

    async fn create_publication_schema(pool: &sqlx::PgPool) -> anyhow::Result<()> {
        sqlx::raw_sql(
            r#"
            CREATE TABLE public.repo_releases(release_id bigint PRIMARY KEY, repo_id bigint NOT NULL, html_url text NOT NULL, tag_name text NOT NULL, name text, body text);
            CREATE TABLE public.content_work_items(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, model_profile text NOT NULL, source_snapshot_json text NOT NULL, configuration_fingerprint text NOT NULL, status text NOT NULL, priority integer NOT NULL DEFAULT 0, cache_hit boolean NOT NULL DEFAULT false, token_estimate integer NOT NULL DEFAULT 0, attempt_count integer NOT NULL DEFAULT 0, evidence_status text NOT NULL DEFAULT 'not_required', finished_at timestamptz, created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile));
            CREATE TABLE public.content_work_identities(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, created_at text NOT NULL, updated_at text NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version));
            CREATE TABLE public.content_work_identity_members(work_item_id text PRIMARY KEY, identity_id text NOT NULL, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, source_hash text NOT NULL, protocol_version text NOT NULL, linked_at text NOT NULL, UNIQUE (identity_id, work_item_id));
            CREATE TABLE public.content_result_projections(id text PRIMARY KEY, canonical_resource_type text NOT NULL, canonical_resource_id text NOT NULL, pipeline text NOT NULL, variant text NOT NULL, target_lang text NOT NULL, protocol_version text NOT NULL, model_profile text NOT NULL, source_hash text NOT NULL, work_item_id text NOT NULL, active_work_item_id text, payload_json text NOT NULL, published_at timestamptz NOT NULL, updated_at timestamptz NOT NULL, UNIQUE (canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash));
            CREATE TABLE public.content_current_result_projections(identity_id text PRIMARY KEY, work_item_id text NOT NULL, active_work_item_id text, source_projection_id text, payload_json text NOT NULL, published_at text NOT NULL, updated_at text NOT NULL);
            CREATE TABLE public.content_processing_control(id integer PRIMARY KEY, mode text NOT NULL, switch_token text, updated_at timestamptz NOT NULL);
            INSERT INTO public.content_processing_control(id, mode, updated_at) VALUES (1, 'global', now());
            CREATE TABLE public.content_identity_upgrade_control(id bigint PRIMARY KEY, status text NOT NULL, phase text NOT NULL);
            INSERT INTO public.content_identity_upgrade_control(id, status, phase) VALUES (1, 'completed', 'complete');
            "#,
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    /// A finished release translation publishes a projection that the public
    /// detail read resolves as `ready`, and a republish of the same source
    /// revision does not duplicate it.
    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_release_completion_publishes_a_readable_projection() -> anyhow::Result<()> {
        use sqlx::PgPool;
        let admin_url = std::env::var("TEST_PG_TRANSLATE_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("translate_publish_{}", crate::local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;
        let result = async {
            create_publication_schema(&pool).await?;
            sqlx::query(
                "INSERT INTO public.repo_releases(release_id, repo_id, html_url, tag_name, name, body) VALUES (42, 7, 'https://github.com/acme/demo/releases/tag/v1', 'v1', 'v1', 'Release notes body')",
            )
            .execute(&pool)
            .await?;
            let target = release_target_for_test(42, "Release notes body");

            let mut tx = pool.begin().await?;
            publish_finished_translation(
                &mut tx,
                &target,
                Some("项目 v1 发布"),
                Some("发布说明正文"),
            )
            .await?;
            tx.commit().await?;

            let overlay = crate::postgres_content_overlay::OverlayState::load(&pool).await;
            let source = crate::postgres_content_overlay::ReleaseSource {
                release_id: 42,
                repo_full_name: "acme/demo",
                repo_identity: "acme/demo",
                tag_name: "v1",
                name: Some("v1"),
                body: Some("Release notes body"),
                previous_tag_name: None,
            };
            let projection =
                crate::postgres_content_overlay::public_release_translation(&pool, overlay, &source)
                    .await?
                    .expect("public detail resolves the published projection");
            assert_eq!(projection.status, "ready");
            assert_eq!(projection.title().as_deref(), Some("项目 v1 发布"));

            // Republishing the same source revision stays a single row.
            let mut tx = pool.begin().await?;
            publish_finished_translation(
                &mut tx,
                &target,
                Some("项目 v1 发布"),
                Some("发布说明正文"),
            )
            .await?;
            tx.commit().await?;
            let projection_rows: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM public.content_result_projections")
                    .fetch_one(&pool)
                    .await?;
            assert_eq!(projection_rows, 1, "a retry must not duplicate the projection");
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

    /// A release edited after the attempt was enqueued describes text the
    /// reader can no longer resolve, so its output is not published.
    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_stale_release_source_is_not_published() -> anyhow::Result<()> {
        use sqlx::PgPool;
        let admin_url = std::env::var("TEST_PG_TRANSLATE_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("translate_stale_{}", crate::local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;
        let result = async {
            create_publication_schema(&pool).await?;
            // The attempt translated the old body; the row now holds new text.
            sqlx::query(
                "INSERT INTO public.repo_releases(release_id, repo_id, html_url, tag_name, name, body) VALUES (42, 7, 'https://github.com/acme/demo/releases/tag/v1', 'v1', 'v1', 'Edited body')",
            )
            .execute(&pool)
            .await?;
            let target = release_target_for_test(42, "Original body");

            let mut tx = pool.begin().await?;
            publish_finished_translation(
                &mut tx,
                &target,
                Some("项目 v1 发布"),
                Some("旧正文译文"),
            )
            .await?;
            tx.commit().await?;

            let projections: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM public.content_result_projections")
                    .fetch_one(&pool)
                    .await?;
            let currents: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.content_current_result_projections",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(projections, 0, "a stale release must not publish a projection");
            assert_eq!(currents, 0, "a stale release must not publish a current projection");
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

    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_start_attempt_accepts_imported_text_timestamps_and_bigint_attempt_no()
    -> anyhow::Result<()> {
        use sqlx::PgPool;
        let admin_url = std::env::var("TEST_PG_TRANSLATE_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("translate_attempt_{}", crate::local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;
        let result = async {
            sqlx::raw_sql(
                r#"
                CREATE TABLE translation_work_items(id text PRIMARY KEY,status text NOT NULL,started_at text,updated_at text NOT NULL);
                CREATE TABLE translation_requests(id text PRIMARY KEY,status text NOT NULL,started_at text,updated_at text NOT NULL);
                CREATE TABLE translation_attempt_events(id text PRIMARY KEY,work_item_id text NOT NULL,request_id text,scope_user_id text NOT NULL,kind text NOT NULL,variant text NOT NULL,entity_id text NOT NULL,target_lang text NOT NULL,attempt_no bigint NOT NULL,trigger text NOT NULL,event_type text NOT NULL,result_status text,error_code text,error_summary text,failure_class text,retry_eligible bigint NOT NULL DEFAULT 0,next_retry_at text,llm_call_ids_json text NOT NULL DEFAULT '[]',created_at text NOT NULL,UNIQUE(work_item_id,attempt_no,event_type));
                "#,
            )
            .execute(&pool)
            .await?;
            let now = chrono::Utc::now();
            sqlx::query(
                "INSERT INTO translation_work_items(id,status,started_at,updated_at) VALUES('work-a','queued',NULL,$1)",
            )
            .bind(now)
            .execute(&pool)
            .await?;
            sqlx::query(
                "INSERT INTO translation_requests(id,status,started_at,updated_at) VALUES('request-a','queued',NULL,$1)",
            )
            .bind(now)
            .execute(&pool)
            .await?;
            let mut tx = pool.begin().await?;
            let attempt_no = start_attempt(
                &mut tx,
                "work-a",
                Some("request-a"),
                "user-a",
                TargetKind::ReleaseDetail,
                "detail",
                "release-a",
                now,
            )
            .await?;
            tx.commit().await?;
            assert_eq!(attempt_no, 1);
            let events: Vec<(String, String)> = sqlx::query_as(
                "SELECT event_type,id FROM translation_attempt_events WHERE work_item_id='work-a' ORDER BY event_type",
            )
            .fetch_all(&pool)
            .await?;
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].0, "attempt_queued");
            assert_eq!(events[1].0, "attempt_started");
            assert!(events.iter().all(|(_, id)| !id.is_empty()));
            let work_status: String =
                sqlx::query_scalar("SELECT status FROM translation_work_items WHERE id='work-a'")
                    .fetch_one(&pool)
                    .await?;
            assert_eq!(work_status, "running");
            let request_status: String =
                sqlx::query_scalar("SELECT status FROM translation_requests WHERE id='request-a'")
                    .fetch_one(&pool)
                    .await?;
            assert_eq!(request_status, "running");
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

    #[test]
    fn route_candidates_uses_only_ready_models() {
        let now = chrono::Utc::now();
        let models = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let mut health = std::collections::HashMap::new();
        health.insert(
            "a".to_owned(),
            ModelRouteHealth {
                cooldown_until: Some(now + chrono::Duration::minutes(5)),
                last_failure_class: Some("rate_limited".to_owned()),
            },
        );
        health.insert(
            "c".to_owned(),
            ModelRouteHealth {
                cooldown_until: Some(now + chrono::Duration::minutes(1)),
                last_failure_class: Some("transient".to_owned()),
            },
        );
        assert_eq!(
            route_candidates(&models, &health, now),
            vec!["b".to_owned()]
        );
    }

    #[test]
    fn route_candidates_probes_single_earliest_cooling_model() {
        let now = chrono::Utc::now();
        let models = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let mut health = std::collections::HashMap::new();
        health.insert(
            "a".to_owned(),
            ModelRouteHealth {
                cooldown_until: Some(now + chrono::Duration::minutes(5)),
                last_failure_class: Some("rate_limited".to_owned()),
            },
        );
        health.insert(
            "b".to_owned(),
            ModelRouteHealth {
                cooldown_until: Some(now + chrono::Duration::minutes(2)),
                last_failure_class: Some("transient".to_owned()),
            },
        );
        health.insert(
            "c".to_owned(),
            ModelRouteHealth {
                cooldown_until: Some(now + chrono::Duration::minutes(1)),
                last_failure_class: Some("transient".to_owned()),
            },
        );
        assert_eq!(
            route_candidates(&models, &health, now),
            vec!["c".to_owned()]
        );
    }

    #[test]
    fn route_candidates_skips_credential_parked_models() {
        let now = chrono::Utc::now();
        let models = vec!["parked".to_owned(), "ready".to_owned()];
        let mut health = std::collections::HashMap::new();
        health.insert(
            "parked".to_owned(),
            ModelRouteHealth {
                cooldown_until: Some(now + chrono::Duration::hours(1)),
                last_failure_class: Some("configuration".to_owned()),
            },
        );
        assert_eq!(
            route_candidates(&models, &health, now),
            vec!["ready".to_owned()]
        );
    }

    #[test]
    fn route_candidates_returns_empty_when_every_model_is_parked() {
        let now = chrono::Utc::now();
        let models = vec!["a".to_owned(), "b".to_owned()];
        let health = models
            .iter()
            .map(|model| {
                (
                    model.clone(),
                    ModelRouteHealth {
                        cooldown_until: Some(now + chrono::Duration::hours(1)),
                        last_failure_class: Some("configuration".to_owned()),
                    },
                )
            })
            .collect();
        assert!(route_candidates(&models, &health, now).is_empty());
    }

    #[test]
    fn route_candidates_matches_normalized_health_keys() {
        let now = chrono::Utc::now();
        let models = vec!["Model-A".to_owned(), "ready".to_owned()];
        let mut health = std::collections::HashMap::new();
        health.insert(
            "model-a".to_owned(),
            ModelRouteHealth {
                cooldown_until: Some(now + chrono::Duration::minutes(5)),
                last_failure_class: Some("rate_limited".to_owned()),
            },
        );
        assert_eq!(
            route_candidates(&models, &health, now),
            vec!["ready".to_owned()]
        );
    }

    #[test]
    fn route_candidates_treats_cleared_credential_park_as_ready() {
        let now = chrono::Utc::now();
        let models = vec!["cleared".to_owned()];
        let mut health = std::collections::HashMap::new();
        health.insert(
            "cleared".to_owned(),
            ModelRouteHealth {
                cooldown_until: None,
                last_failure_class: Some("configuration".to_owned()),
            },
        );
        assert_eq!(
            route_candidates(&models, &health, now),
            vec!["cleared".to_owned()]
        );
    }

    #[test]
    fn credential_rejection_and_upstream_classes_are_distinct() {
        assert!(is_credential_rejection(Some(401)));
        assert!(is_credential_rejection(Some(403)));
        assert!(!is_credential_rejection(Some(400)));
        assert_eq!(
            classify_upstream_failure(429, "rate limited"),
            crate::llm_contract::LlmFailureClass::RateLimited
        );
        assert_eq!(
            classify_upstream_failure(401, "unauthorized"),
            crate::llm_contract::LlmFailureClass::Configuration
        );
        assert_eq!(
            classify_upstream_failure(500, "upstream error"),
            crate::llm_contract::LlmFailureClass::Transient
        );
    }

    #[test]
    fn chunk_chars_clamp_for_small_and_large_models() {
        assert_eq!(chunk_chars_for_input_limit(100), 2_000);
        assert_eq!(chunk_chars_for_input_limit(4_000), 12_000);
        assert_eq!(chunk_chars_for_input_limit(100_000), 12_000);
    }

    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_model_health_cools_after_two_failures() -> anyhow::Result<()> {
        use sqlx::PgPool;
        let admin_url = std::env::var("TEST_PG_TRANSLATE_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("translate_health_{}", crate::local_id::generate_local_id());
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
            record_model_failure(
                &pool,
                "model-a",
                crate::llm_contract::LlmFailureClass::RateLimited,
            )
            .await;
            record_model_failure(
                &pool,
                "model-a",
                crate::llm_contract::LlmFailureClass::RateLimited,
            )
            .await;
            let (count, cooldown, class): (
                i64,
                Option<chrono::DateTime<chrono::Utc>>,
                Option<String>,
            ) = sqlx::query_as(
                "SELECT relevant_failure_count, cooldown_until, last_failure_class FROM public.llm_model_health WHERE model = 'model-a'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(count, 2);
            assert!(cooldown.is_some());
            assert_eq!(class.as_deref(), Some("rate_limited"));
            park_model(&pool, "model-b").await;
            let (parked_until, parked_class): (Option<chrono::DateTime<chrono::Utc>>, Option<String>) = sqlx::query_as(
                "SELECT cooldown_until, last_failure_class FROM public.llm_model_health WHERE model = 'model-b'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(parked_class.as_deref(), Some("configuration"));
            assert!(
                parked_until.expect("configuration failure parks the model")
                    > chrono::Utc::now() + chrono::Duration::hours(23)
            );
            let now = chrono::Utc::now();
            sqlx::query(
                "INSERT INTO public.llm_model_health(model, relevant_failure_count, window_started_at, cooldown_until, last_failure_class, created_at, updated_at) VALUES ('model-c', 2, $1, $2, 'rate_limited', $3, $3)",
            )
            .bind(now - chrono::Duration::minutes(11))
            .bind(now + chrono::Duration::minutes(5))
            .bind(now)
            .execute(&pool)
            .await?;
            record_model_failure(
                &pool,
                "model-c",
                crate::llm_contract::LlmFailureClass::RateLimited,
            )
            .await;
            let (count, cooldown): (i64, Option<chrono::DateTime<chrono::Utc>>) =
                sqlx::query_as(
                    "SELECT relevant_failure_count, cooldown_until FROM public.llm_model_health WHERE model = 'model-c'",
                )
                .fetch_one(&pool)
                .await?;
            assert_eq!(count, 1, "the expired failure window restarts the count");
            assert!(
                cooldown.expect("an active cooldown must survive window rollover")
                    > now + chrono::Duration::minutes(4)
            );
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

    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_route_models_falls_back_only_when_persisted_list_is_empty()
    -> anyhow::Result<()> {
        use sqlx::PgPool;
        let admin_url = std::env::var("TEST_PG_TRANSLATE_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!("translate_routes_{}", crate::local_id::generate_local_id());
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;
        let result = async {
            sqlx::raw_sql(
                "CREATE TABLE public.admin_runtime_settings(id integer PRIMARY KEY, llm_models_json text NOT NULL);",
            )
            .execute(&pool)
            .await?;
            sqlx::query(
                "INSERT INTO public.admin_runtime_settings(id, llm_models_json) VALUES (1, '[]')",
            )
            .execute(&pool)
            .await?;
            assert_eq!(
                route_models(&pool, "fallback").await?,
                vec!["fallback".to_owned()]
            );
            sqlx::query("DELETE FROM public.admin_runtime_settings WHERE id = 1")
                .execute(&pool)
                .await?;
            let missing_error = route_models(&pool, "fallback")
                .await
                .expect_err("a missing routing settings row must fail closed");
            assert_eq!(
                missing_error.class,
                crate::llm_contract::LlmFailureClass::Configuration
            );
            sqlx::query(
                "INSERT INTO public.admin_runtime_settings(id, llm_models_json) VALUES (1, '[]')",
            )
            .execute(&pool)
            .await?;
            sqlx::query(
                "UPDATE public.admin_runtime_settings SET llm_models_json = '[\"alpha\", \"beta\"]' WHERE id = 1",
            )
            .execute(&pool)
            .await?;
            assert_eq!(
                route_models(&pool, "fallback").await?,
                vec!["alpha".to_owned(), "beta".to_owned()]
            );
            sqlx::query(
                "UPDATE public.admin_runtime_settings SET llm_models_json = 'not-json' WHERE id = 1",
            )
            .execute(&pool)
            .await?;
            let parse_error = route_models(&pool, "fallback")
                .await
                .expect_err("malformed routing settings must fail closed");
            assert_eq!(
                parse_error.class,
                crate::llm_contract::LlmFailureClass::Configuration
            );
            sqlx::query("DROP TABLE public.admin_runtime_settings")
                .execute(&pool)
                .await?;
            let read_error = route_models(&pool, "fallback")
                .await
                .expect_err("unreadable routing settings must fail closed");
            assert_eq!(read_error.class, crate::llm_contract::LlmFailureClass::Transient);
            sqlx::raw_sql(
                "CREATE TABLE public.llm_model_health(model text PRIMARY KEY, cooldown_until timestamptz, last_failure_class text);",
            )
            .execute(&pool)
            .await?;
            sqlx::query("DROP TABLE public.llm_model_health")
                .execute(&pool)
                .await?;
            let health_error = load_route_health(&pool)
                .await
                .expect_err("unreadable health must fail closed");
            assert_eq!(health_error.class, crate::llm_contract::LlmFailureClass::Transient);
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

    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_translate_complete_routes_failures_and_audits_switch() -> anyhow::Result<()> {
        use axum::{Json, Router, http::StatusCode, routing::post};
        use sqlx::PgPool;
        use std::sync::atomic::{AtomicBool, Ordering};
        let admin_url = std::env::var("TEST_PG_TRANSLATE_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!(
            "translate_failover_{}",
            crate::local_id::generate_local_id()
        );
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;
        let observed = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
        let fail_all = std::sync::Arc::new(AtomicBool::new(false));
        let recorder = observed.clone();
        let handler_fail_all = fail_all.clone();
        let app = Router::new().route(
            "/chat/completions",
            post(move |Json(payload): Json<serde_json::Value>| {
                let recorder = recorder.clone();
                async move {
                    let model = payload["model"].as_str().unwrap_or_default().to_owned();
                    recorder.lock().await.push(model.clone());
                    let status = if model == "model-a" || handler_fail_all.load(Ordering::SeqCst) {
                        StatusCode::TOO_MANY_REQUESTS
                    } else if model == "park-401" {
                        StatusCode::UNAUTHORIZED
                    } else if model == "park-403" {
                        StatusCode::FORBIDDEN
                    } else if model == "bad-400" {
                        StatusCode::BAD_REQUEST
                    } else if model == "bad-404" {
                        StatusCode::NOT_FOUND
                    } else if model == "bad-422" {
                        StatusCode::UNPROCESSABLE_ENTITY
                    } else {
                        StatusCode::OK
                    };
                    if status != StatusCode::OK {
                        return (
                            status,
                            Json(serde_json::json!({"error": {"message": "upstream rejected"}})),
                        );
                    }
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "id": "req-1",
                            "model": model,
                            "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                        })),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test ai app");
        });
        let ai = crate::config::AiConfig {
            base_url: url::Url::parse(&format!("http://{addr}/"))?,
            model: "fallback".to_owned(),
            api_key: "test-key".to_owned(),
        };
        let http = reqwest::Client::new();
        let result = async {
            sqlx::raw_sql(
                r#"
                CREATE TABLE public.admin_runtime_settings(id integer PRIMARY KEY, llm_models_json text NOT NULL);
                INSERT INTO public.admin_runtime_settings(id, llm_models_json) VALUES (1, '["model-a", "model-b"]');
                CREATE TABLE public.llm_model_health(model text PRIMARY KEY, relevant_failure_count bigint NOT NULL DEFAULT 0, window_started_at timestamptz, cooldown_until timestamptz, last_failure_class text, last_failure_at timestamptz, created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL);
                CREATE TABLE public.llm_calls(id text PRIMARY KEY, status text NOT NULL, source text NOT NULL, model text NOT NULL, requested_by text NOT NULL, parent_task_id text, parent_task_type text, max_tokens bigint, attempt_count bigint, scheduler_wait_ms bigint, prompt_text text, input_messages_json text, created_at text NOT NULL, updated_at text NOT NULL, started_at text, final_model text, provider_request_id text, provider_http_status bigint, input_tokens bigint, output_tokens bigint, total_tokens bigint, finish_reason text, failure_class text, error_text text, response_text text, duration_ms bigint, fallback_count bigint, recovery_attempt_count bigint, finished_at text, runtime_owner_id text, lease_heartbeat_at text);
                CREATE TABLE public.llm_call_events(id text PRIMARY KEY, call_id text NOT NULL, event_type text NOT NULL, status text NOT NULL, source text NOT NULL, requested_by text NOT NULL, parent_task_id text, failure_class text, model text NOT NULL, attempt bigint NOT NULL, payload_json text NOT NULL, created_at text NOT NULL);
                CREATE TABLE public.translation_attempt_llm_calls(id text PRIMARY KEY, work_item_id text NOT NULL, attempt_no bigint NOT NULL, llm_call_id text NOT NULL, stage text NOT NULL, ordinal bigint NOT NULL, relation_role text NOT NULL, evidence_availability text NOT NULL, created_at timestamptz NOT NULL);
                "#,
            )
            .execute(&pool)
            .await?;
            let mut audit = audit_recorder(
                &pool,
                &http,
                &ai,
                "test.source",
                "user-a",
                "work-a",
                1,
                None,
            );
            let content = audit.complete("body", "system", "user", 64).await?;
            assert_eq!(content, "ok");
            assert_eq!(
                observed.lock().await.clone(),
                vec![
                    "model-a".to_owned(),
                    "model-a".to_owned(),
                    "model-b".to_owned()
                ]
            );
            let rows: Vec<LlmCallAuditRow> = sqlx::query_as(
                    "SELECT c.status, c.model, c.final_model, c.fallback_count, c.failure_class FROM public.llm_calls c JOIN public.translation_attempt_llm_calls l ON l.llm_call_id = c.id ORDER BY l.ordinal",
                )
                .fetch_all(&pool)
                .await?;
            assert_eq!(rows.len(), 3);
            assert_eq!(rows[0].0, "failed");
            assert_eq!(rows[0].1, "model-a");
            assert_eq!(rows[0].3, 0);
            assert_eq!(rows[0].4.as_deref(), Some("rate_limited"));
            assert_eq!(rows[1].0, "failed");
            assert_eq!(rows[1].1, "model-a");
            assert_eq!(rows[1].3, 0);
            assert_eq!(rows[1].4.as_deref(), Some("rate_limited"));
            assert_eq!(rows[2].0, "succeeded");
            assert_eq!(rows[2].1, "model-b");
            assert_eq!(rows[2].2.as_deref(), Some("model-b"));
            assert_eq!(rows[2].3, 1, "one model switch is one fallback");
            assert_eq!(rows[2].4, None);
            let (count, cooldown, class): (
                i64,
                Option<chrono::DateTime<chrono::Utc>>,
                Option<String>,
            ) = sqlx::query_as(
                "SELECT relevant_failure_count, cooldown_until, last_failure_class FROM public.llm_model_health WHERE model = 'model-a'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(count, 1, "one logical model failure, not one per attempt");
            assert!(cooldown.is_none());
            assert_eq!(class.as_deref(), Some("rate_limited"));
            fail_all.store(true, Ordering::SeqCst);
            sqlx::query(
                "UPDATE public.admin_runtime_settings SET llm_models_json = '[\"f1\", \"f2\", \"f3\", \"f4\", \"f5\"]' WHERE id = 1",
            )
            .execute(&pool)
            .await?;
            let failure = audit
                .complete("body", "system", "user", 64)
                .await
                .expect_err("all failing candidates must exhaust the attempt budget");
            assert_eq!(failure.class, crate::llm_contract::LlmFailureClass::RateLimited);
            let observed_after_budget = observed.lock().await.clone();
            assert_eq!(observed_after_budget.len(), 11);
            assert_eq!(
                observed_after_budget[3..].to_vec(),
                vec![
                    "f1".to_owned(),
                    "f1".to_owned(),
                    "f2".to_owned(),
                    "f2".to_owned(),
                    "f3".to_owned(),
                    "f3".to_owned(),
                    "f4".to_owned(),
                    "f4".to_owned()
                ]
            );
            let calls: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM public.llm_calls")
                .fetch_one(&pool)
                .await?;
            assert_eq!(calls, 11, "the logical call stops after eight attempts");
            let f5_health: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM public.llm_model_health WHERE model = 'f5'")
                    .fetch_one(&pool)
                    .await?;
            assert_eq!(f5_health, 0, "the ninth candidate is never attempted");
            fail_all.store(false, Ordering::SeqCst);
            sqlx::query(
                "UPDATE public.admin_runtime_settings SET llm_models_json = '[\"park-401\", \"park-403\", \"after-park\"]' WHERE id = 1",
            )
            .execute(&pool)
            .await?;
            let content = audit.complete("body", "system", "user", 64).await?;
            assert_eq!(content, "ok");
            let observed_after_park = observed.lock().await.clone();
            assert_eq!(observed_after_park.len(), 14);
            assert_eq!(
                observed_after_park[11..].to_vec(),
                vec![
                    "park-401".to_owned(),
                    "park-403".to_owned(),
                    "after-park".to_owned()
                ]
            );
            let parked_health: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.llm_model_health WHERE model IN ('park-401', 'park-403') AND last_failure_class = 'configuration' AND cooldown_until > $1",
            )
            .bind(chrono::Utc::now() + chrono::Duration::hours(23))
            .fetch_one(&pool)
            .await?;
            assert_eq!(parked_health, 2);
            let after_park_health: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.llm_model_health WHERE model = 'after-park'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(after_park_health, 0);
            let last_row: (String, i64) = sqlx::query_as(
                "SELECT c.model, c.fallback_count FROM public.llm_calls c JOIN public.translation_attempt_llm_calls l ON l.llm_call_id = c.id ORDER BY l.ordinal DESC LIMIT 1",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(last_row.0, "after-park");
            assert_eq!(last_row.1, 2);
            sqlx::query(
                "UPDATE public.admin_runtime_settings SET llm_models_json = '[\"bad-400\", \"bad-404\", \"bad-422\", \"after-bad\"]' WHERE id = 1",
            )
            .execute(&pool)
            .await?;
            let content = audit.complete("body", "system", "user", 64).await?;
            assert_eq!(content, "ok");
            let observed_after_bad = observed.lock().await.clone();
            assert_eq!(observed_after_bad.len(), 18);
            assert_eq!(
                observed_after_bad[14..].to_vec(),
                vec![
                    "bad-400".to_owned(),
                    "bad-404".to_owned(),
                    "bad-422".to_owned(),
                    "after-bad".to_owned()
                ]
            );
            let bad_health: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.llm_model_health WHERE model IN ('bad-400', 'bad-404', 'bad-422')",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(bad_health, 0, "request-level 4xx must not cool models");
            let after_bad_health: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM public.llm_model_health WHERE model = 'after-bad'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(after_bad_health, 0);
            let last_row: (String, i64) = sqlx::query_as(
                "SELECT c.model, c.fallback_count FROM public.llm_calls c JOIN public.translation_attempt_llm_calls l ON l.llm_call_id = c.id ORDER BY l.ordinal DESC LIMIT 1",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(last_row.0, "after-bad");
            assert_eq!(last_row.1, 3);
            let calls: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM public.llm_calls")
                .fetch_one(&pool)
                .await?;
            assert_eq!(calls, 18);
            sqlx::raw_sql(
                "DROP TABLE public.llm_model_health; CREATE TABLE public.llm_model_health(model text PRIMARY KEY, cooldown_until text, last_failure_class text); INSERT INTO public.llm_model_health(model, cooldown_until, last_failure_class) VALUES ('broken', 'not-a-time', 'rate_limited');",
            )
            .execute(&pool)
            .await?;
            let before_health_failure = observed.lock().await.len();
            let failure = audit
                .complete("body", "system", "user", 64)
                .await
                .expect_err("malformed health must fail closed");
            assert_eq!(failure.class, crate::llm_contract::LlmFailureClass::Transient);
            assert_eq!(observed.lock().await.len(), before_health_failure);
            let calls: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM public.llm_calls")
                .fetch_one(&pool)
                .await?;
            assert_eq!(calls, 18, "malformed health must not create a provider call");
            Ok::<_, anyhow::Error>(())
        }
        .await;
        server.abort();
        pool.close().await;
        sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
            .execute(&admin)
            .await?;
        admin.close().await;
        result
    }
    #[tokio::test]
    #[ignore = "requires TEST_PG_TRANSLATE_URL pointing at an isolated PostgreSQL 18 cluster"]
    async fn postgres_reconciliation_closes_only_terminal_requests() -> anyhow::Result<()> {
        use sqlx::PgPool;
        let admin_url = std::env::var("TEST_PG_TRANSLATE_URL")?;
        let admin = PgPool::connect(&admin_url).await?;
        let database = format!(
            "translate_reconcile_{}",
            crate::local_id::generate_local_id()
        );
        sqlx::query(&format!("CREATE DATABASE {database}"))
            .execute(&admin)
            .await?;
        let mut url = url::Url::parse(&admin_url)?;
        url.set_path(&database);
        let pool = PgPool::connect(url.as_str()).await?;
        let result = async {
            sqlx::raw_sql(
                r#"
                CREATE TABLE public.translation_work_items(id text PRIMARY KEY, status text NOT NULL, result_status text, title_zh text, summary_md text, body_md text, error_text text);
                CREATE TABLE public.translation_requests(id text PRIMARY KEY, work_item_id text, status text NOT NULL, result_status text, title_zh text, summary_md text, body_md text, error_text text, finished_at timestamptz, updated_at timestamptz NOT NULL);
                CREATE TABLE public.job_tasks(id text PRIMARY KEY, task_type text NOT NULL, status text NOT NULL, payload_json text NOT NULL);
                INSERT INTO public.translation_work_items(id,status,result_status,title_zh,summary_md,body_md,error_text) VALUES
                  ('wf','failed','error',NULL,NULL,NULL,'LLM upstream rate limited'),
                  ('wc','completed','ready','标题','- 变更','- 变更',NULL),
                  ('wq','queued',NULL,NULL,NULL,NULL,NULL),
                  ('wl','failed','error',NULL,NULL,NULL,'LLM upstream rate limited'),
                  ('wr','failed','error',NULL,NULL,NULL,'AI response missing content');
                INSERT INTO public.translation_requests(id,work_item_id,status,updated_at) VALUES
                  ('rf','wf','queued',now()),
                  ('rc','wc','queued',now()),
                  ('rq','wq','queued',now()),
                  ('rl','wl','queued',now()),
                  ('rr','wr','running',now());
                INSERT INTO public.job_tasks(id,task_type,status,payload_json) VALUES
                  ('task-l','translation.generate','queued','{"work_item_id":"wl"}');
                "#,
            )
            .execute(&pool)
            .await?;
            let closed = super::reconcile_terminal_translation_requests(&pool).await?;
            assert_eq!(closed, 3, "failed, completed and orphaned-running rows");
            let status = |id: &'static str| {
                let pool = pool.clone();
                async move {
                    let row: (String, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
                        "SELECT status, result_status, title_zh, error_text FROM public.translation_requests WHERE id=$1",
                    )
                    .bind(id)
                    .fetch_one(&pool)
                    .await?;
                    Ok::<_, sqlx::Error>(row)
                }
            };
            let failed = status("rf").await?;
            assert_eq!(failed.0, "failed");
            assert_eq!(failed.1.as_deref(), Some("error"));
            assert_eq!(failed.3.as_deref(), Some("LLM upstream rate limited"));
            let completed = status("rc").await?;
            assert_eq!(completed.0, "completed");
            assert_eq!(completed.1.as_deref(), Some("ready"));
            assert_eq!(completed.2.as_deref(), Some("标题"));
            assert_eq!(completed.3, None);
            let orphaned_running = status("rr").await?;
            assert_eq!(orphaned_running.0, "failed");
            let still_queued = status("rq").await?;
            assert_eq!(still_queued.0, "queued", "active work stays queued");
            let with_live_task = status("rl").await?;
            assert_eq!(with_live_task.0, "queued", "a live task keeps the request active");
            let second = super::reconcile_terminal_translation_requests(&pool).await?;
            assert_eq!(second, 0, "reconciliation is idempotent");

            // Imported production tables store these columns as text; the repair
            // must still match and write typed values.
            sqlx::raw_sql(
                r#"
                ALTER TABLE public.translation_requests
                    ALTER COLUMN finished_at TYPE text USING finished_at::text,
                    ALTER COLUMN updated_at TYPE text USING updated_at::text;
                INSERT INTO public.translation_work_items(id,status,result_status,error_text)
                  VALUES ('wtext','failed','error','provider exploded');
                INSERT INTO public.translation_requests(id,work_item_id,status,updated_at)
                  VALUES ('rtext','wtext','queued', now()::text);
                "#,
            )
            .execute(&pool)
            .await?;
            let closed_text = super::reconcile_terminal_translation_requests(&pool).await?;
            assert_eq!(closed_text, 1, "text-schema request is closed too");
            let text_row: (String, Option<String>) = sqlx::query_as(
                "SELECT status, error_text FROM public.translation_requests WHERE id='rtext'",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(text_row.0, "failed");
            assert_eq!(text_row.1.as_deref(), Some("provider exploded"));
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
