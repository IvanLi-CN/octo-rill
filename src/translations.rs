use std::{
    collections::{BTreeMap, HashMap, HashSet},
    convert::Infallible,
    sync::Arc,
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
    pub items: Vec<TranslationRequestItemInput>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<TranslationResultItem>>,
}

#[derive(Debug, Serialize)]
struct TranslationRequestStreamEvent {
    event: String,
    request_id: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    batch_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    items: Option<Vec<TranslationResultItem>>,
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
    pub requested_by: Option<i64>,
    pub scope_user_id: i64,
    pub item_count: i64,
    pub completed_item_count: i64,
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
    pub items: Vec<TranslationResultItem>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AdminTranslationBatchListItem {
    pub id: String,
    pub status: String,
    pub trigger_reason: String,
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

#[allow(dead_code)]
#[derive(Debug, Clone, sqlx::FromRow)]
struct WorkItemRow {
    id: String,
    dedupe_key: String,
    scope_user_id: i64,
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
#[derive(Debug, Clone)]
struct ClaimedBatch {
    id: String,
    partition_key: String,
    target_lang: String,
    protocol_version: String,
    model_profile: String,
    trigger_reason: String,
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

pub fn spawn_translation_scheduler(state: Arc<AppState>) -> tokio::task::AbortHandle {
    tokio::spawn(async move {
        loop {
            if let Err(err) = run_translation_scheduler_once(state.as_ref()).await {
                warn!(?err, "translation scheduler tick failed");
            }
            sleep(TRANSLATION_BATCH_SCAN_INTERVAL).await;
        }
    })
    .abort_handle()
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
    let items = normalize_request_items(&req.items)?;
    let request_id = create_translation_request(state.as_ref(), user_id, mode, &items).await?;

    match mode {
        "async" => {
            let status = current_request_status(state.as_ref(), &request_id).await?;
            Ok(Json(TranslationRequestResponse {
                request_id,
                status,
                items: None,
            })
            .into_response())
        }
        "wait" => {
            let detail = wait_for_request_terminal(state.as_ref(), user_id, &request_id).await?;
            Ok(Json(detail_to_public_response(detail)).into_response())
        }
        "stream" => Ok(stream_translation_request_response(
            state, user_id, request_id,
        )),
        _ => Err(ApiError::bad_request("unsupported mode")),
    }
}

pub async fn get_translation_request(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(request_id): Path<String>,
) -> Result<Json<TranslationRequestResponse>, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    let detail = load_translation_request_detail(state.as_ref(), user_id, &request_id).await?;
    Ok(Json(detail_to_public_response(detail)))
}

pub async fn stream_translation_request(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(request_id): Path<String>,
) -> Result<Response, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    ensure_request_owner(state.as_ref(), user_id, &request_id).await?;
    Ok(stream_translation_request_response(
        state, user_id, request_id,
    ))
}

pub async fn admin_get_translation_status(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<AdminTranslationStatusResponse>, ApiError> {
    let _acting_user_id = api::require_admin_user_id(state.as_ref(), &session).await?;
    let _llm_runtime = ai::llm_scheduler_runtime_status().await;
    let since = (Utc::now() - chrono::Duration::hours(24)).to_rfc3339();

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
    let last_batch_finished_at = sqlx::query_scalar::<_, Option<String>>(
        r#"SELECT finished_at FROM translation_batches WHERE finished_at IS NOT NULL ORDER BY finished_at DESC LIMIT 1"#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::internal)?;

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

    let items = sqlx::query_as::<_, AdminTranslationRequestListItem>(
        r#"
        SELECT id, status, source, requested_by, scope_user_id, item_count, completed_item_count,
               created_at, started_at, finished_at, updated_at
        FROM translation_requests
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
    let request = sqlx::query_as::<_, AdminTranslationRequestListItem>(
        r#"
        SELECT id, status, source, requested_by, scope_user_id, item_count, completed_item_count,
               created_at, started_at, finished_at, updated_at
        FROM translation_requests
        WHERE id = ?
        LIMIT 1
        "#,
    )
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
    let items = load_translation_result_items_by_request(state.as_ref(), &request_id).await?;
    Ok(Json(AdminTranslationRequestDetailResponse {
        request,
        items,
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
        SELECT id, status, trigger_reason, item_count, estimated_input_tokens,
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
    let batch = sqlx::query_as::<_, AdminTranslationBatchListItem>(
        r#"
        SELECT id, status, trigger_reason, item_count, estimated_input_tokens,
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
    user_id: i64,
    mode: &str,
    items: &[TranslationRequestItemInput],
) -> Result<String, ApiError> {
    let now = Utc::now().to_rfc3339();
    let request_id = format!("req_{}", uuid::Uuid::new_v4().simple());
    let source = derive_request_source(items);
    let mut tx = state.pool.begin().await.map_err(ApiError::internal)?;

    sqlx::query(
        r#"
        INSERT INTO translation_requests (
          id, mode, source, requested_by, scope_user_id, status, item_count, completed_item_count,
          created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, 'queued', ?, 0, ?, ?)
        "#,
    )
    .bind(request_id.as_str())
    .bind(mode)
    .bind(source.as_str())
    .bind(user_id)
    .bind(user_id)
    .bind(i64::try_from(items.len()).unwrap_or(i64::MAX))
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let mut completed = 0_i64;
    for item in items {
        if insert_request_item(&mut tx, &request_id, user_id, item, &now).await? {
            completed += 1;
        }
    }

    let status = if completed == i64::try_from(items.len()).unwrap_or(i64::MAX) {
        "completed"
    } else {
        "queued"
    };
    let finished_at = (status == "completed").then_some(now.clone());

    sqlx::query(
        r#"
        UPDATE translation_requests
        SET status = ?, completed_item_count = ?, finished_at = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(status)
    .bind(completed)
    .bind(finished_at.as_deref())
    .bind(now.as_str())
    .bind(request_id.as_str())
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    Ok(request_id)
}

async fn insert_request_item(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: &str,
    user_id: i64,
    item: &TranslationRequestItemInput,
    now: &str,
) -> Result<bool, ApiError> {
    let source_hash = build_source_hash(item);
    let source_blocks_json =
        serde_json::to_string(&item.source_blocks).map_err(ApiError::internal)?;
    let target_slots_json =
        serde_json::to_string(&item.target_slots).map_err(ApiError::internal)?;
    let request_item_id = sqlx::query(
        r#"
        INSERT INTO translation_request_items (
          request_id, producer_ref, kind, variant, entity_id, target_lang, max_wait_ms,
          source_hash, source_blocks_json, target_slots_json, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(request_id)
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
    .map_err(ApiError::internal)?
    .last_insert_rowid();

    if let Some(cached) = load_cached_result(tx, user_id, item, &source_hash).await? {
        apply_request_item_result(tx, request_item_id, None, &cached, now).await?;
        return Ok(true);
    }

    let work_item = load_existing_work_item(tx, user_id, item, &source_hash).await?;
    match work_item {
        Some(existing)
            if existing.result_status.is_some()
                && matches!(existing.status.as_str(), "completed" | "failed") =>
        {
            let result = terminal_result_from_work_row(&existing, item.producer_ref.clone());
            apply_request_item_result(
                tx,
                request_item_id,
                Some(existing.id.as_str()),
                &result,
                now,
            )
            .await?;
            Ok(true)
        }
        Some(existing) => {
            attach_request_item_to_work_item(tx, request_item_id, &existing.id, now).await?;
            Ok(false)
        }
        None => {
            let work_item_id = create_work_item(tx, user_id, item, &source_hash, now).await?;
            attach_request_item_to_work_item(tx, request_item_id, &work_item_id, now).await?;
            Ok(false)
        }
    }
}

async fn load_cached_result(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
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
    user_id: i64,
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
    user_id: i64,
    item: &TranslationRequestItemInput,
    source_hash: &str,
    now: &str,
) -> Result<String, ApiError> {
    let id = format!("work_{}", uuid::Uuid::new_v4().simple());
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

async fn attach_request_item_to_work_item(
    tx: &mut Transaction<'_, Sqlite>,
    request_item_id: i64,
    work_item_id: &str,
    now: &str,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        UPDATE translation_request_items
        SET work_item_id = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(work_item_id)
    .bind(now)
    .bind(request_item_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    sqlx::query(
        r#"
        INSERT OR IGNORE INTO translation_work_watchers (work_item_id, request_item_id, created_at)
        VALUES (?, ?, ?)
        "#,
    )
    .bind(work_item_id)
    .bind(request_item_id)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn apply_request_item_result(
    tx: &mut Transaction<'_, Sqlite>,
    request_item_id: i64,
    work_item_id: Option<&str>,
    result: &TranslationResultItem,
    now: &str,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        UPDATE translation_request_items
        SET work_item_id = COALESCE(?, work_item_id),
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
    .bind(result.status.as_str())
    .bind(result.title_zh.as_deref())
    .bind(result.summary_md.as_deref())
    .bind(result.body_md.as_deref())
    .bind(result.error.as_deref())
    .bind(now)
    .bind(now)
    .bind(request_item_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn run_translation_scheduler_once(state: &AppState) -> Result<()> {
    let Some(batch) = claim_next_batch(state).await? else {
        return Ok(());
    };
    execute_claimed_batch(state, batch).await
}

async fn claim_next_batch(state: &AppState) -> Result<Option<ClaimedBatch>> {
    let first = sqlx::query_as::<_, WorkItemRow>(
        r#"
        SELECT id, dedupe_key, scope_user_id, kind, variant, entity_id, target_lang, protocol_version,
               model_profile, source_hash, source_blocks_json, target_slots_json, token_estimate,
               deadline_at, status, batch_id, result_status, title_zh, summary_md, body_md,
               error_text, cache_hit, created_at, started_at, finished_at, updated_at
        FROM translation_work_items
        WHERE status = 'queued'
        ORDER BY deadline_at ASC, created_at ASC
        LIMIT 1
        "#,
    )
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
    let candidates = sqlx::query_as::<_, WorkItemRow>(
        r#"
        SELECT id, dedupe_key, scope_user_id, kind, variant, entity_id, target_lang, protocol_version,
               model_profile, source_hash, source_blocks_json, target_slots_json, token_estimate,
               deadline_at, status, batch_id, result_status, title_zh, summary_md, body_md,
               error_text, cache_hit, created_at, started_at, finished_at, updated_at
        FROM translation_work_items
        WHERE status = 'queued'
          AND target_lang = ?
          AND protocol_version = ?
          AND model_profile = ?
        ORDER BY created_at ASC, id ASC
        LIMIT 200
        "#,
    )
    .bind(first.target_lang.as_str())
    .bind(first.protocol_version.as_str())
    .bind(first.model_profile.as_str())
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
    let batch_id = format!("batch_{}", uuid::Uuid::new_v4().simple());
    let partition_key = format!(
        "{}:{}:{}",
        first.target_lang, first.protocol_version, first.model_profile
    );
    let now_str = now.to_rfc3339();
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO translation_batches (
          id, partition_key, protocol_version, model_profile, target_lang, trigger_reason,
          item_count, estimated_input_tokens, status, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'queued', ?, ?)
        "#,
    )
    .bind(batch_id.as_str())
    .bind(partition_key.as_str())
    .bind(first.protocol_version.as_str())
    .bind(first.model_profile.as_str())
    .bind(first.target_lang.as_str())
    .bind(trigger_reason.as_str())
    .bind(i64::try_from(selected.len()).unwrap_or(i64::MAX))
    .bind(token_sum)
    .bind(now_str.as_str())
    .bind(now_str.as_str())
    .execute(&mut *tx)
    .await?;

    for (index, item) in selected.iter().enumerate() {
        let producer_count = sqlx::query_scalar::<_, i64>(
            r#"SELECT COUNT(*) FROM translation_work_watchers WHERE work_item_id = ?"#,
        )
        .bind(item.id.as_str())
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query(
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
        .await?;

        sqlx::query(
            r#"
            INSERT INTO translation_batch_items (
              batch_id, work_item_id, item_index, kind, variant, entity_id, producer_count,
              token_estimate, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
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
        partition_key,
        target_lang: first.target_lang,
        protocol_version: first.protocol_version,
        model_profile: first.model_profile,
        trigger_reason,
        estimated_input_tokens: token_sum,
        items: selected,
    }))
}

async fn execute_claimed_batch(state: &AppState, batch: ClaimedBatch) -> Result<()> {
    let now = Utc::now().to_rfc3339();
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
        Ok(results) => finalize_batch_success(state, &batch, results).await,
        Err(err) => finalize_batch_failure(state, &batch, err.into()).await,
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

    let mut release_groups: BTreeMap<i64, Vec<&WorkItemRow>> = BTreeMap::new();
    let mut detail_groups: BTreeMap<i64, Vec<&WorkItemRow>> = BTreeMap::new();
    let mut notification_groups: BTreeMap<i64, Vec<&WorkItemRow>> = BTreeMap::new();

    for item in &batch.items {
        match item.kind.as_str() {
            "release_summary" => {
                release_groups
                    .entry(item.scope_user_id)
                    .or_default()
                    .push(item);
            }
            "release_detail" => {
                detail_groups
                    .entry(item.scope_user_id)
                    .or_default()
                    .push(item);
            }
            "notification" => {
                notification_groups
                    .entry(item.scope_user_id)
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
        let response = api::translate_releases_batch_for_user(state, user_id, &release_ids).await?;
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
                item.scope_user_id,
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
                item.scope_user_id,
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
    let mut request_ids = HashSet::new();
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

        let watchers = sqlx::query(
            r#"
            SELECT w.request_item_id, r.request_id, r.producer_ref, r.entity_id, r.kind, r.variant
            FROM translation_work_watchers w
            JOIN translation_request_items r ON r.id = w.request_item_id
            WHERE w.work_item_id = ?
            "#,
        )
        .bind(result.work_item_id.as_str())
        .fetch_all(&mut *tx)
        .await?;
        for watcher in watchers {
            let request_item_id: i64 = watcher.try_get("request_item_id")?;
            let request_id: String = watcher.try_get("request_id")?;
            let producer_ref: String = watcher.try_get("producer_ref")?;
            let entity_id: String = watcher.try_get("entity_id")?;
            let kind: String = watcher.try_get("kind")?;
            let variant: String = watcher.try_get("variant")?;
            let item_result = TranslationResultItem {
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
            apply_request_item_result(
                &mut tx,
                request_item_id,
                Some(result.work_item_id.as_str()),
                &item_result,
                now.as_str(),
            )
            .await?;
            request_ids.insert(request_id);
        }

        if result.result_status == "ready"
            && let Some(work_item) = batch
                .items
                .iter()
                .find(|item| item.id == result.work_item_id)
        {
            upsert_cached_translation(
                &mut tx,
                work_item.scope_user_id,
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

    refresh_requests_after_completion(&mut tx, request_ids.into_iter().collect(), now.as_str())
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
    let mut request_ids = HashSet::new();
    for item in &batch.items {
        let result = TranslationResultItem {
            producer_ref: String::new(),
            entity_id: item.entity_id.clone(),
            kind: item.kind.clone(),
            variant: item.variant.clone(),
            status: "error".to_owned(),
            title_zh: None,
            summary_md: None,
            body_md: None,
            error: Some(message.clone()),
            work_item_id: Some(item.id.clone()),
            batch_id: Some(batch.id.clone()),
        };
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
        let watchers = sqlx::query(
            r#"
            SELECT w.request_item_id, r.request_id, r.producer_ref, r.entity_id, r.kind, r.variant
            FROM translation_work_watchers w
            JOIN translation_request_items r ON r.id = w.request_item_id
            WHERE w.work_item_id = ?
            "#,
        )
        .bind(item.id.as_str())
        .fetch_all(&mut *tx)
        .await?;
        for watcher in watchers {
            let request_item_id: i64 = watcher.try_get("request_item_id")?;
            let request_id: String = watcher.try_get("request_id")?;
            let producer_ref: String = watcher.try_get("producer_ref")?;
            let entity_id: String = watcher.try_get("entity_id")?;
            let kind: String = watcher.try_get("kind")?;
            let variant: String = watcher.try_get("variant")?;
            let mut request_result = result.clone();
            request_result.producer_ref = producer_ref;
            request_result.entity_id = entity_id;
            request_result.kind = kind;
            request_result.variant = variant;
            apply_request_item_result(
                &mut tx,
                request_item_id,
                Some(item.id.as_str()),
                &request_result,
                now.as_str(),
            )
            .await?;
            request_ids.insert(request_id);
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
    refresh_requests_after_completion(&mut tx, request_ids.into_iter().collect(), now.as_str())
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn refresh_requests_after_completion(
    tx: &mut Transaction<'_, Sqlite>,
    request_ids: Vec<String>,
    now: &str,
) -> Result<()> {
    for request_id in request_ids {
        let counts = sqlx::query(
            r#"
            SELECT item_count, completed_item_count FROM translation_requests WHERE id = ? LIMIT 1
            "#,
        )
        .bind(request_id.as_str())
        .fetch_optional(&mut **tx)
        .await?;
        if counts.is_none() {
            continue;
        }
        let total = sqlx::query_scalar::<_, i64>(
            r#"SELECT COUNT(*) FROM translation_request_items WHERE request_id = ?"#,
        )
        .bind(request_id.as_str())
        .fetch_one(&mut **tx)
        .await?;
        let completed = sqlx::query_scalar::<_, i64>(
            r#"SELECT COUNT(*) FROM translation_request_items WHERE request_id = ? AND result_status IS NOT NULL"#,
        )
        .bind(request_id.as_str())
        .fetch_one(&mut **tx)
        .await?;
        let status = if completed >= total {
            "completed"
        } else {
            "running"
        };
        sqlx::query(
            r#"
            UPDATE translation_requests
            SET status = ?, completed_item_count = ?, finished_at = CASE WHEN ? = 'completed' THEN ? ELSE finished_at END, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(status)
        .bind(completed)
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
    query.push(
        r#"
        WHERE status = 'queued' AND id IN (
          SELECT DISTINCT r.request_id
          FROM translation_request_items r
          WHERE r.work_item_id IN (
        "#,
    );
    {
        let mut separated = query.separated(", ");
        for id in work_item_ids {
            separated.push_bind(id);
        }
    }
    query.push(") )");
    query.build().execute(&state.pool).await?;
    Ok(())
}

fn stream_translation_request_response(
    state: Arc<AppState>,
    user_id: i64,
    request_id: String,
) -> Response {
    let stream = async_stream::stream! {
        let mut last_phase = String::new();
        loop {
            let snapshot = load_translation_request_detail(state.as_ref(), user_id, &request_id).await;
            match snapshot {
                Ok(detail) => {
                    let phase = derive_request_stream_phase(&detail);
                    if phase != last_phase {
                        let event = TranslationRequestStreamEvent {
                            event: phase.clone(),
                            request_id: request_id.clone(),
                            status: detail.request.status.clone(),
                            batch_ids: Some(detail.items.iter().filter_map(|item| item.batch_id.clone()).collect::<HashSet<_>>().into_iter().collect()),
                            items: if matches!(phase.as_str(), "completed" | "failed") { Some(detail.items.clone()) } else { None },
                            error: if phase == "failed" {
                                detail.items.iter().find_map(|item| item.error.clone())
                            } else { None },
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
                        batch_ids: None,
                        items: None,
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
    request: AdminTranslationRequestListItem,
    items: Vec<TranslationResultItem>,
}

async fn wait_for_request_terminal(
    state: &AppState,
    user_id: i64,
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

async fn current_request_status(state: &AppState, request_id: &str) -> Result<String, ApiError> {
    sqlx::query_scalar::<_, String>(
        r#"SELECT status FROM translation_requests WHERE id = ? LIMIT 1"#,
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
    })
}

async fn ensure_request_owner(
    state: &AppState,
    user_id: i64,
    request_id: &str,
) -> Result<(), ApiError> {
    let owner = sqlx::query_scalar::<_, i64>(
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
    user_id: i64,
    request_id: &str,
) -> Result<LoadedRequestDetail, ApiError> {
    let request = sqlx::query_as::<_, AdminTranslationRequestListItem>(
        r#"
        SELECT id, status, source, requested_by, scope_user_id, item_count, completed_item_count,
               created_at, started_at, finished_at, updated_at
        FROM translation_requests
        WHERE id = ? AND scope_user_id = ?
        LIMIT 1
        "#,
    )
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
    let items = load_translation_result_items_by_request(state, request_id).await?;
    Ok(LoadedRequestDetail { request, items })
}

async fn load_translation_result_items_by_request(
    state: &AppState,
    request_id: &str,
) -> Result<Vec<TranslationResultItem>, ApiError> {
    let rows = sqlx::query(
        r#"
        SELECT producer_ref, entity_id, kind, variant,
               COALESCE(result_status, 'queued') AS result_status,
               title_zh, summary_md, body_md, error_text, work_item_id,
               (SELECT batch_id FROM translation_work_items w WHERE w.id = r.work_item_id) AS batch_id
        FROM translation_request_items r
        WHERE request_id = ?
        ORDER BY id ASC
        "#,
    )
    .bind(request_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(rows
        .into_iter()
        .map(|row| TranslationResultItem {
            producer_ref: row.get("producer_ref"),
            entity_id: row.get("entity_id"),
            kind: row.get("kind"),
            variant: row.get("variant"),
            status: row.get("result_status"),
            title_zh: row.get("title_zh"),
            summary_md: row.get("summary_md"),
            body_md: row.get("body_md"),
            error: row.get("error_text"),
            work_item_id: row.get("work_item_id"),
            batch_id: row.get("batch_id"),
        })
        .collect())
}

async fn load_translation_result_items_by_batch(
    state: &AppState,
    batch_id: &str,
) -> Result<Vec<TranslationResultItem>, ApiError> {
    let rows = sqlx::query(
        r#"
        SELECT w.id AS work_item_id, b.work_item_id AS batch_work_item_id, b.entity_id, b.kind, b.variant,
               COALESCE(b.result_status, w.result_status, 'queued') AS result_status,
               w.title_zh, w.summary_md, w.body_md, COALESCE(b.error_text, w.error_text) AS error_text,
               MIN(r.producer_ref) AS producer_ref
        FROM translation_batch_items b
        JOIN translation_work_items w ON w.id = b.work_item_id
        LEFT JOIN translation_work_watchers watcher ON watcher.work_item_id = w.id
        LEFT JOIN translation_request_items r ON r.id = watcher.request_item_id
        WHERE b.batch_id = ?
        GROUP BY w.id, b.work_item_id, b.entity_id, b.kind, b.variant, b.result_status, w.result_status,
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
    TranslationRequestResponse {
        request_id: detail.request.id,
        status: detail.request.status,
        items: Some(detail.items),
    }
}

fn derive_request_stream_phase(detail: &LoadedRequestDetail) -> String {
    if detail.request.status == "completed" {
        return "completed".to_owned();
    }
    if detail.request.status == "failed" {
        return "failed".to_owned();
    }
    if detail.items.iter().any(|item| item.batch_id.is_some()) {
        if detail.items.iter().any(|item| item.status == "queued") {
            return "batched".to_owned();
        }
        return "running".to_owned();
    }
    "queued".to_owned()
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

fn build_dedupe_key(user_id: i64, item: &TranslationRequestItemInput, source_hash: &str) -> String {
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
    items
        .first()
        .map(|item| {
            item.producer_ref
                .split(':')
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("translation")
                .to_owned()
        })
        .unwrap_or_else(|| "translation".to_owned())
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
    user_id: i64,
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
        INSERT INTO ai_translations (user_id, entity_type, entity_id, lang, source_hash, title, summary, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id, entity_type, entity_id, lang)
        DO UPDATE SET source_hash = excluded.source_hash,
                      title = excluded.title,
                      summary = excluded.summary,
                      updated_at = excluded.updated_at
        "#,
    )
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

    #[tokio::test]
    async fn create_translation_request_dedupes_work_items() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("123");

        create_translation_request(state.as_ref(), 1, "async", std::slice::from_ref(&item))
            .await
            .expect("first request created");
        create_translation_request(state.as_ref(), 1, "async", std::slice::from_ref(&item))
            .await
            .expect("second request created");

        let work_items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translation_work_items")
            .fetch_one(&pool)
            .await
            .expect("count work items");
        let watchers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translation_work_watchers")
            .fetch_one(&pool)
            .await
            .expect("count watchers");
        let requests: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translation_requests")
            .fetch_one(&pool)
            .await
            .expect("count requests");

        assert_eq!(requests, 2);
        assert_eq!(work_items, 1);
        assert_eq!(watchers, 2);
    }

    #[tokio::test]
    async fn scheduler_finishes_disabled_requests_without_job_tasks() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let mut item = sample_release_item("123");
        item.max_wait_ms = 0;

        let request_id =
            create_translation_request(state.as_ref(), 1, "wait", std::slice::from_ref(&item))
                .await
                .expect("request created");

        run_translation_scheduler_once(state.as_ref())
            .await
            .expect("scheduler tick");

        let request_status: String =
            sqlx::query_scalar("SELECT status FROM translation_requests WHERE id = ?")
                .bind(request_id.as_str())
                .fetch_one(&pool)
                .await
                .expect("load request status");
        let item_status: String = sqlx::query_scalar(
            "SELECT COALESCE(result_status, '') FROM translation_request_items WHERE request_id = ? LIMIT 1",
        )
        .bind(request_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load item status");
        let task_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_tasks")
            .fetch_one(&pool)
            .await
            .expect("count job tasks");

        assert_eq!(request_status, "completed");
        assert_eq!(item_status, "disabled");
        assert_eq!(task_count, 0);
    }

    fn sample_release_item(entity_id: &str) -> TranslationRequestItemInput {
        TranslationRequestItemInput {
            producer_ref: entity_id.to_owned(),
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
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
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
        .bind(id)
        .bind(30_000_000_i64 + id)
        .bind(login)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .expect("seed user");
    }
}
