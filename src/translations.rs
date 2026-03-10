use std::{
    collections::{BTreeMap, HashMap, HashSet},
    convert::Infallible,
    sync::{Arc, OnceLock},
    time::Duration,
};

use anyhow::Result;
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, Transaction};
use tokio::time::sleep;
use tower_sessions::Session;
use tracing::warn;

use crate::{ai, api, error::ApiError, state::AppState};

const TRANSLATION_PROTOCOL_VERSION: &str = "translation-request.v1";
const TRANSLATION_MODEL_PROFILE_DISABLED: &str = "ai-disabled";
const TRANSLATION_BATCH_MAX_TOKENS: u32 = 1_800;
const TRANSLATION_BATCH_SCAN_INTERVAL: Duration = Duration::from_millis(250);
const TRANSLATION_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(150);
const TRANSLATION_STREAM_POLL_INTERVAL: Duration = Duration::from_millis(250);
const TRANSLATION_MAX_ITEMS_PER_REQUEST: usize = 60;
const TRANSLATION_MIN_WAIT_MS: i64 = 0;
const TRANSLATION_MAX_WAIT_MS: i64 = 60_000;
const TRANSLATION_WORKER_CONCURRENCY: i64 = 4;
const TRANSLATION_USER_DEDICATED_WORKER_SLOT: i64 = 4;

static TRANSLATION_WORKER_RUNTIME: OnceLock<
    tokio::sync::RwLock<Vec<TranslationWorkerRuntimeState>>,
> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationSourceBlock {
    pub slot: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationRequestItemInput {
    pub producer_ref: String,
    pub kind: String,
    pub variant: String,
    pub entity_id: String,
    pub target_lang: String,
    pub max_wait_ms: i64,
    pub source_blocks: Vec<TranslationSourceBlock>,
    pub target_slots: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TranslationSubmitRequest {
    pub mode: String,
    #[serde(default)]
    pub item: Option<TranslationRequestItemInput>,
    #[serde(default)]
    pub items: Option<Vec<TranslationRequestItemInput>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TranslationResultItem {
    pub producer_ref: String,
    pub entity_id: String,
    pub kind: String,
    pub variant: String,
    pub status: String,
    pub title_zh: Option<String>,
    pub summary_md: Option<String>,
    pub body_md: Option<String>,
    pub error: Option<String>,
    pub work_item_id: Option<String>,
    pub batch_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TranslationRequestResponse {
    pub request_id: String,
    pub status: String,
    pub result: TranslationResultItem,
}

#[derive(Debug, Serialize)]
pub struct TranslationBatchSubmitItemResponse {
    pub request_id: String,
    pub status: String,
    pub producer_ref: String,
    pub entity_id: String,
    pub kind: String,
    pub variant: String,
}

#[derive(Debug, Serialize)]
pub struct TranslationBatchSubmitResponse {
    pub requests: Vec<TranslationBatchSubmitItemResponse>,
}

#[derive(Debug, Serialize)]
struct TranslationRequestStreamEvent {
    event: String,
    request_id: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    batch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<TranslationResultItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TranslationLegacyRemovedResponse {
    ok: bool,
    error: TranslationLegacyRemovedError,
}

#[derive(Debug, Serialize)]
struct TranslationLegacyRemovedError {
    code: &'static str,
    message: &'static str,
}

#[derive(Debug, Deserialize)]
pub struct AdminTranslationListQuery {
    pub status: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AdminTranslationStatusResponse {
    pub scheduler_enabled: bool,
    pub llm_enabled: bool,
    pub scan_interval_ms: i64,
    pub batch_token_threshold: i64,
    pub worker_concurrency: i64,
    pub idle_workers: i64,
    pub busy_workers: i64,
    pub workers: Vec<AdminTranslationWorkerStatus>,
    pub queued_requests: i64,
    pub queued_work_items: i64,
    pub running_batches: i64,
    pub requests_24h: i64,
    pub completed_batches_24h: i64,
    pub failed_batches_24h: i64,
    pub avg_wait_ms_24h: Option<i64>,
    pub last_batch_finished_at: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminTranslationRequestListItem {
    pub id: String,
    pub status: String,
    pub source: String,
    pub request_origin: String,
    pub requested_by: Option<String>,
    pub scope_user_id: String,
    pub producer_ref: String,
    pub kind: String,
    pub variant: String,
    pub entity_id: String,
    pub batch_id: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct AdminTranslationRequestsResponse {
    pub items: Vec<AdminTranslationRequestListItem>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, Serialize)]
pub struct AdminTranslationRequestDetailResponse {
    pub request: AdminTranslationRequestListItem,
    pub result: TranslationResultItem,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminTranslationBatchListItem {
    pub id: String,
    pub status: String,
    pub trigger_reason: String,
    pub worker_slot: i64,
    pub request_count: i64,
    pub item_count: i64,
    pub estimated_input_tokens: i64,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct AdminTranslationBatchesResponse {
    pub items: Vec<AdminTranslationBatchListItem>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminTranslationLinkedLlmCall {
    pub id: String,
    pub status: String,
    pub source: String,
    pub model: String,
    pub scheduler_wait_ms: i64,
    pub duration_ms: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct AdminTranslationBatchDetailResponse {
    pub batch: AdminTranslationBatchListItem,
    pub items: Vec<TranslationResultItem>,
    pub llm_calls: Vec<AdminTranslationLinkedLlmCall>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdminTranslationWorkerStatus {
    pub worker_id: String,
    pub worker_slot: i64,
    pub worker_kind: String,
    pub status: String,
    pub current_batch_id: Option<String>,
    pub request_count: i64,
    pub work_item_count: i64,
    pub trigger_reason: Option<String>,
    pub updated_at: String,
    pub error_text: Option<String>,
}

#[derive(Debug, Clone)]
struct TranslationWorkerRuntimeState {
    worker_id: String,
    worker_slot: i64,
    worker_kind: String,
    status: String,
    current_batch_id: Option<String>,
    request_count: i64,
    work_item_count: i64,
    trigger_reason: Option<String>,
    updated_at: String,
    error_text: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct TranslationWorkerProfile {
    worker_slot: i64,
    worker_kind: &'static str,
}

#[allow(dead_code)]
#[derive(Debug, Clone, sqlx::FromRow)]
struct WorkItemRow {
    id: String,
    dedupe_key: String,
    scope_user_id: String,
    kind: String,
    variant: String,
    entity_id: String,
    target_lang: String,
    protocol_version: String,
    model_profile: String,
    source_hash: String,
    source_blocks_json: String,
    target_slots_json: String,
    token_estimate: i64,
    deadline_at: String,
    status: String,
    batch_id: Option<String>,
    result_status: Option<String>,
    title_zh: Option<String>,
    summary_md: Option<String>,
    body_md: Option<String>,
    error_text: Option<String>,
    cache_hit: i64,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    updated_at: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, sqlx::FromRow)]
struct ClaimCandidateRow {
    id: String,
    dedupe_key: String,
    scope_user_id: String,
    kind: String,
    variant: String,
    entity_id: String,
    target_lang: String,
    protocol_version: String,
    model_profile: String,
    source_hash: String,
    source_blocks_json: String,
    target_slots_json: String,
    token_estimate: i64,
    deadline_at: String,
    status: String,
    batch_id: Option<String>,
    result_status: Option<String>,
    title_zh: Option<String>,
    summary_md: Option<String>,
    body_md: Option<String>,
    error_text: Option<String>,
    cache_hit: i64,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    updated_at: String,
    request_origin: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, sqlx::FromRow)]
struct RequestRow {
    id: String,
    mode: String,
    source: String,
    request_origin: String,
    requested_by: Option<String>,
    scope_user_id: String,
    producer_ref: String,
    kind: String,
    variant: String,
    entity_id: String,
    target_lang: String,
    max_wait_ms: i64,
    work_item_id: Option<String>,
    status: String,
    result_status: Option<String>,
    title_zh: Option<String>,
    summary_md: Option<String>,
    body_md: Option<String>,
    error_text: Option<String>,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    updated_at: String,
    work_item_status: Option<String>,
    batch_id: Option<String>,
}

impl RequestRow {
    fn effective_status(&self) -> &str {
        if self.status == "queued" && self.work_item_status.as_deref() == Some("running") {
            "running"
        } else {
            self.status.as_str()
        }
    }

    fn to_admin_request_list_item(&self) -> AdminTranslationRequestListItem {
        AdminTranslationRequestListItem {
            id: self.id.clone(),
            status: self.effective_status().to_owned(),
            source: self.source.clone(),
            request_origin: self.request_origin.clone(),
            requested_by: self.requested_by.clone(),
            scope_user_id: self.scope_user_id.clone(),
            producer_ref: self.producer_ref.clone(),
            kind: self.kind.clone(),
            variant: self.variant.clone(),
            entity_id: self.entity_id.clone(),
            batch_id: self.batch_id.clone(),
            created_at: self.created_at.clone(),
            started_at: self.started_at.clone(),
            finished_at: self.finished_at.clone(),
            updated_at: self.updated_at.clone(),
        }
    }

    fn to_result(&self) -> TranslationResultItem {
        TranslationResultItem {
            producer_ref: self.producer_ref.clone(),
            entity_id: self.entity_id.clone(),
            kind: self.kind.clone(),
            variant: self.variant.clone(),
            status: self.result_status.clone().unwrap_or_else(|| {
                pending_result_status_from_work_status(self.work_item_status.as_deref()).to_owned()
            }),
            title_zh: self.title_zh.clone(),
            summary_md: self.summary_md.clone(),
            body_md: self.body_md.clone(),
            error: self.error_text.clone(),
            work_item_id: self.work_item_id.clone(),
            batch_id: self.batch_id.clone(),
        }
    }
}

fn pending_result_status_from_work_status(work_item_status: Option<&str>) -> &'static str {
    match work_item_status {
        Some("running") => "running",
        _ => "queued",
    }
}

fn queued_request_result(
    item: &TranslationRequestItemInput,
    work_item_id: Option<String>,
) -> TranslationResultItem {
    TranslationResultItem {
        producer_ref: item.producer_ref.clone(),
        entity_id: item.entity_id.clone(),
        kind: item.kind.clone(),
        variant: item.variant.clone(),
        status: "queued".to_owned(),
        title_zh: None,
        summary_md: None,
        body_md: None,
        error: None,
        work_item_id,
        batch_id: None,
    }
}

fn request_status_from_result_status(result_status: &str) -> &'static str {
    match result_status {
        "ready" | "disabled" => "completed",
        "missing" | "error" => "failed",
        _ => "queued",
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct ClaimedBatch {
    id: String,
    worker_slot: i64,
    partition_key: String,
    target_lang: String,
    protocol_version: String,
    model_profile: String,
    trigger_reason: String,
    request_count: i64,
    estimated_input_tokens: i64,
    items: Vec<WorkItemRow>,
}

#[derive(Debug, Clone)]
struct TerminalWorkResult {
    work_item_id: String,
    result_status: String,
    title_zh: Option<String>,
    summary_md: Option<String>,
    body_md: Option<String>,
    error: Option<String>,
}

impl ClaimCandidateRow {
    fn into_work_item(self) -> WorkItemRow {
        WorkItemRow {
            id: self.id,
            dedupe_key: self.dedupe_key,
            scope_user_id: self.scope_user_id,
            kind: self.kind,
            variant: self.variant,
            entity_id: self.entity_id,
            target_lang: self.target_lang,
            protocol_version: self.protocol_version,
            model_profile: self.model_profile,
            source_hash: self.source_hash,
            source_blocks_json: self.source_blocks_json,
            target_slots_json: self.target_slots_json,
            token_estimate: self.token_estimate,
            deadline_at: self.deadline_at,
            status: self.status,
            batch_id: self.batch_id,
            result_status: self.result_status,
            title_zh: self.title_zh,
            summary_md: self.summary_md,
            body_md: self.body_md,
            error_text: self.error_text,
            cache_hit: self.cache_hit,
            created_at: self.created_at,
            started_at: self.started_at,
            finished_at: self.finished_at,
            updated_at: self.updated_at,
        }
    }
}

impl TranslationWorkerRuntimeState {
    fn to_admin_status(&self) -> AdminTranslationWorkerStatus {
        AdminTranslationWorkerStatus {
            worker_id: self.worker_id.clone(),
            worker_slot: self.worker_slot,
            worker_kind: self.worker_kind.clone(),
            status: self.status.clone(),
            current_batch_id: self.current_batch_id.clone(),
            request_count: self.request_count,
            work_item_count: self.work_item_count,
            trigger_reason: self.trigger_reason.clone(),
            updated_at: self.updated_at.clone(),
            error_text: self.error_text.clone(),
        }
    }
}

fn translation_worker_profiles() -> [TranslationWorkerProfile; 4] {
    [
        TranslationWorkerProfile {
            worker_slot: 1,
            worker_kind: "general",
        },
        TranslationWorkerProfile {
            worker_slot: 2,
            worker_kind: "general",
        },
        TranslationWorkerProfile {
            worker_slot: 3,
            worker_kind: "general",
        },
        TranslationWorkerProfile {
            worker_slot: TRANSLATION_USER_DEDICATED_WORKER_SLOT,
            worker_kind: "user_dedicated",
        },
    ]
}

fn translation_worker_runtime() -> &'static tokio::sync::RwLock<Vec<TranslationWorkerRuntimeState>>
{
    TRANSLATION_WORKER_RUNTIME.get_or_init(|| {
        let now = Utc::now().to_rfc3339();
        tokio::sync::RwLock::new(
            translation_worker_profiles()
                .into_iter()
                .map(|profile| TranslationWorkerRuntimeState {
                    worker_id: translation_worker_id(profile.worker_slot),
                    worker_slot: profile.worker_slot,
                    worker_kind: profile.worker_kind.to_owned(),
                    status: "idle".to_owned(),
                    current_batch_id: None,
                    request_count: 0,
                    work_item_count: 0,
                    trigger_reason: None,
                    updated_at: now.clone(),
                    error_text: None,
                })
                .collect(),
        )
    })
}

fn translation_worker_id(worker_slot: i64) -> String {
    format!("translation-worker-{worker_slot}")
}

async fn update_translation_worker_runtime(
    profile: TranslationWorkerProfile,
    status: &str,
    current_batch_id: Option<&str>,
    request_count: i64,
    work_item_count: i64,
    trigger_reason: Option<&str>,
    error_text: Option<&str>,
) {
    let runtime = translation_worker_runtime();
    let mut guard = runtime.write().await;
    let Some(entry) = guard
        .iter_mut()
        .find(|entry| entry.worker_slot == profile.worker_slot)
    else {
        return;
    };

    let next_batch_id = current_batch_id.map(str::to_owned);
    let next_trigger_reason = trigger_reason.map(str::to_owned);
    let next_error_text = error_text.map(str::to_owned);
    let changed = entry.status != status
        || entry.current_batch_id != next_batch_id
        || entry.request_count != request_count
        || entry.work_item_count != work_item_count
        || entry.trigger_reason != next_trigger_reason
        || entry.error_text != next_error_text;
    if !changed {
        return;
    }

    entry.status = status.to_owned();
    entry.current_batch_id = next_batch_id;
    entry.request_count = request_count;
    entry.work_item_count = work_item_count;
    entry.trigger_reason = next_trigger_reason;
    entry.error_text = next_error_text;
    entry.updated_at = Utc::now().to_rfc3339();
}

pub async fn translation_worker_runtime_statuses() -> Vec<AdminTranslationWorkerStatus> {
    translation_worker_runtime()
        .read()
        .await
        .iter()
        .map(TranslationWorkerRuntimeState::to_admin_status)
        .collect()
}

#[cfg(test)]
async fn reset_translation_worker_runtime_for_tests() {
    let runtime = translation_worker_runtime();
    let now = Utc::now().to_rfc3339();
    let mut guard = runtime.write().await;
    *guard = translation_worker_profiles()
        .into_iter()
        .map(|profile| TranslationWorkerRuntimeState {
            worker_id: translation_worker_id(profile.worker_slot),
            worker_slot: profile.worker_slot,
            worker_kind: profile.worker_kind.to_owned(),
            status: "idle".to_owned(),
            current_batch_id: None,
            request_count: 0,
            work_item_count: 0,
            trigger_reason: None,
            updated_at: now.clone(),
            error_text: None,
        })
        .collect();
}

fn claim_origin_case_sql() -> &'static str {
    r#"
    CASE
      WHEN EXISTS (
        SELECT 1
        FROM translation_requests tr
        WHERE tr.work_item_id = w.id
          AND tr.request_origin = 'user'
      ) THEN 'user'
      ELSE 'system'
    END
    "#
}

fn request_row_select_sql() -> &'static str {
    r#"
    SELECT r.id, r.mode, r.source, r.request_origin, r.requested_by, r.scope_user_id,
           r.producer_ref, r.kind, r.variant, r.entity_id, r.target_lang, r.max_wait_ms,
           r.work_item_id, r.status, r.result_status, r.title_zh, r.summary_md, r.body_md,
           r.error_text,
           r.created_at, r.started_at, r.finished_at, r.updated_at,
           (SELECT status FROM translation_work_items w WHERE w.id = r.work_item_id) AS work_item_status,
           (SELECT batch_id FROM translation_work_items w WHERE w.id = r.work_item_id) AS batch_id
    FROM translation_requests r
    "#
}
pub fn spawn_translation_scheduler(state: Arc<AppState>) -> Vec<tokio::task::AbortHandle> {
    translation_worker_profiles()
        .into_iter()
        .map(|profile| {
            let state = state.clone();
            tokio::spawn(async move {
                update_translation_worker_runtime(profile, "idle", None, 0, 0, None, None).await;
                loop {
                    if let Err(err) = run_translation_scheduler_once(state.as_ref(), profile).await
                    {
                        warn!(
                            ?err,
                            worker_slot = profile.worker_slot,
                            "translation scheduler tick failed"
                        );
                        let error_text = err.to_string();
                        update_translation_worker_runtime(
                            profile,
                            "error",
                            None,
                            0,
                            0,
                            None,
                            Some(error_text.as_str()),
                        )
                        .await;
                    }
                    sleep(TRANSLATION_BATCH_SCAN_INTERVAL).await;
                }
            })
            .abort_handle()
        })
        .collect::<Vec<_>>()
}

pub async fn reject_legacy_translation_routes() -> Response {
    (
        StatusCode::GONE,
        Json(TranslationLegacyRemovedResponse {
            ok: false,
            error: TranslationLegacyRemovedError {
                code: "translation_scheduler_required",
                message: "legacy translation endpoints were removed; use /api/translate/requests",
            },
        }),
    )
        .into_response()
}

pub async fn submit_translation_request(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<TranslationSubmitRequest>,
) -> Result<Response, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    let mode = normalize_mode(req.mode.trim())?;

    match normalize_submit_payload(mode, req)? {
        NormalizedTranslationSubmit::Single(item) => {
            let created = create_translation_request(state.as_ref(), &user_id, mode, &item).await?;
            match mode {
                "async" => Ok(Json(created.to_public_response()).into_response()),
                "wait" => {
                    let detail =
                        wait_for_request_terminal(state.as_ref(), &user_id, &created.request_id)
                            .await?;
                    Ok(Json(detail_to_public_response(detail)).into_response())
                }
                "stream" => Ok(stream_translation_request_response(
                    state,
                    user_id.clone(),
                    created.request_id,
                )),
                _ => Err(ApiError::bad_request("unsupported mode")),
            }
        }
        NormalizedTranslationSubmit::Batch(items) => {
            if mode != "async" {
                return Err(ApiError::bad_request(
                    "batch translation requests only support async mode",
                ));
            }
            let created =
                create_translation_requests_batch(state.as_ref(), &user_id, mode, &items).await?;
            Ok(Json(TranslationBatchSubmitResponse {
                requests: created
                    .into_iter()
                    .map(|request| request.to_batch_response())
                    .collect(),
            })
            .into_response())
        }
    }
}

pub async fn get_translation_request(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(request_id): Path<String>,
) -> Result<Json<TranslationRequestResponse>, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    let request_id = api::parse_local_id_param(request_id, "request_id")?;
    let detail = load_translation_request_detail(state.as_ref(), &user_id, &request_id).await?;
    Ok(Json(detail_to_public_response(detail)))
}

pub async fn stream_translation_request(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(request_id): Path<String>,
) -> Result<Response, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    let request_id = api::parse_local_id_param(request_id, "request_id")?;
    ensure_request_owner(state.as_ref(), &user_id, &request_id).await?;
    Ok(stream_translation_request_response(
        state,
        user_id.clone(),
        request_id,
    ))
}

pub async fn admin_get_translation_status(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<AdminTranslationStatusResponse>, ApiError> {
    let _acting_user_id = api::require_admin_user_id(state.as_ref(), &session).await?;
    let _llm_runtime = ai::llm_scheduler_runtime_status().await;
    let since = (Utc::now() - chrono::Duration::hours(24)).to_rfc3339();
    let workers = translation_worker_runtime_statuses().await;
    let idle_workers = i64::try_from(
        workers
            .iter()
            .filter(|worker| worker.status == "idle")
            .count(),
    )
    .unwrap_or(i64::MAX);
    let busy_workers = i64::try_from(
        workers
            .iter()
            .filter(|worker| worker.status == "running")
            .count(),
    )
    .unwrap_or(i64::MAX);

    let queued_requests = scalar_i64(
        state.as_ref(),
        "SELECT COUNT(*) FROM translation_requests WHERE status = 'queued'",
        &[],
    )
    .await?;
    let queued_work_items = scalar_i64(
        state.as_ref(),
        "SELECT COUNT(*) FROM translation_work_items WHERE status = 'queued'",
        &[],
    )
    .await?;
    let running_batches = scalar_i64(
        state.as_ref(),
        "SELECT COUNT(*) FROM translation_batches WHERE status = 'running'",
        &[],
    )
    .await?;
    let requests_24h = scalar_i64(
        state.as_ref(),
        "SELECT COUNT(*) FROM translation_requests WHERE created_at >= ?",
        &[&since],
    )
    .await?;
    let completed_batches_24h = scalar_i64(
        state.as_ref(),
        "SELECT COUNT(*) FROM translation_batches WHERE created_at >= ? AND status = 'completed'",
        &[&since],
    )
    .await?;
    let failed_batches_24h = scalar_i64(
        state.as_ref(),
        "SELECT COUNT(*) FROM translation_batches WHERE created_at >= ? AND status = 'failed'",
        &[&since],
    )
    .await?;
    let avg_wait_ms_24h = sqlx::query_scalar::<_, Option<f64>>(
        r#"
        SELECT AVG((julianday(deadline_at) - julianday(created_at)) * 86400000.0)
        FROM translation_work_items
        WHERE finished_at IS NOT NULL AND created_at >= ?
        "#,
    )
    .bind(since.as_str())
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?
    .map(|v| v.round() as i64);
    let last_batch_finished_at = load_last_batch_finished_at(state.as_ref()).await?;

    let budget = i64::from(
        ai::compute_input_budget_with_source(state.as_ref(), TRANSLATION_BATCH_MAX_TOKENS)
            .await
            .input_budget,
    );

    Ok(Json(AdminTranslationStatusResponse {
        scheduler_enabled: true,
        llm_enabled: state.config.ai.is_some(),
        scan_interval_ms: i64::try_from(TRANSLATION_BATCH_SCAN_INTERVAL.as_millis())
            .unwrap_or(i64::MAX),
        batch_token_threshold: budget,
        worker_concurrency: TRANSLATION_WORKER_CONCURRENCY,
        idle_workers,
        busy_workers,
        workers,
        queued_requests,
        queued_work_items,
        running_batches,
        requests_24h,
        completed_batches_24h,
        failed_batches_24h,
        avg_wait_ms_24h,
        last_batch_finished_at,
    }))
}

pub async fn admin_list_translation_requests(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(query): Query<AdminTranslationListQuery>,
) -> Result<Json<AdminTranslationRequestsResponse>, ApiError> {
    let _acting_user_id = api::require_admin_user_id(state.as_ref(), &session).await?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;
    let status = query.status.unwrap_or_else(|| "all".to_owned());

    let total = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM translation_requests
        WHERE (? = 'all' OR status = ?)
        "#,
    )
    .bind(status.as_str())
    .bind(status.as_str())
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let request_rows_sql = format!(
        r#"{}
        WHERE (? = 'all' OR r.status = ?)
        ORDER BY CASE WHEN r.status IN ('queued', 'running') THEN 0 ELSE 1 END ASC,
                 r.updated_at DESC,
                 r.id DESC
        LIMIT ? OFFSET ?
        "#,
        request_row_select_sql(),
    );
    let items = sqlx::query_as::<_, RequestRow>(&request_rows_sql)
        .bind(status.as_str())
        .bind(status.as_str())
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::internal)?
        .into_iter()
        .map(|row| row.to_admin_request_list_item())
        .collect();

    Ok(Json(AdminTranslationRequestsResponse {
        items,
        page,
        page_size,
        total,
    }))
}

pub async fn admin_get_translation_request_detail(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(request_id): Path<String>,
) -> Result<Json<AdminTranslationRequestDetailResponse>, ApiError> {
    let _acting_user_id = api::require_admin_user_id(state.as_ref(), &session).await?;
    let request_id = api::parse_local_id_param(request_id, "request_id")?;
    let request_row_sql = format!(
        r#"{}
        WHERE r.id = ?
        LIMIT 1
        "#,
        request_row_select_sql(),
    );
    let request = sqlx::query_as::<_, RequestRow>(&request_row_sql)
        .bind(request_id.as_str())
        .fetch_optional(&state.pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "translation request not found",
            )
        })?;
    Ok(Json(AdminTranslationRequestDetailResponse {
        request: request.to_admin_request_list_item(),
        result: request.to_result(),
    }))
}

pub async fn admin_list_translation_batches(
    State(state): State<Arc<AppState>>,
    session: Session,
    Query(query): Query<AdminTranslationListQuery>,
) -> Result<Json<AdminTranslationBatchesResponse>, ApiError> {
    let _acting_user_id = api::require_admin_user_id(state.as_ref(), &session).await?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;
    let status = query.status.unwrap_or_else(|| "all".to_owned());

    let total = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM translation_batches
        WHERE (? = 'all' OR status = ?)
        "#,
    )
    .bind(status.as_str())
    .bind(status.as_str())
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    let items = sqlx::query_as::<_, AdminTranslationBatchListItem>(
        r#"
        SELECT id, status, trigger_reason, worker_slot, request_count, item_count, estimated_input_tokens,
               created_at, started_at, finished_at, updated_at
        FROM translation_batches
        WHERE (? = 'all' OR status = ?)
        ORDER BY created_at DESC, id DESC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(status.as_str())
    .bind(status.as_str())
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(AdminTranslationBatchesResponse {
        items,
        page,
        page_size,
        total,
    }))
}

pub async fn admin_get_translation_batch_detail(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(batch_id): Path<String>,
) -> Result<Json<AdminTranslationBatchDetailResponse>, ApiError> {
    let _acting_user_id = api::require_admin_user_id(state.as_ref(), &session).await?;
    let batch_id = api::parse_local_id_param(batch_id, "batch_id")?;
    let batch = sqlx::query_as::<_, AdminTranslationBatchListItem>(
        r#"
        SELECT id, status, trigger_reason, worker_slot, request_count, item_count, estimated_input_tokens,
               created_at, started_at, finished_at, updated_at
        FROM translation_batches
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(batch_id.as_str())
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "translation batch not found",
        )
    })?;

    let items = load_translation_result_items_by_batch(state.as_ref(), &batch_id).await?;
    let llm_calls = sqlx::query_as::<_, AdminTranslationLinkedLlmCall>(
        r#"
        SELECT id, status, source, model, scheduler_wait_ms, duration_ms, created_at
        FROM llm_calls
        WHERE parent_translation_batch_id = ?
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .bind(batch_id.as_str())
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(AdminTranslationBatchDetailResponse {
        batch,
        items,
        llm_calls,
    }))
}

async fn create_translation_request(
    state: &AppState,
    user_id: &str,
    mode: &str,
    item: &TranslationRequestItemInput,
) -> Result<CreatedTranslationRequest, ApiError> {
    create_translation_request_with_origin(state, user_id, mode, item, "user").await
}

async fn create_translation_requests_batch(
    state: &AppState,
    user_id: &str,
    mode: &str,
    items: &[TranslationRequestItemInput],
) -> Result<Vec<CreatedTranslationRequest>, ApiError> {
    create_translation_requests_batch_with_origin(state, user_id, mode, items, "user").await
}

async fn create_translation_request_with_origin(
    state: &AppState,
    user_id: &str,
    mode: &str,
    item: &TranslationRequestItemInput,
    request_origin: &str,
) -> Result<CreatedTranslationRequest, ApiError> {
    let now = Utc::now().to_rfc3339();
    let mut tx = state.pool.begin().await.map_err(ApiError::internal)?;
    let created =
        insert_translation_request(&mut tx, user_id, mode, item, request_origin, now.as_str())
            .await?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(created)
}

async fn create_translation_requests_batch_with_origin(
    state: &AppState,
    user_id: &str,
    mode: &str,
    items: &[TranslationRequestItemInput],
    request_origin: &str,
) -> Result<Vec<CreatedTranslationRequest>, ApiError> {
    let now = Utc::now().to_rfc3339();
    let mut tx = state.pool.begin().await.map_err(ApiError::internal)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(
            insert_translation_request(&mut tx, user_id, mode, item, request_origin, now.as_str())
                .await?,
        );
    }
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(out)
}

async fn insert_translation_request(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    mode: &str,
    item: &TranslationRequestItemInput,
    request_origin: &str,
    now: &str,
) -> Result<CreatedTranslationRequest, ApiError> {
    let request_id = crate::local_id::generate_local_id();
    let source_hash = build_source_hash(item);
    let source_blocks_json =
        serde_json::to_string(&item.source_blocks).map_err(ApiError::internal)?;
    let target_slots_json =
        serde_json::to_string(&item.target_slots).map_err(ApiError::internal)?;
    let source = derive_request_source(std::slice::from_ref(item));

    sqlx::query(
        r#"
        INSERT INTO translation_requests (
          id, mode, source, request_origin, requested_by, scope_user_id, producer_ref, kind,
          variant, entity_id, target_lang, max_wait_ms, source_hash, source_blocks_json,
          target_slots_json, status, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'queued', ?, ?)
        "#,
    )
    .bind(request_id.as_str())
    .bind(mode)
    .bind(source.as_str())
    .bind(request_origin)
    .bind(user_id)
    .bind(user_id)
    .bind(item.producer_ref.as_str())
    .bind(item.kind.as_str())
    .bind(item.variant.as_str())
    .bind(item.entity_id.as_str())
    .bind(item.target_lang.as_str())
    .bind(item.max_wait_ms)
    .bind(source_hash.as_str())
    .bind(source_blocks_json.as_str())
    .bind(target_slots_json.as_str())
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    if let Some(cached) = load_cached_result(tx, user_id, item, &source_hash).await? {
        let status = request_status_from_result_status(cached.status.as_str());
        apply_request_result(tx, request_id.as_str(), None, &cached, now).await?;
        return Ok(CreatedTranslationRequest {
            request_id,
            status: status.to_owned(),
            result: cached,
        });
    }

    let work_item = load_existing_work_item(tx, user_id, item, &source_hash).await?;
    match work_item {
        Some(existing)
            if existing.result_status.is_some()
                && matches!(existing.status.as_str(), "completed" | "failed") =>
        {
            let result = terminal_result_from_work_row(&existing, item.producer_ref.clone());
            let status = request_status_from_result_status(result.status.as_str()).to_owned();
            apply_request_result(
                tx,
                request_id.as_str(),
                Some(existing.id.as_str()),
                &result,
                now,
            )
            .await?;
            Ok(CreatedTranslationRequest {
                request_id,
                status,
                result,
            })
        }
        Some(existing) => {
            let status = if existing.status == "running" {
                "running"
            } else {
                "queued"
            }
            .to_owned();
            let result = pending_result_from_work_row(&existing, item.producer_ref.clone());
            attach_request_to_work_item(
                tx,
                request_id.as_str(),
                &existing.id,
                status.as_str(),
                now,
            )
            .await?;
            Ok(CreatedTranslationRequest {
                request_id,
                status,
                result,
            })
        }
        None => {
            let work_item_id = create_work_item(tx, user_id, item, &source_hash, now).await?;
            attach_request_to_work_item(tx, request_id.as_str(), &work_item_id, "queued", now)
                .await?;
            Ok(CreatedTranslationRequest {
                request_id,
                status: "queued".to_owned(),
                result: queued_request_result(item, Some(work_item_id)),
            })
        }
    }
}

async fn attach_request_to_work_item(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: &str,
    work_item_id: &str,
    request_status: &str,
    now: &str,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        UPDATE translation_requests
        SET work_item_id = ?,
            status = ?,
            started_at = CASE
                WHEN ? = 'running' THEN COALESCE(started_at, ?)
                ELSE started_at
            END,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(work_item_id)
    .bind(request_status)
    .bind(request_status)
    .bind(now)
    .bind(now)
    .bind(request_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn apply_request_result(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: &str,
    work_item_id: Option<&str>,
    result: &TranslationResultItem,
    now: &str,
) -> Result<(), ApiError> {
    let status = request_status_from_result_status(result.status.as_str());
    sqlx::query(
        r#"
        UPDATE translation_requests
        SET work_item_id = COALESCE(?, work_item_id),
            status = ?,
            result_status = ?,
            title_zh = ?,
            summary_md = ?,
            body_md = ?,
            error_text = ?,
            finished_at = ?,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(work_item_id)
    .bind(status)
    .bind(result.status.as_str())
    .bind(result.title_zh.as_deref())
    .bind(result.summary_md.as_deref())
    .bind(result.body_md.as_deref())
    .bind(result.error.as_deref())
    .bind(now)
    .bind(now)
    .bind(request_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn load_cached_result(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    item: &TranslationRequestItemInput,
    source_hash: &str,
) -> Result<Option<TranslationResultItem>, ApiError> {
    let Some(entity_type) = map_entity_type(item.kind.as_str()) else {
        return Ok(None);
    };
    let row = sqlx::query(
        r#"
        SELECT source_hash, title, summary
        FROM ai_translations
        WHERE user_id = ? AND entity_type = ? AND entity_id = ? AND lang = ?
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(entity_type)
    .bind(item.entity_id.as_str())
    .bind(item.target_lang.as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        return Ok(None);
    };
    let stored_hash: String = row.try_get("source_hash").map_err(ApiError::internal)?;
    if stored_hash != source_hash {
        return Ok(None);
    }
    let title: Option<String> = row.try_get("title").map_err(ApiError::internal)?;
    let summary: Option<String> = row.try_get("summary").map_err(ApiError::internal)?;
    let mut result = TranslationResultItem {
        producer_ref: item.producer_ref.clone(),
        entity_id: item.entity_id.clone(),
        kind: item.kind.clone(),
        variant: item.variant.clone(),
        status: "ready".to_owned(),
        title_zh: None,
        summary_md: None,
        body_md: None,
        error: None,
        work_item_id: None,
        batch_id: None,
    };
    if item.target_slots.iter().any(|slot| slot == "title_zh") {
        result.title_zh = title.clone();
    }
    if item.target_slots.iter().any(|slot| slot == "summary_md") {
        result.summary_md = summary.clone();
    }
    if item.target_slots.iter().any(|slot| slot == "body_md") {
        result.body_md = summary.clone();
    }
    Ok(Some(result))
}

async fn load_existing_work_item(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    item: &TranslationRequestItemInput,
    source_hash: &str,
) -> Result<Option<WorkItemRow>, ApiError> {
    let dedupe_key = build_dedupe_key(user_id, item, source_hash);
    sqlx::query_as::<_, WorkItemRow>(
        r#"
        SELECT id, dedupe_key, scope_user_id, kind, variant, entity_id, target_lang, protocol_version,
               model_profile, source_hash, source_blocks_json, target_slots_json, token_estimate,
               deadline_at, status, batch_id, result_status, title_zh, summary_md, body_md,
               error_text, cache_hit, created_at, started_at, finished_at, updated_at
        FROM translation_work_items
        WHERE dedupe_key = ?
        LIMIT 1
        "#,
    )
    .bind(dedupe_key.as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::internal)
}

async fn create_work_item(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    item: &TranslationRequestItemInput,
    source_hash: &str,
    now: &str,
) -> Result<String, ApiError> {
    let id = crate::local_id::generate_local_id();
    let model_profile = current_model_profile();
    let token_estimate = estimate_item_tokens(item);
    let deadline_at =
        (Utc::now() + chrono::Duration::milliseconds(item.max_wait_ms.max(0))).to_rfc3339();
    let source_blocks_json =
        serde_json::to_string(&item.source_blocks).map_err(ApiError::internal)?;
    let target_slots_json =
        serde_json::to_string(&item.target_slots).map_err(ApiError::internal)?;
    let dedupe_key = build_dedupe_key(user_id, item, source_hash);

    sqlx::query(
        r#"
        INSERT INTO translation_work_items (
          id, dedupe_key, scope_user_id, kind, variant, entity_id, target_lang, protocol_version,
          model_profile, source_hash, source_blocks_json, target_slots_json, token_estimate,
          deadline_at, status, cache_hit, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'queued', 0, ?, ?)
        "#,
    )
    .bind(id.as_str())
    .bind(dedupe_key.as_str())
    .bind(user_id)
    .bind(item.kind.as_str())
    .bind(item.variant.as_str())
    .bind(item.entity_id.as_str())
    .bind(item.target_lang.as_str())
    .bind(TRANSLATION_PROTOCOL_VERSION)
    .bind(model_profile.as_str())
    .bind(source_hash)
    .bind(source_blocks_json.as_str())
    .bind(target_slots_json.as_str())
    .bind(i64::from(token_estimate))
    .bind(deadline_at.as_str())
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    Ok(id)
}

async fn run_translation_scheduler_once(
    state: &AppState,
    worker: TranslationWorkerProfile,
) -> Result<()> {
    let Some(batch) = claim_next_batch(state, worker).await? else {
        update_translation_worker_runtime(worker, "idle", None, 0, 0, None, None).await;
        return Ok(());
    };
    execute_claimed_batch(state, batch).await
}

async fn claim_next_batch(
    state: &AppState,
    worker: TranslationWorkerProfile,
) -> Result<Option<ClaimedBatch>> {
    let claim_origin_case = claim_origin_case_sql();
    let first_query = if worker.worker_kind == "user_dedicated" {
        format!(
            r#"
            SELECT w.id, w.dedupe_key, w.scope_user_id, w.kind, w.variant, w.entity_id, w.target_lang,
                   w.protocol_version, w.model_profile, w.source_hash, w.source_blocks_json,
                   w.target_slots_json, w.token_estimate, w.deadline_at, w.status, w.batch_id,
                   w.result_status, w.title_zh, w.summary_md, w.body_md, w.error_text, w.cache_hit,
                   w.created_at, w.started_at, w.finished_at, w.updated_at,
                   {claim_origin_case} AS request_origin
            FROM translation_work_items w
            WHERE w.status = 'queued'
              AND {claim_origin_case} = 'user'
            ORDER BY w.deadline_at ASC, w.created_at ASC
            LIMIT 1
            "#,
        )
    } else {
        format!(
            r#"
            SELECT w.id, w.dedupe_key, w.scope_user_id, w.kind, w.variant, w.entity_id, w.target_lang,
                   w.protocol_version, w.model_profile, w.source_hash, w.source_blocks_json,
                   w.target_slots_json, w.token_estimate, w.deadline_at, w.status, w.batch_id,
                   w.result_status, w.title_zh, w.summary_md, w.body_md, w.error_text, w.cache_hit,
                   w.created_at, w.started_at, w.finished_at, w.updated_at,
                   {claim_origin_case} AS request_origin
            FROM translation_work_items w
            WHERE w.status = 'queued'
            ORDER BY w.deadline_at ASC, w.created_at ASC
            LIMIT 1
            "#,
        )
    };
    let first = sqlx::query_as::<_, ClaimCandidateRow>(&first_query)
        .fetch_optional(&state.pool)
        .await?;
    let Some(first) = first else {
        return Ok(None);
    };

    let budget = i64::from(
        ai::compute_input_budget_with_source(state, TRANSLATION_BATCH_MAX_TOKENS)
            .await
            .input_budget,
    );
    let candidates_query = format!(
        r#"
        SELECT w.id, w.dedupe_key, w.scope_user_id, w.kind, w.variant, w.entity_id, w.target_lang,
               w.protocol_version, w.model_profile, w.source_hash, w.source_blocks_json,
               w.target_slots_json, w.token_estimate, w.deadline_at, w.status, w.batch_id,
               w.result_status, w.title_zh, w.summary_md, w.body_md, w.error_text, w.cache_hit,
               w.created_at, w.started_at, w.finished_at, w.updated_at,
               {claim_origin_case} AS request_origin
        FROM translation_work_items w
        WHERE w.status = 'queued'
          AND w.target_lang = ?
          AND w.protocol_version = ?
          AND w.model_profile = ?
          AND {claim_origin_case} = ?
        ORDER BY w.rowid ASC
        LIMIT 200
        "#,
    );
    let candidates = sqlx::query_as::<_, ClaimCandidateRow>(&candidates_query)
        .bind(first.target_lang.as_str())
        .bind(first.protocol_version.as_str())
        .bind(first.model_profile.as_str())
        .bind(first.request_origin.as_str())
        .fetch_all(&state.pool)
        .await?;

    let now = Utc::now();
    let earliest_deadline = parse_ts(first.deadline_at.as_str())?;
    let deadline_due = earliest_deadline <= now;
    let mut selected = Vec::new();
    let mut token_sum = 0_i64;
    let mut trigger_reason = None;
    for item in candidates {
        token_sum += item.token_estimate.max(1);
        selected.push(item);
        if token_sum >= budget {
            trigger_reason = Some("token_threshold");
            break;
        }
    }
    if trigger_reason.is_none() {
        if deadline_due {
            trigger_reason = Some("deadline");
        } else {
            return Ok(None);
        }
    }
    let trigger_reason = trigger_reason.unwrap_or("deadline").to_owned();
    let batch_id = crate::local_id::generate_local_id();
    let partition_key = format!(
        "{}:{}:{}",
        first.target_lang, first.protocol_version, first.model_profile
    );
    let now_str = now.to_rfc3339();
    let mut tx = state.pool.begin().await?;
    let mut request_ids = HashSet::new();
    for item in &selected {
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT id
            FROM translation_requests
            WHERE work_item_id = ?
            "#,
        )
        .bind(item.id.as_str())
        .fetch_all(&mut *tx)
        .await?;
        request_ids.extend(rows);
    }
    let request_count = i64::try_from(request_ids.len()).unwrap_or(i64::MAX);

    sqlx::query(
        r#"
        INSERT INTO translation_batches (
          id, partition_key, protocol_version, model_profile, target_lang, trigger_reason,
          worker_slot, request_count, item_count, estimated_input_tokens, status, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'queued', ?, ?)
        "#,
    )
    .bind(batch_id.as_str())
    .bind(partition_key.as_str())
    .bind(first.protocol_version.as_str())
    .bind(first.model_profile.as_str())
    .bind(first.target_lang.as_str())
    .bind(trigger_reason.as_str())
    .bind(worker.worker_slot)
    .bind(request_count)
    .bind(i64::try_from(selected.len()).unwrap_or(i64::MAX))
    .bind(token_sum)
    .bind(now_str.as_str())
    .bind(now_str.as_str())
    .execute(&mut *tx)
    .await?;

    for (index, item) in selected.iter().enumerate() {
        let producer_count = sqlx::query_scalar::<_, i64>(
            r#"SELECT COUNT(*) FROM translation_requests WHERE work_item_id = ?"#,
        )
        .bind(item.id.as_str())
        .fetch_one(&mut *tx)
        .await?;

        let rows_affected = sqlx::query(
            r#"
            UPDATE translation_work_items
            SET status = 'batched', batch_id = ?, updated_at = ?
            WHERE id = ? AND status = 'queued'
            "#,
        )
        .bind(batch_id.as_str())
        .bind(now_str.as_str())
        .bind(item.id.as_str())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if rows_affected != 1 {
            return Ok(None);
        }

        sqlx::query(
            r#"
            INSERT INTO translation_batch_items (
              id, batch_id, work_item_id, item_index, kind, variant, entity_id, producer_count,
              token_estimate, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(crate::local_id::generate_local_id())
        .bind(batch_id.as_str())
        .bind(item.id.as_str())
        .bind(i64::try_from(index).unwrap_or(i64::MAX))
        .bind(item.kind.as_str())
        .bind(item.variant.as_str())
        .bind(item.entity_id.as_str())
        .bind(producer_count)
        .bind(item.token_estimate)
        .bind(now_str.as_str())
        .bind(now_str.as_str())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    mark_requests_running_for_work_items(
        state,
        selected.iter().map(|item| item.id.as_str()).collect(),
    )
    .await?;

    Ok(Some(ClaimedBatch {
        id: batch_id,
        worker_slot: worker.worker_slot,
        partition_key,
        target_lang: first.target_lang,
        protocol_version: first.protocol_version,
        model_profile: first.model_profile,
        trigger_reason,
        request_count,
        estimated_input_tokens: token_sum,
        items: selected
            .into_iter()
            .map(ClaimCandidateRow::into_work_item)
            .collect(),
    }))
}

async fn execute_claimed_batch(state: &AppState, batch: ClaimedBatch) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let worker = TranslationWorkerProfile {
        worker_slot: batch.worker_slot,
        worker_kind: if batch.worker_slot == TRANSLATION_USER_DEDICATED_WORKER_SLOT {
            "user_dedicated"
        } else {
            "general"
        },
    };
    sqlx::query(
        r#"
        UPDATE translation_batches
        SET status = 'running', started_at = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(batch.id.as_str())
    .execute(&state.pool)
    .await?;
    update_translation_worker_runtime(
        worker,
        "running",
        Some(batch.id.as_str()),
        batch.request_count,
        i64::try_from(batch.items.len()).unwrap_or(i64::MAX),
        Some(batch.trigger_reason.as_str()),
        None,
    )
    .await;
    for item in &batch.items {
        sqlx::query(
            r#"
            UPDATE translation_work_items
            SET status = 'running', started_at = COALESCE(started_at, ?), updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(item.id.as_str())
        .execute(&state.pool)
        .await?;
    }

    let context = ai::LlmCallContext {
        source: format!("translation.scheduler.{}", batch.trigger_reason),
        requested_by: None,
        parent_task_id: None,
        parent_task_type: None,
        parent_translation_batch_id: Some(batch.id.clone()),
    };

    let result = ai::with_llm_call_context(context, async {
        resolve_batch_results(state, &batch).await
    })
    .await;

    match result {
        Ok(results) => {
            let res = finalize_batch_success(state, &batch, results).await;
            update_translation_worker_runtime(worker, "idle", None, 0, 0, None, None).await;
            res
        }
        Err(err) => {
            let error = err.to_string();
            let res = finalize_batch_failure(state, &batch, err.into()).await;
            update_translation_worker_runtime(
                worker,
                "error",
                None,
                0,
                0,
                None,
                Some(error.as_str()),
            )
            .await;
            res
        }
    }
}

async fn resolve_batch_results(
    state: &AppState,
    batch: &ClaimedBatch,
) -> Result<Vec<TerminalWorkResult>, ApiError> {
    if state.config.ai.is_none() {
        return Ok(batch
            .items
            .iter()
            .map(|item| TerminalWorkResult {
                work_item_id: item.id.clone(),
                result_status: "disabled".to_owned(),
                title_zh: None,
                summary_md: None,
                body_md: None,
                error: None,
            })
            .collect());
    }

    let mut out = Vec::with_capacity(batch.items.len());

    let mut release_groups: BTreeMap<String, Vec<&WorkItemRow>> = BTreeMap::new();
    let mut detail_groups: BTreeMap<String, Vec<&WorkItemRow>> = BTreeMap::new();
    let mut notification_groups: BTreeMap<String, Vec<&WorkItemRow>> = BTreeMap::new();

    for item in &batch.items {
        match item.kind.as_str() {
            "release_summary" => {
                release_groups
                    .entry(item.scope_user_id.clone())
                    .or_default()
                    .push(item);
            }
            "release_detail" => {
                detail_groups
                    .entry(item.scope_user_id.clone())
                    .or_default()
                    .push(item);
            }
            "notification" => {
                notification_groups
                    .entry(item.scope_user_id.clone())
                    .or_default()
                    .push(item);
            }
            _ => {
                out.push(TerminalWorkResult {
                    work_item_id: item.id.clone(),
                    result_status: "error".to_owned(),
                    title_zh: None,
                    summary_md: None,
                    body_md: None,
                    error: Some(format!("unsupported translation kind: {}", item.kind)),
                });
            }
        }
    }

    for (user_id, items) in release_groups {
        let release_ids = items
            .iter()
            .filter_map(|item| item.entity_id.parse::<i64>().ok())
            .collect::<Vec<_>>();
        let response =
            api::translate_releases_batch_for_user(state, user_id.as_str(), &release_ids).await?;
        let by_id = response
            .items
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect::<HashMap<_, _>>();
        for item in items {
            let result = if let Some(translated) = by_id.get(&item.entity_id) {
                terminal_result_from_batch_item(item, translated)
            } else {
                TerminalWorkResult {
                    work_item_id: item.id.clone(),
                    result_status: "error".to_owned(),
                    title_zh: None,
                    summary_md: None,
                    body_md: None,
                    error: Some("translation result missing".to_owned()),
                }
            };
            out.push(result);
        }
    }

    for (_user_id, items) in detail_groups {
        for item in items {
            let result = match api::translate_release_detail_for_user(
                state,
                item.scope_user_id.as_str(),
                item.entity_id.as_str(),
            )
            .await
            {
                Ok(translated) => terminal_result_from_single_response(item, &translated),
                Err(err) if err.code() == "not_found" => TerminalWorkResult {
                    work_item_id: item.id.clone(),
                    result_status: "missing".to_owned(),
                    title_zh: None,
                    summary_md: None,
                    body_md: None,
                    error: Some("release not found".to_owned()),
                },
                Err(err) => TerminalWorkResult {
                    work_item_id: item.id.clone(),
                    result_status: "error".to_owned(),
                    title_zh: None,
                    summary_md: None,
                    body_md: None,
                    error: Some(err.to_string()),
                },
            };
            out.push(result);
        }
    }

    for (_user_id, items) in notification_groups {
        for item in items {
            let result = match api::translate_notification_for_user(
                state,
                item.scope_user_id.clone(),
                item.entity_id.as_str(),
            )
            .await
            {
                Ok(translated) => terminal_result_from_single_response(item, &translated),
                Err(err) if err.code() == "not_found" => TerminalWorkResult {
                    work_item_id: item.id.clone(),
                    result_status: "missing".to_owned(),
                    title_zh: None,
                    summary_md: None,
                    body_md: None,
                    error: Some("notification not found".to_owned()),
                },
                Err(err) => TerminalWorkResult {
                    work_item_id: item.id.clone(),
                    result_status: "error".to_owned(),
                    title_zh: None,
                    summary_md: None,
                    body_md: None,
                    error: Some(err.to_string()),
                },
            };
            out.push(result);
        }
    }

    Ok(out)
}

fn terminal_result_from_batch_item(
    item: &WorkItemRow,
    translated: &api::TranslateBatchItem,
) -> TerminalWorkResult {
    let mut out = TerminalWorkResult {
        work_item_id: item.id.clone(),
        result_status: translated.status.clone(),
        title_zh: translated.title.clone(),
        summary_md: None,
        body_md: None,
        error: translated.error.clone(),
    };
    if item.kind == "release_detail" {
        out.body_md = translated.summary.clone();
    } else {
        out.summary_md = translated.summary.clone();
    }
    out
}

fn terminal_result_from_single_response(
    item: &WorkItemRow,
    translated: &api::TranslateResponse,
) -> TerminalWorkResult {
    let mut out = TerminalWorkResult {
        work_item_id: item.id.clone(),
        result_status: translated.status.clone(),
        title_zh: translated.title.clone(),
        summary_md: None,
        body_md: None,
        error: None,
    };
    if item.kind == "release_detail" {
        out.body_md = translated.summary.clone();
    } else {
        out.summary_md = translated.summary.clone();
    }
    out
}

async fn finalize_batch_success(
    state: &AppState,
    batch: &ClaimedBatch,
    results: Vec<TerminalWorkResult>,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let mut tx = state.pool.begin().await?;
    for result in &results {
        sqlx::query(
            r#"
            UPDATE translation_work_items
            SET status = 'completed',
                result_status = ?,
                title_zh = ?,
                summary_md = ?,
                body_md = ?,
                error_text = ?,
                finished_at = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(result.result_status.as_str())
        .bind(result.title_zh.as_deref())
        .bind(result.summary_md.as_deref())
        .bind(result.body_md.as_deref())
        .bind(result.error.as_deref())
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(result.work_item_id.as_str())
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE translation_batch_items
            SET result_status = ?, error_text = ?, updated_at = ?
            WHERE batch_id = ? AND work_item_id = ?
            "#,
        )
        .bind(result.result_status.as_str())
        .bind(result.error.as_deref())
        .bind(now.as_str())
        .bind(batch.id.as_str())
        .bind(result.work_item_id.as_str())
        .execute(&mut *tx)
        .await?;

        let requests = sqlx::query(
            r#"
            SELECT id, producer_ref, entity_id, kind, variant
            FROM translation_requests
            WHERE work_item_id = ?
            "#,
        )
        .bind(result.work_item_id.as_str())
        .fetch_all(&mut *tx)
        .await?;
        for request in requests {
            let request_id: String = request.try_get("id")?;
            let producer_ref: String = request.try_get("producer_ref")?;
            let entity_id: String = request.try_get("entity_id")?;
            let kind: String = request.try_get("kind")?;
            let variant: String = request.try_get("variant")?;
            let request_result = TranslationResultItem {
                producer_ref,
                entity_id,
                kind,
                variant,
                status: result.result_status.clone(),
                title_zh: result.title_zh.clone(),
                summary_md: result.summary_md.clone(),
                body_md: result.body_md.clone(),
                error: result.error.clone(),
                work_item_id: Some(result.work_item_id.clone()),
                batch_id: Some(batch.id.clone()),
            };
            apply_request_result(
                &mut tx,
                request_id.as_str(),
                Some(result.work_item_id.as_str()),
                &request_result,
                now.as_str(),
            )
            .await?;
        }

        if result.result_status == "ready"
            && let Some(work_item) = batch
                .items
                .iter()
                .find(|item| item.id == result.work_item_id)
        {
            upsert_cached_translation(
                &mut tx,
                &work_item.scope_user_id,
                work_item.kind.as_str(),
                work_item.entity_id.as_str(),
                work_item.target_lang.as_str(),
                work_item.source_hash.as_str(),
                result.title_zh.as_deref(),
                result.summary_md.as_deref().or(result.body_md.as_deref()),
                now.as_str(),
            )
            .await?;
        }
    }

    sqlx::query(
        r#"
        UPDATE translation_batches
        SET status = 'completed', finished_at = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(batch.id.as_str())
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

async fn finalize_batch_failure(
    state: &AppState,
    batch: &ClaimedBatch,
    err: anyhow::Error,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let message = err.to_string();
    let mut tx = state.pool.begin().await?;
    for item in &batch.items {
        sqlx::query(
            r#"
            UPDATE translation_work_items
            SET status = 'failed', result_status = 'error', error_text = ?, finished_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(message.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(item.id.as_str())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE translation_batch_items
            SET result_status = 'error', error_text = ?, updated_at = ?
            WHERE batch_id = ? AND work_item_id = ?
            "#,
        )
        .bind(message.as_str())
        .bind(now.as_str())
        .bind(batch.id.as_str())
        .bind(item.id.as_str())
        .execute(&mut *tx)
        .await?;
        let requests = sqlx::query(
            r#"
            SELECT id, producer_ref, entity_id, kind, variant
            FROM translation_requests
            WHERE work_item_id = ?
            "#,
        )
        .bind(item.id.as_str())
        .fetch_all(&mut *tx)
        .await?;
        for request in requests {
            let request_id: String = request.try_get("id")?;
            let producer_ref: String = request.try_get("producer_ref")?;
            let entity_id: String = request.try_get("entity_id")?;
            let kind: String = request.try_get("kind")?;
            let variant: String = request.try_get("variant")?;
            let request_result = TranslationResultItem {
                producer_ref,
                entity_id,
                kind,
                variant,
                status: "error".to_owned(),
                title_zh: None,
                summary_md: None,
                body_md: None,
                error: Some(message.clone()),
                work_item_id: Some(item.id.clone()),
                batch_id: Some(batch.id.clone()),
            };
            apply_request_result(
                &mut tx,
                request_id.as_str(),
                Some(item.id.as_str()),
                &request_result,
                now.as_str(),
            )
            .await?;
        }
    }
    sqlx::query(
        r#"
        UPDATE translation_batches
        SET status = 'failed', error_text = ?, finished_at = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(message.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(batch.id.as_str())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
async fn refresh_requests_after_completion(
    tx: &mut Transaction<'_, Sqlite>,
    request_ids: Vec<String>,
    now: &str,
) -> Result<()> {
    for request_id in request_ids {
        let result_status = sqlx::query_scalar::<_, Option<String>>(
            r#"SELECT result_status FROM translation_requests WHERE id = ? LIMIT 1"#,
        )
        .bind(request_id.as_str())
        .fetch_optional(&mut **tx)
        .await?;
        let Some(result_status) = result_status.flatten() else {
            continue;
        };
        let status = request_status_from_result_status(result_status.as_str());
        sqlx::query(
            r#"
            UPDATE translation_requests
            SET status = ?,
                finished_at = CASE WHEN ? IN ('completed', 'failed') THEN ? ELSE finished_at END,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(status)
        .bind(status)
        .bind(now)
        .bind(now)
        .bind(request_id.as_str())
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn mark_requests_running_for_work_items(
    state: &AppState,
    work_item_ids: Vec<&str>,
) -> Result<()> {
    if work_item_ids.is_empty() {
        return Ok(());
    }
    let now = Utc::now().to_rfc3339();
    let mut query = sqlx::QueryBuilder::<Sqlite>::new(
        r#"
        UPDATE translation_requests
        SET status = 'running', started_at = COALESCE(started_at, "#,
    );
    query.push_bind(now.as_str());
    query.push(r#"), updated_at = "#);
    query.push_bind(now.as_str());
    query.push(r#" WHERE status = 'queued' AND work_item_id IN ("#);
    {
        let mut separated = query.separated(", ");
        for id in work_item_ids {
            separated.push_bind(id);
        }
    }
    query.push(")");
    query.build().execute(&state.pool).await?;
    Ok(())
}

fn stream_translation_request_response(
    state: Arc<AppState>,
    user_id: String,
    request_id: String,
) -> Response {
    let stream = async_stream::stream! {
        let mut last_phase = String::new();
        loop {
            let snapshot = load_translation_request_detail(state.as_ref(), &user_id, &request_id).await;
            match snapshot {
                Ok(detail) => {
                    let phase = derive_request_stream_phase(&detail);
                    if phase != last_phase {
                        let event = TranslationRequestStreamEvent {
                            event: phase.clone(),
                            request_id: request_id.clone(),
                            status: detail.request.effective_status().to_owned(),
                            batch_id: detail.result.batch_id.clone(),
                            result: matches!(phase.as_str(), "completed" | "failed")
                                .then(|| detail.result.clone()),
                            error: if phase == "failed" {
                                detail.result.error.clone()
                            } else {
                                None
                            },
                        };
                        let mut payload = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_owned());
                        payload.push('\n');
                        yield Ok::<_, Infallible>(axum::body::Bytes::from(payload));
                        last_phase = phase.clone();
                    }
                    if matches!(detail.request.status.as_str(), "completed" | "failed") {
                        break;
                    }
                }
                Err(err) => {
                    let event = TranslationRequestStreamEvent {
                        event: "failed".to_owned(),
                        request_id: request_id.clone(),
                        status: "failed".to_owned(),
                        batch_id: None,
                        result: None,
                        error: Some(err.to_string()),
                    };
                    let mut payload = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_owned());
                    payload.push('\n');
                    yield Ok::<_, Infallible>(axum::body::Bytes::from(payload));
                    break;
                }
            }
            sleep(TRANSLATION_STREAM_POLL_INTERVAL).await;
        }
    };

    let body = Body::from_stream(stream);
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

#[derive(Debug)]
struct LoadedRequestDetail {
    request: RequestRow,
    result: TranslationResultItem,
}

#[derive(Debug)]
enum NormalizedTranslationSubmit {
    Single(TranslationRequestItemInput),
    Batch(Vec<TranslationRequestItemInput>),
}

fn normalize_submit_payload(
    mode: &str,
    req: TranslationSubmitRequest,
) -> Result<NormalizedTranslationSubmit, ApiError> {
    match (req.item, req.items) {
        (Some(_), Some(_)) => Err(ApiError::bad_request(
            "item and items are mutually exclusive",
        )),
        (None, None) => Err(ApiError::bad_request("item or items is required")),
        (Some(item), None) => {
            let mut items = normalize_request_items(std::slice::from_ref(&item))?;
            Ok(NormalizedTranslationSubmit::Single(items.remove(0)))
        }
        (None, Some(items)) => {
            if mode != "async" {
                return Err(ApiError::bad_request(
                    "wait/stream supports only a single item; batch requests must use async",
                ));
            }
            Ok(NormalizedTranslationSubmit::Batch(normalize_request_items(
                &items,
            )?))
        }
    }
}

#[derive(Debug, Clone)]
struct CreatedTranslationRequest {
    request_id: String,
    status: String,
    result: TranslationResultItem,
}

impl CreatedTranslationRequest {
    fn to_public_response(&self) -> TranslationRequestResponse {
        TranslationRequestResponse {
            request_id: self.request_id.clone(),
            status: self.status.clone(),
            result: self.result.clone(),
        }
    }

    fn to_batch_response(&self) -> TranslationBatchSubmitItemResponse {
        TranslationBatchSubmitItemResponse {
            request_id: self.request_id.clone(),
            status: self.status.clone(),
            producer_ref: self.result.producer_ref.clone(),
            entity_id: self.result.entity_id.clone(),
            kind: self.result.kind.clone(),
            variant: self.result.variant.clone(),
        }
    }
}

async fn wait_for_request_terminal(
    state: &AppState,
    user_id: &str,
    request_id: &str,
) -> Result<LoadedRequestDetail, ApiError> {
    loop {
        let detail = load_translation_request_detail(state, user_id, request_id).await?;
        if matches!(detail.request.status.as_str(), "completed" | "failed") {
            return Ok(detail);
        }
        sleep(TRANSLATION_WAIT_POLL_INTERVAL).await;
    }
}

async fn ensure_request_owner(
    state: &AppState,
    user_id: &str,
    request_id: &str,
) -> Result<(), ApiError> {
    let owner = sqlx::query_scalar::<_, String>(
        r#"SELECT scope_user_id FROM translation_requests WHERE id = ? LIMIT 1"#,
    )
    .bind(request_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "translation request not found",
        )
    })?;
    if owner != user_id {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "translation request not found",
        ));
    }
    Ok(())
}

async fn load_translation_request_detail(
    state: &AppState,
    user_id: &str,
    request_id: &str,
) -> Result<LoadedRequestDetail, ApiError> {
    let request_row_sql = format!(
        r#"{}
        WHERE r.id = ? AND r.scope_user_id = ?
        LIMIT 1
        "#,
        request_row_select_sql(),
    );
    let request = sqlx::query_as::<_, RequestRow>(&request_row_sql)
        .bind(request_id)
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "translation request not found",
            )
        })?;
    let result = request.to_result();
    Ok(LoadedRequestDetail { request, result })
}

async fn load_translation_result_items_by_batch(
    state: &AppState,
    batch_id: &str,
) -> Result<Vec<TranslationResultItem>, ApiError> {
    let rows = sqlx::query(
        r#"
        SELECT w.id AS work_item_id, b.entity_id, b.kind, b.variant,
               COALESCE(b.result_status, w.result_status, 'queued') AS result_status,
               w.title_zh, w.summary_md, w.body_md, COALESCE(b.error_text, w.error_text) AS error_text,
               MIN(r.producer_ref) AS producer_ref
        FROM translation_batch_items b
        JOIN translation_work_items w ON w.id = b.work_item_id
        LEFT JOIN translation_requests r ON r.work_item_id = w.id
        WHERE b.batch_id = ?
        GROUP BY w.id, b.entity_id, b.kind, b.variant, b.result_status, w.result_status,
                 w.title_zh, w.summary_md, w.body_md, b.error_text, w.error_text
        ORDER BY b.item_index ASC, w.id ASC
        "#,
    )
    .bind(batch_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;

    Ok(rows
        .into_iter()
        .map(|row| TranslationResultItem {
            producer_ref: row
                .get::<Option<String>, _>("producer_ref")
                .unwrap_or_else(|| "(shared)".to_owned()),
            entity_id: row.get("entity_id"),
            kind: row.get("kind"),
            variant: row.get("variant"),
            status: row.get("result_status"),
            title_zh: row.get("title_zh"),
            summary_md: row.get("summary_md"),
            body_md: row.get("body_md"),
            error: row.get("error_text"),
            work_item_id: row.get("work_item_id"),
            batch_id: Some(batch_id.to_owned()),
        })
        .collect())
}

fn detail_to_public_response(detail: LoadedRequestDetail) -> TranslationRequestResponse {
    let request_id = detail.request.id.clone();
    let status = detail.request.effective_status().to_owned();
    TranslationRequestResponse {
        request_id,
        status,
        result: detail.result,
    }
}

fn derive_request_stream_phase(detail: &LoadedRequestDetail) -> String {
    match detail.request.effective_status() {
        "completed" => "completed".to_owned(),
        "failed" => "failed".to_owned(),
        "running" => "running".to_owned(),
        _ if detail.result.batch_id.is_some() => "batched".to_owned(),
        _ => "queued".to_owned(),
    }
}

fn normalize_mode(raw: &str) -> Result<&'static str, ApiError> {
    match raw {
        "async" => Ok("async"),
        "wait" => Ok("wait"),
        "stream" => Ok("stream"),
        _ => Err(ApiError::bad_request("mode must be async, wait, or stream")),
    }
}

fn normalize_request_items(
    raw_items: &[TranslationRequestItemInput],
) -> Result<Vec<TranslationRequestItemInput>, ApiError> {
    if raw_items.is_empty() {
        return Err(ApiError::bad_request("items is required"));
    }
    if raw_items.len() > TRANSLATION_MAX_ITEMS_PER_REQUEST {
        return Err(ApiError::bad_request(format!(
            "items supports at most {TRANSLATION_MAX_ITEMS_PER_REQUEST} items"
        )));
    }
    let mut out = Vec::with_capacity(raw_items.len());
    for item in raw_items {
        let producer_ref = item.producer_ref.trim();
        let kind = item.kind.trim();
        let variant = item.variant.trim();
        let entity_id = item.entity_id.trim();
        let target_lang = item.target_lang.trim();
        if producer_ref.is_empty()
            || kind.is_empty()
            || variant.is_empty()
            || entity_id.is_empty()
            || target_lang.is_empty()
        {
            return Err(ApiError::bad_request(
                "producer_ref, kind, variant, entity_id, target_lang are required",
            ));
        }
        if !matches!(kind, "release_summary" | "release_detail" | "notification") {
            return Err(ApiError::bad_request(format!(
                "unsupported translation kind: {kind}"
            )));
        }
        if target_lang != "zh-CN" {
            return Err(ApiError::bad_request("only zh-CN is supported"));
        }
        if item.source_blocks.is_empty() {
            return Err(ApiError::bad_request("source_blocks is required"));
        }
        if item.target_slots.is_empty() {
            return Err(ApiError::bad_request("target_slots is required"));
        }
        let max_wait_ms = item
            .max_wait_ms
            .clamp(TRANSLATION_MIN_WAIT_MS, TRANSLATION_MAX_WAIT_MS);
        let mut seen_slots = HashSet::new();
        let source_blocks = item
            .source_blocks
            .iter()
            .map(|block| {
                let slot = block.slot.trim();
                if !matches!(slot, "title" | "excerpt" | "body_markdown" | "metadata") {
                    return Err(ApiError::bad_request(format!(
                        "unsupported source block slot: {slot}"
                    )));
                }
                let text = block.text.trim().to_owned();
                if text.is_empty() {
                    return Err(ApiError::bad_request("source block text cannot be empty"));
                }
                Ok(TranslationSourceBlock {
                    slot: slot.to_owned(),
                    text,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let target_slots = item
            .target_slots
            .iter()
            .map(|slot| {
                let slot = slot.trim();
                if !matches!(slot, "title_zh" | "summary_md" | "body_md") {
                    return Err(ApiError::bad_request(format!(
                        "unsupported target slot: {slot}"
                    )));
                }
                if !seen_slots.insert(slot.to_owned()) {
                    return Err(ApiError::bad_request(format!(
                        "duplicate target slot: {slot}"
                    )));
                }
                Ok(slot.to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        out.push(TranslationRequestItemInput {
            producer_ref: producer_ref.to_owned(),
            kind: kind.to_owned(),
            variant: variant.to_owned(),
            entity_id: entity_id.to_owned(),
            target_lang: target_lang.to_owned(),
            max_wait_ms,
            source_blocks,
            target_slots,
        });
    }
    Ok(out)
}

fn build_source_hash(item: &TranslationRequestItemInput) -> String {
    let blocks_json =
        serde_json::to_string(&item.source_blocks).unwrap_or_else(|_| "[]".to_owned());
    let slots_json = serde_json::to_string(&item.target_slots).unwrap_or_else(|_| "[]".to_owned());
    ai::sha256_hex(&format!(
        "version={TRANSLATION_PROTOCOL_VERSION}\nkind={}\nvariant={}\nlang={}\nentity={}\nblocks={}\ntarget_slots={}\n",
        item.kind, item.variant, item.target_lang, item.entity_id, blocks_json, slots_json,
    ))
}

fn build_dedupe_key(
    user_id: &str,
    item: &TranslationRequestItemInput,
    source_hash: &str,
) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}:{}",
        user_id,
        item.kind,
        item.variant,
        item.entity_id,
        item.target_lang,
        TRANSLATION_PROTOCOL_VERSION,
        source_hash
    )
}

fn estimate_item_tokens(item: &TranslationRequestItemInput) -> u32 {
    let block_tokens = item
        .source_blocks
        .iter()
        .map(|block| ai::estimate_text_tokens(block.text.as_str()).saturating_add(16))
        .sum::<u32>();
    block_tokens
        .saturating_add(
            u32::try_from(item.target_slots.len())
                .unwrap_or(u32::MAX)
                .saturating_mul(12),
        )
        .saturating_add(48)
}

fn derive_request_source(items: &[TranslationRequestItemInput]) -> String {
    let sources = items
        .iter()
        .filter_map(derive_item_source)
        .collect::<HashSet<_>>();
    match sources.len() {
        0 => "translation".to_owned(),
        1 => sources
            .into_iter()
            .next()
            .unwrap_or_else(|| "translation".to_owned()),
        _ => "mixed".to_owned(),
    }
}

fn derive_item_source(item: &TranslationRequestItemInput) -> Option<String> {
    if let Some(source) = item
        .producer_ref
        .split(':')
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.chars().all(|ch| ch.is_ascii_digit()))
    {
        return Some(source.to_owned());
    }
    match (item.kind.as_str(), item.variant.as_str()) {
        ("release_summary", "feed_card") => Some("feed.auto_translate".to_owned()),
        ("release_detail", _) => Some("release_detail".to_owned()),
        ("notification", _) => Some("notification".to_owned()),
        _ => Some(item.kind.clone()),
    }
}

fn current_model_profile() -> String {
    std::env::var("AI_MODEL")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| TRANSLATION_MODEL_PROFILE_DISABLED.to_owned())
}

fn map_entity_type(kind: &str) -> Option<&'static str> {
    match kind {
        "release_summary" => Some("release"),
        "release_detail" => Some("release_detail"),
        "notification" => Some("notification"),
        _ => None,
    }
}

fn pending_result_from_work_row(row: &WorkItemRow, producer_ref: String) -> TranslationResultItem {
    TranslationResultItem {
        producer_ref,
        entity_id: row.entity_id.clone(),
        kind: row.kind.clone(),
        variant: row.variant.clone(),
        status: pending_result_status_from_work_status(Some(row.status.as_str())).to_owned(),
        title_zh: None,
        summary_md: None,
        body_md: None,
        error: None,
        work_item_id: Some(row.id.clone()),
        batch_id: row.batch_id.clone(),
    }
}

fn terminal_result_from_work_row(row: &WorkItemRow, producer_ref: String) -> TranslationResultItem {
    TranslationResultItem {
        producer_ref,
        entity_id: row.entity_id.clone(),
        kind: row.kind.clone(),
        variant: row.variant.clone(),
        status: row
            .result_status
            .clone()
            .unwrap_or_else(|| "error".to_owned()),
        title_zh: row.title_zh.clone(),
        summary_md: row.summary_md.clone(),
        body_md: row.body_md.clone(),
        error: row.error_text.clone(),
        work_item_id: Some(row.id.clone()),
        batch_id: row.batch_id.clone(),
    }
}

fn parse_ts(raw: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(raw)?.with_timezone(&Utc))
}

#[allow(clippy::too_many_arguments)]
async fn upsert_cached_translation(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    kind: &str,
    entity_id: &str,
    target_lang: &str,
    source_hash: &str,
    title: Option<&str>,
    summary: Option<&str>,
    now: &str,
) -> Result<()> {
    let Some(entity_type) = map_entity_type(kind) else {
        return Ok(());
    };
    sqlx::query(
        r#"
        INSERT INTO ai_translations (id, user_id, entity_type, entity_id, lang, source_hash, title, summary, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id, entity_type, entity_id, lang)
        DO UPDATE SET source_hash = excluded.source_hash,
                      title = excluded.title,
                      summary = excluded.summary,
                      updated_at = excluded.updated_at
        "#,
    )
    .bind(crate::local_id::generate_local_id())
    .bind(user_id)
    .bind(entity_type)
    .bind(entity_id)
    .bind(target_lang)
    .bind(source_hash)
    .bind(title)
    .bind(summary)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn scalar_i64(state: &AppState, sql: &str, binds: &[&str]) -> Result<i64, ApiError> {
    let mut query = sqlx::query_scalar::<_, i64>(sql);
    for bind in binds {
        query = query.bind(*bind);
    }
    query
        .fetch_one(&state.pool)
        .await
        .map_err(ApiError::internal)
}

async fn load_last_batch_finished_at(state: &AppState) -> Result<Option<String>, ApiError> {
    sqlx::query_scalar::<_, Option<String>>(
        r#"SELECT MAX(finished_at) FROM translation_batches WHERE finished_at IS NOT NULL"#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, sync::Arc};

    use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
    use url::Url;

    use super::*;
    use crate::{
        config::{AppConfig, GitHubOAuthConfig},
        crypto::EncryptionKey,
        state::build_oauth_client,
    };

    #[test]
    fn normalize_request_items_rejects_unsupported_kind() {
        let err = normalize_request_items(&[TranslationRequestItemInput {
            producer_ref: "r1".to_owned(),
            kind: "other".to_owned(),
            variant: "feed_card".to_owned(),
            entity_id: "123".to_owned(),
            target_lang: "zh-CN".to_owned(),
            max_wait_ms: 1000,
            source_blocks: vec![TranslationSourceBlock {
                slot: "title".to_owned(),
                text: "hello".to_owned(),
            }],
            target_slots: vec!["title_zh".to_owned()],
        }])
        .expect_err("unsupported kind should fail");
        assert_eq!(err.code(), "bad_request");
    }

    #[test]
    fn normalize_submit_payload_rejects_wait_mode_batch_items() {
        let err = normalize_submit_payload(
            "wait",
            TranslationSubmitRequest {
                mode: "wait".to_owned(),
                item: None,
                items: Some(vec![sample_release_item("123")]),
            },
        )
        .expect_err("wait mode batch payload should fail");

        assert_eq!(err.code(), "bad_request");
        assert_eq!(
            err.to_string(),
            "wait/stream supports only a single item; batch requests must use async"
        );
    }

    #[test]
    fn normalize_submit_payload_accepts_single_item_contract() {
        let item = sample_release_item("123");
        let normalized = normalize_submit_payload(
            "stream",
            TranslationSubmitRequest {
                mode: "stream".to_owned(),
                item: Some(item.clone()),
                items: None,
            },
        )
        .expect("single item should normalize");

        match normalized {
            NormalizedTranslationSubmit::Single(normalized_item) => {
                assert_eq!(normalized_item.entity_id, item.entity_id);
                assert_eq!(normalized_item.producer_ref, item.producer_ref);
            }
            NormalizedTranslationSubmit::Batch(_) => panic!("expected single-item payload"),
        }
    }

    #[tokio::test]
    async fn created_request_public_response_always_includes_result() {
        let item = sample_release_item("response-item");
        let created = CreatedTranslationRequest {
            request_id: "req-1".to_owned(),
            status: "queued".to_owned(),
            result: queued_request_result(&item, Some("work-1".to_owned())),
        };

        let response = created.to_public_response();
        assert_eq!(response.request_id, "req-1");
        assert_eq!(response.status, "queued");
        assert_eq!(response.result.entity_id, item.entity_id);
        assert_eq!(response.result.producer_ref, item.producer_ref);
    }

    #[tokio::test]
    async fn create_translation_request_reuses_running_work_item_status() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("123");

        let first = create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("first request created");
        let work_item_id = first
            .result
            .work_item_id
            .clone()
            .expect("work item id for first request");
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            UPDATE translation_work_items
            SET status = 'running', batch_id = 'batch-running-1', started_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(work_item_id.as_str())
        .execute(&pool)
        .await
        .expect("mark work item running");

        let second = create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("second request created");
        assert_eq!(second.status, "running");
        assert_eq!(second.result.status, "running");
        assert_eq!(second.result.batch_id.as_deref(), Some("batch-running-1"));

        let stored_status: String =
            sqlx::query_scalar("SELECT status FROM translation_requests WHERE id = ?")
                .bind(second.request_id.as_str())
                .fetch_one(&pool)
                .await
                .expect("load stored request status");
        assert_eq!(stored_status, "running");

        let detail =
            load_translation_request_detail(state.as_ref(), "1", second.request_id.as_str())
                .await
                .expect("load request detail");

        assert_eq!(
            detail.request.to_admin_request_list_item().status,
            "running"
        );
        assert_eq!(detail.result.status, "running");
        assert_eq!(detail.result.batch_id.as_deref(), Some("batch-running-1"));
        assert_eq!(derive_request_stream_phase(&detail), "running");
    }

    #[tokio::test]
    async fn create_translation_request_dedupes_work_items() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("123");

        let first = create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("first request created");
        let second = create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("second request created");

        let work_items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translation_work_items")
            .fetch_one(&pool)
            .await
            .expect("count work items");
        let requests: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translation_requests")
            .fetch_one(&pool)
            .await
            .expect("count requests");
        let attached_requests: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM translation_requests WHERE work_item_id IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("count attached requests");
        let distinct_work_item_ids: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT work_item_id) FROM translation_requests WHERE id IN (?, ?)",
        )
        .bind(first.request_id.as_str())
        .bind(second.request_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("count distinct work item ids");

        assert_eq!(requests, 2);
        assert_eq!(work_items, 1);
        assert_eq!(attached_requests, 2);
        assert_eq!(distinct_work_item_ids, 1);
    }

    #[tokio::test]
    async fn scheduler_finishes_disabled_requests_without_job_tasks() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let mut item = sample_release_item("123");
        item.max_wait_ms = 0;

        let created = create_translation_request(state.as_ref(), "1", "wait", &item)
            .await
            .expect("request created");

        run_translation_scheduler_once(
            state.as_ref(),
            TranslationWorkerProfile {
                worker_slot: 1,
                worker_kind: "general",
            },
        )
        .await
        .expect("scheduler tick");

        let request_status: String =
            sqlx::query_scalar("SELECT status FROM translation_requests WHERE id = ?")
                .bind(created.request_id.as_str())
                .fetch_one(&pool)
                .await
                .expect("load request status");
        let result_status: String = sqlx::query_scalar(
            "SELECT COALESCE(result_status, '') FROM translation_requests WHERE id = ? LIMIT 1",
        )
        .bind(created.request_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load request result status");
        let task_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_tasks")
            .fetch_one(&pool)
            .await
            .expect("count job tasks");

        assert_eq!(request_status, "completed");
        assert_eq!(result_status, "disabled");
        assert_eq!(task_count, 0);
    }

    #[tokio::test]
    async fn refresh_requests_marks_terminal_errors_as_failed() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("123");
        let created = create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("request created");
        let now = Utc::now().to_rfc3339();
        let mut tx = pool.begin().await.expect("begin tx");
        sqlx::query(
            r#"
            UPDATE translation_requests
            SET result_status = 'error', error_text = 'boom', updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(now.as_str())
        .bind(created.request_id.as_str())
        .execute(&mut *tx)
        .await
        .expect("mark request failed");
        refresh_requests_after_completion(&mut tx, vec![created.request_id.clone()], now.as_str())
            .await
            .expect("refresh request status");
        tx.commit().await.expect("commit tx");

        let request_status: String =
            sqlx::query_scalar("SELECT status FROM translation_requests WHERE id = ?")
                .bind(created.request_id.as_str())
                .fetch_one(&pool)
                .await
                .expect("load request status");

        assert_eq!(request_status, "failed");
    }

    #[tokio::test]
    async fn create_translation_request_with_origin_persists_request_origin() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        reset_translation_worker_runtime_for_tests().await;
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("123");

        let created =
            create_translation_request_with_origin(state.as_ref(), "1", "async", &item, "system")
                .await
                .expect("system request created");

        let request_origin: String =
            sqlx::query_scalar("SELECT request_origin FROM translation_requests WHERE id = ?")
                .bind(created.request_id.as_str())
                .fetch_one(&pool)
                .await
                .expect("load request origin");

        assert_eq!(request_origin, "system");
    }

    #[tokio::test]
    async fn load_translation_request_detail_includes_request_origin() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        reset_translation_worker_runtime_for_tests().await;
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("detail-origin");

        let created =
            create_translation_request_with_origin(state.as_ref(), "1", "async", &item, "system")
                .await
                .expect("system request created");

        let detail =
            load_translation_request_detail(state.as_ref(), "1", created.request_id.as_str())
                .await
                .expect("load request detail");

        assert_eq!(detail.request.request_origin, "system");
        assert_eq!(detail.result.producer_ref, item.producer_ref);
    }

    #[tokio::test]
    async fn scheduler_clears_worker_error_after_successful_idle_scan() {
        let pool = setup_pool().await;
        let state = setup_state(pool);
        reset_translation_worker_runtime_for_tests().await;
        let worker = TranslationWorkerProfile {
            worker_slot: TRANSLATION_USER_DEDICATED_WORKER_SLOT,
            worker_kind: "user_dedicated",
        };

        update_translation_worker_runtime(
            worker,
            "error",
            None,
            0,
            0,
            None,
            Some("temporary failure"),
        )
        .await;

        run_translation_scheduler_once(state.as_ref(), worker)
            .await
            .expect("idle scan succeeds");

        let workers = translation_worker_runtime_statuses().await;
        let dedicated = workers
            .iter()
            .find(|entry| entry.worker_slot == TRANSLATION_USER_DEDICATED_WORKER_SLOT)
            .expect("dedicated worker exists");

        assert_eq!(dedicated.status, "idle");
        assert_eq!(dedicated.error_text, None);
    }

    #[tokio::test]
    async fn claim_next_batch_routes_user_dedicated_worker_only_to_user_requests() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        reset_translation_worker_runtime_for_tests().await;
        seed_user(&pool, 1, "octo").await;

        let mut system_item = sample_release_item("111");
        system_item.max_wait_ms = 0;
        let mut user_item = sample_release_item("222");
        user_item.max_wait_ms = 0;

        create_translation_request_with_origin(
            state.as_ref(),
            "1",
            "async",
            &system_item,
            "system",
        )
        .await
        .expect("system request created");
        create_translation_request_with_origin(state.as_ref(), "1", "async", &user_item, "user")
            .await
            .expect("user request created");

        let dedicated_batch = claim_next_batch(
            state.as_ref(),
            TranslationWorkerProfile {
                worker_slot: TRANSLATION_USER_DEDICATED_WORKER_SLOT,
                worker_kind: "user_dedicated",
            },
        )
        .await
        .expect("claim user batch")
        .expect("user batch exists");

        assert_eq!(
            dedicated_batch.worker_slot,
            TRANSLATION_USER_DEDICATED_WORKER_SLOT
        );
        assert_eq!(dedicated_batch.request_count, 1);
        assert_eq!(dedicated_batch.items.len(), 1);
        assert_eq!(dedicated_batch.items[0].entity_id, "222");

        let general_batch = claim_next_batch(
            state.as_ref(),
            TranslationWorkerProfile {
                worker_slot: 1,
                worker_kind: "general",
            },
        )
        .await
        .expect("claim general batch")
        .expect("general batch exists");

        assert_eq!(general_batch.worker_slot, 1);
        assert_eq!(general_batch.items[0].entity_id, "111");
    }

    #[tokio::test]
    async fn scheduler_persists_batch_worker_slot_and_request_count() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        reset_translation_worker_runtime_for_tests().await;
        seed_user(&pool, 1, "octo").await;

        let mut item = sample_release_item("123");
        item.max_wait_ms = 0;
        create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("request created");

        run_translation_scheduler_once(
            state.as_ref(),
            TranslationWorkerProfile {
                worker_slot: 2,
                worker_kind: "general",
            },
        )
        .await
        .expect("scheduler tick");

        let row = sqlx::query_as::<_, AdminTranslationBatchListItem>(
            r#"
            SELECT id, status, trigger_reason, worker_slot, request_count, item_count, estimated_input_tokens,
                   created_at, started_at, finished_at, updated_at
            FROM translation_batches
            LIMIT 1
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("load batch");

        assert_eq!(row.worker_slot, 2);
        assert_eq!(row.request_count, 1);
    }

    #[tokio::test]
    async fn translation_worker_runtime_statuses_report_updates() {
        reset_translation_worker_runtime_for_tests().await;
        update_translation_worker_runtime(
            TranslationWorkerProfile {
                worker_slot: 4,
                worker_kind: "user_dedicated",
            },
            "running",
            Some("batch-1"),
            2,
            3,
            Some("deadline"),
            None,
        )
        .await;

        let workers = translation_worker_runtime_statuses().await;
        let dedicated = workers
            .iter()
            .find(|worker| worker.worker_slot == 4)
            .expect("dedicated worker exists");

        assert_eq!(workers.len(), TRANSLATION_WORKER_CONCURRENCY as usize);
        assert_eq!(dedicated.status, "running");
        assert_eq!(dedicated.current_batch_id.as_deref(), Some("batch-1"));
        assert_eq!(dedicated.request_count, 2);
        assert_eq!(dedicated.work_item_count, 3);
    }

    #[tokio::test]
    async fn admin_request_ordering_prioritizes_active_then_recent_updates() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;

        let completed = create_translation_request(
            state.as_ref(),
            "1",
            "async",
            &sample_release_item("completed"),
        )
        .await
        .expect("completed request created");
        let queued = create_translation_request(
            state.as_ref(),
            "1",
            "async",
            &sample_release_item("queued"),
        )
        .await
        .expect("queued request created");
        let running = create_translation_request(
            state.as_ref(),
            "1",
            "async",
            &sample_release_item("running"),
        )
        .await
        .expect("running request created");
        let failed = create_translation_request(
            state.as_ref(),
            "1",
            "async",
            &sample_release_item("failed"),
        )
        .await
        .expect("failed request created");

        for (created, status, result_status, updated_at) in [
            (
                &completed,
                "completed",
                Some("ready"),
                "2026-03-07T00:00:03Z",
            ),
            (&queued, "queued", None, "2026-03-07T00:00:01Z"),
            (&running, "running", None, "2026-03-07T00:00:04Z"),
            (&failed, "failed", Some("error"), "2026-03-07T00:00:02Z"),
        ] {
            let started_at = if status == "queued" {
                None
            } else {
                Some(updated_at)
            };
            let finished_at = if matches!(status, "completed" | "failed") {
                Some(updated_at)
            } else {
                None
            };
            sqlx::query(
                r#"
                UPDATE translation_requests
                SET status = ?,
                    result_status = ?,
                    started_at = ?,
                    finished_at = ?,
                    updated_at = ?
                WHERE id = ?
                "#,
            )
            .bind(status)
            .bind(result_status)
            .bind(started_at)
            .bind(finished_at)
            .bind(updated_at)
            .bind(created.request_id.as_str())
            .execute(&pool)
            .await
            .expect("update request ordering fixture");
        }

        let request_rows_sql = format!(
            r#"{}
            ORDER BY CASE WHEN r.status IN ('queued', 'running') THEN 0 ELSE 1 END ASC,
                     r.updated_at DESC,
                     r.id DESC
            "#,
            request_row_select_sql(),
        );
        let rows = sqlx::query_as::<_, RequestRow>(&request_rows_sql)
            .fetch_all(&state.pool)
            .await
            .expect("load ordered requests");

        let ids = rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                running.request_id.as_str(),
                queued.request_id.as_str(),
                completed.request_id.as_str(),
                failed.request_id.as_str(),
            ],
        );
    }

    #[test]
    fn stream_event_omits_result_until_terminal_phase() {
        let detail = LoadedRequestDetail {
            request: RequestRow {
                id: "req-1".to_owned(),
                mode: "stream".to_owned(),
                source: "feed.auto_translate".to_owned(),
                request_origin: "user".to_owned(),
                requested_by: Some("1".to_owned()),
                scope_user_id: "1".to_owned(),
                producer_ref: "feed.auto_translate:release:123".to_owned(),
                kind: "release_summary".to_owned(),
                variant: "feed_card".to_owned(),
                entity_id: "123".to_owned(),
                target_lang: "zh-CN".to_owned(),
                max_wait_ms: 1500,
                work_item_id: Some("work-1".to_owned()),
                status: "running".to_owned(),
                result_status: None,
                title_zh: None,
                summary_md: None,
                body_md: None,
                error_text: None,
                created_at: "2026-03-07T00:00:00Z".to_owned(),
                started_at: Some("2026-03-07T00:00:01Z".to_owned()),
                finished_at: None,
                updated_at: "2026-03-07T00:00:01Z".to_owned(),
                work_item_status: Some("running".to_owned()),
                batch_id: Some("batch-1".to_owned()),
            },
            result: TranslationResultItem {
                producer_ref: "feed.auto_translate:release:123".to_owned(),
                entity_id: "123".to_owned(),
                kind: "release_summary".to_owned(),
                variant: "feed_card".to_owned(),
                status: "queued".to_owned(),
                title_zh: None,
                summary_md: None,
                body_md: None,
                error: None,
                work_item_id: Some("work-1".to_owned()),
                batch_id: Some("batch-1".to_owned()),
            },
        };

        let phase = derive_request_stream_phase(&detail);
        let event = TranslationRequestStreamEvent {
            event: phase.clone(),
            request_id: detail.request.id.clone(),
            status: detail.request.effective_status().to_owned(),
            batch_id: detail.result.batch_id.clone(),
            result: matches!(phase.as_str(), "completed" | "failed").then(|| detail.result.clone()),
            error: None,
        };

        assert_eq!(phase, "running");
        assert_eq!(event.status, "running");
        assert!(event.result.is_none());
        assert_eq!(event.batch_id.as_deref(), Some("batch-1"));
    }

    #[test]
    fn derive_request_stream_phase_uses_running_request_status() {
        let detail = LoadedRequestDetail {
            request: RequestRow {
                id: "req-1".to_owned(),
                mode: "stream".to_owned(),
                source: "feed.auto_translate".to_owned(),
                request_origin: "user".to_owned(),
                requested_by: Some("1".to_owned()),
                scope_user_id: "1".to_owned(),
                producer_ref: "feed.auto_translate:release:123".to_owned(),
                kind: "release_summary".to_owned(),
                variant: "feed_card".to_owned(),
                entity_id: "123".to_owned(),
                target_lang: "zh-CN".to_owned(),
                max_wait_ms: 1500,
                work_item_id: Some("work-1".to_owned()),
                status: "running".to_owned(),
                result_status: None,
                title_zh: None,
                summary_md: None,
                body_md: None,
                error_text: None,
                created_at: "2026-03-07T00:00:00Z".to_owned(),
                started_at: Some("2026-03-07T00:00:01Z".to_owned()),
                finished_at: None,
                updated_at: "2026-03-07T00:00:01Z".to_owned(),
                work_item_status: Some("running".to_owned()),
                batch_id: Some("batch-1".to_owned()),
            },
            result: TranslationResultItem {
                producer_ref: "feed.auto_translate:release:123".to_owned(),
                entity_id: "123".to_owned(),
                kind: "release_summary".to_owned(),
                variant: "feed_card".to_owned(),
                status: "queued".to_owned(),
                title_zh: None,
                summary_md: None,
                body_md: None,
                error: None,
                work_item_id: Some("work-1".to_owned()),
                batch_id: Some("batch-1".to_owned()),
            },
        };

        assert_eq!(derive_request_stream_phase(&detail), "running");
    }

    #[tokio::test]
    async fn load_translation_request_detail_returns_single_result() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("request-single");

        let created = create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("request created");

        let detail =
            load_translation_request_detail(state.as_ref(), "1", created.request_id.as_str())
                .await
                .expect("load request detail");

        assert_eq!(detail.request.id, created.request_id);
        assert_eq!(detail.result.producer_ref, item.producer_ref);
        assert_eq!(detail.result.entity_id, item.entity_id);
        assert_eq!(detail.result.kind, item.kind);
        assert_eq!(detail.result.variant, item.variant);
        assert_eq!(detail.result.status, "queued");
    }

    #[tokio::test]
    async fn load_last_batch_finished_at_returns_none_for_empty_tables() {
        let pool = setup_pool().await;
        let state = setup_state(pool);

        let last_finished = load_last_batch_finished_at(state.as_ref())
            .await
            .expect("load last finished batch time");

        assert_eq!(last_finished, None);
    }

    #[test]
    fn derive_request_source_falls_back_to_kind_mapping() {
        let feed_item = TranslationRequestItemInput {
            producer_ref: "123".to_owned(),
            kind: "release_summary".to_owned(),
            variant: "feed_card".to_owned(),
            entity_id: "123".to_owned(),
            target_lang: "zh-CN".to_owned(),
            max_wait_ms: 1000,
            source_blocks: vec![TranslationSourceBlock {
                slot: "title".to_owned(),
                text: "hello".to_owned(),
            }],
            target_slots: vec!["title_zh".to_owned()],
        };
        let detail_item = TranslationRequestItemInput {
            producer_ref: "456".to_owned(),
            kind: "release_detail".to_owned(),
            variant: "detail_card".to_owned(),
            entity_id: "456".to_owned(),
            target_lang: "zh-CN".to_owned(),
            max_wait_ms: 1000,
            source_blocks: vec![TranslationSourceBlock {
                slot: "title".to_owned(),
                text: "hello".to_owned(),
            }],
            target_slots: vec!["title_zh".to_owned()],
        };

        assert_eq!(derive_request_source(&[feed_item]), "feed.auto_translate");
        assert_eq!(derive_request_source(&[detail_item]), "release_detail");
    }

    fn sample_release_item(entity_id: &str) -> TranslationRequestItemInput {
        TranslationRequestItemInput {
            producer_ref: format!("feed.auto_translate:release:{entity_id}"),
            kind: "release_summary".to_owned(),
            variant: "feed_card".to_owned(),
            entity_id: entity_id.to_owned(),
            target_lang: "zh-CN".to_owned(),
            max_wait_ms: 1500,
            source_blocks: vec![
                TranslationSourceBlock {
                    slot: "title".to_owned(),
                    text: format!("Release {entity_id}"),
                },
                TranslationSourceBlock {
                    slot: "excerpt".to_owned(),
                    text: "- change A
- change B"
                        .to_owned(),
                },
            ],
            target_slots: vec!["title_zh".to_owned(), "summary_md".to_owned()],
        }
    }

    async fn setup_pool() -> SqlitePool {
        let database_path = std::env::temp_dir().join(format!(
            "octo-rill-test-{}.db",
            crate::local_id::generate_local_id(),
        ));
        let database_url = format!("sqlite://{}?mode=rwc", database_path.to_string_lossy());
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(database_url.as_str())
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
            task_log_dir: std::env::temp_dir().join("octo-rill-task-logs-translation-tests"),
            job_worker_concurrency: 2,
            encryption_key: encryption_key.clone(),
            github: GitHubOAuthConfig {
                client_id: "test-client-id".to_owned(),
                client_secret: "test-client-secret".to_owned(),
                redirect_url: Url::parse("http://127.0.0.1:58090/auth/callback")
                    .expect("parse github redirect"),
            },
            ai: None,
            ai_model_context_limit: None,
            ai_daily_at_local: None,
        };
        let oauth = build_oauth_client(&config).expect("build oauth client");
        Arc::new(AppState {
            config,
            pool,
            http: reqwest::Client::new(),
            oauth,
            encryption_key,
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
        .bind(30_000_000_i64 + id)
        .bind(login)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .expect("seed user");
    }
}
