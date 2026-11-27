//! Wire types for the admin translation read surfaces.
//!
//! The PostgreSQL admin handlers and the retiring SQLite admin handlers expose
//! the same request/batch/attempt shapes. Keeping them here means both read
//! models serialize one definition instead of two drifting copies.

use serde::{Deserialize, Serialize};

use crate::api::AdminBusinessOutcome;
use crate::translation_error::classify_translation_error;
use crate::translation_types::TranslationResultItem;

#[derive(Debug, Deserialize)]
pub struct AdminTranslationListQuery {
    pub status: Option<String>,
    pub task_group: Option<String>,
    pub discovered_from: Option<String>,
    pub discovered_before: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct AdminTranslationAttemptListQuery {
    pub entity_id: Option<String>,
    pub request_id: Option<String>,
    pub work_item_id: Option<String>,
    pub kind: Option<String>,
    pub variant: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AdminTranslationStatusResponse {
    pub scheduler_enabled: bool,
    pub llm_enabled: bool,
    pub scan_interval_ms: i64,
    pub batch_token_threshold: i64,
    pub ai_model_context_limit: Option<i64>,
    pub effective_model_input_limit: i64,
    pub effective_model_input_limit_source: String,
    pub general_worker_concurrency: i64,
    pub dedicated_worker_concurrency: i64,
    pub worker_concurrency: i64,
    pub target_general_worker_concurrency: i64,
    pub target_dedicated_worker_concurrency: i64,
    pub target_worker_concurrency: i64,
    pub idle_workers: i64,
    pub busy_workers: i64,
    pub workers: Vec<AdminTranslationWorkerStatus>,
    pub queued_requests: i64,
    pub queued_work_items: i64,
    pub running_batches: i64,
    pub requests_24h: i64,
    pub completed_batches_24h: i64,
    pub clean_completed_batches_24h: i64,
    pub completed_with_issues_batches_24h: i64,
    pub failed_batches_24h: i64,
    pub error_work_items_24h: i64,
    pub missing_work_items_24h: i64,
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
    pub retry_count: i64,
    pub created_at: String,
    pub started_at: Option<String>,
    pub last_attempt_at: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminTranslationAttemptEvent {
    pub id: i64,
    pub work_item_id: String,
    pub request_id: Option<String>,
    pub batch_id: Option<String>,
    pub scope_user_id: String,
    pub kind: String,
    pub variant: String,
    pub entity_id: String,
    pub target_lang: String,
    pub attempt_no: i64,
    pub trigger: String,
    pub event_type: String,
    pub result_status: Option<String>,
    pub error_code: Option<String>,
    pub error_summary: Option<String>,
    pub failure_class: Option<String>,
    pub retry_eligible: bool,
    pub next_retry_at: Option<String>,
    pub llm_call_ids: Vec<String>,
    pub llm_calls: Vec<AdminTranslationLinkedLlmCall>,
    pub processing_stage: Option<String>,
    pub provider_status: Option<String>,
    pub output_contract_status: Option<String>,
    pub retry_disposition: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct AdminTranslationAttemptEventsResponse {
    pub items: Vec<AdminTranslationAttemptEvent>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, Serialize)]
pub struct AdminTranslationRequestDetailResponse {
    pub request: AdminTranslationRequestListItem,
    pub result: AdminTranslationRequestResultItem,
    pub attempts: Vec<AdminTranslationAttemptEvent>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AdminTranslationBatchResultSummary {
    pub ready: i64,
    pub error: i64,
    pub missing: i64,
    pub disabled: i64,
    pub queued: i64,
    pub running: i64,
}

#[derive(Debug, Clone, Serialize)]
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
    pub result_summary: AdminTranslationBatchResultSummary,
    pub business_outcome: AdminBusinessOutcome,
}

#[derive(Debug, Serialize)]
pub struct AdminTranslationBatchesResponse {
    pub items: Vec<AdminTranslationBatchListItem>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, Deserialize)]
pub struct AdminTranslationLinkedLlmCall {
    pub id: String,
    pub status: String,
    pub source: String,
    pub model: String,
    pub scheduler_wait_ms: i64,
    pub duration_ms: Option<i64>,
    #[sqlx(default)]
    pub stage: Option<String>,
    #[sqlx(default)]
    pub relation_role: Option<String>,
    #[sqlx(default)]
    pub evidence_availability: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct AdminTranslationBatchDetailResponse {
    pub batch: AdminTranslationBatchListItem,
    pub items: Vec<AdminTranslationBatchResultItem>,
    pub llm_calls: Vec<AdminTranslationLinkedLlmCall>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct AdminTranslationBatchRow {
    pub(crate) id: String,
    pub(crate) status: String,
    pub(crate) trigger_reason: String,
    pub(crate) worker_slot: i64,
    pub(crate) request_count: i64,
    pub(crate) item_count: i64,
    pub(crate) estimated_input_tokens: i64,
    pub(crate) error_text: Option<String>,
    pub(crate) created_at: String,
    pub(crate) started_at: Option<String>,
    pub(crate) finished_at: Option<String>,
    pub(crate) updated_at: String,
}

impl RequestRow {
    pub(crate) fn effective_status(&self) -> &str {
        if self.status == "queued"
            && work_item_status_counts_as_running(self.work_item_status.as_deref())
        {
            "running"
        } else {
            self.status.as_str()
        }
    }

    pub(crate) fn to_admin_request_list_item(&self) -> AdminTranslationRequestListItem {
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
            retry_count: self.retry_count,
            created_at: self.created_at.clone(),
            started_at: self.started_at.clone(),
            last_attempt_at: self.last_attempt_at.clone(),
            finished_at: self.finished_at.clone(),
            updated_at: self.updated_at.clone(),
        }
    }

    pub(crate) fn to_result(&self) -> TranslationResultItem {
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
            reading: None,
            error: self.error_text.clone(),
            work_item_id: self.work_item_id.clone(),
            batch_id: self.batch_id.clone(),
        }
    }
}

fn work_item_status_counts_as_running(work_item_status: Option<&str>) -> bool {
    matches!(work_item_status, Some("running" | "batched"))
}

fn pending_result_status_from_work_status(work_item_status: Option<&str>) -> &'static str {
    match work_item_status {
        Some("queued") | None => "queued",
        _ => "running",
    }
}

/// Column list shared by the admin translation request list and detail read
/// models. `postgres_server` rewrites the table qualifiers for PostgreSQL.
pub(crate) fn request_row_select_sql() -> &'static str {
    r#"
    SELECT r.id, r.mode, r.source, r.request_origin, r.requested_by, r.scope_user_id,
           r.producer_ref, r.kind, r.variant, r.entity_id, r.target_lang, r.max_wait_ms,
           r.work_item_id, r.status, r.result_status, r.title_zh, r.summary_md, r.body_md,
           r.error_text,
           r.created_at, r.started_at, r.finished_at, r.updated_at,
           (SELECT status FROM translation_work_items w WHERE w.id = r.work_item_id) AS work_item_status,
           (SELECT batch_id FROM translation_work_items w WHERE w.id = r.work_item_id) AS batch_id,
           COALESCE((SELECT retry_count FROM translation_work_items w WHERE w.id = r.work_item_id), 0) AS retry_count,
           (SELECT MAX(created_at) FROM translation_attempt_events e WHERE e.work_item_id = r.work_item_id) AS last_attempt_at
    FROM translation_requests r
    "#
}

/// Classify one erroring result item into the admin response shape: the item's
/// error text is replaced by the human summary and the structured fields carry
/// the code, summary and no raw detail.
pub(crate) fn admin_translation_result_item(
    item: TranslationResultItem,
) -> AdminTranslationRequestResultItem {
    let classified = (item.status == "error")
        .then(|| classify_translation_error(item.error.as_deref()))
        .flatten();
    let mut item = item;
    if let Some(classified) = classified.as_ref() {
        item.error = Some(classified.summary.to_owned());
    }
    AdminTranslationRequestResultItem {
        item,
        error_code: classified.as_ref().map(|value| value.code.to_owned()),
        error_summary: classified.as_ref().map(|value| value.summary.to_owned()),
        error_detail: None,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AdminTranslationBatchResultItem {
    #[serde(flatten)]
    pub item: TranslationResultItem,
    pub error_code: Option<String>,
    pub error_summary: Option<String>,
    pub error_detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdminTranslationRequestResultItem {
    #[serde(flatten)]
    pub item: TranslationResultItem,
    pub error_code: Option<String>,
    pub error_summary: Option<String>,
    pub error_detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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
    pub error_code: Option<String>,
    pub error_summary: Option<String>,
    pub error_detail: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct RequestRow {
    pub(crate) id: String,
    // Selected by the row query so `sqlx::FromRow` keeps one mapping shape;
    // the admin read model reports neither column today.
    #[allow(dead_code)]
    pub(crate) mode: String,
    pub(crate) source: String,
    pub(crate) request_origin: String,
    pub(crate) requested_by: Option<String>,
    pub(crate) scope_user_id: String,
    pub(crate) producer_ref: String,
    pub(crate) kind: String,
    pub(crate) variant: String,
    pub(crate) entity_id: String,
    #[allow(dead_code)]
    pub(crate) target_lang: String,
    pub(crate) max_wait_ms: i64,
    pub(crate) work_item_id: Option<String>,
    pub(crate) status: String,
    pub(crate) result_status: Option<String>,
    pub(crate) title_zh: Option<String>,
    pub(crate) summary_md: Option<String>,
    pub(crate) body_md: Option<String>,
    pub(crate) error_text: Option<String>,
    pub(crate) created_at: String,
    pub(crate) started_at: Option<String>,
    pub(crate) finished_at: Option<String>,
    pub(crate) updated_at: String,
    pub(crate) work_item_status: Option<String>,
    pub(crate) batch_id: Option<String>,
    pub(crate) retry_count: i64,
    pub(crate) last_attempt_at: Option<String>,
}
