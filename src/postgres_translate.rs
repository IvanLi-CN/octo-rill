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
use chrono::Utc;
use futures_util::stream;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tower_sessions::Session;
use tracing::warn;

use crate::{
    api::{TranslateBatchItem, TranslateBatchResponse, TranslateResponse},
    config::AiConfig,
    local_id,
    postgres_server::PostgresServerState,
    translations::{TranslationSourceBlock, translation_error_summary},
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

        let tiebreak = crate::content_processing::release_source_revision_content_tiebreak(&[
            html_url.as_str(),
            tag_name.as_str(),
            title.as_str(),
            body.as_str(),
        ]);
        let observed_at = crate::content_processing::authoritative_release_revision(
            updated_at.as_deref().unwrap_or_default(),
            detected_at.as_deref(),
        );
        let source_hash = crate::api::release_detail_source_hash(&repo_full_name, &title, &body);
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

        let tiebreak = crate::content_processing::notification_source_revision_tiebreak(
            thread_id.as_str(),
            Some(repo_full_name.as_str()),
            Some(title.as_str()),
            Some(reason.as_str()),
            Some(subject_type.as_str()),
        );
        let source_hash = crate::ai::sha256_hex(&format!(
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
            &target.entity_id,
            now,
        )
        .await?;
    }
    tx.commit().await?;

    Ok(RequestScope {
        request_id: Some(request_id),
        work_item_id,
        attempt_no,
    })
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
    entity_id: &str,
    now: chrono::DateTime<Utc>,
) -> Result<i32, sqlx::Error> {
    let attempt_no = sqlx::query_scalar::<_, i32>(
        "SELECT COALESCE(MAX(attempt_no), 0) + 1 FROM public.translation_attempt_events WHERE work_item_id = $1",
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
    .bind(kind.work_variant())
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
    scope: &RequestScope,
    outcome: AttemptOutcome<'_>,
    call_ids_json: &str,
    now: chrono::DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let summary_slot = outcome.summary.filter(|value| !value.trim().is_empty());
    let mut tx = pool.begin().await?;
    if outcome.result_status == "ready" {
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
        "SELECT scope_user_id, kind, entity_id, status, source_blocks_json, target_slots_json FROM public.translation_work_items WHERE id = $1",
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
            &entity_id,
            now,
        )
        .await
        .context("start PostgreSQL translation attempt")?;
        tx.commit().await.context("commit PostgreSQL translation")?;
        RequestScope {
            request_id,
            work_item_id: work_item_id.to_owned(),
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

            let chunks = split_markdown_chunks(&target.body, body_chunk_chars(audit.ai));
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

fn body_chunk_chars(ai: &AiConfig) -> usize {
    let (input_limit, _) = crate::ai::resolve_model_input_limit_for_model(ai.model.as_str());
    ((input_limit as usize).saturating_mul(3)).clamp(2_000, 12_000)
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
struct LlmFailure {
    class: crate::ai::LlmFailureClass,
    message: String,
}

impl LlmFailure {
    fn empty_content() -> Self {
        let class = crate::ai::LlmFailureClass::EmptyContent;
        Self {
            class,
            message: class.safe_message().to_owned(),
        }
    }

    fn transient() -> Self {
        let class = crate::ai::LlmFailureClass::Transient;
        Self {
            class,
            message: class.safe_message().to_owned(),
        }
    }

    fn configuration() -> Self {
        let class = crate::ai::LlmFailureClass::Configuration;
        Self {
            class,
            message: class.safe_message().to_owned(),
        }
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
        let call_id = local_id::generate_local_id();
        let ordinal = self.ordinal;
        self.ordinal += 1;
        let prompt_text = truncate_chars(&format!("{system}\n\n{user}"), 8_000);
        let inserted = insert_call(
            self.pool,
            &call_id,
            self.source,
            self.ai.model.as_str(),
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
        let outcome = self.invoke(system, user, max_tokens).await;
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
                model: self.ai.model.clone(),
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
            let _ = finalize_call(self.pool, &call_id, &audit).await;
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

        outcome
            .map(|response| response.content)
            .map_err(|failure| failure.error)
    }

    async fn invoke(
        &self,
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
                "model": self.ai.model,
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
            let class = classify_upstream_failure(status.as_u16(), detail.as_str());
            return Err(CallFailure {
                error: LlmFailure {
                    class,
                    message: class.safe_message().to_owned(),
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
                .unwrap_or(self.ai.model.as_str())
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

fn classify_upstream_failure(status: u16, detail: &str) -> crate::ai::LlmFailureClass {
    let lowered = detail.to_ascii_lowercase();
    let model_rejected = lowered.contains("model not found")
        || lowered.contains("invalid_model_error")
        || lowered.contains("insufficient_quota")
        || lowered.contains("model_not_found");
    if status == 429 {
        return crate::ai::LlmFailureClass::RateLimited;
    }
    if model_rejected || matches!(status, 400..=499) {
        return crate::ai::LlmFailureClass::Configuration;
    }
    crate::ai::LlmFailureClass::Transient
}

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

async fn finalize_call(pool: &PgPool, call_id: &str, audit: &CallAudit) -> Result<(), sqlx::Error> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE public.llm_calls SET status = $1, model = $2, final_model = $2, provider_request_id = $3, provider_http_status = $4, input_tokens = $5, output_tokens = $6, total_tokens = $7, finish_reason = $8, failure_class = $9, error_text = $10, response_text = $11, duration_ms = $12, fallback_count = 0, recovery_attempt_count = 0, finished_at = $13, updated_at = $13, runtime_owner_id = NULL, lease_heartbeat_at = NULL WHERE id = $14",
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
