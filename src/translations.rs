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
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use tokio::time::sleep;
use tower_sessions::Session;
use tracing::warn;

use crate::{admin_runtime, ai, api, error::ApiError, runtime, state::AppState};

const TRANSLATION_PROTOCOL_VERSION: &str = "translation-request.v1";
const TRANSLATION_MODEL_PROFILE_DISABLED: &str = "ai-disabled";
const TRANSLATION_BATCH_MAX_TOKENS: u32 = 1_800;
const TRANSLATION_BATCH_SCAN_INTERVAL: Duration = Duration::from_millis(250);
const TRANSLATION_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(150);
const TRANSLATION_STREAM_POLL_INTERVAL: Duration = Duration::from_millis(250);
const TRANSLATION_MAX_ITEMS_PER_REQUEST: usize = 60;
const TRANSLATION_MIN_WAIT_MS: i64 = 0;
const TRANSLATION_MAX_WAIT_MS: i64 = 60_000;
const MAX_TRANSLATION_WORKER_CONCURRENCY: usize = 64;
pub const DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY: usize = 3;
pub const DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY: usize = 1;

static TRANSLATION_BATCH_CLAIM_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

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

#[derive(Debug, Deserialize)]
pub struct TranslationResolveRequest {
    pub items: Vec<TranslationRequestItemInput>,
    #[serde(default)]
    pub retry_on_error: bool,
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
pub struct TranslationResolveResponse {
    pub items: Vec<TranslationResultItem>,
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
    pub failed_batches_24h: i64,
    pub avg_wait_ms_24h: Option<i64>,
    pub last_batch_finished_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AdminTranslationRuntimeConfigUpdateRequest {
    pub general_worker_concurrency: i64,
    pub dedicated_worker_concurrency: i64,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranslationRuntimeConfig {
    pub general_worker_concurrency: usize,
    pub dedicated_worker_concurrency: usize,
}

impl Default for TranslationRuntimeConfig {
    fn default() -> Self {
        Self {
            general_worker_concurrency: DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY,
            dedicated_worker_concurrency: DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY,
        }
    }
}

impl TranslationRuntimeConfig {
    pub fn new(general_worker_concurrency: usize, dedicated_worker_concurrency: usize) -> Self {
        Self {
            general_worker_concurrency: general_worker_concurrency.max(1),
            dedicated_worker_concurrency: dedicated_worker_concurrency.max(1),
        }
    }

    fn total_worker_concurrency(self) -> usize {
        self.general_worker_concurrency + self.dedicated_worker_concurrency
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct RunningBatchRuntimeRow {
    id: String,
    worker_id: String,
    worker_slot: i64,
    worker_kind: String,
    request_count: i64,
    item_count: i64,
    trigger_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranslationWorkerProfile {
    worker_id: String,
    worker_slot: i64,
    worker_kind: String,
}

#[derive(Debug, Clone, Copy)]
struct TranslationWorkerRuntimeUpdate<'a> {
    status: &'a str,
    current_batch_id: Option<&'a str>,
    request_count: i64,
    work_item_count: i64,
    trigger_reason: Option<&'a str>,
    error_text: Option<&'a str>,
}

impl<'a> TranslationWorkerRuntimeUpdate<'a> {
    const fn idle() -> Self {
        Self {
            status: "idle",
            current_batch_id: None,
            request_count: 0,
            work_item_count: 0,
            trigger_reason: None,
            error_text: None,
        }
    }

    const fn running(
        current_batch_id: &'a str,
        request_count: i64,
        work_item_count: i64,
        trigger_reason: &'a str,
    ) -> Self {
        Self {
            status: "running",
            current_batch_id: Some(current_batch_id),
            request_count,
            work_item_count,
            trigger_reason: Some(trigger_reason),
            error_text: None,
        }
    }

    const fn error(error_text: &'a str) -> Self {
        Self {
            status: "error",
            current_batch_id: None,
            request_count: 0,
            work_item_count: 0,
            trigger_reason: None,
            error_text: Some(error_text),
        }
    }
}

#[derive(Debug)]
pub struct TranslationSchedulerController {
    desired_config: tokio::sync::RwLock<TranslationRuntimeConfig>,
    runtime: tokio::sync::RwLock<Vec<TranslationWorkerRuntimeState>>,
    worker_abort_handles: tokio::sync::Mutex<HashMap<String, tokio::task::AbortHandle>>,
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
        if self.status == "queued"
            && work_item_status_counts_as_running(self.work_item_status.as_deref())
        {
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

#[allow(dead_code)]
#[derive(Debug, Clone, sqlx::FromRow)]
struct TranslationStateRow {
    id: String,
    user_id: String,
    entity_type: String,
    entity_id: String,
    lang: String,
    source_hash: String,
    status: String,
    title: Option<String>,
    summary: Option<String>,
    error_text: Option<String>,
    active_work_item_id: Option<String>,
    created_at: String,
    updated_at: String,
}

impl TranslationStateRow {
    fn to_result(&self, item: &TranslationRequestItemInput) -> TranslationResultItem {
        let mut result = TranslationResultItem {
            producer_ref: item.producer_ref.clone(),
            entity_id: item.entity_id.clone(),
            kind: item.kind.clone(),
            variant: item.variant.clone(),
            status: self.status.clone(),
            title_zh: None,
            summary_md: None,
            body_md: None,
            error: self.error_text.clone(),
            work_item_id: self.active_work_item_id.clone(),
            batch_id: None,
        };
        if self.status == "ready" {
            if item.target_slots.iter().any(|slot| slot == "title_zh") {
                result.title_zh = self.title.clone();
            }
            if item.target_slots.iter().any(|slot| slot == "summary_md") {
                result.summary_md = self.summary.clone();
            }
            if item.target_slots.iter().any(|slot| slot == "body_md") {
                result.body_md = self.summary.clone();
            }
        }
        result
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

fn request_status_from_work_item_status(work_item_status: &str) -> &'static str {
    if work_item_status == "queued" {
        "queued"
    } else {
        "running"
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct ClaimedBatch {
    id: String,
    worker_id: String,
    worker_slot: i64,
    worker_kind: String,
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
    fn idle(profile: &TranslationWorkerProfile) -> Self {
        Self {
            worker_id: profile.worker_id.clone(),
            worker_slot: profile.worker_slot,
            worker_kind: profile.worker_kind.clone(),
            status: "idle".to_owned(),
            current_batch_id: None,
            request_count: 0,
            work_item_count: 0,
            trigger_reason: None,
            updated_at: Utc::now().to_rfc3339(),
            error_text: None,
        }
    }

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

impl TranslationSchedulerController {
    pub fn new(config: TranslationRuntimeConfig) -> Self {
        let config = TranslationRuntimeConfig::new(
            config.general_worker_concurrency,
            config.dedicated_worker_concurrency,
        );
        let runtime = translation_worker_profiles(config)
            .into_iter()
            .map(|profile| TranslationWorkerRuntimeState::idle(&profile))
            .collect();
        Self {
            desired_config: tokio::sync::RwLock::new(config),
            runtime: tokio::sync::RwLock::new(runtime),
            worker_abort_handles: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    pub async fn desired_config(&self) -> TranslationRuntimeConfig {
        *self.desired_config.read().await
    }

    pub async fn runtime_statuses(&self) -> Vec<AdminTranslationWorkerStatus> {
        let mut runtime = self.runtime.read().await.clone();
        runtime.sort_by(|left, right| {
            left.worker_slot
                .cmp(&right.worker_slot)
                .then_with(|| left.worker_id.cmp(&right.worker_id))
        });
        runtime
            .into_iter()
            .map(|entry| entry.to_admin_status())
            .collect()
    }

    pub async fn abort_all(&self) {
        let mut handles = self.worker_abort_handles.lock().await;
        for (_, handle) in handles.drain() {
            handle.abort();
        }
    }

    pub async fn apply_runtime_config(
        self: &Arc<Self>,
        state: Arc<AppState>,
        config: TranslationRuntimeConfig,
    ) -> Result<TranslationRuntimeConfig> {
        let config = self.sync_runtime_with_config(&state.pool, config).await?;
        self.ensure_workers_running(state).await;
        Ok(config)
    }

    pub async fn spawn_initial_workers(self: &Arc<Self>, state: Arc<AppState>) {
        self.ensure_workers_running(state).await;
    }

    async fn sync_runtime_with_config(
        &self,
        pool: &SqlitePool,
        config: TranslationRuntimeConfig,
    ) -> Result<TranslationRuntimeConfig> {
        let config = TranslationRuntimeConfig::new(
            config.general_worker_concurrency,
            config.dedicated_worker_concurrency,
        );
        if *self.desired_config.read().await == config {
            return Ok(config);
        }
        let _claim_guard = translation_batch_claim_lock().lock().await;
        *self.desired_config.write().await = config;

        let desired_profiles = translation_worker_profiles(config);
        let desired_worker_ids = desired_profiles
            .iter()
            .map(|profile| profile.worker_id.as_str())
            .collect::<HashSet<_>>();
        let mut runtime = self.runtime.write().await;
        let previous_topology = runtime
            .iter()
            .map(|entry| {
                (
                    entry.worker_id.clone(),
                    (entry.worker_slot, entry.worker_kind.clone()),
                )
            })
            .collect::<HashMap<_, _>>();
        runtime.retain(|entry| {
            desired_worker_ids.contains(entry.worker_id.as_str())
                || entry.current_batch_id.is_some()
        });
        for profile in &desired_profiles {
            if let Some(entry) = runtime
                .iter_mut()
                .find(|entry| entry.worker_id == profile.worker_id)
            {
                entry.worker_kind = profile.worker_kind.clone();
            } else {
                runtime.push(TranslationWorkerRuntimeState::idle(profile));
            }
        }
        reconcile_worker_runtime_slots(&mut runtime, config);
        let batch_slot_updates = collect_running_batch_slot_updates(&runtime, &previous_topology);
        let topology_changed = translation_runtime_topology_changed(&runtime, &previous_topology);
        if topology_changed {
            refresh_translation_runtime_updated_at(&mut runtime);
        }
        drop(runtime);
        sync_running_batch_slot_updates(pool, &batch_slot_updates).await?;
        Ok(config)
    }

    async fn ensure_workers_running(self: &Arc<Self>, state: Arc<AppState>) {
        let desired_profiles = translation_worker_profiles(self.desired_config().await);
        let mut handles = self.worker_abort_handles.lock().await;
        handles.retain(|_, handle| !handle.is_finished());
        for profile in desired_profiles {
            if handles.contains_key(&profile.worker_id) {
                continue;
            }
            let worker_id = profile.worker_id.clone();
            let abort_handle = self.spawn_worker_task(state.clone(), worker_id.clone());
            handles.insert(worker_id, abort_handle);
        }
    }

    async fn run_worker_loop(self: Arc<Self>, state: Arc<AppState>, worker_id: String) {
        if let Some(profile) = self.profile_by_worker_id(worker_id.as_str()).await {
            self.update_worker_runtime(&profile, TranslationWorkerRuntimeUpdate::idle())
                .await;
        }

        loop {
            let Some(profile) = self.profile_by_worker_id(worker_id.as_str()).await else {
                if let Err(err) = self
                    .remove_worker_runtime(&state.pool, worker_id.as_str())
                    .await
                {
                    warn!(
                        ?err,
                        worker_id = worker_id.as_str(),
                        "failed to remove drained translation worker runtime"
                    );
                }
                break;
            };

            if let Err(err) = run_translation_scheduler_once(state.as_ref(), profile.clone()).await
            {
                warn!(
                    ?err,
                    worker_id = profile.worker_id.as_str(),
                    worker_slot = profile.worker_slot,
                    "translation scheduler tick failed"
                );
                let error_text = err.to_string();
                self.update_worker_runtime(
                    &profile,
                    TranslationWorkerRuntimeUpdate::error(error_text.as_str()),
                )
                .await;
            }

            sleep(TRANSLATION_BATCH_SCAN_INTERVAL).await;
        }

        self.finish_worker_exit(state, worker_id).await;
    }

    async fn worker_is_desired(&self, worker_id: &str) -> bool {
        self.profile_by_worker_id(worker_id).await.is_some()
    }

    async fn current_profile_by_worker_id(
        &self,
        worker_id: &str,
    ) -> Option<TranslationWorkerProfile> {
        if let Some(profile) = self
            .runtime
            .read()
            .await
            .iter()
            .find(|entry| entry.worker_id == worker_id)
            .map(|entry| {
                translation_worker_profile_from_runtime(
                    entry.worker_id.as_str(),
                    entry.worker_kind.as_str(),
                    entry.worker_slot,
                )
            })
        {
            return Some(profile);
        }

        self.profile_by_worker_id(worker_id).await
    }

    async fn profile_by_worker_id(&self, worker_id: &str) -> Option<TranslationWorkerProfile> {
        translation_worker_profiles(self.desired_config().await)
            .into_iter()
            .find(|profile| profile.worker_id == worker_id)
    }

    async fn finish_worker_exit(self: &Arc<Self>, state: Arc<AppState>, worker_id: String) {
        if self.unregister_worker(worker_id.as_str()).await {
            self.schedule_worker_reconcile(state);
        }
    }

    async fn unregister_worker(&self, worker_id: &str) -> bool {
        self.worker_abort_handles.lock().await.remove(worker_id);
        self.worker_is_desired(worker_id).await
    }

    fn schedule_worker_reconcile(self: &Arc<Self>, state: Arc<AppState>) {
        let controller = Arc::clone(self);
        tokio::spawn(async move {
            controller.ensure_workers_running(state).await;
        });
    }

    fn spawn_worker_task(
        self: &Arc<Self>,
        state: Arc<AppState>,
        worker_id: String,
    ) -> tokio::task::AbortHandle {
        let controller = Arc::clone(self);
        tokio::spawn(async move {
            controller.run_worker_loop(state, worker_id).await;
        })
        .abort_handle()
    }

    async fn remove_worker_runtime(&self, pool: &SqlitePool, worker_id: &str) -> Result<()> {
        let desired_config = self.desired_config().await;
        let mut runtime = self.runtime.write().await;
        let previous_topology = runtime
            .iter()
            .map(|entry| {
                (
                    entry.worker_id.clone(),
                    (entry.worker_slot, entry.worker_kind.clone()),
                )
            })
            .collect::<HashMap<_, _>>();
        runtime.retain(|entry| entry.worker_id != worker_id);
        reconcile_worker_runtime_slots(&mut runtime, desired_config);
        let batch_slot_updates = collect_running_batch_slot_updates(&runtime, &previous_topology);
        if translation_runtime_topology_changed(&runtime, &previous_topology) {
            refresh_translation_runtime_updated_at(&mut runtime);
        }
        drop(runtime);
        sync_running_batch_slot_updates(pool, &batch_slot_updates).await?;
        Ok(())
    }

    async fn update_worker_runtime(
        &self,
        profile: &TranslationWorkerProfile,
        update: TranslationWorkerRuntimeUpdate<'_>,
    ) {
        let desired_config = self.desired_config().await;
        let mut runtime = self.runtime.write().await;
        let entry = if let Some(index) = runtime
            .iter()
            .position(|entry| entry.worker_id == profile.worker_id)
        {
            &mut runtime[index]
        } else {
            runtime.push(TranslationWorkerRuntimeState::idle(profile));
            runtime
                .last_mut()
                .expect("translation worker runtime entry should exist after insert")
        };

        let next_batch_id = update.current_batch_id.map(str::to_owned);
        let next_trigger_reason = update.trigger_reason.map(str::to_owned);
        let next_error_text = update.error_text.map(str::to_owned);
        let changed = entry.worker_slot != profile.worker_slot
            || entry.worker_kind != profile.worker_kind
            || entry.status != update.status
            || entry.current_batch_id != next_batch_id
            || entry.request_count != update.request_count
            || entry.work_item_count != update.work_item_count
            || entry.trigger_reason != next_trigger_reason
            || entry.error_text != next_error_text;
        if !changed {
            return;
        }

        entry.worker_slot = profile.worker_slot;
        entry.worker_kind = profile.worker_kind.clone();
        entry.status = update.status.to_owned();
        entry.current_batch_id = next_batch_id;
        entry.request_count = update.request_count;
        entry.work_item_count = update.work_item_count;
        entry.trigger_reason = next_trigger_reason;
        entry.error_text = next_error_text;
        entry.updated_at = Utc::now().to_rfc3339();
        reconcile_worker_runtime_slots(&mut runtime, desired_config);
    }
}

fn translation_worker_profiles(config: TranslationRuntimeConfig) -> Vec<TranslationWorkerProfile> {
    let mut profiles = Vec::with_capacity(config.total_worker_concurrency());
    for worker_index in 1..=config.general_worker_concurrency {
        profiles.push(TranslationWorkerProfile {
            worker_id: translation_worker_id("general", worker_index),
            worker_slot: i64::try_from(worker_index).unwrap_or(i64::MAX),
            worker_kind: "general".to_owned(),
        });
    }
    for worker_index in 1..=config.dedicated_worker_concurrency {
        let slot = config.general_worker_concurrency + worker_index;
        profiles.push(TranslationWorkerProfile {
            worker_id: translation_worker_id("user_dedicated", worker_index),
            worker_slot: i64::try_from(slot).unwrap_or(i64::MAX),
            worker_kind: "user_dedicated".to_owned(),
        });
    }
    profiles
}

fn translation_worker_profile_from_runtime(
    worker_id: &str,
    worker_kind: &str,
    worker_slot: i64,
) -> TranslationWorkerProfile {
    TranslationWorkerProfile {
        worker_id: worker_id.to_owned(),
        worker_slot,
        worker_kind: worker_kind.to_owned(),
    }
}

fn reconcile_worker_runtime_slots(
    runtime: &mut [TranslationWorkerRuntimeState],
    config: TranslationRuntimeConfig,
) {
    let desired_profiles = translation_worker_profiles(config);
    let desired_profiles_by_worker_id = desired_profiles
        .iter()
        .map(|profile| (profile.worker_id.as_str(), profile))
        .collect::<HashMap<_, _>>();
    runtime.sort_by(|left, right| {
        let left_profile = desired_profiles_by_worker_id
            .get(left.worker_id.as_str())
            .copied();
        let right_profile = desired_profiles_by_worker_id
            .get(right.worker_id.as_str())
            .copied();
        translation_worker_runtime_sort_key(left, left_profile)
            .cmp(&translation_worker_runtime_sort_key(right, right_profile))
    });

    // Keep the board topology continuous even while drained workers finish in the background.
    for (index, entry) in runtime.iter_mut().enumerate() {
        if let Some(profile) = desired_profiles_by_worker_id.get(entry.worker_id.as_str()) {
            entry.worker_kind = profile.worker_kind.clone();
        }
        entry.worker_slot = i64::try_from(index + 1).unwrap_or(i64::MAX);
    }
}

fn translation_worker_runtime_sort_key<'a>(
    entry: &'a TranslationWorkerRuntimeState,
    desired_profile: Option<&'a TranslationWorkerProfile>,
) -> (i32, i32, i64, &'a str) {
    let worker_kind = desired_profile
        .map(|profile| profile.worker_kind.as_str())
        .unwrap_or(entry.worker_kind.as_str());
    let worker_slot = desired_profile
        .map(|profile| profile.worker_slot)
        .unwrap_or(entry.worker_slot);
    (
        translation_worker_kind_priority(worker_kind),
        if desired_profile.is_some() { 0 } else { 1 },
        worker_slot,
        entry.worker_id.as_str(),
    )
}

fn translation_worker_kind_priority(worker_kind: &str) -> i32 {
    match worker_kind {
        "general" => 0,
        "user_dedicated" => 1,
        _ => 2,
    }
}

fn translation_runtime_topology_changed(
    runtime: &[TranslationWorkerRuntimeState],
    previous_topology: &HashMap<String, (i64, String)>,
) -> bool {
    runtime.len() != previous_topology.len()
        || runtime.iter().any(|entry| {
            previous_topology
                .get(entry.worker_id.as_str())
                .map(|(worker_slot, worker_kind)| {
                    *worker_slot != entry.worker_slot || worker_kind != &entry.worker_kind
                })
                .unwrap_or(true)
        })
}

fn refresh_translation_runtime_updated_at(runtime: &mut [TranslationWorkerRuntimeState]) {
    let updated_at = Utc::now().to_rfc3339();
    for entry in runtime.iter_mut() {
        if entry.status == "running" {
            continue;
        }
        entry.updated_at = updated_at.clone();
    }
}

fn translation_worker_id(worker_kind: &str, worker_index: usize) -> String {
    format!(
        "translation-worker-{}-{worker_index}",
        worker_kind.replace('_', "-")
    )
}

fn translation_batch_claim_lock() -> &'static tokio::sync::Mutex<()> {
    TRANSLATION_BATCH_CLAIM_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn update_translation_worker_runtime(
    state: &AppState,
    profile: &TranslationWorkerProfile,
    update: TranslationWorkerRuntimeUpdate<'_>,
) {
    state
        .translation_scheduler
        .update_worker_runtime(profile, update)
        .await;
}

pub async fn translation_worker_runtime_statuses(
    state: &AppState,
) -> Vec<AdminTranslationWorkerStatus> {
    state.translation_scheduler.runtime_statuses().await
}

#[cfg(test)]
async fn reset_translation_worker_runtime_for_tests(state: &AppState) {
    state.translation_scheduler.abort_all().await;
    let config = state.translation_scheduler.desired_config().await;
    state
        .translation_scheduler
        .sync_runtime_with_config(&state.pool, config)
        .await
        .expect("reset translation runtime");
}

#[derive(Debug, Clone)]
struct RunningBatchSlotUpdate {
    batch_id: String,
    worker_id: String,
    worker_slot: i64,
    worker_kind: String,
}

fn collect_running_batch_slot_updates(
    runtime: &[TranslationWorkerRuntimeState],
    previous_topology: &HashMap<String, (i64, String)>,
) -> Vec<RunningBatchSlotUpdate> {
    runtime
        .iter()
        .filter_map(|entry| {
            let batch_id = entry.current_batch_id.as_ref()?;
            let previous = previous_topology.get(entry.worker_id.as_str());
            let slot_changed = previous
                .map(|(worker_slot, worker_kind)| {
                    *worker_slot != entry.worker_slot || *worker_kind != entry.worker_kind
                })
                .unwrap_or(true);
            if !slot_changed {
                return None;
            }
            Some(RunningBatchSlotUpdate {
                batch_id: batch_id.clone(),
                worker_id: entry.worker_id.clone(),
                worker_slot: entry.worker_slot,
                worker_kind: entry.worker_kind.clone(),
            })
        })
        .collect()
}

async fn sync_running_batch_slot_updates(
    pool: &SqlitePool,
    updates: &[RunningBatchSlotUpdate],
) -> Result<()> {
    if updates.is_empty() {
        return Ok(());
    }
    let updated_at = Utc::now().to_rfc3339();
    for update in updates {
        sqlx::query(
            r#"
            UPDATE translation_batches
            SET worker_slot = ?, worker_kind = ?, updated_at = ?
            WHERE id = ? AND status = 'running' AND worker_id = ?
            "#,
        )
        .bind(update.worker_slot)
        .bind(update.worker_kind.as_str())
        .bind(updated_at.as_str())
        .bind(update.batch_id.as_str())
        .bind(update.worker_id.as_str())
        .execute(pool)
        .await?;
    }
    Ok(())
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

fn request_effective_status_sql(status_column: &str, work_item_status_column: &str) -> String {
    format!(
        "CASE WHEN {status_column} = 'queued' AND {work_item_status_column} IN ('running', 'batched') THEN 'running' ELSE {status_column} END"
    )
}

pub async fn spawn_translation_scheduler(state: Arc<AppState>) {
    state
        .translation_scheduler
        .spawn_initial_workers(state.clone())
        .await;
}

pub fn spawn_translation_recovery_task(state: Arc<AppState>) -> tokio::task::AbortHandle {
    tokio::spawn(async move {
        loop {
            if let Err(err) = recover_runtime_state(state.as_ref()).await {
                warn!(?err, "translation recovery sweep failed");
            }
            sleep(runtime::RUNTIME_LEASE_HEARTBEAT_INTERVAL).await;
        }
    })
    .abort_handle()
}

pub async fn recover_runtime_state_on_startup(state: &AppState) -> Result<()> {
    recover_runtime_state_with_mode(state, runtime::RuntimeRecoveryMode::Startup).await
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

pub async fn resolve_translation_results(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<TranslationResolveRequest>,
) -> Result<Json<TranslationResolveResponse>, ApiError> {
    let user_id = api::require_active_user_id(state.as_ref(), &session).await?;
    let items = normalize_request_items(&req.items)?;
    let items =
        resolve_translation_results_for_user(state.as_ref(), &user_id, &items, req.retry_on_error)
            .await?;
    Ok(Json(TranslationResolveResponse { items }))
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
    admin_runtime::sync_persisted_runtime_settings(state.clone())
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(
        load_admin_translation_status_response(state.as_ref()).await?,
    ))
}

pub async fn admin_patch_translation_runtime_config(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(req): Json<AdminTranslationRuntimeConfigUpdateRequest>,
) -> Result<Json<AdminTranslationStatusResponse>, ApiError> {
    let _acting_user_id = api::require_admin_user_id(state.as_ref(), &session).await?;
    let general_worker_concurrency = parse_positive_worker_concurrency(
        req.general_worker_concurrency,
        "general_worker_concurrency",
    )?;
    let dedicated_worker_concurrency = parse_positive_worker_concurrency(
        req.dedicated_worker_concurrency,
        "dedicated_worker_concurrency",
    )?;
    validate_translation_worker_concurrency_total(
        general_worker_concurrency,
        dedicated_worker_concurrency,
    )?;

    admin_runtime::update_translation_runtime_settings(
        &state.pool,
        general_worker_concurrency,
        dedicated_worker_concurrency,
    )
    .await
    .map_err(ApiError::internal)?;
    admin_runtime::sync_persisted_runtime_settings(state.clone())
        .await
        .map_err(ApiError::internal)?;

    Ok(Json(
        load_admin_translation_status_response(state.as_ref()).await?,
    ))
}

async fn load_admin_translation_status_response(
    state: &AppState,
) -> Result<AdminTranslationStatusResponse, ApiError> {
    let since = (Utc::now() - chrono::Duration::hours(24)).to_rfc3339();
    let runtime_config = state.translation_scheduler.desired_config().await;
    let workers = translation_worker_runtime_statuses(state).await;
    let general_worker_concurrency = i64::try_from(
        workers
            .iter()
            .filter(|worker| worker.worker_kind == "general")
            .count(),
    )
    .unwrap_or(i64::MAX);
    let dedicated_worker_concurrency = i64::try_from(
        workers
            .iter()
            .filter(|worker| worker.worker_kind == "user_dedicated")
            .count(),
    )
    .unwrap_or(i64::MAX);
    let worker_concurrency = i64::try_from(workers.len()).unwrap_or(i64::MAX);
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
        state,
        "SELECT COUNT(*) FROM translation_requests WHERE status = 'queued'",
        &[],
    )
    .await?;
    let queued_work_items = scalar_i64(
        state,
        "SELECT COUNT(*) FROM translation_work_items WHERE status = 'queued'",
        &[],
    )
    .await?;
    let running_batches = scalar_i64(
        state,
        "SELECT COUNT(*) FROM translation_batches WHERE status = 'running'",
        &[],
    )
    .await?;
    let requests_24h = scalar_i64(
        state,
        "SELECT COUNT(*) FROM translation_requests WHERE created_at >= ?",
        &[&since],
    )
    .await?;
    let completed_batches_24h = scalar_i64(
        state,
        "SELECT COUNT(*) FROM translation_batches WHERE created_at >= ? AND status = 'completed'",
        &[&since],
    )
    .await?;
    let failed_batches_24h = scalar_i64(
        state,
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
    let last_batch_finished_at = load_last_batch_finished_at(state).await?;

    let budget = i64::from(translation_batch_input_budget(state).await);

    Ok(AdminTranslationStatusResponse {
        scheduler_enabled: true,
        llm_enabled: state.config.ai.is_some(),
        scan_interval_ms: i64::try_from(TRANSLATION_BATCH_SCAN_INTERVAL.as_millis())
            .unwrap_or(i64::MAX),
        batch_token_threshold: budget,
        general_worker_concurrency,
        dedicated_worker_concurrency,
        worker_concurrency,
        target_general_worker_concurrency: i64::try_from(runtime_config.general_worker_concurrency)
            .unwrap_or(i64::MAX),
        target_dedicated_worker_concurrency: i64::try_from(
            runtime_config.dedicated_worker_concurrency,
        )
        .unwrap_or(i64::MAX),
        target_worker_concurrency: i64::try_from(runtime_config.total_worker_concurrency())
            .unwrap_or(i64::MAX),
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
    })
}

async fn translation_batch_input_budget(state: &AppState) -> u32 {
    ai::compute_input_budget_with_source(state, TRANSLATION_BATCH_MAX_TOKENS)
        .await
        .input_budget
        .clamp(1, TRANSLATION_BATCH_MAX_TOKENS)
}

fn parse_positive_worker_concurrency(value: i64, field: &str) -> Result<usize, ApiError> {
    let parsed = usize::try_from(value)
        .map_err(|_| ApiError::bad_request(format!("{field} must be a positive integer")))?;
    if parsed == 0 {
        return Err(ApiError::bad_request(format!(
            "{field} must be a positive integer"
        )));
    }
    if parsed > MAX_TRANSLATION_WORKER_CONCURRENCY {
        return Err(ApiError::bad_request(format!(
            "{field} must be a positive integer <= {MAX_TRANSLATION_WORKER_CONCURRENCY}"
        )));
    }
    Ok(parsed)
}

fn validate_translation_worker_concurrency_total(
    general_worker_concurrency: usize,
    dedicated_worker_concurrency: usize,
) -> Result<(), ApiError> {
    let total = general_worker_concurrency
        .checked_add(dedicated_worker_concurrency)
        .ok_or_else(|| {
            ApiError::bad_request(format!(
                "general_worker_concurrency + dedicated_worker_concurrency must be <= {MAX_TRANSLATION_WORKER_CONCURRENCY}"
            ))
        })?;
    if total > MAX_TRANSLATION_WORKER_CONCURRENCY {
        return Err(ApiError::bad_request(format!(
            "general_worker_concurrency + dedicated_worker_concurrency must be <= {MAX_TRANSLATION_WORKER_CONCURRENCY}"
        )));
    }
    Ok(())
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

    let effective_status_sql = request_effective_status_sql("status", "work_item_status");
    let total_sql = format!(
        r#"
        SELECT COUNT(*)
        FROM ({request_rows_base}) request_rows
        WHERE (? = 'all' OR {effective_status_sql} = ?)
        "#,
        request_rows_base = request_row_select_sql(),
        effective_status_sql = effective_status_sql,
    );
    let total = sqlx::query_scalar::<_, i64>(&total_sql)
        .bind(status.as_str())
        .bind(status.as_str())
        .fetch_one(&state.pool)
        .await
        .map_err(ApiError::internal)?;

    let request_rows_sql = format!(
        r#"
        SELECT *
        FROM ({request_rows_base}) request_rows
        WHERE (? = 'all' OR {effective_status_sql} = ?)
        ORDER BY CASE WHEN {effective_status_sql} IN ('queued', 'running') THEN 0 ELSE 1 END ASC,
                 updated_at DESC,
                 id DESC
        LIMIT ? OFFSET ?
        "#,
        request_rows_base = request_row_select_sql(),
        effective_status_sql = effective_status_sql,
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
    refresh_live_batch_runtime_for_request(state, &created).await?;
    Ok(created)
}

async fn resolve_translation_results_for_user(
    state: &AppState,
    user_id: &str,
    items: &[TranslationRequestItemInput],
    retry_on_error: bool,
) -> Result<Vec<TranslationResultItem>, ApiError> {
    let now = Utc::now().to_rfc3339();
    let mut tx = state.pool.begin().await.map_err(ApiError::internal)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let result = ensure_translation_result_for_item(
            &mut tx,
            user_id,
            item,
            "user",
            now.as_str(),
            retry_on_error,
        )
        .await?;
        out.push(result);
    }
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(out)
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
    for created in &out {
        refresh_live_batch_runtime_for_request(state, created).await?;
    }
    Ok(out)
}

async fn ensure_translation_result_for_item(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    item: &TranslationRequestItemInput,
    request_origin: &str,
    now: &str,
    retry_on_error: bool,
) -> Result<TranslationResultItem, ApiError> {
    let source_hash = build_source_hash(item);
    if let Some(existing) = load_translation_state_row(tx, user_id, item).await?
        && existing.source_hash == source_hash
    {
        match existing.status.as_str() {
            "ready" | "disabled" | "missing" => return Ok(existing.to_result(item)),
            "error" if !retry_on_error => return Ok(existing.to_result(item)),
            _ => {}
        }

        let work_item = if let Some(work_item_id) = existing.active_work_item_id.as_deref() {
            load_work_item_by_id(tx, work_item_id)
                .await?
                .filter(|row| row.source_hash == source_hash)
        } else {
            None
        }
        .or(load_existing_work_item(tx, user_id, item, &source_hash).await?);

        if let Some(work_item) = work_item {
            if work_item.result_status.is_some()
                && matches!(work_item.status.as_str(), "completed" | "failed")
            {
                let result = terminal_result_from_work_row(&work_item, item.producer_ref.clone());
                persist_translation_terminal_state(
                    tx,
                    user_id,
                    item.kind.as_str(),
                    item.entity_id.as_str(),
                    item.target_lang.as_str(),
                    source_hash.as_str(),
                    result.status.as_str(),
                    result.title_zh.as_deref(),
                    result.summary_md.as_deref().or(result.body_md.as_deref()),
                    result.error.as_deref(),
                    work_item.id.as_str(),
                    now,
                )
                .await?;
                if retry_on_error && result.status == "error" {
                    reset_retryable_terminal_work_item(tx, work_item.id.as_str(), now).await?;
                    let request_id = if let Some(existing_request_id) =
                        load_latest_request_id_for_work_item(
                            tx,
                            work_item.id.as_str(),
                            source_hash.as_str(),
                        )
                        .await?
                    {
                        reset_request_for_retry(tx, existing_request_id.as_str(), now).await?;
                        existing_request_id
                    } else {
                        insert_translation_request_record(
                            tx,
                            user_id,
                            "async",
                            item,
                            request_origin,
                            source_hash.as_str(),
                            now,
                        )
                        .await?
                    };
                    attach_request_to_work_item(
                        tx,
                        request_id.as_str(),
                        work_item.id.as_str(),
                        "queued",
                        now,
                    )
                    .await?;
                    upsert_translation_demand_state(
                        tx,
                        user_id,
                        item,
                        source_hash.as_str(),
                        "queued",
                        work_item.id.as_str(),
                        now,
                    )
                    .await?;
                    return Ok(queued_request_result(item, Some(work_item.id)));
                }
                return Ok(result);
            }

            let pending = pending_result_from_work_row(&work_item, item.producer_ref.clone());
            upsert_translation_demand_state(
                tx,
                user_id,
                item,
                source_hash.as_str(),
                pending.status.as_str(),
                work_item.id.as_str(),
                now,
            )
            .await?;
            return Ok(pending);
        }

        if existing.status == "error" && !retry_on_error {
            return Ok(existing.to_result(item));
        }
    }

    let request_id = insert_translation_request_record(
        tx,
        user_id,
        "async",
        item,
        request_origin,
        source_hash.as_str(),
        now,
    )
    .await?;
    ensure_translation_result_demand(
        tx,
        user_id,
        item,
        source_hash.as_str(),
        request_id.as_str(),
        retry_on_error,
        now,
    )
    .await
}

async fn insert_translation_request_record(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    mode: &str,
    item: &TranslationRequestItemInput,
    request_origin: &str,
    source_hash: &str,
    now: &str,
) -> Result<String, ApiError> {
    let request_id = crate::local_id::generate_local_id();
    let source_blocks_json =
        serde_json::to_string(&item.source_blocks).map_err(ApiError::internal)?;
    let target_slots_json =
        serde_json::to_string(&item.target_slots).map_err(ApiError::internal)?;
    let source = derive_request_source(std::slice::from_ref(item));

    sqlx::query_scalar(
        r#"
        INSERT INTO translation_requests (
          id, mode, source, request_origin, requested_by, scope_user_id, producer_ref, kind,
          variant, entity_id, target_lang, max_wait_ms, source_hash, source_blocks_json,
          target_slots_json, status, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'queued', ?, ?)
        ON CONFLICT(
          mode, request_origin, scope_user_id, producer_ref, kind, variant, entity_id,
          target_lang, source_hash
        )
        DO UPDATE SET source = excluded.source,
                      requested_by = excluded.requested_by,
                      max_wait_ms = excluded.max_wait_ms,
                      source_blocks_json = excluded.source_blocks_json,
                      target_slots_json = excluded.target_slots_json,
                      updated_at = excluded.updated_at
        RETURNING id
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
    .bind(source_hash)
    .bind(source_blocks_json.as_str())
    .bind(target_slots_json.as_str())
    .bind(now)
    .bind(now)
    .fetch_one(&mut **tx)
    .await
    .map_err(ApiError::internal)
}

async fn insert_translation_request(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    mode: &str,
    item: &TranslationRequestItemInput,
    request_origin: &str,
    now: &str,
) -> Result<CreatedTranslationRequest, ApiError> {
    let source_hash = build_source_hash(item);
    let request_id = insert_translation_request_record(
        tx,
        user_id,
        mode,
        item,
        request_origin,
        source_hash.as_str(),
        now,
    )
    .await?;

    if let Some(cached) = load_cached_result(tx, user_id, item, &source_hash).await? {
        let status = request_status_from_result_status(cached.status.as_str());
        apply_request_result(tx, request_id.as_str(), None, &cached, now).await?;
        return Ok(CreatedTranslationRequest {
            request_id,
            status: status.to_owned(),
            result: cached,
        });
    }

    if let Some(work_item_id) = create_work_item(tx, user_id, item, &source_hash, now).await? {
        attach_request_to_work_item(tx, request_id.as_str(), &work_item_id, "queued", now).await?;
        upsert_translation_demand_state(
            tx,
            user_id,
            item,
            source_hash.as_str(),
            "queued",
            work_item_id.as_str(),
            now,
        )
        .await?;
        return Ok(CreatedTranslationRequest {
            request_id,
            status: "queued".to_owned(),
            result: queued_request_result(item, Some(work_item_id)),
        });
    }

    let existing = load_existing_work_item(tx, user_id, item, &source_hash)
        .await?
        .ok_or_else(|| ApiError::internal("translation work item missing after dedupe conflict"))?;

    if existing.result_status.as_deref() == Some("error")
        && matches!(existing.status.as_str(), "completed" | "failed")
    {
        reset_retryable_terminal_work_item(tx, existing.id.as_str(), now).await?;
        attach_request_to_work_item(tx, request_id.as_str(), &existing.id, "queued", now).await?;
        upsert_translation_demand_state(
            tx,
            user_id,
            item,
            source_hash.as_str(),
            "queued",
            existing.id.as_str(),
            now,
        )
        .await?;
        return Ok(CreatedTranslationRequest {
            request_id,
            status: "queued".to_owned(),
            result: queued_request_result(item, Some(existing.id.clone())),
        });
    }

    if existing.result_status.is_some()
        && matches!(existing.status.as_str(), "completed" | "failed")
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
        persist_translation_terminal_state(
            tx,
            user_id,
            item.kind.as_str(),
            item.entity_id.as_str(),
            item.target_lang.as_str(),
            source_hash.as_str(),
            result.status.as_str(),
            result.title_zh.as_deref(),
            result.summary_md.as_deref().or(result.body_md.as_deref()),
            result.error.as_deref(),
            existing.id.as_str(),
            now,
        )
        .await?;
        if let Some(batch_id) = existing.batch_id.as_deref() {
            refresh_batch_request_counters(tx, batch_id, existing.id.as_str(), now).await?;
        }
        return Ok(CreatedTranslationRequest {
            request_id,
            status,
            result,
        });
    }

    let status = request_status_from_work_item_status(existing.status.as_str()).to_owned();
    let result = pending_result_from_work_row(&existing, item.producer_ref.clone());
    attach_request_to_work_item(tx, request_id.as_str(), &existing.id, status.as_str(), now)
        .await?;
    upsert_translation_demand_state(
        tx,
        user_id,
        item,
        source_hash.as_str(),
        result.status.as_str(),
        existing.id.as_str(),
        now,
    )
    .await?;
    if let Some(batch_id) = existing.batch_id.as_deref() {
        refresh_batch_request_counters(tx, batch_id, existing.id.as_str(), now).await?;
    }
    Ok(CreatedTranslationRequest {
        request_id,
        status,
        result,
    })
}

async fn ensure_translation_result_demand(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    item: &TranslationRequestItemInput,
    source_hash: &str,
    request_id: &str,
    retry_on_error: bool,
    now: &str,
) -> Result<TranslationResultItem, ApiError> {
    if let Some(work_item_id) = create_work_item(tx, user_id, item, source_hash, now).await? {
        attach_request_to_work_item(tx, request_id, &work_item_id, "queued", now).await?;
        upsert_translation_demand_state(
            tx,
            user_id,
            item,
            source_hash,
            "queued",
            work_item_id.as_str(),
            now,
        )
        .await?;
        return Ok(queued_request_result(item, Some(work_item_id)));
    }

    let existing = load_existing_work_item(tx, user_id, item, source_hash)
        .await?
        .ok_or_else(|| ApiError::internal("translation work item missing after dedupe conflict"))?;

    if existing.result_status.is_some()
        && matches!(existing.status.as_str(), "completed" | "failed")
    {
        let result = terminal_result_from_work_row(&existing, item.producer_ref.clone());
        persist_translation_terminal_state(
            tx,
            user_id,
            item.kind.as_str(),
            item.entity_id.as_str(),
            item.target_lang.as_str(),
            source_hash,
            result.status.as_str(),
            result.title_zh.as_deref(),
            result.summary_md.as_deref().or(result.body_md.as_deref()),
            result.error.as_deref(),
            existing.id.as_str(),
            now,
        )
        .await?;
        if retry_on_error && result.status == "error" {
            reset_retryable_terminal_work_item(tx, existing.id.as_str(), now).await?;
            attach_request_to_work_item(tx, request_id, existing.id.as_str(), "queued", now)
                .await?;
            upsert_translation_demand_state(
                tx,
                user_id,
                item,
                source_hash,
                "queued",
                existing.id.as_str(),
                now,
            )
            .await?;
            return Ok(queued_request_result(item, Some(existing.id)));
        }
        apply_request_result(tx, request_id, Some(existing.id.as_str()), &result, now).await?;
        if let Some(batch_id) = existing.batch_id.as_deref() {
            refresh_batch_request_counters(tx, batch_id, existing.id.as_str(), now).await?;
        }
        return Ok(result);
    }

    let status = request_status_from_work_item_status(existing.status.as_str()).to_owned();
    let result = pending_result_from_work_row(&existing, item.producer_ref.clone());
    attach_request_to_work_item(tx, request_id, &existing.id, status.as_str(), now).await?;
    upsert_translation_demand_state(
        tx,
        user_id,
        item,
        source_hash,
        result.status.as_str(),
        existing.id.as_str(),
        now,
    )
    .await?;
    if let Some(batch_id) = existing.batch_id.as_deref() {
        refresh_batch_request_counters(tx, batch_id, existing.id.as_str(), now).await?;
    }
    Ok(result)
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

async fn load_latest_request_id_for_work_item(
    tx: &mut Transaction<'_, Sqlite>,
    work_item_id: &str,
    source_hash: &str,
) -> Result<Option<String>, ApiError> {
    sqlx::query_scalar(
        r#"
        SELECT id
        FROM translation_requests
        WHERE work_item_id = ? AND source_hash = ?
        ORDER BY created_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(work_item_id)
    .bind(source_hash)
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::internal)
}

async fn reset_request_for_retry(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: &str,
    now: &str,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        UPDATE translation_requests
        SET status = 'queued',
            result_status = NULL,
            title_zh = NULL,
            summary_md = NULL,
            body_md = NULL,
            error_text = NULL,
            started_at = NULL,
            finished_at = NULL,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(now)
    .bind(request_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn refresh_batch_request_counters(
    tx: &mut Transaction<'_, Sqlite>,
    batch_id: &str,
    work_item_id: &str,
    now: &str,
) -> Result<(), ApiError> {
    let producer_count = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*) FROM translation_requests WHERE work_item_id = ?"#,
    )
    .bind(work_item_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    sqlx::query(
        r#"
        UPDATE translation_batch_items
        SET producer_count = ?, updated_at = ?
        WHERE batch_id = ? AND work_item_id = ?
        "#,
    )
    .bind(producer_count)
    .bind(now)
    .bind(batch_id)
    .bind(work_item_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    let request_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM translation_requests
        WHERE work_item_id IN (
            SELECT work_item_id
            FROM translation_batch_items
            WHERE batch_id = ?
        )
        "#,
    )
    .bind(batch_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    sqlx::query(
        r#"
        UPDATE translation_batches
        SET request_count = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(request_count)
    .bind(now)
    .bind(batch_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    Ok(())
}

async fn refresh_live_batch_runtime_for_request(
    state: &AppState,
    created: &CreatedTranslationRequest,
) -> Result<(), ApiError> {
    if created.status != "running" {
        return Ok(());
    }
    let Some(batch_id) = created.result.batch_id.as_deref() else {
        return Ok(());
    };
    let row = sqlx::query_as::<_, RunningBatchRuntimeRow>(
        r#"
        SELECT id, worker_id, worker_slot, worker_kind, request_count, item_count, trigger_reason
        FROM translation_batches
        WHERE id = ? AND status = 'running'
        "#,
    )
    .bind(batch_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    let Some(row) = row else {
        return Ok(());
    };
    let profile = state
        .translation_scheduler
        .current_profile_by_worker_id(row.worker_id.as_str())
        .await
        .unwrap_or_else(|| {
            translation_worker_profile_from_runtime(
                row.worker_id.as_str(),
                row.worker_kind.as_str(),
                row.worker_slot,
            )
        });
    update_translation_worker_runtime(
        state,
        &profile,
        TranslationWorkerRuntimeUpdate::running(
            row.id.as_str(),
            row.request_count,
            row.item_count,
            row.trigger_reason.as_str(),
        ),
    )
    .await;
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

async fn load_translation_state_row(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    item: &TranslationRequestItemInput,
) -> Result<Option<TranslationStateRow>, ApiError> {
    let Some(entity_type) = map_entity_type(item.kind.as_str()) else {
        return Ok(None);
    };
    sqlx::query_as::<_, TranslationStateRow>(
        r#"
        SELECT id, user_id, entity_type, entity_id, lang, source_hash, status,
               title, summary, error_text, active_work_item_id, created_at, updated_at
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
    .map_err(ApiError::internal)
}

async fn load_cached_result(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    item: &TranslationRequestItemInput,
    source_hash: &str,
) -> Result<Option<TranslationResultItem>, ApiError> {
    let Some(row) = load_translation_state_row(tx, user_id, item).await? else {
        return Ok(None);
    };
    if row.source_hash != source_hash
        || !matches!(row.status.as_str(), "ready" | "disabled" | "missing")
    {
        return Ok(None);
    }
    Ok(Some(row.to_result(item)))
}

async fn upsert_translation_demand_state(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    item: &TranslationRequestItemInput,
    source_hash: &str,
    status: &str,
    work_item_id: &str,
    now: &str,
) -> Result<(), ApiError> {
    let Some(entity_type) = map_entity_type(item.kind.as_str()) else {
        return Ok(());
    };
    sqlx::query(
        r#"
        INSERT INTO ai_translations (
          id, user_id, entity_type, entity_id, lang, source_hash, status, title, summary,
          error_text, active_work_item_id, created_at, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL, NULL, ?, ?, ?)
        ON CONFLICT(user_id, entity_type, entity_id, lang)
        DO UPDATE SET source_hash = excluded.source_hash,
                      status = excluded.status,
                      title = NULL,
                      summary = NULL,
                      error_text = NULL,
                      active_work_item_id = excluded.active_work_item_id,
                      updated_at = excluded.updated_at
        WHERE ai_translations.source_hash = excluded.source_hash
           OR ai_translations.updated_at <= excluded.updated_at
        "#,
    )
    .bind(crate::local_id::generate_local_id())
    .bind(user_id)
    .bind(entity_type)
    .bind(item.entity_id.as_str())
    .bind(item.target_lang.as_str())
    .bind(source_hash)
    .bind(status)
    .bind(work_item_id)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn persist_translation_terminal_state(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    kind: &str,
    entity_id: &str,
    target_lang: &str,
    source_hash: &str,
    result_status: &str,
    title: Option<&str>,
    summary: Option<&str>,
    error_text: Option<&str>,
    work_item_id: &str,
    now: &str,
) -> Result<(), ApiError> {
    let Some(entity_type) = map_entity_type(kind) else {
        return Ok(());
    };
    let update = sqlx::query(
        r#"
        UPDATE ai_translations
        SET source_hash = ?,
            status = ?,
            title = ?,
            summary = ?,
            error_text = ?,
            active_work_item_id = NULL,
            updated_at = ?
        WHERE user_id = ?
          AND entity_type = ?
          AND entity_id = ?
          AND lang = ?
          AND (source_hash = ? OR active_work_item_id = ?)
        "#,
    )
    .bind(source_hash)
    .bind(result_status)
    .bind(title)
    .bind(summary)
    .bind(error_text)
    .bind(now)
    .bind(user_id)
    .bind(entity_type)
    .bind(entity_id)
    .bind(target_lang)
    .bind(source_hash)
    .bind(work_item_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    if update.rows_affected() > 0 {
        return Ok(());
    }

    sqlx::query(
        r#"
        INSERT INTO ai_translations (
          id, user_id, entity_type, entity_id, lang, source_hash, status, title, summary,
          error_text, active_work_item_id, created_at, updated_at
        )
        SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?
        WHERE NOT EXISTS (
          SELECT 1
          FROM ai_translations
          WHERE user_id = ? AND entity_type = ? AND entity_id = ? AND lang = ?
        )
        "#,
    )
    .bind(crate::local_id::generate_local_id())
    .bind(user_id)
    .bind(entity_type)
    .bind(entity_id)
    .bind(target_lang)
    .bind(source_hash)
    .bind(result_status)
    .bind(title)
    .bind(summary)
    .bind(error_text)
    .bind(now)
    .bind(now)
    .bind(user_id)
    .bind(entity_type)
    .bind(entity_id)
    .bind(target_lang)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn mark_translation_states_running_for_work_items(
    tx: &mut Transaction<'_, Sqlite>,
    work_item_ids: Vec<&str>,
    now: &str,
) -> Result<(), ApiError> {
    if work_item_ids.is_empty() {
        return Ok(());
    }
    let mut query = sqlx::QueryBuilder::<Sqlite>::new(
        r#"
        UPDATE ai_translations
        SET status = 'running',
            updated_at = "#,
    );
    query.push_bind(now);
    query.push(r#" WHERE status = 'queued' AND active_work_item_id IN ("#);
    {
        let mut separated = query.separated(", ");
        for id in work_item_ids {
            separated.push_bind(id);
        }
    }
    query.push(")");
    query
        .build()
        .execute(&mut **tx)
        .await
        .map_err(ApiError::internal)?;
    Ok(())
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

async fn load_work_item_by_id(
    tx: &mut Transaction<'_, Sqlite>,
    work_item_id: &str,
) -> Result<Option<WorkItemRow>, ApiError> {
    sqlx::query_as::<_, WorkItemRow>(
        r#"
        SELECT id, dedupe_key, scope_user_id, kind, variant, entity_id, target_lang, protocol_version,
               model_profile, source_hash, source_blocks_json, target_slots_json, token_estimate,
               deadline_at, status, batch_id, result_status, title_zh, summary_md, body_md,
               error_text, cache_hit, created_at, started_at, finished_at, updated_at
        FROM translation_work_items
        WHERE id = ?
        LIMIT 1
        "#,
    )
    .bind(work_item_id)
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
) -> Result<Option<String>, ApiError> {
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

    let result = sqlx::query(
        r#"
        INSERT INTO translation_work_items (
          id, dedupe_key, scope_user_id, kind, variant, entity_id, target_lang, protocol_version,
          model_profile, source_hash, source_blocks_json, target_slots_json, token_estimate,
          deadline_at, status, cache_hit, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'queued', 0, ?, ?)
        ON CONFLICT(dedupe_key) DO NOTHING
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

    if result.rows_affected() == 0 {
        Ok(None)
    } else {
        Ok(Some(id))
    }
}

async fn reset_retryable_terminal_work_item(
    tx: &mut Transaction<'_, Sqlite>,
    work_item_id: &str,
    now: &str,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        UPDATE translation_work_items
        SET status = 'queued',
            batch_id = NULL,
            result_status = NULL,
            title_zh = NULL,
            summary_md = NULL,
            body_md = NULL,
            error_text = NULL,
            started_at = NULL,
            finished_at = NULL,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(now)
    .bind(work_item_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

async fn run_translation_scheduler_once(
    state: &AppState,
    worker: TranslationWorkerProfile,
) -> Result<()> {
    if !state
        .translation_scheduler
        .worker_is_desired(worker.worker_id.as_str())
        .await
    {
        state
            .translation_scheduler
            .remove_worker_runtime(&state.pool, worker.worker_id.as_str())
            .await?;
        return Ok(());
    }

    let Some(batch) = claim_next_batch(state, worker.clone()).await? else {
        if state
            .translation_scheduler
            .worker_is_desired(worker.worker_id.as_str())
            .await
        {
            update_translation_worker_runtime(
                state,
                &worker,
                TranslationWorkerRuntimeUpdate::idle(),
            )
            .await;
        } else {
            state
                .translation_scheduler
                .remove_worker_runtime(&state.pool, worker.worker_id.as_str())
                .await?;
        }
        return Ok(());
    };
    execute_claimed_batch(state, batch).await
}

async fn claim_next_batch(
    state: &AppState,
    worker: TranslationWorkerProfile,
) -> Result<Option<ClaimedBatch>> {
    let _claim_guard = translation_batch_claim_lock().lock().await;
    if !state
        .translation_scheduler
        .worker_is_desired(worker.worker_id.as_str())
        .await
    {
        return Ok(None);
    }
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

    let budget = i64::from(translation_batch_input_budget(state).await);
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
          worker_id, worker_slot, worker_kind, request_count, item_count, estimated_input_tokens,
          status, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'queued', ?, ?)
        "#,
    )
    .bind(batch_id.as_str())
    .bind(partition_key.as_str())
    .bind(first.protocol_version.as_str())
    .bind(first.model_profile.as_str())
    .bind(first.target_lang.as_str())
    .bind(trigger_reason.as_str())
    .bind(worker.worker_id.as_str())
    .bind(worker.worker_slot)
    .bind(worker.worker_kind.as_str())
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
    mark_requests_running_for_work_items_in_tx(
        &mut tx,
        selected.iter().map(|item| item.id.as_str()).collect(),
        now_str.as_str(),
    )
    .await?;
    tx.commit().await?;

    Ok(Some(ClaimedBatch {
        id: batch_id,
        worker_id: worker.worker_id.clone(),
        worker_slot: worker.worker_slot,
        worker_kind: worker.worker_kind.clone(),
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
    let worker = translation_worker_profile_from_runtime(
        batch.worker_id.as_str(),
        batch.worker_kind.as_str(),
        batch.worker_slot,
    );
    sqlx::query(
        r#"
        UPDATE translation_batches
        SET status = 'running',
            started_at = ?,
            runtime_owner_id = ?,
            lease_heartbeat_at = ?,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(now.as_str())
    .bind(state.runtime_owner_id.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(batch.id.as_str())
    .execute(&state.pool)
    .await?;
    update_translation_worker_runtime(
        state,
        &worker,
        TranslationWorkerRuntimeUpdate::running(
            batch.id.as_str(),
            batch.request_count,
            i64::try_from(batch.items.len()).unwrap_or(i64::MAX),
            batch.trigger_reason.as_str(),
        ),
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
    let heartbeat =
        spawn_translation_batch_lease_heartbeat(Arc::new(state.clone()), batch.id.clone());

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
            heartbeat.stop().await;
            if state
                .translation_scheduler
                .worker_is_desired(worker.worker_id.as_str())
                .await
            {
                update_translation_worker_runtime(
                    state,
                    &worker,
                    TranslationWorkerRuntimeUpdate::idle(),
                )
                .await;
            } else {
                state
                    .translation_scheduler
                    .remove_worker_runtime(&state.pool, worker.worker_id.as_str())
                    .await?;
            }
            res
        }
        Err(err) => {
            let error = err.to_string();
            let res = finalize_batch_failure(state, &batch, err.into()).await;
            heartbeat.stop().await;
            if state
                .translation_scheduler
                .worker_is_desired(worker.worker_id.as_str())
                .await
            {
                update_translation_worker_runtime(
                    state,
                    &worker,
                    TranslationWorkerRuntimeUpdate::error(error.as_str()),
                )
                .await;
            } else {
                state
                    .translation_scheduler
                    .remove_worker_runtime(&state.pool, worker.worker_id.as_str())
                    .await?;
            }
            res
        }
    }
}

fn spawn_translation_batch_lease_heartbeat(
    state: Arc<AppState>,
    batch_id: String,
) -> runtime::LeaseHeartbeat {
    runtime::spawn_lease_heartbeat(
        "translation_batches",
        runtime::RUNTIME_LEASE_HEARTBEAT_INTERVAL,
        move || {
            let state = state.clone();
            let batch_id = batch_id.clone();
            async move { heartbeat_translation_batch_lease(state.as_ref(), batch_id.as_str()).await }
        },
    )
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

        if let Some(work_item) = batch
            .items
            .iter()
            .find(|item| item.id == result.work_item_id)
        {
            persist_translation_terminal_state(
                &mut tx,
                &work_item.scope_user_id,
                work_item.kind.as_str(),
                work_item.entity_id.as_str(),
                work_item.target_lang.as_str(),
                work_item.source_hash.as_str(),
                result.result_status.as_str(),
                result.title_zh.as_deref(),
                result.summary_md.as_deref().or(result.body_md.as_deref()),
                result.error.as_deref(),
                work_item.id.as_str(),
                now.as_str(),
            )
            .await?;
        }
    }

    sqlx::query(
        r#"
        UPDATE translation_batches
        SET status = 'completed',
            finished_at = ?,
            runtime_owner_id = NULL,
            lease_heartbeat_at = NULL,
            updated_at = ?
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
    fail_batch_with_message(
        &mut tx,
        batch.id.as_str(),
        &batch.items,
        message.as_str(),
        now.as_str(),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn heartbeat_translation_batch_lease(state: &AppState, batch_id: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE translation_batches
        SET lease_heartbeat_at = ?, updated_at = ?
        WHERE id = ?
          AND status = 'running'
          AND runtime_owner_id = ?
        "#,
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(batch_id)
    .bind(state.runtime_owner_id.as_str())
    .execute(&state.pool)
    .await?;
    Ok(())
}

async fn load_batch_work_items(
    tx: &mut Transaction<'_, Sqlite>,
    batch_id: &str,
) -> Result<Vec<WorkItemRow>, ApiError> {
    sqlx::query_as::<_, WorkItemRow>(
        r#"
        SELECT
          w.id, w.dedupe_key, w.scope_user_id, w.kind, w.variant, w.entity_id, w.target_lang,
          w.protocol_version, w.model_profile, w.source_hash, w.source_blocks_json,
          w.target_slots_json, w.token_estimate, w.deadline_at, w.status, w.batch_id,
          w.result_status, w.title_zh, w.summary_md, w.body_md, w.error_text, w.cache_hit,
          w.created_at, w.started_at, w.finished_at, w.updated_at
        FROM translation_batch_items bi
        JOIN translation_work_items w ON w.id = bi.work_item_id
        WHERE bi.batch_id = ?
        ORDER BY bi.item_index ASC
        "#,
    )
    .bind(batch_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(ApiError::internal)
}

async fn fail_batch_with_message(
    tx: &mut Transaction<'_, Sqlite>,
    batch_id: &str,
    items: &[WorkItemRow],
    message: &str,
    now: &str,
) -> Result<(), ApiError> {
    for item in items {
        sqlx::query(
            r#"
            UPDATE translation_work_items
            SET status = 'failed',
                result_status = 'error',
                error_text = ?,
                finished_at = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(message)
        .bind(now)
        .bind(now)
        .bind(item.id.as_str())
        .execute(&mut **tx)
        .await
        .map_err(ApiError::internal)?;
        sqlx::query(
            r#"
            UPDATE translation_batch_items
            SET result_status = 'error', error_text = ?, updated_at = ?
            WHERE batch_id = ? AND work_item_id = ?
            "#,
        )
        .bind(message)
        .bind(now)
        .bind(batch_id)
        .bind(item.id.as_str())
        .execute(&mut **tx)
        .await
        .map_err(ApiError::internal)?;
        let requests = sqlx::query(
            r#"
            SELECT id, producer_ref, entity_id, kind, variant
            FROM translation_requests
            WHERE work_item_id = ?
            "#,
        )
        .bind(item.id.as_str())
        .fetch_all(&mut **tx)
        .await
        .map_err(ApiError::internal)?;
        for request in requests {
            let request_id: String = request.try_get("id").map_err(ApiError::internal)?;
            let producer_ref: String = request
                .try_get("producer_ref")
                .map_err(ApiError::internal)?;
            let entity_id: String = request.try_get("entity_id").map_err(ApiError::internal)?;
            let kind: String = request.try_get("kind").map_err(ApiError::internal)?;
            let variant: String = request.try_get("variant").map_err(ApiError::internal)?;
            let request_result = TranslationResultItem {
                producer_ref,
                entity_id,
                kind,
                variant,
                status: "error".to_owned(),
                title_zh: None,
                summary_md: None,
                body_md: None,
                error: Some(message.to_owned()),
                work_item_id: Some(item.id.clone()),
                batch_id: Some(batch_id.to_owned()),
            };
            apply_request_result(
                tx,
                request_id.as_str(),
                Some(item.id.as_str()),
                &request_result,
                now,
            )
            .await?;
        }

        persist_translation_terminal_state(
            tx,
            item.scope_user_id.as_str(),
            item.kind.as_str(),
            item.entity_id.as_str(),
            item.target_lang.as_str(),
            item.source_hash.as_str(),
            "error",
            None,
            None,
            Some(message),
            item.id.as_str(),
            now,
        )
        .await?;
    }
    sqlx::query(
        r#"
        UPDATE translation_batches
        SET status = 'failed',
            error_text = ?,
            finished_at = ?,
            runtime_owner_id = NULL,
            lease_heartbeat_at = NULL,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(message)
    .bind(now)
    .bind(now)
    .bind(batch_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
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
    struct StaleBatchRow {
        id: String,
        runtime_owner_id: Option<String>,
        lease_heartbeat_at: Option<String>,
    }

    let now = Utc::now();
    let cutoff = runtime::stale_cutoff_timestamp(now);
    let stale_batches = match mode {
        runtime::RuntimeRecoveryMode::Startup => {
            sqlx::query_as::<_, StaleBatchRow>(
                r#"
                SELECT id, runtime_owner_id, lease_heartbeat_at
                FROM translation_batches
                WHERE status = 'running'
                  AND (
                    runtime_owner_id IS NULL
                    OR lease_heartbeat_at IS NULL
                    OR julianday(lease_heartbeat_at) <= julianday(?)
                    OR (
                      runtime_owner_id != ?
                      AND NOT EXISTS (
                        SELECT 1
                        FROM runtime_owners
                        WHERE runtime_owner_id = translation_batches.runtime_owner_id
                          AND julianday(lease_heartbeat_at) > julianday(?)
                      )
                    )
                  )
                ORDER BY created_at ASC, id ASC
                "#,
            )
            .bind(cutoff.as_str())
            .bind(state.runtime_owner_id.as_str())
            .bind(cutoff.as_str())
            .fetch_all(&state.pool)
            .await?
        }
        runtime::RuntimeRecoveryMode::Sweep => {
            sqlx::query_as::<_, StaleBatchRow>(
                r#"
                SELECT id, runtime_owner_id, lease_heartbeat_at
                FROM translation_batches
                WHERE status = 'running'
                  AND (
                    runtime_owner_id IS NULL
                    OR lease_heartbeat_at IS NULL
                    OR julianday(lease_heartbeat_at) <= julianday(?)
                  )
                ORDER BY created_at ASC, id ASC
                "#,
            )
            .bind(cutoff.as_str())
            .fetch_all(&state.pool)
            .await?
        }
    };

    for batch in stale_batches {
        ai::recover_linked_llm_calls_for_batch(
            state,
            batch.id.as_str(),
            runtime::RUNTIME_LEASE_EXPIRED_ERROR,
            batch.runtime_owner_id.as_deref(),
            batch.lease_heartbeat_at.as_deref(),
        )
        .await?;

        let mut tx = state.pool.begin().await?;
        let items = load_batch_work_items(&mut tx, batch.id.as_str()).await?;
        let now = Utc::now().to_rfc3339();
        fail_batch_with_message(
            &mut tx,
            batch.id.as_str(),
            &items,
            runtime::RUNTIME_LEASE_EXPIRED_ERROR,
            now.as_str(),
        )
        .await?;
        tx.commit().await?;
    }

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

async fn mark_requests_running_for_work_items_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    work_item_ids: Vec<&str>,
    now: &str,
) -> Result<()> {
    if work_item_ids.is_empty() {
        return Ok(());
    }
    let state_work_item_ids = work_item_ids.clone();
    let mut query = sqlx::QueryBuilder::<Sqlite>::new(
        r#"
        UPDATE translation_requests
        SET status = 'running', started_at = COALESCE(started_at, "#,
    );
    query.push_bind(now);
    query.push(r#"), updated_at = "#);
    query.push_bind(now);
    query.push(r#" WHERE status = 'queued' AND work_item_id IN ("#);
    {
        let mut separated = query.separated(", ");
        for id in work_item_ids {
            separated.push_bind(id);
        }
    }
    query.push(")");
    query.build().execute(&mut **tx).await?;
    mark_translation_states_running_for_work_items(tx, state_work_item_ids, now).await?;
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
        let created_at =
            parse_ts(detail.request.created_at.as_str()).map_err(ApiError::internal)?;
        let wait_budget = chrono::Duration::milliseconds(detail.request.max_wait_ms.max(0));
        if Utc::now() >= created_at + wait_budget {
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
    use std::{net::SocketAddr, sync::Arc, time::Duration};

    use sqlx::{
        SqlitePool,
        sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    };
    use url::Url;

    use super::*;
    use crate::{
        config::{AppConfig, GitHubOAuthConfig},
        crypto::EncryptionKey,
        state::build_oauth_client,
    };

    fn test_dedicated_worker_slot() -> i64 {
        i64::try_from(DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY + 1)
            .expect("dedicated worker slot should fit in i64")
    }

    fn test_worker_profile(worker_slot: i64, worker_kind: &str) -> TranslationWorkerProfile {
        let worker_index = if worker_kind == "user_dedicated" {
            usize::try_from(
                worker_slot.saturating_sub(DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY as i64),
            )
            .unwrap_or(1)
            .max(1)
        } else {
            usize::try_from(worker_slot).unwrap_or(1).max(1)
        };
        TranslationWorkerProfile {
            worker_id: translation_worker_id(worker_kind, worker_index),
            worker_slot,
            worker_kind: worker_kind.to_owned(),
        }
    }

    #[test]
    fn parse_positive_worker_concurrency_rejects_values_above_max() {
        let overflow =
            i64::try_from(MAX_TRANSLATION_WORKER_CONCURRENCY).expect("max fits in i64") + 1;
        let err = parse_positive_worker_concurrency(overflow, "general_worker_concurrency")
            .expect_err("overflow concurrency should fail");

        assert!(
            err.to_string().contains(&format!(
                "general_worker_concurrency must be a positive integer <= {MAX_TRANSLATION_WORKER_CONCURRENCY}"
            )),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_translation_worker_concurrency_total_rejects_combined_overflow() {
        let err =
            validate_translation_worker_concurrency_total(MAX_TRANSLATION_WORKER_CONCURRENCY, 1)
                .expect_err("combined worker concurrency should fail");

        assert!(
            err.to_string().contains(&format!(
                "general_worker_concurrency + dedicated_worker_concurrency must be <= {MAX_TRANSLATION_WORKER_CONCURRENCY}"
            )),
            "unexpected error: {err}"
        );
    }

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
    async fn create_translation_request_reuses_batched_work_item_updates_batch_counters() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("batched-window");

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
            INSERT INTO translation_batches (
              id, partition_key, protocol_version, model_profile, target_lang, trigger_reason,
              worker_slot, request_count, item_count, estimated_input_tokens, status, created_at,
              updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'queued', ?, ?)
            "#,
        )
        .bind("batch-queued-window")
        .bind("general:release_summary:feed_card:zh-CN")
        .bind(TRANSLATION_PROTOCOL_VERSION)
        .bind(current_model_profile())
        .bind("zh-CN")
        .bind("deadline_due")
        .bind(1_i64)
        .bind(1_i64)
        .bind(1_i64)
        .bind(128_i64)
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&pool)
        .await
        .expect("insert batch");

        sqlx::query(
            r#"
            INSERT INTO translation_batch_items (
              id, batch_id, work_item_id, item_index, kind, variant, entity_id, producer_count,
              token_estimate, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("batch-item-1")
        .bind("batch-queued-window")
        .bind(work_item_id.as_str())
        .bind(0_i64)
        .bind(item.kind.as_str())
        .bind(item.variant.as_str())
        .bind(item.entity_id.as_str())
        .bind(1_i64)
        .bind(128_i64)
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&pool)
        .await
        .expect("insert batch item");

        sqlx::query(
            r#"
            UPDATE translation_work_items
            SET status = 'batched', batch_id = 'batch-queued-window', updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(now.as_str())
        .bind(work_item_id.as_str())
        .execute(&pool)
        .await
        .expect("mark work item batched");

        let second = create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("second request created");
        assert_eq!(second.status, "running");
        assert_eq!(second.result.status, "running");
        assert_eq!(
            second.result.batch_id.as_deref(),
            Some("batch-queued-window")
        );

        let stored =
            sqlx::query("SELECT status, started_at FROM translation_requests WHERE id = ?")
                .bind(second.request_id.as_str())
                .fetch_one(&pool)
                .await
                .expect("load stored request state");
        assert_eq!(stored.get::<String, _>("status"), "running");
        assert!(stored.get::<Option<String>, _>("started_at").is_some());

        let batch = sqlx::query("SELECT request_count FROM translation_batches WHERE id = ?")
            .bind("batch-queued-window")
            .fetch_one(&pool)
            .await
            .expect("load batch counters");
        assert_eq!(batch.get::<i64, _>("request_count"), 1);

        let batch_item = sqlx::query(
            "SELECT producer_count FROM translation_batch_items WHERE batch_id = ? AND work_item_id = ?",
        )
        .bind("batch-queued-window")
        .bind(work_item_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load batch item counters");
        assert_eq!(batch_item.get::<i64, _>("producer_count"), 1);
    }

    #[tokio::test]
    async fn create_translation_request_retries_terminal_error_work_item() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("batched-terminal-window");

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
            INSERT INTO translation_batches (
              id, partition_key, protocol_version, model_profile, target_lang, trigger_reason,
              worker_slot, request_count, item_count, estimated_input_tokens, status, created_at,
              updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'completed', ?, ?)
            "#,
        )
        .bind("batch-terminal-window")
        .bind("general:release_summary:feed_card:zh-CN")
        .bind(TRANSLATION_PROTOCOL_VERSION)
        .bind(current_model_profile())
        .bind("zh-CN")
        .bind("deadline_due")
        .bind(1_i64)
        .bind(1_i64)
        .bind(1_i64)
        .bind(128_i64)
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&pool)
        .await
        .expect("insert batch");

        sqlx::query(
            r#"
            INSERT INTO translation_batch_items (
              id, batch_id, work_item_id, item_index, kind, variant, entity_id, producer_count,
              token_estimate, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("batch-item-terminal-1")
        .bind("batch-terminal-window")
        .bind(work_item_id.as_str())
        .bind(0_i64)
        .bind(item.kind.as_str())
        .bind(item.variant.as_str())
        .bind(item.entity_id.as_str())
        .bind(1_i64)
        .bind(128_i64)
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&pool)
        .await
        .expect("insert batch item");

        sqlx::query(
            r#"
            UPDATE translation_work_items
            SET status = 'failed',
                batch_id = 'batch-terminal-window',
                result_status = 'error',
                error_text = 'boom',
                finished_at = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(work_item_id.as_str())
        .execute(&pool)
        .await
        .expect("mark work item failed");

        let second = create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("second request created");
        assert_eq!(second.status, "queued");
        assert_eq!(second.result.status, "queued");
        assert_eq!(second.result.batch_id, None);
        assert_eq!(
            second.result.work_item_id.as_deref(),
            Some(work_item_id.as_str())
        );

        let batch = sqlx::query("SELECT request_count FROM translation_batches WHERE id = ?")
            .bind("batch-terminal-window")
            .fetch_one(&pool)
            .await
            .expect("load batch counters");
        assert_eq!(batch.get::<i64, _>("request_count"), 1);

        let batch_item = sqlx::query(
            "SELECT producer_count FROM translation_batch_items WHERE batch_id = ? AND work_item_id = ?",
        )
        .bind("batch-terminal-window")
        .bind(work_item_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load batch item counters");
        assert_eq!(batch_item.get::<i64, _>("producer_count"), 1);

        let stored_request = sqlx::query(
            "SELECT status, result_status, work_item_id FROM translation_requests WHERE id = ?",
        )
        .bind(second.request_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load retried request");
        assert_eq!(stored_request.get::<String, _>("status"), "queued");
        assert!(
            stored_request
                .get::<Option<String>, _>("result_status")
                .is_none()
        );
        assert_eq!(
            stored_request
                .get::<Option<String>, _>("work_item_id")
                .as_deref(),
            Some(work_item_id.as_str())
        );

        let refreshed_work_item = sqlx::query(
            r#"
            SELECT status, batch_id, result_status, error_text, started_at, finished_at
            FROM translation_work_items
            WHERE id = ?
            "#,
        )
        .bind(work_item_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load refreshed work item");
        assert_eq!(refreshed_work_item.get::<String, _>("status"), "queued");
        assert!(
            refreshed_work_item
                .get::<Option<String>, _>("batch_id")
                .is_none()
        );
        assert!(
            refreshed_work_item
                .get::<Option<String>, _>("result_status")
                .is_none()
        );
        assert!(
            refreshed_work_item
                .get::<Option<String>, _>("error_text")
                .is_none()
        );
        assert!(
            refreshed_work_item
                .get::<Option<String>, _>("started_at")
                .is_none()
        );
        assert!(
            refreshed_work_item
                .get::<Option<String>, _>("finished_at")
                .is_none()
        );
    }

    #[tokio::test]
    async fn create_translation_request_reuses_existing_request_row() {
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
        let distinct_work_item_ids: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT work_item_id) FROM translation_requests WHERE id IN (?, ?)",
        )
        .bind(first.request_id.as_str())
        .bind(second.request_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("count distinct work item ids");

        assert_eq!(requests, 1);
        assert_eq!(work_items, 1);
        assert_eq!(distinct_work_item_ids, 1);
        assert_eq!(first.request_id, second.request_id);
    }

    #[tokio::test]
    async fn create_translation_request_reuses_existing_request_row_under_concurrency() {
        let pool = setup_pool_with_max_connections(4).await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("concurrent-123");

        let first_state = state.clone();
        let second_state = state.clone();
        let first_item = item.clone();
        let second_item = item.clone();

        let (first, second) = tokio::join!(
            create_translation_request(first_state.as_ref(), "1", "async", &first_item),
            create_translation_request(second_state.as_ref(), "1", "async", &second_item),
        );

        let first = first.expect("first concurrent request created");
        let second = second.expect("second concurrent request created");

        let requests: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translation_requests")
            .fetch_one(&pool)
            .await
            .expect("count requests");
        let work_items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translation_work_items")
            .fetch_one(&pool)
            .await
            .expect("count work items");
        let distinct_work_item_ids: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT work_item_id) FROM translation_requests WHERE id IN (?, ?)",
        )
        .bind(first.request_id.as_str())
        .bind(second.request_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("count distinct work item ids");

        assert_eq!(requests, 1);
        assert_eq!(work_items, 1);
        assert_eq!(distinct_work_item_ids, 1);
        assert_eq!(first.request_id, second.request_id);
    }

    #[tokio::test]
    async fn resolve_translation_results_reuses_existing_request_row() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("resolve-123");

        let first = resolve_translation_results_for_user(
            state.as_ref(),
            "1",
            std::slice::from_ref(&item),
            false,
        )
        .await
        .expect("first resolve call");
        let second = resolve_translation_results_for_user(
            state.as_ref(),
            "1",
            std::slice::from_ref(&item),
            false,
        )
        .await
        .expect("second resolve call");

        let requests: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translation_requests")
            .fetch_one(&pool)
            .await
            .expect("count requests");
        let work_items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translation_work_items")
            .fetch_one(&pool)
            .await
            .expect("count work items");
        let result_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ai_translations")
            .fetch_one(&pool)
            .await
            .expect("count result rows");

        assert_eq!(requests, 1);
        assert_eq!(work_items, 1);
        assert_eq!(result_rows, 1);
        assert_eq!(first[0].status, "queued");
        assert_eq!(second[0].status, "queued");
        assert_eq!(first[0].work_item_id, second[0].work_item_id);
    }

    #[tokio::test]
    async fn resolve_translation_results_dedupes_request_rows_under_concurrency() {
        let pool = setup_pool_with_max_connections(4).await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("resolve-concurrent-123");

        let first_state = state.clone();
        let second_state = state.clone();
        let first_item = item.clone();
        let second_item = item.clone();

        let (first, second) = tokio::join!(
            resolve_translation_results_for_user(
                first_state.as_ref(),
                "1",
                std::slice::from_ref(&first_item),
                false,
            ),
            resolve_translation_results_for_user(
                second_state.as_ref(),
                "1",
                std::slice::from_ref(&second_item),
                false,
            ),
        );

        let first = first.expect("first concurrent resolve");
        let second = second.expect("second concurrent resolve");

        let requests: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translation_requests")
            .fetch_one(&pool)
            .await
            .expect("count requests");
        let work_items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translation_work_items")
            .fetch_one(&pool)
            .await
            .expect("count work items");

        assert_eq!(requests, 1);
        assert_eq!(work_items, 1);
        assert_eq!(first[0].work_item_id, second[0].work_item_id);
    }

    #[tokio::test]
    async fn resolve_translation_results_reuses_ready_result_row_without_requests() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("ready-123");
        let source_hash = build_source_hash(&item);

        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status, title, summary,
              error_text, active_work_item_id, created_at, updated_at
            )
            VALUES (?, ?, 'release', ?, 'zh-CN', ?, 'ready', ?, ?, NULL, NULL, ?, ?)
            "#,
        )
        .bind(crate::local_id::generate_local_id())
        .bind("1")
        .bind(item.entity_id.as_str())
        .bind(source_hash.as_str())
        .bind("已翻译标题")
        .bind("- 已翻译摘要")
        .bind("2026-03-30T00:00:00Z")
        .bind("2026-03-30T00:00:00Z")
        .execute(&pool)
        .await
        .expect("seed ready translation row");

        let first = resolve_translation_results_for_user(
            state.as_ref(),
            "1",
            std::slice::from_ref(&item),
            false,
        )
        .await
        .expect("first resolve call");
        let second = resolve_translation_results_for_user(
            state.as_ref(),
            "1",
            std::slice::from_ref(&item),
            false,
        )
        .await
        .expect("second resolve call");

        let requests: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translation_requests")
            .fetch_one(&pool)
            .await
            .expect("count requests");
        let work_items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translation_work_items")
            .fetch_one(&pool)
            .await
            .expect("count work items");

        assert_eq!(requests, 0);
        assert_eq!(work_items, 0);
        assert_eq!(first[0].status, "ready");
        assert_eq!(second[0].status, "ready");
        assert_eq!(first[0].title_zh.as_deref(), Some("已翻译标题"));
        assert_eq!(first[0].summary_md.as_deref(), Some("- 已翻译摘要"));
        assert_eq!(first[0].work_item_id, None);
        assert_eq!(second[0].work_item_id, None);
    }

    #[tokio::test]
    async fn resolve_translation_results_keeps_terminal_error_without_retry_flag() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("retry-123");

        let created = create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("request created");
        let work_item_id = created
            .result
            .work_item_id
            .clone()
            .expect("work item attached");

        sqlx::query(
            r#"
            UPDATE translation_requests
            SET status = 'failed',
                result_status = 'error',
                error_text = 'boom',
                finished_at = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind("2026-03-30T00:00:01Z")
        .bind("2026-03-30T00:00:01Z")
        .bind(created.request_id.as_str())
        .execute(&pool)
        .await
        .expect("mark request failed");

        sqlx::query(
            r#"
            UPDATE translation_work_items
            SET status = 'failed',
                result_status = 'error',
                error_text = 'boom',
                finished_at = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind("2026-03-30T00:00:01Z")
        .bind("2026-03-30T00:00:01Z")
        .bind(work_item_id.as_str())
        .execute(&pool)
        .await
        .expect("mark work item failed");

        let resolved = resolve_translation_results_for_user(
            state.as_ref(),
            "1",
            std::slice::from_ref(&item),
            false,
        )
        .await
        .expect("resolve after error without retry");

        let requests: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translation_requests")
            .fetch_one(&pool)
            .await
            .expect("count requests");
        let request_status: String =
            sqlx::query_scalar("SELECT status FROM translation_requests WHERE id = ? LIMIT 1")
                .bind(created.request_id.as_str())
                .fetch_one(&pool)
                .await
                .expect("load request status");
        let request_result_status: Option<String> = sqlx::query_scalar(
            "SELECT result_status FROM translation_requests WHERE id = ? LIMIT 1",
        )
        .bind(created.request_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load request result status");
        let work_item_status: String =
            sqlx::query_scalar("SELECT status FROM translation_work_items WHERE id = ? LIMIT 1")
                .bind(work_item_id.as_str())
                .fetch_one(&pool)
                .await
                .expect("load work item status");

        assert_eq!(requests, 1);
        assert_eq!(resolved[0].status, "error");
        assert_eq!(request_status, "failed");
        assert_eq!(request_result_status.as_deref(), Some("error"));
        assert_eq!(work_item_status, "failed");
        assert_eq!(
            resolved[0].work_item_id.as_deref(),
            Some(work_item_id.as_str())
        );
    }

    #[tokio::test]
    async fn resolve_translation_results_retries_terminal_error_when_forced() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("retry-forced-123");

        let created = create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("request created");
        let replay = create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("second request created");
        let work_item_id = created
            .result
            .work_item_id
            .clone()
            .expect("work item attached");
        assert_eq!(
            replay.result.work_item_id.as_deref(),
            Some(work_item_id.as_str())
        );
        assert_eq!(replay.request_id, created.request_id);

        sqlx::query(
            r#"
            UPDATE translation_requests
            SET status = 'failed',
                result_status = 'error',
                error_text = 'boom',
                finished_at = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind("2026-03-30T00:00:01Z")
        .bind("2026-03-30T00:00:01Z")
        .bind(created.request_id.as_str())
        .execute(&pool)
        .await
        .expect("mark first request failed");

        sqlx::query(
            r#"
            UPDATE translation_requests
            SET status = 'failed',
                result_status = 'error',
                error_text = 'boom',
                finished_at = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind("2026-03-30T00:00:02Z")
        .bind("2026-03-30T00:00:02Z")
        .bind(replay.request_id.as_str())
        .execute(&pool)
        .await
        .expect("mark second request failed");

        sqlx::query(
            r#"
            UPDATE translation_work_items
            SET status = 'failed',
                result_status = 'error',
                error_text = 'boom',
                finished_at = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind("2026-03-30T00:00:01Z")
        .bind("2026-03-30T00:00:01Z")
        .bind(work_item_id.as_str())
        .execute(&pool)
        .await
        .expect("mark work item failed");

        let resolved = resolve_translation_results_for_user(
            state.as_ref(),
            "1",
            std::slice::from_ref(&item),
            true,
        )
        .await
        .expect("resolve after forced retry");

        let requests: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translation_requests")
            .fetch_one(&pool)
            .await
            .expect("count requests");
        let request_rows: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
            r#"
            SELECT id, status, result_status, work_item_id
            FROM translation_requests
            ORDER BY created_at ASC, id ASC
            "#,
        )
        .fetch_all(&pool)
        .await
        .expect("load request rows");
        let work_item_status: String =
            sqlx::query_scalar("SELECT status FROM translation_work_items WHERE id = ? LIMIT 1")
                .bind(work_item_id.as_str())
                .fetch_one(&pool)
                .await
                .expect("load work item status");

        assert_eq!(requests, 1);
        assert_eq!(resolved[0].status, "queued");
        assert_eq!(request_rows.len(), 1);
        assert_eq!(request_rows[0].0, created.request_id);
        assert_eq!(request_rows[0].1, "queued");
        assert_eq!(request_rows[0].2, None);
        assert_eq!(request_rows[0].3.as_deref(), Some(work_item_id.as_str()));
        assert_eq!(work_item_status, "queued");
        assert_eq!(
            resolved[0].work_item_id.as_deref(),
            Some(work_item_id.as_str())
        );
    }

    #[tokio::test]
    async fn upsert_translation_demand_state_keeps_newer_source_hash() {
        let pool = setup_pool().await;
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("source-guard");
        let newer_hash = build_source_hash(&item);
        let older_hash = "older-source-hash";

        sqlx::query(
            r#"
            INSERT INTO ai_translations (
              id, user_id, entity_type, entity_id, lang, source_hash, status, title, summary,
              error_text, active_work_item_id, created_at, updated_at
            )
            VALUES (?, ?, 'release', ?, 'zh-CN', ?, 'running', NULL, NULL, NULL, ?, ?, ?)
            "#,
        )
        .bind(crate::local_id::generate_local_id())
        .bind("1")
        .bind(item.entity_id.as_str())
        .bind(newer_hash.as_str())
        .bind("new-work-item")
        .bind("2026-03-30T00:00:02Z")
        .bind("2026-03-30T00:00:02Z")
        .execute(&pool)
        .await
        .expect("seed newer translation state");

        let mut tx = pool.begin().await.expect("begin tx");
        upsert_translation_demand_state(
            &mut tx,
            "1",
            &item,
            older_hash,
            "queued",
            "old-work-item",
            "2026-03-30T00:00:01Z",
        )
        .await
        .expect("stale demand upsert");
        tx.commit().await.expect("commit tx");

        let row: (String, String, String) = sqlx::query_as(
            r#"
            SELECT source_hash, status, active_work_item_id
            FROM ai_translations
            WHERE user_id = ? AND entity_type = 'release' AND entity_id = ? AND lang = 'zh-CN'
            LIMIT 1
            "#,
        )
        .bind("1")
        .bind(item.entity_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load translation state");

        assert_eq!(row.0, newer_hash);
        assert_eq!(row.1, "running");
        assert_eq!(row.2, "new-work-item");
    }

    #[tokio::test]
    async fn wait_for_request_terminal_returns_latest_snapshot_after_wait_budget() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;
        let mut item = sample_release_item("wait-budget");
        item.max_wait_ms = 0;

        let created = create_translation_request(state.as_ref(), "1", "wait", &item)
            .await
            .expect("request created");

        let detail = wait_for_request_terminal(state.as_ref(), "1", created.request_id.as_str())
            .await
            .expect("wait result loaded");

        assert_eq!(detail.request.id, created.request_id);
        assert_eq!(detail.request.status, "queued");
        assert_eq!(detail.result.status, "queued");
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

        run_translation_scheduler_once(state.as_ref(), test_worker_profile(1, "general"))
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
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
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
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
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
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
        let worker = test_worker_profile(test_dedicated_worker_slot(), "user_dedicated");

        update_translation_worker_runtime(
            state.as_ref(),
            &worker,
            TranslationWorkerRuntimeUpdate::error("temporary failure"),
        )
        .await;

        run_translation_scheduler_once(state.as_ref(), worker)
            .await
            .expect("idle scan succeeds");

        let workers = translation_worker_runtime_statuses(state.as_ref()).await;
        let dedicated = workers
            .iter()
            .find(|entry| entry.worker_slot == test_dedicated_worker_slot())
            .expect("dedicated worker exists");

        assert_eq!(dedicated.status, "idle");
        assert_eq!(dedicated.error_text, None);
    }

    #[tokio::test]
    async fn claim_next_batch_routes_user_dedicated_worker_only_to_user_requests() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
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
            test_worker_profile(test_dedicated_worker_slot(), "user_dedicated"),
        )
        .await
        .expect("claim user batch")
        .expect("user batch exists");

        assert_eq!(dedicated_batch.worker_slot, test_dedicated_worker_slot());
        assert_eq!(dedicated_batch.request_count, 1);
        assert_eq!(dedicated_batch.items.len(), 1);
        assert_eq!(dedicated_batch.items[0].entity_id, "222");

        let general_batch = claim_next_batch(state.as_ref(), test_worker_profile(1, "general"))
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
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
        seed_user(&pool, 1, "octo").await;

        let mut item = sample_release_item("123");
        item.max_wait_ms = 0;
        create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("request created");

        run_translation_scheduler_once(state.as_ref(), test_worker_profile(2, "general"))
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
    async fn recover_runtime_state_marks_stale_batches_requests_and_linked_llm_calls_failed() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
        seed_user(&pool, 1, "octo").await;

        let mut item = sample_release_item("recover-stale-batch");
        item.max_wait_ms = 0;
        let created = create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("request created");
        let batch = claim_next_batch(state.as_ref(), test_worker_profile(1, "general"))
            .await
            .expect("claim batch")
            .expect("batch exists");
        let work_item_id = batch.items[0].id.clone();
        let stale_at = "2026-03-06T00:00:00Z";

        sqlx::query(
            r#"
            UPDATE translation_batches
            SET status = 'running',
                started_at = ?,
                runtime_owner_id = ?,
                lease_heartbeat_at = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(stale_at)
        .bind("old-runtime-owner")
        .bind(stale_at)
        .bind(stale_at)
        .bind(batch.id.as_str())
        .execute(&pool)
        .await
        .expect("mark batch stale");
        sqlx::query(
            r#"
            UPDATE translation_work_items
            SET status = 'running', started_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(stale_at)
        .bind(stale_at)
        .bind(work_item_id.as_str())
        .execute(&pool)
        .await
        .expect("mark work item running");
        sqlx::query(
            r#"
            INSERT INTO llm_calls (
              id, status, source, model, requested_by, parent_task_id, parent_task_type,
              parent_translation_batch_id, max_tokens, attempt_count, scheduler_wait_ms,
              duration_ms, prompt_text, response_text, error_text, created_at, started_at,
              finished_at, updated_at, input_messages_json, output_messages_json, input_tokens,
              output_tokens, cached_input_tokens, total_tokens, first_token_wait_ms,
              runtime_owner_id, lease_heartbeat_at
            ) VALUES (?, 'running', ?, ?, NULL, NULL, NULL, ?, ?, ?, ?, NULL, ?, NULL, NULL, ?, ?, NULL, ?, '[]', NULL, NULL, NULL, NULL, NULL, NULL, ?, ?)
            "#,
        )
        .bind("linked-llm-recover")
        .bind("translation.scheduler.deadline")
        .bind(current_model_profile())
        .bind(batch.id.as_str())
        .bind(512_i64)
        .bind(1_i64)
        .bind(0_i64)
        .bind("prompt")
        .bind(stale_at)
        .bind(stale_at)
        .bind(stale_at)
        .bind("old-runtime-owner")
        .bind(stale_at)
        .execute(&pool)
        .await
        .expect("insert linked llm call");

        recover_runtime_state(state.as_ref())
            .await
            .expect("recover runtime state");

        let batch_row = sqlx::query(
            r#"
            SELECT status, error_text, runtime_owner_id, lease_heartbeat_at
            FROM translation_batches
            WHERE id = ?
            "#,
        )
        .bind(batch.id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load recovered batch");
        assert_eq!(batch_row.get::<String, _>("status"), "failed");
        assert_eq!(
            batch_row.get::<Option<String>, _>("error_text").as_deref(),
            Some(crate::runtime::RUNTIME_LEASE_EXPIRED_ERROR)
        );
        assert_eq!(batch_row.get::<Option<String>, _>("runtime_owner_id"), None);
        assert_eq!(
            batch_row.get::<Option<String>, _>("lease_heartbeat_at"),
            None
        );

        let request_row = sqlx::query(
            r#"
            SELECT status, result_status, error_text
            FROM translation_requests
            WHERE id = ?
            "#,
        )
        .bind(created.request_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load recovered request");
        assert_eq!(request_row.get::<String, _>("status"), "failed");
        assert_eq!(
            request_row
                .get::<Option<String>, _>("result_status")
                .as_deref(),
            Some("error")
        );
        assert_eq!(
            request_row
                .get::<Option<String>, _>("error_text")
                .as_deref(),
            Some(crate::runtime::RUNTIME_LEASE_EXPIRED_ERROR)
        );

        let llm_row = sqlx::query(
            r#"
            SELECT status, error_text, runtime_owner_id, lease_heartbeat_at
            FROM llm_calls
            WHERE id = ?
            "#,
        )
        .bind("linked-llm-recover")
        .fetch_one(&pool)
        .await
        .expect("load recovered llm call");
        assert_eq!(llm_row.get::<String, _>("status"), "failed");
        assert_eq!(
            llm_row.get::<Option<String>, _>("error_text").as_deref(),
            Some(crate::runtime::RUNTIME_LEASE_EXPIRED_ERROR)
        );
        assert_eq!(llm_row.get::<Option<String>, _>("runtime_owner_id"), None);
        assert_eq!(llm_row.get::<Option<String>, _>("lease_heartbeat_at"), None);
    }

    #[tokio::test]
    async fn recover_runtime_state_keeps_live_current_owner_batches_running() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
        seed_user(&pool, 1, "octo").await;

        let mut item = sample_release_item("recover-live-batch");
        item.max_wait_ms = 0;
        create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("request created");
        let batch = claim_next_batch(state.as_ref(), test_worker_profile(1, "general"))
            .await
            .expect("claim batch")
            .expect("batch exists");
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            UPDATE translation_batches
            SET status = 'running',
                started_at = ?,
                runtime_owner_id = ?,
                lease_heartbeat_at = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(now.as_str())
        .bind(state.runtime_owner_id.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(batch.id.as_str())
        .execute(&pool)
        .await
        .expect("mark batch live");

        recover_runtime_state(state.as_ref())
            .await
            .expect("recover runtime state");

        let status = sqlx::query_scalar::<_, String>(
            r#"SELECT status FROM translation_batches WHERE id = ?"#,
        )
        .bind(batch.id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load live batch status");
        assert_eq!(status, "running");
    }

    #[tokio::test]
    async fn recover_runtime_state_keeps_live_foreign_owner_batches_running() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
        seed_user(&pool, 1, "octo").await;

        let mut item = sample_release_item("recover-foreign-live-batch");
        item.max_wait_ms = 0;
        create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("request created");
        let batch = claim_next_batch(state.as_ref(), test_worker_profile(1, "general"))
            .await
            .expect("claim batch")
            .expect("batch exists");
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            UPDATE translation_batches
            SET status = 'running',
                started_at = ?,
                runtime_owner_id = ?,
                lease_heartbeat_at = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(now.as_str())
        .bind("other-runtime-owner")
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(batch.id.as_str())
        .execute(&pool)
        .await
        .expect("mark foreign-owner batch live");

        recover_runtime_state(state.as_ref())
            .await
            .expect("recover runtime state");

        let row = sqlx::query(
            r#"
            SELECT status, runtime_owner_id, lease_heartbeat_at
            FROM translation_batches
            WHERE id = ?
            "#,
        )
        .bind(batch.id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load foreign-owner batch");

        assert_eq!(row.get::<String, _>("status"), "running");
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
    async fn recover_runtime_state_on_startup_reclaims_foreign_owner_batches_without_live_owner_lease()
     {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
        seed_user(&pool, 1, "octo").await;

        let mut item = sample_release_item("recover-startup-foreign-batch");
        item.max_wait_ms = 0;
        let created = create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("request created");
        let batch = claim_next_batch(state.as_ref(), test_worker_profile(1, "general"))
            .await
            .expect("claim batch")
            .expect("batch exists");
        let work_item_id = batch.items[0].id.clone();
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            UPDATE translation_batches
            SET status = 'running',
                started_at = ?,
                runtime_owner_id = ?,
                lease_heartbeat_at = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(now.as_str())
        .bind("other-runtime-owner")
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(batch.id.as_str())
        .execute(&pool)
        .await
        .expect("mark startup foreign-owner batch live");
        sqlx::query(
            r#"
            UPDATE translation_work_items
            SET status = 'running', started_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(work_item_id.as_str())
        .execute(&pool)
        .await
        .expect("mark work item running");
        sqlx::query(
            r#"
            INSERT INTO llm_calls (
              id, status, source, model, requested_by, parent_task_id, parent_task_type,
              parent_translation_batch_id, max_tokens, attempt_count, scheduler_wait_ms,
              duration_ms, prompt_text, response_text, error_text, created_at, started_at,
              finished_at, updated_at, input_messages_json, output_messages_json, input_tokens,
              output_tokens, cached_input_tokens, total_tokens, first_token_wait_ms,
              runtime_owner_id, lease_heartbeat_at
            ) VALUES (?, 'running', ?, ?, NULL, NULL, NULL, ?, ?, ?, ?, NULL, ?, NULL, NULL, ?, ?, NULL, ?, '[]', NULL, NULL, NULL, NULL, NULL, NULL, ?, ?)
            "#,
        )
        .bind("linked-llm-startup-recover")
        .bind("translation.scheduler.deadline")
        .bind(current_model_profile())
        .bind(batch.id.as_str())
        .bind(512_i64)
        .bind(1_i64)
        .bind(0_i64)
        .bind("prompt")
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .bind("other-runtime-owner")
        .bind(now.as_str())
        .execute(&pool)
        .await
        .expect("insert linked llm call");

        recover_runtime_state_on_startup(state.as_ref())
            .await
            .expect("startup recover runtime state");

        let batch_row = sqlx::query(
            r#"
            SELECT status, error_text, runtime_owner_id, lease_heartbeat_at
            FROM translation_batches
            WHERE id = ?
            "#,
        )
        .bind(batch.id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load recovered startup batch");
        assert_eq!(batch_row.get::<String, _>("status"), "failed");
        assert_eq!(
            batch_row.get::<Option<String>, _>("error_text").as_deref(),
            Some(crate::runtime::RUNTIME_LEASE_EXPIRED_ERROR)
        );

        let request_row = sqlx::query(
            r#"
            SELECT status, result_status, error_text
            FROM translation_requests
            WHERE id = ?
            "#,
        )
        .bind(created.request_id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load recovered startup request");
        assert_eq!(request_row.get::<String, _>("status"), "failed");
        assert_eq!(
            request_row
                .get::<Option<String>, _>("result_status")
                .as_deref(),
            Some("error")
        );
        assert_eq!(
            request_row
                .get::<Option<String>, _>("error_text")
                .as_deref(),
            Some(crate::runtime::RUNTIME_LEASE_EXPIRED_ERROR)
        );

        let llm_row = sqlx::query(
            r#"
            SELECT status, error_text
            FROM llm_calls
            WHERE id = ?
            "#,
        )
        .bind("linked-llm-startup-recover")
        .fetch_one(&pool)
        .await
        .expect("load recovered startup llm call");
        assert_eq!(llm_row.get::<String, _>("status"), "failed");
        assert_eq!(
            llm_row.get::<Option<String>, _>("error_text").as_deref(),
            Some(crate::runtime::RUNTIME_LEASE_EXPIRED_ERROR)
        );
    }

    #[tokio::test]
    async fn recover_runtime_state_on_startup_keeps_live_foreign_owner_batches_running() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
        seed_user(&pool, 1, "octo").await;

        let mut item = sample_release_item("recover-startup-live-foreign-batch");
        item.max_wait_ms = 0;
        create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("request created");
        let batch = claim_next_batch(state.as_ref(), test_worker_profile(1, "general"))
            .await
            .expect("claim batch")
            .expect("batch exists");
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            UPDATE translation_batches
            SET status = 'running',
                started_at = ?,
                runtime_owner_id = ?,
                lease_heartbeat_at = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(now.as_str())
        .bind("other-runtime-owner")
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(batch.id.as_str())
        .execute(&pool)
        .await
        .expect("mark startup live foreign-owner batch");
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
            FROM translation_batches
            WHERE id = ?
            "#,
        )
        .bind(batch.id.as_str())
        .fetch_one(&pool)
        .await
        .expect("load startup live foreign-owner batch");

        assert_eq!(row.get::<String, _>("status"), "running");
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
    async fn create_translation_request_refreshes_running_batch_worker_runtime() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("runtime-refresh");

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
            INSERT INTO translation_batches (
              id, partition_key, protocol_version, model_profile, target_lang, trigger_reason,
              worker_id, worker_slot, worker_kind, request_count, item_count,
              estimated_input_tokens, status, created_at, started_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'running', ?, ?, ?)
            "#,
        )
        .bind("batch-runtime-refresh")
        .bind("general:release_summary:feed_card:zh-CN")
        .bind(TRANSLATION_PROTOCOL_VERSION)
        .bind(current_model_profile())
        .bind("zh-CN")
        .bind("deadline_due")
        .bind("translation-worker-general-2")
        .bind(2_i64)
        .bind("general")
        .bind(1_i64)
        .bind(1_i64)
        .bind(128_i64)
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&pool)
        .await
        .expect("insert running batch");

        sqlx::query(
            r#"
            INSERT INTO translation_batch_items (
              id, batch_id, work_item_id, item_index, kind, variant, entity_id, producer_count,
              token_estimate, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("batch-runtime-item")
        .bind("batch-runtime-refresh")
        .bind(work_item_id.as_str())
        .bind(0_i64)
        .bind(item.kind.as_str())
        .bind(item.variant.as_str())
        .bind(item.entity_id.as_str())
        .bind(1_i64)
        .bind(128_i64)
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&pool)
        .await
        .expect("insert running batch item");

        sqlx::query(
            r#"
            UPDATE translation_work_items
            SET status = 'running', batch_id = 'batch-runtime-refresh', started_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(work_item_id.as_str())
        .execute(&pool)
        .await
        .expect("mark work item running");

        create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("second request created");

        let workers = translation_worker_runtime_statuses(state.as_ref()).await;
        let worker = workers
            .iter()
            .find(|entry| entry.worker_slot == 2)
            .expect("worker exists");

        assert_eq!(worker.status, "running");
        assert_eq!(
            worker.current_batch_id.as_deref(),
            Some("batch-runtime-refresh")
        );
        assert_eq!(worker.request_count, 1);
        assert_eq!(worker.work_item_count, 1);
    }

    #[tokio::test]
    async fn create_translation_request_refreshes_running_batch_into_live_slot_after_runtime_resize()
     {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
        seed_user(&pool, 1, "octo").await;
        let item = sample_release_item("runtime-refresh-resized");

        let first = create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("first request created");
        let work_item_id = first
            .result
            .work_item_id
            .clone()
            .expect("work item id for first request");
        let now = Utc::now().to_rfc3339();
        let worker_id = translation_worker_id("user_dedicated", 1);

        sqlx::query(
            r#"
            INSERT INTO translation_batches (
              id, partition_key, protocol_version, model_profile, target_lang, trigger_reason,
              worker_id, worker_slot, worker_kind, request_count, item_count,
              estimated_input_tokens, status, created_at, started_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'running', ?, ?, ?)
            "#,
        )
        .bind("batch-runtime-refresh-resized")
        .bind("general:release_summary:feed_card:zh-CN")
        .bind(TRANSLATION_PROTOCOL_VERSION)
        .bind(current_model_profile())
        .bind("zh-CN")
        .bind("deadline_due")
        .bind(worker_id.as_str())
        .bind(test_dedicated_worker_slot())
        .bind("user_dedicated")
        .bind(1_i64)
        .bind(1_i64)
        .bind(128_i64)
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&pool)
        .await
        .expect("insert running batch");

        sqlx::query(
            r#"
            INSERT INTO translation_batch_items (
              id, batch_id, work_item_id, item_index, kind, variant, entity_id, producer_count,
              token_estimate, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("batch-runtime-resized-item")
        .bind("batch-runtime-refresh-resized")
        .bind(work_item_id.as_str())
        .bind(0_i64)
        .bind(item.kind.as_str())
        .bind(item.variant.as_str())
        .bind(item.entity_id.as_str())
        .bind(1_i64)
        .bind(128_i64)
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&pool)
        .await
        .expect("insert running batch item");

        sqlx::query(
            r#"
            UPDATE translation_work_items
            SET status = 'running', batch_id = 'batch-runtime-refresh-resized', started_at = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(work_item_id.as_str())
        .execute(&pool)
        .await
        .expect("mark work item running");

        update_translation_worker_runtime(
            state.as_ref(),
            &translation_worker_profile_from_runtime(
                worker_id.as_str(),
                "user_dedicated",
                test_dedicated_worker_slot(),
            ),
            TranslationWorkerRuntimeUpdate::running(
                "batch-runtime-refresh-resized",
                1,
                1,
                "deadline_due",
            ),
        )
        .await;

        let resized_general_concurrency = DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY
            .saturating_sub(1)
            .max(1);
        state
            .translation_scheduler
            .sync_runtime_with_config(
                &state.pool,
                TranslationRuntimeConfig::new(
                    resized_general_concurrency,
                    DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY,
                ),
            )
            .await
            .expect("shrink runtime");
        let expected_slot =
            i64::try_from(resized_general_concurrency + 1).expect("expected slot fits in i64");

        create_translation_request(state.as_ref(), "1", "async", &item)
            .await
            .expect("second request created");

        let workers = translation_worker_runtime_statuses(state.as_ref()).await;
        let worker = workers
            .iter()
            .find(|entry| entry.worker_id == worker_id)
            .expect("worker exists");

        assert_eq!(worker.worker_slot, expected_slot);
        assert_eq!(worker.status, "running");
        assert_eq!(
            worker.current_batch_id.as_deref(),
            Some("batch-runtime-refresh-resized")
        );
        assert_eq!(worker.request_count, 1);
        assert_eq!(worker.work_item_count, 1);

        update_translation_worker_runtime(
            state.as_ref(),
            &translation_worker_profile_from_runtime(
                worker_id.as_str(),
                "user_dedicated",
                expected_slot,
            ),
            TranslationWorkerRuntimeUpdate::idle(),
        )
        .await;

        let workers = translation_worker_runtime_statuses(state.as_ref()).await;
        let worker = workers
            .iter()
            .find(|entry| entry.worker_id == worker_id)
            .expect("worker exists after idle");

        assert_eq!(worker.worker_slot, expected_slot);
        assert_eq!(worker.status, "idle");
        assert_eq!(worker.current_batch_id, None);
    }

    #[tokio::test]
    async fn runtime_resize_updates_running_batch_slot_metadata() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
        let worker_id = translation_worker_id("user_dedicated", 1);
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO translation_batches (
              id, partition_key, protocol_version, model_profile, target_lang, trigger_reason,
              worker_id, worker_slot, worker_kind, request_count, item_count,
              estimated_input_tokens, status, created_at, started_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'running', ?, ?, ?)
            "#,
        )
        .bind("batch-runtime-slot-sync")
        .bind("general:release_summary:feed_card:zh-CN")
        .bind(TRANSLATION_PROTOCOL_VERSION)
        .bind(current_model_profile())
        .bind("zh-CN")
        .bind("deadline_due")
        .bind(worker_id.as_str())
        .bind(test_dedicated_worker_slot())
        .bind("user_dedicated")
        .bind(1_i64)
        .bind(1_i64)
        .bind(128_i64)
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&pool)
        .await
        .expect("insert running batch");

        update_translation_worker_runtime(
            state.as_ref(),
            &translation_worker_profile_from_runtime(
                worker_id.as_str(),
                "user_dedicated",
                test_dedicated_worker_slot(),
            ),
            TranslationWorkerRuntimeUpdate::running("batch-runtime-slot-sync", 1, 1, "deadline"),
        )
        .await;

        state
            .translation_scheduler
            .sync_runtime_with_config(&state.pool, TranslationRuntimeConfig::new(2, 1))
            .await
            .expect("shrink runtime");

        let row = sqlx::query_as::<_, AdminTranslationBatchListItem>(
            r#"
            SELECT id, status, trigger_reason, worker_slot, request_count, item_count,
                   estimated_input_tokens, created_at, started_at, finished_at, updated_at
            FROM translation_batches
            WHERE id = ?
            "#,
        )
        .bind("batch-runtime-slot-sync")
        .fetch_one(&pool)
        .await
        .expect("load running batch");

        assert_eq!(row.worker_slot, 3);
    }

    #[tokio::test]
    async fn runtime_resize_keeps_unique_slots_for_retained_running_workers() {
        let pool = setup_pool().await;
        let state = setup_state(pool);
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;

        update_translation_worker_runtime(
            state.as_ref(),
            &test_worker_profile(3, "general"),
            TranslationWorkerRuntimeUpdate::running("batch-general-3", 1, 1, "deadline"),
        )
        .await;

        state
            .translation_scheduler
            .sync_runtime_with_config(
                &state.pool,
                TranslationRuntimeConfig::new(2, DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY),
            )
            .await
            .expect("resize runtime");

        let workers = translation_worker_runtime_statuses(state.as_ref()).await;
        let slots = workers
            .iter()
            .map(|worker| worker.worker_slot)
            .collect::<Vec<_>>();
        let dedicated = workers
            .iter()
            .find(|worker| worker.worker_id == translation_worker_id("user_dedicated", 1))
            .expect("dedicated worker exists");
        let retained_general = workers
            .iter()
            .find(|worker| worker.worker_id == translation_worker_id("general", 3))
            .expect("retained worker exists");

        assert_eq!(slots, vec![1, 2, 3, 4]);
        assert_eq!(retained_general.worker_slot, 3);
        assert_eq!(retained_general.status, "running");
        assert_eq!(dedicated.worker_slot, 4);
    }

    #[tokio::test]
    async fn runtime_resize_moves_running_dedicated_worker_after_new_general_slot() {
        let pool = setup_pool().await;
        let state = setup_state(pool);
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;

        update_translation_worker_runtime(
            state.as_ref(),
            &test_worker_profile(test_dedicated_worker_slot(), "user_dedicated"),
            TranslationWorkerRuntimeUpdate::running("batch-dedicated-1", 1, 1, "deadline"),
        )
        .await;

        state
            .translation_scheduler
            .sync_runtime_with_config(&state.pool, TranslationRuntimeConfig::new(4, 1))
            .await
            .expect("expand runtime");

        let workers = translation_worker_runtime_statuses(state.as_ref()).await;
        let slots = workers
            .iter()
            .map(|worker| worker.worker_slot)
            .collect::<Vec<_>>();
        let general = workers
            .iter()
            .find(|worker| worker.worker_id == translation_worker_id("general", 4))
            .expect("new general worker exists");
        let dedicated = workers
            .iter()
            .find(|worker| worker.worker_id == translation_worker_id("user_dedicated", 1))
            .expect("dedicated worker exists");

        assert_eq!(slots, vec![1, 2, 3, 4, 5]);
        assert_eq!(general.worker_slot, 4);
        assert_eq!(dedicated.worker_slot, 5);
        assert_eq!(dedicated.status, "running");
    }

    #[tokio::test]
    async fn runtime_resize_moves_running_dedicated_worker_into_compacted_slot() {
        let pool = setup_pool().await;
        let state = setup_state(pool);
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;

        update_translation_worker_runtime(
            state.as_ref(),
            &test_worker_profile(test_dedicated_worker_slot(), "user_dedicated"),
            TranslationWorkerRuntimeUpdate::running("batch-dedicated-1", 1, 1, "deadline"),
        )
        .await;

        state
            .translation_scheduler
            .sync_runtime_with_config(&state.pool, TranslationRuntimeConfig::new(2, 1))
            .await
            .expect("shrink runtime");

        let workers = translation_worker_runtime_statuses(state.as_ref()).await;
        let slots = workers
            .iter()
            .map(|worker| worker.worker_slot)
            .collect::<Vec<_>>();
        let dedicated = workers
            .iter()
            .find(|worker| worker.worker_id == translation_worker_id("user_dedicated", 1))
            .expect("dedicated worker exists");

        assert_eq!(slots, vec![1, 2, 3]);
        assert_eq!(dedicated.worker_slot, 3);
        assert_eq!(dedicated.status, "running");
    }

    #[tokio::test]
    async fn claim_next_batch_skips_removed_worker_after_runtime_resize() {
        let pool = setup_pool().await;
        let state = setup_state(pool);
        seed_user(&state.pool, 1, "octo").await;

        create_translation_request(
            state.as_ref(),
            "1",
            "async",
            &sample_release_item("claim-removed-worker"),
        )
        .await
        .expect("request created");

        state
            .translation_scheduler
            .sync_runtime_with_config(
                &state.pool,
                TranslationRuntimeConfig::new(2, DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY),
            )
            .await
            .expect("shrink runtime");

        let batch = claim_next_batch(state.as_ref(), test_worker_profile(3, "general"))
            .await
            .expect("claim result");
        assert!(batch.is_none());
    }

    #[tokio::test]
    async fn apply_runtime_config_waits_for_inflight_claims_before_removing_worker() {
        let pool = setup_pool().await;
        let state = setup_state(pool);
        let claim_guard = translation_batch_claim_lock().lock().await;
        let scheduler = Arc::clone(&state.translation_scheduler);
        let resize_state = state.clone();
        let resize = tokio::spawn(async move {
            scheduler
                .apply_runtime_config(resize_state, TranslationRuntimeConfig::new(2, 1))
                .await
                .expect("apply runtime config")
        });

        tokio::time::sleep(Duration::from_millis(25)).await;
        assert_eq!(
            state
                .translation_scheduler
                .desired_config()
                .await
                .general_worker_concurrency,
            DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY
        );

        drop(claim_guard);

        let applied = resize.await.expect("resize task should finish");
        assert_eq!(applied.general_worker_concurrency, 2);
        assert!(
            !state
                .translation_scheduler
                .worker_is_desired("translation-worker-general-3")
                .await
        );
    }

    #[tokio::test]
    async fn unregister_worker_respawns_desired_worker_after_readd() {
        let pool = setup_pool().await;
        let state = setup_state(pool);
        let worker_id = translation_worker_id("general", 3);
        let stale_task = tokio::spawn(async {
            sleep(Duration::from_secs(60)).await;
        });

        {
            let mut handles = state
                .translation_scheduler
                .worker_abort_handles
                .lock()
                .await;
            handles.insert(worker_id.clone(), stale_task.abort_handle());
        }

        state
            .translation_scheduler
            .sync_runtime_with_config(
                &state.pool,
                TranslationRuntimeConfig::new(2, DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY),
            )
            .await
            .expect("shrink runtime");

        state
            .translation_scheduler
            .apply_runtime_config(
                state.clone(),
                TranslationRuntimeConfig::new(3, DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY),
            )
            .await
            .expect("apply translation runtime config");

        state
            .translation_scheduler
            .finish_worker_exit(state.clone(), worker_id.clone())
            .await;

        tokio::task::yield_now().await;

        let handles = state
            .translation_scheduler
            .worker_abort_handles
            .lock()
            .await;
        assert!(handles.contains_key(worker_id.as_str()));
        stale_task.abort();
    }

    #[tokio::test]
    async fn translation_worker_runtime_statuses_report_updates() {
        let pool = setup_pool().await;
        let state = setup_state(pool);
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
        update_translation_worker_runtime(
            state.as_ref(),
            &test_worker_profile(test_dedicated_worker_slot(), "user_dedicated"),
            TranslationWorkerRuntimeUpdate::running("batch-1", 2, 3, "deadline"),
        )
        .await;

        let workers = translation_worker_runtime_statuses(state.as_ref()).await;
        let dedicated = workers
            .iter()
            .find(|worker| worker.worker_slot == test_dedicated_worker_slot())
            .expect("dedicated worker exists");

        assert_eq!(
            workers.len(),
            DEFAULT_TRANSLATION_GENERAL_WORKER_CONCURRENCY
                + DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY
        );
        assert_eq!(dedicated.status, "running");
        assert_eq!(dedicated.current_batch_id.as_deref(), Some("batch-1"));
        assert_eq!(dedicated.request_count, 2);
        assert_eq!(dedicated.work_item_count, 3);
    }

    #[tokio::test]
    async fn runtime_resize_refreshes_remaining_worker_timestamps_for_sse() {
        let pool = setup_pool().await;
        let state = setup_state(pool);
        let stale_updated_at = "2026-03-07T00:00:00Z";

        {
            let mut runtime = state.translation_scheduler.runtime.write().await;
            for entry in runtime.iter_mut() {
                entry.updated_at = stale_updated_at.to_owned();
            }
        }

        state
            .translation_scheduler
            .sync_runtime_with_config(
                &state.pool,
                TranslationRuntimeConfig::new(2, DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY),
            )
            .await
            .expect("shrink runtime");

        let workers = translation_worker_runtime_statuses(state.as_ref()).await;
        assert_eq!(workers.len(), 3);
        assert!(
            workers
                .iter()
                .all(|worker| worker.updated_at != stale_updated_at)
        );
        assert_eq!(
            workers
                .iter()
                .map(|worker| worker.worker_slot)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[tokio::test]
    async fn runtime_resize_preserves_running_worker_timestamps() {
        let pool = setup_pool().await;
        let state = setup_state(pool);
        let running_worker_id = translation_worker_id("user_dedicated", 1);
        let running_updated_at = "2026-03-07T00:00:01Z";
        let stale_updated_at = "2026-03-07T00:00:00Z";

        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
        update_translation_worker_runtime(
            state.as_ref(),
            &test_worker_profile(test_dedicated_worker_slot(), "user_dedicated"),
            TranslationWorkerRuntimeUpdate::running("batch-running-dedicated", 1, 1, "deadline"),
        )
        .await;

        {
            let mut runtime = state.translation_scheduler.runtime.write().await;
            for entry in runtime.iter_mut() {
                entry.updated_at = if entry.worker_id == running_worker_id {
                    running_updated_at.to_owned()
                } else {
                    stale_updated_at.to_owned()
                };
            }
        }

        state
            .translation_scheduler
            .sync_runtime_with_config(
                &state.pool,
                TranslationRuntimeConfig::new(2, DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY),
            )
            .await
            .expect("shrink runtime");

        let workers = translation_worker_runtime_statuses(state.as_ref()).await;
        let dedicated = workers
            .iter()
            .find(|worker| worker.worker_id == running_worker_id)
            .expect("dedicated worker exists after shrink");

        assert_eq!(dedicated.status, "running");
        assert_eq!(dedicated.worker_slot, 3);
        assert_eq!(dedicated.updated_at, running_updated_at);
        assert!(
            workers
                .iter()
                .filter(|worker| worker.worker_id != running_worker_id)
                .all(|worker| worker.updated_at != stale_updated_at)
        );
    }

    #[tokio::test]
    async fn drained_worker_exit_refreshes_remaining_worker_timestamps_for_sse() {
        let pool = setup_pool().await;
        let state = setup_state(pool);
        let worker_id = translation_worker_id("general", 3);
        let stale_updated_at = "2026-03-07T00:00:00Z";

        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
        update_translation_worker_runtime(
            state.as_ref(),
            &test_worker_profile(3, "general"),
            TranslationWorkerRuntimeUpdate::running("batch-general-3", 1, 1, "deadline"),
        )
        .await;

        state
            .translation_scheduler
            .sync_runtime_with_config(
                &state.pool,
                TranslationRuntimeConfig::new(2, DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY),
            )
            .await
            .expect("shrink runtime");

        update_translation_worker_runtime(
            state.as_ref(),
            &translation_worker_profile_from_runtime(worker_id.as_str(), "general", 3),
            TranslationWorkerRuntimeUpdate::idle(),
        )
        .await;

        {
            let mut runtime = state.translation_scheduler.runtime.write().await;
            for entry in runtime.iter_mut() {
                entry.updated_at = stale_updated_at.to_owned();
            }
        }

        state
            .translation_scheduler
            .remove_worker_runtime(&state.pool, worker_id.as_str())
            .await
            .expect("remove drained worker runtime");

        let workers = translation_worker_runtime_statuses(state.as_ref()).await;
        assert_eq!(workers.len(), 3);
        assert!(workers.iter().all(|worker| worker.worker_id != worker_id));
        assert!(
            workers
                .iter()
                .all(|worker| worker.updated_at != stale_updated_at)
        );
        assert_eq!(
            workers
                .iter()
                .map(|worker| worker.worker_slot)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[tokio::test]
    async fn drained_worker_exit_updates_running_batch_slot_metadata() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let drained_worker_id = translation_worker_id("general", 3);
        let dedicated_worker_id = translation_worker_id("user_dedicated", 1);
        let now = Utc::now().to_rfc3339();
        let running_updated_at = "2026-03-07T00:00:01Z";
        let stale_updated_at = "2026-03-07T00:00:00Z";

        reset_translation_worker_runtime_for_tests(state.as_ref()).await;
        sqlx::query(
            r#"
            INSERT INTO translation_batches (
              id, partition_key, protocol_version, model_profile, target_lang, trigger_reason,
              worker_id, worker_slot, worker_kind, request_count, item_count,
              estimated_input_tokens, status, created_at, started_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'running', ?, ?, ?)
            "#,
        )
        .bind("batch-drained-slot-sync")
        .bind("general:release_summary:feed_card:zh-CN")
        .bind(TRANSLATION_PROTOCOL_VERSION)
        .bind(current_model_profile())
        .bind("zh-CN")
        .bind("deadline_due")
        .bind(dedicated_worker_id.as_str())
        .bind(test_dedicated_worker_slot())
        .bind("user_dedicated")
        .bind(1_i64)
        .bind(1_i64)
        .bind(128_i64)
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&pool)
        .await
        .expect("insert running dedicated batch");

        update_translation_worker_runtime(
            state.as_ref(),
            &translation_worker_profile_from_runtime(
                dedicated_worker_id.as_str(),
                "user_dedicated",
                test_dedicated_worker_slot(),
            ),
            TranslationWorkerRuntimeUpdate::running("batch-drained-slot-sync", 1, 1, "deadline"),
        )
        .await;
        update_translation_worker_runtime(
            state.as_ref(),
            &test_worker_profile(3, "general"),
            TranslationWorkerRuntimeUpdate::running("batch-general-3", 1, 1, "deadline"),
        )
        .await;

        state
            .translation_scheduler
            .sync_runtime_with_config(
                &state.pool,
                TranslationRuntimeConfig::new(2, DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY),
            )
            .await
            .expect("shrink runtime");

        let before_exit = sqlx::query(
            r#"
            SELECT worker_slot
            FROM translation_batches
            WHERE id = ?
            "#,
        )
        .bind("batch-drained-slot-sync")
        .fetch_one(&pool)
        .await
        .expect("fetch batch before drained worker exit");
        assert_eq!(before_exit.get::<i64, _>("worker_slot"), 4);

        update_translation_worker_runtime(
            state.as_ref(),
            &translation_worker_profile_from_runtime(drained_worker_id.as_str(), "general", 3),
            TranslationWorkerRuntimeUpdate::idle(),
        )
        .await;
        {
            let mut runtime = state.translation_scheduler.runtime.write().await;
            for entry in runtime.iter_mut() {
                entry.updated_at = if entry.worker_id == dedicated_worker_id {
                    running_updated_at.to_owned()
                } else {
                    stale_updated_at.to_owned()
                };
            }
        }

        state
            .translation_scheduler
            .remove_worker_runtime(&state.pool, drained_worker_id.as_str())
            .await
            .expect("remove drained worker runtime");

        let batch_row = sqlx::query(
            r#"
            SELECT worker_slot, worker_kind
            FROM translation_batches
            WHERE id = ?
            "#,
        )
        .bind("batch-drained-slot-sync")
        .fetch_one(&pool)
        .await
        .expect("fetch batch after drained worker exit");
        assert_eq!(batch_row.get::<i64, _>("worker_slot"), 3);
        assert_eq!(
            batch_row.get::<String, _>("worker_kind"),
            "user_dedicated".to_owned()
        );

        let workers = translation_worker_runtime_statuses(state.as_ref()).await;
        let dedicated = workers
            .iter()
            .find(|worker| worker.worker_id == dedicated_worker_id)
            .expect("dedicated worker remains after drained exit");
        assert_eq!(dedicated.status, "running");
        assert_eq!(dedicated.worker_slot, 3);
        assert_eq!(dedicated.updated_at, running_updated_at);
        assert!(
            workers
                .iter()
                .filter(|worker| worker.worker_id != dedicated_worker_id)
                .all(|worker| worker.updated_at != stale_updated_at)
        );
    }

    #[tokio::test]
    async fn admin_translation_status_reports_live_counts_during_worker_drain() {
        let pool = setup_pool().await;
        let state = setup_state(pool);
        reset_translation_worker_runtime_for_tests(state.as_ref()).await;

        update_translation_worker_runtime(
            state.as_ref(),
            &test_worker_profile(3, "general"),
            TranslationWorkerRuntimeUpdate::running("batch-general-3", 1, 1, "deadline"),
        )
        .await;

        state
            .translation_scheduler
            .sync_runtime_with_config(
                &state.pool,
                TranslationRuntimeConfig::new(2, DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY),
            )
            .await
            .expect("shrink runtime");

        let status = load_admin_translation_status_response(state.as_ref())
            .await
            .expect("load translation status");

        assert_eq!(status.general_worker_concurrency, 3);
        assert_eq!(
            status.dedicated_worker_concurrency,
            i64::try_from(DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY).unwrap_or(i64::MAX)
        );
        assert_eq!(status.worker_concurrency, 4);
        assert_eq!(
            status.idle_workers + status.busy_workers,
            status.worker_concurrency
        );
        assert_eq!(status.target_general_worker_concurrency, 2);
        assert_eq!(
            status.target_dedicated_worker_concurrency,
            i64::try_from(DEFAULT_TRANSLATION_DEDICATED_WORKER_CONCURRENCY).unwrap_or(i64::MAX)
        );
        assert_eq!(status.target_worker_concurrency, 3);
        assert_eq!(
            status.batch_token_threshold,
            i64::from(TRANSLATION_BATCH_MAX_TOKENS)
        );
        assert_eq!(status.workers.len(), 4);
    }

    #[tokio::test]
    async fn admin_translation_status_syncs_persisted_runtime_settings() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        let now = "2026-03-28T11:00:00Z";
        sqlx::query(
            r#"
            INSERT INTO admin_runtime_settings (
              id,
              llm_max_concurrency,
              translation_general_worker_concurrency,
              translation_dedicated_worker_concurrency,
              created_at,
              updated_at
            )
            VALUES (1, 1, 5, 2, ?, ?)
            "#,
        )
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert runtime settings");

        admin_runtime::sync_persisted_runtime_settings(state.clone())
            .await
            .expect("sync runtime settings");
        let status = load_admin_translation_status_response(state.as_ref())
            .await
            .expect("load translation status");

        assert_eq!(status.general_worker_concurrency, 5);
        assert_eq!(status.dedicated_worker_concurrency, 2);
        assert_eq!(status.worker_concurrency, 7);
        assert_eq!(status.target_general_worker_concurrency, 5);
        assert_eq!(status.target_dedicated_worker_concurrency, 2);
        assert_eq!(status.target_worker_concurrency, 7);
        assert_eq!(
            status.batch_token_threshold,
            i64::from(TRANSLATION_BATCH_MAX_TOKENS)
        );
        assert_eq!(status.workers.len(), 7);

        state.translation_scheduler.abort_all().await;
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

        let effective_status_sql = request_effective_status_sql("status", "work_item_status");
        let request_rows_sql = format!(
            r#"
            SELECT *
            FROM ({request_rows_base}) request_rows
            ORDER BY CASE WHEN {effective_status_sql} IN ('queued', 'running') THEN 0 ELSE 1 END ASC,
                     updated_at DESC,
                     id DESC
            "#,
            request_rows_base = request_row_select_sql(),
            effective_status_sql = effective_status_sql,
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

    #[tokio::test]
    async fn admin_request_running_filter_uses_effective_status() {
        let pool = setup_pool().await;
        let state = setup_state(pool.clone());
        seed_user(&pool, 1, "octo").await;

        let promoted = create_translation_request(
            state.as_ref(),
            "1",
            "async",
            &sample_release_item("promoted"),
        )
        .await
        .expect("promoted request created");
        let queued = create_translation_request(
            state.as_ref(),
            "1",
            "async",
            &sample_release_item("plain-queued"),
        )
        .await
        .expect("queued request created");

        let promoted_work_item_id = promoted
            .result
            .work_item_id
            .clone()
            .expect("promoted request work item");
        sqlx::query(
            r#"
            UPDATE translation_work_items
            SET status = 'batched',
                batch_id = 'batch-running-1',
                started_at = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind("2026-03-07T00:00:03Z")
        .bind("2026-03-07T00:00:03Z")
        .bind(promoted_work_item_id.as_str())
        .execute(&pool)
        .await
        .expect("promote work item to batched");

        sqlx::query("UPDATE translation_requests SET updated_at = ? WHERE id = ?")
            .bind("2026-03-07T00:00:03Z")
            .bind(promoted.request_id.as_str())
            .execute(&pool)
            .await
            .expect("update promoted request timestamp");
        sqlx::query("UPDATE translation_requests SET updated_at = ? WHERE id = ?")
            .bind("2026-03-07T00:00:02Z")
            .bind(queued.request_id.as_str())
            .execute(&pool)
            .await
            .expect("update queued request timestamp");

        let effective_status_sql = request_effective_status_sql("status", "work_item_status");
        let request_rows_sql = format!(
            r#"
            SELECT *
            FROM ({request_rows_base}) request_rows
            WHERE {effective_status_sql} = 'running'
            ORDER BY CASE WHEN {effective_status_sql} IN ('queued', 'running') THEN 0 ELSE 1 END ASC,
                     updated_at DESC,
                     id DESC
            "#,
            request_rows_base = request_row_select_sql(),
            effective_status_sql = effective_status_sql,
        );
        let rows = sqlx::query_as::<_, RequestRow>(&request_rows_sql)
            .fetch_all(&state.pool)
            .await
            .expect("load running-filter requests");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, promoted.request_id);
        assert_eq!(rows[0].effective_status(), "running");
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
        setup_pool_with_max_connections(1).await
    }

    async fn setup_pool_with_max_connections(max_connections: u32) -> SqlitePool {
        let database_path = std::env::temp_dir().join(format!(
            "octo-rill-test-{}.db",
            crate::local_id::generate_local_id(),
        ));
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
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
            ai_max_concurrency: 1,
            ai_model_context_limit: None,
            ai_daily_at_local: None,
        };
        let oauth = build_oauth_client(&config).expect("build oauth client");
        Arc::new(AppState {
            llm_scheduler: Arc::new(crate::ai::LlmScheduler::new(config.ai_max_concurrency)),
            translation_scheduler: Arc::new(TranslationSchedulerController::new(
                TranslationRuntimeConfig::default(),
            )),
            config,
            pool,
            http: reqwest::Client::new(),
            oauth,
            encryption_key,
            runtime_owner_id: "translation-test-runtime-owner".to_owned(),
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
