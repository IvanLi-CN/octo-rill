use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicI64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::de::{Error as _, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Error as SqlxError, Row, Sqlite, SqlitePool, Transaction};
use tokio::{task::JoinSet, time::sleep};
use tracing::warn;

use crate::release_links::parse_repo_full_name_from_release_url;
use crate::{
    ai, api, content_identity_upgrade, error::ApiError, local_id, state::AppState, translations,
};
use tower_sessions::Session;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentProcessingMode {
    Legacy,
    RollbackFreeze,
    Global,
}

pub const CONTENT_PROCESSING_MIGRATION_VERSION: i64 = 78;

#[derive(Debug, Clone, Copy)]
pub struct ContentProcessingTransitionError {
    pub mode: ContentProcessingMode,
}

impl std::fmt::Display for ContentProcessingTransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "content processing is temporarily frozen in {} mode",
            self.mode.as_str()
        )
    }
}

impl std::error::Error for ContentProcessingTransitionError {}

pub fn transition_error(mode: ContentProcessingMode) -> anyhow::Error {
    anyhow::Error::new(ContentProcessingTransitionError { mode })
}

pub fn api_error_from_anyhow(error: anyhow::Error) -> ApiError {
    if let Some(transition) = error.downcast_ref::<ContentProcessingTransitionError>() {
        return ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "content_processing_transition",
            "content processing is temporarily frozen; poll the request status before retrying",
        )
        .with_details(json!({
            "mode": transition.mode.as_str(),
            "request_id": Value::Null,
            "work_item_id": Value::Null,
            "poll_url": Value::Null,
        }));
    }
    ApiError::internal(error)
}

impl ContentProcessingMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::RollbackFreeze => "rollback_freeze",
            Self::Global => "global",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "legacy" => Ok(Self::Legacy),
            "rollback_freeze" => Ok(Self::RollbackFreeze),
            "global" => Ok(Self::Global),
            other => anyhow::bail!("invalid content processing mode: {other}"),
        }
    }
}

pub async fn current_mode(pool: &SqlitePool) -> Result<ContentProcessingMode> {
    let mode = match sqlx::query_scalar::<_, String>(
        "SELECT mode FROM content_processing_control WHERE id = 1",
    )
    .fetch_optional(pool)
    .await
    {
        Ok(mode) => mode,
        Err(SqlxError::Database(error)) if error.message().contains("no such table") => {
            let migration_table_exists = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
            )
            .fetch_one(pool)
            .await?
                > 0;
            let migration_applied = migration_table_exists
                && sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = ?",
                )
                .bind(CONTENT_PROCESSING_MIGRATION_VERSION)
                .fetch_optional(pool)
                .await?
                .unwrap_or(0)
                    > 0;
            if migration_applied {
                anyhow::bail!("content processing control table is missing after migration 0078");
            }
            return Ok(ContentProcessingMode::Legacy);
        }
        Err(error) => return Err(error).context("failed to load content processing mode"),
    };
    mode.as_deref()
        .map(ContentProcessingMode::parse)
        .transpose()?
        .ok_or_else(|| anyhow::anyhow!("content processing control row is missing"))
}

pub async fn ensure_legacy_writer(pool: &SqlitePool) -> Result<(), ApiError> {
    let mode = current_mode(pool).await.map_err(ApiError::internal)?;
    if mode == ContentProcessingMode::Legacy {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "content_processing_transition",
        format!(
            "content processing is controlled by mode {}; poll the request status before retrying",
            mode.as_str()
        ),
    )
    .with_details(json!({
        "mode": mode.as_str(),
        "request_id": Value::Null,
        "work_item_id": Value::Null,
        "poll_url": Value::Null,
    })))
}

pub async fn ensure_legacy_writer_runtime(pool: &SqlitePool) -> Result<bool> {
    Ok(current_mode(pool).await? == ContentProcessingMode::Legacy)
}

/// Re-check the writer mode after a serialized write transaction has started.
/// The migration is additive, so a pre-0078 database is treated as legacy.
pub async fn legacy_mode_in_transaction(tx: &mut Transaction<'_, Sqlite>) -> Result<bool> {
    let mode = match sqlx::query_scalar::<_, String>(
        "SELECT mode FROM content_processing_control WHERE id = 1",
    )
    .fetch_optional(&mut **tx)
    .await
    {
        Ok(mode) => mode,
        Err(SqlxError::Database(error)) if error.message().contains("no such table") => {
            let migration_table_exists = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
            )
            .fetch_one(&mut **tx)
            .await?
                > 0;
            let migration_applied = migration_table_exists
                && sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = ?",
                )
                .bind(CONTENT_PROCESSING_MIGRATION_VERSION)
                .fetch_optional(&mut **tx)
                .await?
                .unwrap_or(0)
                    > 0;
            if migration_applied {
                anyhow::bail!("content processing control table is missing after migration 0078");
            }
            return Ok(true);
        }
        Err(error) => return Err(error.into()),
    };
    if mode.is_none() {
        let migration_table_exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
        )
        .fetch_one(&mut **tx)
        .await?
            > 0;
        let migration_applied = migration_table_exists
            && sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = ?",
            )
            .bind(CONTENT_PROCESSING_MIGRATION_VERSION)
            .fetch_optional(&mut **tx)
            .await?
            .unwrap_or(0)
                > 0;
        if migration_applied {
            anyhow::bail!("content processing control row is missing after migration 0078");
        }
    }
    Ok(mode
        .as_deref()
        .map(ContentProcessingMode::parse)
        .transpose()?
        .is_none_or(|mode| mode == ContentProcessingMode::Legacy))
}

async fn ensure_global_mode_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<(), ApiError> {
    let mode =
        sqlx::query_scalar::<_, String>("SELECT mode FROM content_processing_control WHERE id = 1")
            .fetch_optional(&mut **tx)
            .await
            .map_err(ApiError::internal)?;
    if mode.as_deref() == Some(ContentProcessingMode::Global.as_str()) {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "content_processing_transition",
        "content processing is not in global mode; poll the request status before retrying",
    ))
}

#[allow(dead_code)]
pub async fn transition_to_global(pool: &SqlitePool, switch_token: &str) -> Result<bool> {
    let mut tx = pool
        .begin()
        .await
        .context("failed to begin content mode transition")?;
    let changed = transition_to_global_in_transaction(&mut tx, switch_token).await?;
    tx.commit()
        .await
        .context("failed to commit content mode transition")?;
    Ok(changed)
}

pub async fn transition_to_global_state(state: &AppState, switch_token: &str) -> Result<bool> {
    let (_lock, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "content_processing_cutover")
        .await
        .context("failed to begin serialized content mode transition")?;
    let changed = transition_to_global_in_transaction(&mut tx, switch_token).await?;
    tx.commit()
        .await
        .context("failed to commit content mode transition")?;
    Ok(changed)
}

pub async fn transition_to_rollback_freeze_state(
    state: &AppState,
    switch_token: &str,
) -> Result<bool> {
    let (_lock, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "content_processing_freeze")
        .await
        .context("failed to begin serialized content freeze")?;
    let changed = transition_to_rollback_freeze_in_transaction(&mut tx, switch_token).await?;
    tx.commit()
        .await
        .context("failed to commit content freeze")?;
    Ok(changed)
}

async fn transition_to_rollback_freeze_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    switch_token: &str,
) -> Result<bool> {
    let mode =
        sqlx::query_scalar::<_, String>("SELECT mode FROM content_processing_control WHERE id = 1")
            .fetch_optional(&mut **tx)
            .await?;
    if mode.as_deref() != Some(ContentProcessingMode::Legacy.as_str()) {
        return Ok(false);
    }
    for table in ["translation_batches", "translation_work_items"] {
        let table_exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(table)
        .fetch_one(&mut **tx)
        .await?
            > 0;
        if table_exists {
            let active = sqlx::query_scalar::<_, i64>(&format!(
                "SELECT COUNT(*) FROM {table} WHERE status NOT IN ('completed', 'failed')"
            ))
            .fetch_one(&mut **tx)
            .await?;
            if active > 0 {
                return Ok(false);
            }
        }
    }
    let changed = sqlx::query(
        "UPDATE content_processing_control SET mode = 'rollback_freeze', switch_token = ?, updated_at = CURRENT_TIMESTAMP WHERE id = 1 AND mode = 'legacy'",
    )
    .bind(switch_token)
    .execute(&mut **tx)
    .await
    .context("failed to transition content processing into freeze")?
    .rows_affected()
        == 1;
    Ok(changed)
}

async fn transition_to_global_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    switch_token: &str,
) -> Result<bool> {
    let mode =
        sqlx::query_scalar::<_, String>("SELECT mode FROM content_processing_control WHERE id = 1")
            .fetch_optional(&mut **tx)
            .await?;
    if mode.as_deref() != Some(ContentProcessingMode::RollbackFreeze.as_str()) {
        return Ok(false);
    }
    for table in ["translation_batches", "translation_work_items"] {
        let table_exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(table)
        .fetch_one(&mut **tx)
        .await?
            > 0;
        if table_exists {
            let active = sqlx::query_scalar::<_, i64>(&format!(
                "SELECT COUNT(*) FROM {table} WHERE status NOT IN ('completed', 'failed')"
            ))
            .fetch_one(&mut **tx)
            .await?;
            if active > 0 {
                return Ok(false);
            }
        }
    }
    record_legacy_observations(tx).await?;
    let changed = sqlx::query(
        "UPDATE content_processing_control SET mode = 'global', switch_token = ?, updated_at = CURRENT_TIMESTAMP WHERE id = 1 AND mode = 'rollback_freeze'",
    )
    .bind(switch_token)
    .execute(&mut **tx)
    .await
        .context("failed to transition content processing mode")?
        .rows_affected()
        == 1;
    Ok(changed)
}

async fn record_legacy_observations(tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
    let has_ai_translations = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'ai_translations'",
    )
    .fetch_one(&mut **tx)
    .await?
        > 0;
    if has_ai_translations {
        sqlx::query(
            "INSERT OR IGNORE INTO content_legacy_observations (id, legacy_table, legacy_primary_key, canonical_resource_type, canonical_resource_id, pipeline, classification, observation_basis_json, observed_at) SELECT 'legacy-cache-' || id, 'ai_translations', id, CASE WHEN entity_type LIKE 'release%' THEN 'release' WHEN entity_type LIKE 'announcement%' THEN 'announcement' WHEN entity_type IN ('notification', 'notification_smart') THEN 'notification' ELSE NULL END, entity_id, CASE WHEN entity_type LIKE '%smart' THEN 'polishing' ELSE 'translation' END, CASE WHEN status = 'ready' AND (NULLIF(trim(title), '') IS NOT NULL OR NULLIF(trim(summary), '') IS NOT NULL) THEN 'legacy_cached' ELSE 'legacy_conflict' END, '{\"source\":\"ai_translations\",\"status\":\"' || replace(status, '\"', '') || '\",\"source_hash\":\"' || replace(source_hash, '\"', '') || '\"}' , CURRENT_TIMESTAMP FROM ai_translations",
        )
        .execute(&mut **tx)
        .await?;
    }

    let has_translation_work_items = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'translation_work_items'",
    )
    .fetch_one(&mut **tx)
    .await?
        > 0;
    if has_translation_work_items {
        let query = if has_ai_translations {
            "INSERT OR IGNORE INTO content_legacy_observations (id, legacy_table, legacy_primary_key, canonical_resource_type, canonical_resource_id, pipeline, classification, observation_basis_json, observed_at) SELECT 'legacy-work-' || w.id, 'translation_work_items', w.id, CASE WHEN w.kind LIKE 'release%' THEN 'release' WHEN w.kind LIKE 'announcement%' THEN 'announcement' WHEN w.kind IN ('notification', 'notification_smart') THEN 'notification' ELSE NULL END, w.entity_id, CASE WHEN w.kind LIKE '%smart' THEN 'polishing' ELSE 'translation' END, CASE WHEN w.status = 'completed' AND COALESCE(w.result_status, '') = 'ready' AND EXISTS (SELECT 1 FROM ai_translations c WHERE c.user_id = w.scope_user_id AND c.entity_id = w.entity_id AND c.lang = w.target_lang AND c.source_hash = w.source_hash AND c.status = 'ready' AND (NULLIF(trim(c.title), '') IS NOT NULL OR NULLIF(trim(c.summary), '') IS NOT NULL)) THEN 'legacy_cached' ELSE 'legacy_conflict' END, '{\"source\":\"translation_work_items\",\"status\":\"' || replace(COALESCE(w.status, ''), '\"', '') || '\",\"source_hash\":\"' || replace(COALESCE(w.source_hash, ''), '\"', '') || '\"}' , CURRENT_TIMESTAMP FROM translation_work_items w"
        } else {
            "INSERT OR IGNORE INTO content_legacy_observations (id, legacy_table, legacy_primary_key, canonical_resource_type, canonical_resource_id, pipeline, classification, observation_basis_json, observed_at) SELECT 'legacy-work-' || w.id, 'translation_work_items', w.id, CASE WHEN w.kind LIKE 'release%' THEN 'release' WHEN w.kind LIKE 'announcement%' THEN 'announcement' WHEN w.kind IN ('notification', 'notification_smart') THEN 'notification' ELSE NULL END, w.entity_id, CASE WHEN w.kind LIKE '%smart' THEN 'polishing' ELSE 'translation' END, 'legacy_conflict', '{\"source\":\"translation_work_items\",\"status\":\"' || replace(COALESCE(w.status, ''), '\"', '') || '\",\"source_hash\":\"' || replace(COALESCE(w.source_hash, ''), '\"', '') || '\"}' , CURRENT_TIMESTAMP FROM translation_work_items w"
        };
        sqlx::query(query).execute(&mut **tx).await?;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct CutoverRequest {
    pub switch_token: String,
}

pub async fn admin_cutover(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(request): Json<CutoverRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let _ = api::require_admin_user_id(state.as_ref(), &session).await?;
    let switch_token = request.switch_token.trim();
    if switch_token.is_empty() {
        return Err(ApiError::bad_request("switch_token is required"));
    }
    if transition_to_global_state(state.as_ref(), switch_token)
        .await
        .map_err(ApiError::internal)?
    {
        return Ok((StatusCode::OK, Json(json!({"mode": "global"}))));
    }
    Err(ApiError::new(
        StatusCode::CONFLICT,
        "content_processing_cutover_not_ready",
        "content processing must be in rollback_freeze before cutover",
    ))
}

pub async fn admin_freeze(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(request): Json<CutoverRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let _ = api::require_admin_user_id(state.as_ref(), &session).await?;
    let switch_token = request.switch_token.trim();
    if switch_token.is_empty() {
        return Err(ApiError::bad_request("switch_token is required"));
    }
    if transition_to_rollback_freeze_state(state.as_ref(), switch_token)
        .await
        .map_err(ApiError::internal)?
    {
        return Ok((StatusCode::OK, Json(json!({"mode": "rollback_freeze"}))));
    }
    Err(ApiError::new(
        StatusCode::CONFLICT,
        "content_processing_freeze_not_ready",
        "content processing must be in legacy mode with no active legacy batches before freeze",
    ))
}

#[derive(Debug, Clone, Serialize)]
pub struct GlobalSubmissionResponse {
    pub request_id: String,
    pub work_item_id: String,
    pub status: String,
    pub poll_url: String,
    pub result: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct SourceSnapshot {
    #[serde(default)]
    source_blocks: Vec<translations::TranslationSourceBlock>,
    #[serde(default)]
    target_slots: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, sqlx::FromRow)]
struct WorkRow {
    id: String,
    canonical_resource_type: String,
    canonical_resource_id: String,
    pipeline: String,
    variant: String,
    target_lang: String,
    source_hash: String,
    protocol_version: String,
    model_profile: String,
    source_snapshot_json: String,
    configuration_fingerprint: String,
    status: String,
    priority: i64,
    cache_hit: i64,
    token_estimate: i64,
    batch_id: Option<String>,
    attempt_count: i64,
    next_retry_at: Option<String>,
    retry_expires_at: Option<String>,
    retry_after_at: Option<String>,
    created_at: String,
    #[sqlx(default)]
    lease_owner: Option<String>,
    #[sqlx(default)]
    lease_expires_at: Option<String>,
    #[sqlx(default)]
    attempt_configuration_snapshot_json: Option<String>,
    #[sqlx(default)]
    attempt_route_snapshot_json: Option<String>,
    #[sqlx(default)]
    attempt_configuration_fingerprint: Option<String>,
}

#[derive(Debug, Clone)]
struct AttemptRouteSnapshot {
    configuration_snapshot_json: String,
    route_snapshot_json: String,
    configuration_fingerprint: String,
    route_models: Vec<String>,
}

const GLOBAL_PROTOCOL_VERSION: &str = "content-processing.v1";
const GLOBAL_WORK_LEASE_SECS: i64 = 5 * 60;
const GLOBAL_MAX_TOKENS: u32 = 3_000;
const GLOBAL_LENGTH_RECOVERY_MAX_TOKENS: u32 = 6_000;
const RETRY_COOLDOWN_SECS: i64 = 5 * 60;
const RETRY_DELAYS_SECS: [i64; 5] = [60, 300, 900, 3600, 14_400];
const PROVIDER_DEFER_SECS: i64 = 10 * 60;

async fn provider_breaker_open(state: &AppState) -> bool {
    let routing = state
        .llm_scheduler
        .routing_status(state.config.ai.as_ref().map(|config| config.model.as_str()))
        .await;
    !routing.model_statuses.is_empty()
        && routing
            .model_statuses
            .iter()
            .all(|model| model.status == "cooldown")
}

fn canonical_identity(
    item: &translations::TranslationRequestItemInput,
) -> (&'static str, &'static str) {
    let resource_type = if item.kind.starts_with("release") {
        "release"
    } else if item.kind.starts_with("announcement") {
        "announcement"
    } else {
        "notification"
    };
    let pipeline = if item.kind.ends_with("_smart") {
        "polishing"
    } else {
        "translation"
    };
    (resource_type, pipeline)
}

fn source_hash(item: &translations::TranslationRequestItemInput) -> Result<String> {
    let source_blocks = item
        .source_blocks
        .iter()
        .filter(|block| {
            block.slot != "source_observed_at" && block.slot != "source_revision_tiebreak"
        })
        .collect::<Vec<_>>();
    let source = serde_json::to_string(&json!({
        "kind": item.kind,
        "variant": item.variant,
        "entity_id": item.entity_id,
        "target_lang": item.target_lang,
        "source_blocks": source_blocks,
        "target_slots": item.target_slots,
    }))?;
    Ok(ai::sha256_hex(&format!(
        "{GLOBAL_PROTOCOL_VERSION}\n{source}"
    )))
}

pub(crate) fn source_hash_for_item(item: &translations::TranslationRequestItemInput) -> String {
    source_hash(item).expect("translation request source fields are serializable")
}

async fn runtime_configuration_fingerprint(state: &AppState, model_profile: &str) -> String {
    let Some(snapshot) = current_attempt_route_snapshot(state).await else {
        return ai::sha256_hex(&format!(
            "{GLOBAL_PROTOCOL_VERSION}\nmodel={model_profile}\nai-disabled"
        ));
    };
    ai::sha256_hex(&format!(
        "{GLOBAL_PROTOCOL_VERSION}\nmodel={model_profile}\n{}",
        snapshot.configuration_fingerprint
    ))
}

async fn current_attempt_route_snapshot(state: &AppState) -> Option<AttemptRouteSnapshot> {
    let config = state.config.ai.as_ref()?;
    if config.api_key.trim().is_empty() || config.base_url.as_str().trim().is_empty() {
        return None;
    }
    let routing = state
        .llm_scheduler
        .routing_status(Some(config.model.as_str()))
        .await;
    if routing.llm_models.is_empty() {
        return None;
    }
    let configuration_snapshot_json = json!({
        "base_url_origin": config.base_url.origin().ascii_serialization(),
        "base_url_sha256": ai::sha256_hex(config.base_url.as_str()),
        "api_key_sha256": ai::sha256_hex(&config.api_key),
    })
    .to_string();
    let route_snapshot_json = serde_json::to_string(&routing.llm_models).ok()?;
    let configuration_fingerprint = ai::sha256_hex(&format!(
        "{GLOBAL_PROTOCOL_VERSION}\n{configuration_snapshot_json}\n{route_snapshot_json}"
    ));
    Some(AttemptRouteSnapshot {
        configuration_snapshot_json,
        route_snapshot_json,
        configuration_fingerprint,
        route_models: routing.llm_models,
    })
}

async fn has_valid_runtime_configuration(state: &AppState) -> bool {
    current_attempt_route_snapshot(state).await.is_some()
}

async fn refresh_model_routes_in_transaction(
    state: &AppState,
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<()> {
    let persisted_models =
        crate::admin_runtime::load_llm_models_in_transaction(tx, &state.config).await?;
    state
        .llm_scheduler
        .set_model_routing(persisted_models)
        .await;
    Ok(())
}

async fn has_valid_runtime_configuration_in_transaction(
    state: &AppState,
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<bool> {
    refresh_model_routes_in_transaction(state, tx).await?;
    Ok(has_valid_runtime_configuration(state).await)
}

pub async fn on_runtime_configuration_reload(state: &AppState) -> Result<()> {
    if current_mode(&state.pool).await? == ContentProcessingMode::Global
        && content_identity_upgrade::is_complete(&state.pool).await?
    {
        let (_permit, mut tx) = state
            .sqlite_writer
            .begin_immediate(&state.pool, "content_processing_startup_reconciliation")
            .await?;
        supersede_stale_work_in_transaction(&mut tx).await?;
        tx.commit().await?;
    }
    let mut requeued = 0_i64;
    loop {
        let (_permit, mut tx) = state
            .sqlite_writer
            .begin_immediate(&state.pool, "content_identity_config_recovery")
            .await?;
        if !has_valid_runtime_configuration_in_transaction(state, &mut tx).await? {
            tx.rollback().await?;
            break;
        }
        let now = Utc::now().to_rfc3339();
        let (batch_requeued, has_more) =
            content_identity_upgrade::requeue_blocked_config_batch(&mut tx, &now).await?;
        tx.commit().await?;
        requeued = requeued.saturating_add(batch_requeued);
        if !has_more {
            break;
        }
    }
    if requeued > 0 {
        tracing::info!(
            requeued,
            "requeued blocked content work after configuration reload"
        );
    }
    Ok(())
}

async fn current_model_profile(state: &AppState) -> String {
    let selected = ai::select_model_for_new_calls(state).await;
    if selected.model.trim().is_empty() {
        "ai-disabled".to_owned()
    } else {
        selected.model
    }
}

fn request_result(work: &WorkRow, projection: Option<Value>) -> Value {
    let mut result = projection
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    // Control metadata is server-owned. A persisted/model-provided field must
    // never be allowed to change the work identity or lifecycle state.
    result["status"] = Value::String(work.status.clone());
    result["work_item_id"] = Value::String(work.id.clone());
    result["batch_id"] = work
        .batch_id
        .clone()
        .map(Value::String)
        .unwrap_or(Value::Null);
    result["entity_id"] = Value::String(work.canonical_resource_id.clone());
    result["kind"] = Value::String(kind_for_work(work));
    result["variant"] = Value::String(work.variant.clone());
    result
}

fn kind_for_work(work: &WorkRow) -> String {
    if work.canonical_resource_type == "notification" {
        return if work.variant == "smart" {
            "notification_smart".to_owned()
        } else {
            "notification".to_owned()
        };
    }
    match work.variant.as_str() {
        "detail" => format!("{}_detail", work.canonical_resource_type),
        "smart" => format!("{}_smart", work.canonical_resource_type),
        _ => format!("{}_summary", work.canonical_resource_type),
    }
}

fn public_response(work: &WorkRow, request_id: &str, projection: Option<Value>) -> Value {
    json!({
        "request_id": request_id,
        "work_item_id": work.id,
        "status": work.status,
        "poll_url": format!("/api/translate/requests/{request_id}"),
        "result": request_result(work, projection),
    })
}

fn active_response(work: &WorkRow, request_id: &str, projection: Option<Value>) -> Value {
    json!({
        "ok": false,
        "error": {
            "code": "content_processing_active",
            "message": "content processing is already queued or running",
            "failure_class": null,
        },
        "request_id": request_id,
        "work_item_id": work.id,
        "status": work.status,
        "last_attempt_status": work.status,
        "poll_url": format!("/api/translate/requests/{request_id}"),
        "result": request_result(work, projection),
    })
}

async fn load_projection(
    tx: &mut Transaction<'_, Sqlite>,
    work: &WorkRow,
) -> Result<Option<Value>> {
    if content_identity_upgrade::is_complete_in_transaction(tx).await? {
        let key = identity_from_work(work);
        let identity_id = content_identity_upgrade::identity_id_for(&key)?;
        let exact = sqlx::query_scalar::<_, String>(
            "SELECT payload_json FROM content_current_result_projections WHERE identity_id = ?",
        )
        .bind(&identity_id)
        .fetch_optional(&mut **tx)
        .await?;
        let payload = if exact.is_some() {
            exact
        } else {
            sqlx::query_scalar::<_, String>(
                "SELECT p.payload_json FROM content_current_result_projections p JOIN content_work_identities i ON i.id = p.identity_id WHERE p.active_work_item_id = ? AND i.canonical_resource_type = ? AND i.canonical_resource_id = ? AND i.pipeline = ? AND i.variant = ? AND i.target_lang = ? AND i.protocol_version = ? AND i.source_hash <> ? ORDER BY julianday(p.published_at) DESC, p.published_at DESC, i.source_hash DESC LIMIT 1",
            )
            .bind(&work.id)
            .bind(&work.canonical_resource_type)
            .bind(&work.canonical_resource_id)
            .bind(&work.pipeline)
            .bind(&work.variant)
            .bind(&work.target_lang)
            .bind(&work.protocol_version)
            .bind(&work.source_hash)
            .fetch_optional(&mut **tx)
            .await?
        };
        return payload
            .map(|raw| serde_json::from_str(&raw).context("invalid current result projection"))
            .transpose();
    }
    let payload = sqlx::query_scalar::<_, String>(
        "SELECT payload_json FROM content_result_projections WHERE canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = ? AND protocol_version = ? AND model_profile = ? AND source_hash = ? LIMIT 1",
    )
    .bind(&work.canonical_resource_type)
    .bind(&work.canonical_resource_id)
    .bind(&work.pipeline)
    .bind(&work.variant)
    .bind(&work.target_lang)
    .bind(&work.protocol_version)
    .bind(&work.model_profile)
    .bind(&work.source_hash)
    .fetch_optional(&mut **tx)
    .await?;
    payload
        .map(|raw| serde_json::from_str(&raw).context("invalid global result projection"))
        .transpose()
}

fn identity_from_work(work: &WorkRow) -> content_identity_upgrade::ContentWorkIdentity {
    content_identity_upgrade::ContentWorkIdentity {
        canonical_resource_type: work.canonical_resource_type.clone(),
        canonical_resource_id: work.canonical_resource_id.clone(),
        pipeline: work.pipeline.clone(),
        variant: work.variant.clone(),
        target_lang: work.target_lang.clone(),
        source_hash: work.source_hash.clone(),
        protocol_version: work.protocol_version.clone(),
    }
}

async fn insert_request_link(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: &str,
    work_item_id: &str,
    user_id: &str,
    mode: &str,
    producer_ref: &str,
) -> Result<()> {
    let authorization = json!({"user_id": user_id, "captured_at": Utc::now().to_rfc3339()});
    sqlx::query(
        "INSERT INTO content_request_links (id, request_id, work_item_id, requester_type, requester_id, authorization_snapshot_json, producer_ref, request_source, delivery_mode, response_status, created_at, updated_at) VALUES (?, ?, ?, 'user', ?, ?, ?, 'api', ?, NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .bind(local_id::generate_local_id().to_string())
    .bind(request_id)
    .bind(work_item_id)
    .bind(user_id)
    .bind(authorization.to_string())
    .bind(producer_ref)
    .bind(mode)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn load_work_by_id(tx: &mut Transaction<'_, Sqlite>, work_item_id: &str) -> Result<WorkRow> {
    sqlx::query_as::<_, WorkRow>(
        "SELECT id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, batch_id, attempt_count, next_retry_at, retry_expires_at, retry_after_at, created_at FROM content_work_items WHERE id = ?",
    )
    .bind(work_item_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(Into::into)
}

async fn load_work_by_id_from_pool(
    pool: &SqlitePool,
    work_item_id: &str,
) -> Result<WorkRow, ApiError> {
    sqlx::query_as::<_, WorkRow>(
        "SELECT id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, batch_id, attempt_count, next_retry_at, retry_expires_at, retry_after_at, created_at FROM content_work_items WHERE id = ?",
    )
    .bind(work_item_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)
}

async fn work_for_identity(
    pool: &SqlitePool,
    key: &content_identity_upgrade::ContentWorkIdentity,
) -> Result<Option<WorkRow>, ApiError> {
    let identity_id = content_identity_upgrade::identity_id_for(key).map_err(ApiError::internal)?;
    let projected_work_item_id = sqlx::query_scalar::<_, String>(
        "SELECT work_item_id FROM content_current_result_projections WHERE identity_id = ?",
    )
    .bind(&identity_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;
    if let Some(work_item_id) = projected_work_item_id {
        return load_work_by_id_from_pool(pool, &work_item_id)
            .await
            .map(Some);
    }
    let work_item_id = sqlx::query_scalar::<_, String>(
        "SELECT w.id FROM content_work_identity_members m JOIN content_work_items w ON w.id = m.work_item_id WHERE m.identity_id = ? ORDER BY CASE w.status WHEN 'queued' THEN 0 WHEN 'running' THEN 1 WHEN 'deferred_provider' THEN 2 WHEN 'blocked_config' THEN 3 WHEN 'failed' THEN 4 WHEN 'ready' THEN 5 ELSE 6 END, w.attempt_count DESC, julianday(w.updated_at) DESC, w.updated_at DESC, w.id DESC LIMIT 1",
    )
    .bind(&identity_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;
    match work_item_id {
        Some(work_item_id) => load_work_by_id_from_pool(pool, &work_item_id)
            .await
            .map(Some),
        None => Ok(None),
    }
}

async fn work_for_identity_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    key: &content_identity_upgrade::ContentWorkIdentity,
) -> Result<Option<WorkRow>> {
    let identity_id = content_identity_upgrade::identity_id_for(key)?;
    let projected_work_item_id = sqlx::query_scalar::<_, String>(
        "SELECT work_item_id FROM content_current_result_projections WHERE identity_id = ?",
    )
    .bind(&identity_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(work_item_id) = projected_work_item_id {
        return load_work_by_id(tx, &work_item_id).await.map(Some);
    }
    let work_item_id = sqlx::query_scalar::<_, String>(
        "SELECT w.id FROM content_work_identity_members m JOIN content_work_items w ON w.id = m.work_item_id WHERE m.identity_id = ? ORDER BY CASE w.status WHEN 'queued' THEN 0 WHEN 'running' THEN 1 WHEN 'deferred_provider' THEN 2 WHEN 'blocked_config' THEN 3 WHEN 'failed' THEN 4 WHEN 'ready' THEN 5 ELSE 6 END, w.attempt_count DESC, julianday(w.updated_at) DESC, w.updated_at DESC, w.id DESC LIMIT 1",
    )
    .bind(&identity_id)
    .fetch_optional(&mut **tx)
    .await?;
    match work_item_id {
        Some(work_item_id) => load_work_by_id(tx, &work_item_id).await.map(Some),
        None => Ok(None),
    }
}

async fn newer_work_for_resource_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    work: &WorkRow,
) -> Result<Option<WorkRow>> {
    newer_work_for_source_in_transaction(
        tx,
        WorkResourceKey {
            canonical_resource_type: &work.canonical_resource_type,
            canonical_resource_id: &work.canonical_resource_id,
            pipeline: &work.pipeline,
            variant: &work.variant,
            target_lang: &work.target_lang,
            protocol_version: &work.protocol_version,
        },
        WorkSourceKey {
            snapshot_json: &work.source_snapshot_json,
            work_item_id: &work.id,
        },
    )
    .await
}

struct WorkResourceKey<'a> {
    canonical_resource_type: &'a str,
    canonical_resource_id: &'a str,
    pipeline: &'a str,
    variant: &'a str,
    target_lang: &'a str,
    protocol_version: &'a str,
}

struct WorkSourceKey<'a> {
    snapshot_json: &'a str,
    work_item_id: &'a str,
}

async fn newer_work_for_source_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    resource: WorkResourceKey<'_>,
    source: WorkSourceKey<'_>,
) -> Result<Option<WorkRow>> {
    let candidates = sqlx::query_as::<_, WorkRow>(
        "SELECT id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, batch_id, attempt_count, next_retry_at, retry_expires_at, retry_after_at, created_at FROM content_work_items WHERE id <> ? AND canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = ? AND protocol_version = ? AND status NOT IN ('cancelled', 'superseded') ORDER BY datetime(created_at) DESC, id DESC",
    )
    .bind(source.work_item_id)
    .bind(resource.canonical_resource_type)
    .bind(resource.canonical_resource_id)
    .bind(resource.pipeline)
    .bind(resource.variant)
    .bind(resource.target_lang)
    .bind(resource.protocol_version)
    .fetch_all(&mut **tx)
    .await?;
    Ok(candidates
        .into_iter()
        .filter(|candidate| {
            source_version_is_newer(&candidate.source_snapshot_json, source.snapshot_json)
        })
        .reduce(|current, candidate| {
            if source_version_is_newer(
                &candidate.source_snapshot_json,
                &current.source_snapshot_json,
            ) {
                candidate
            } else {
                current
            }
        }))
}

pub async fn submit_item(
    state: &AppState,
    user_id: &str,
    mode: &str,
    item: &translations::TranslationRequestItemInput,
) -> Result<(StatusCode, GlobalSubmissionResponse), ApiError> {
    // This is the scheduler admission transaction. API adapters only provide
    // an already-authorized immutable request; provider calls, attempts and
    // terminal projections remain scheduler-worker responsibilities.
    let processing_mode = current_mode(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    match processing_mode {
        ContentProcessingMode::Global => {}
        ContentProcessingMode::RollbackFreeze => {
            let (resource_type, pipeline) = canonical_identity(item);
            let hash = source_hash(item).map_err(ApiError::internal)?;
            let details = sqlx::query(
                "SELECT w.id AS work_item_id, w.status, l.request_id FROM content_work_items w LEFT JOIN content_request_links l ON l.work_item_id = w.id AND l.requester_id = ? WHERE w.canonical_resource_type = ? AND w.canonical_resource_id = ? AND w.pipeline = ? AND w.variant = ? AND w.target_lang = ? AND w.source_hash = ? AND w.protocol_version = ? ORDER BY datetime(w.updated_at) DESC, w.id DESC, datetime(l.created_at) DESC LIMIT 1",
            )
            .bind(user_id)
            .bind(resource_type)
            .bind(&item.entity_id)
            .bind(pipeline)
            .bind(&item.variant)
            .bind(&item.target_lang)
            .bind(&hash)
            .bind(GLOBAL_PROTOCOL_VERSION)
            .fetch_optional(&state.pool)
            .await
            .map_err(ApiError::internal)?
            .map(|row| {
                let request_id = row.get::<Option<String>, _>("request_id");
                json!({
                    "mode": ContentProcessingMode::RollbackFreeze.as_str(),
                    "work_item_id": row.get::<String, _>("work_item_id"),
                    "status": row.get::<String, _>("status"),
                    "request_id": request_id,
                    "poll_url": request_id.map(|id| format!("/api/translate/requests/{id}")),
                })
            })
            .unwrap_or_else(|| json!({
                "mode": ContentProcessingMode::RollbackFreeze.as_str(),
                "request_id": Value::Null,
                "work_item_id": Value::Null,
                "poll_url": Value::Null,
            }));
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "content_processing_transition",
                "content processing is temporarily frozen; poll the request status before retrying",
            )
            .with_details(details));
        }
        ContentProcessingMode::Legacy => {
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "content_processing_legacy",
                "global content processing is not active",
            ));
        }
    }
    let (resource_type, pipeline) = canonical_identity(item);
    let hash = source_hash(item).map_err(ApiError::internal)?;
    let snapshot = serde_json::to_string(&json!({
        "source_blocks": item.source_blocks,
        "target_slots": item.target_slots,
        "source_revision": source_revision_json(item),
    }))
    .map_err(ApiError::internal)?;
    let now = Utc::now().to_rfc3339();
    let request_id = local_id::generate_local_id().to_string();
    let work_id = local_id::generate_local_id().to_string();
    let (_lock, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "content_processing_submit")
        .await
        .map_err(ApiError::internal)?;
    ensure_global_mode_in_transaction(&mut tx).await?;
    let configuration_valid = has_valid_runtime_configuration_in_transaction(state, &mut tx)
        .await
        .map_err(ApiError::internal)?;
    let model_profile = current_model_profile(state).await;
    let identity = content_identity_upgrade::ContentWorkIdentity {
        canonical_resource_type: resource_type.to_owned(),
        canonical_resource_id: item.entity_id.clone(),
        pipeline: pipeline.to_owned(),
        variant: item.variant.clone(),
        target_lang: item.target_lang.clone(),
        source_hash: hash.clone(),
        protocol_version: GLOBAL_PROTOCOL_VERSION.to_owned(),
    };
    let identity_id =
        content_identity_upgrade::ensure_identity_registered(&mut tx, &identity, &now)
            .await
            .map_err(ApiError::internal)?;
    content_identity_upgrade::ensure_all_identity_members(&mut tx, &identity, &identity_id, &now)
        .await
        .map_err(ApiError::internal)?;
    let current_projection =
        content_identity_upgrade::ensure_current_projection_for_key(&mut tx, &identity_id, &now)
            .await
            .map_err(ApiError::internal)?;
    // The registry is authoritative for identity; model_profile only records
    // the provenance of retained model-specific work rows.
    let existing = if let Some(projection) = current_projection.as_ref() {
        Some(
            load_work_by_id(&mut tx, &projection.work_item_id)
                .await
                .map_err(ApiError::internal)?,
        )
    } else {
        sqlx::query_as::<_, WorkRow>(
            "SELECT w.id, w.canonical_resource_type, w.canonical_resource_id, w.pipeline, w.variant, w.target_lang, w.source_hash, w.protocol_version, w.model_profile, w.source_snapshot_json, w.configuration_fingerprint, w.status, w.priority, w.cache_hit, w.token_estimate, w.batch_id, w.attempt_count, w.next_retry_at, w.retry_expires_at, w.retry_after_at, w.created_at FROM content_work_identity_members m JOIN content_work_items w ON w.id = m.work_item_id WHERE m.identity_id = ? ORDER BY CASE w.status WHEN 'queued' THEN 0 WHEN 'running' THEN 1 WHEN 'deferred_provider' THEN 2 WHEN 'blocked_config' THEN 3 WHEN 'failed' THEN 4 WHEN 'ready' THEN 5 ELSE 6 END, w.attempt_count DESC, julianday(w.updated_at) DESC, w.updated_at DESC, w.id DESC LIMIT 1",
        )
        .bind(&identity_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(ApiError::internal)?
    };
    let existing_work = existing.is_some();
    if let Some(existing) = existing.as_ref()
        && let Some(current) = newer_work_for_resource_in_transaction(&mut tx, existing)
            .await
            .map_err(ApiError::internal)?
    {
        record_work_admission_event(
            &mut tx,
            WorkAdmissionEvent {
                work_item_id: &existing.id,
                event_type: "admission_rejected_superseded",
                replaced_by_work_item_id: Some(&current.id),
                source_hash: &existing.source_hash,
                source_snapshot_json: &existing.source_snapshot_json,
                producer_ref: &item.producer_ref,
                requester_id: Some(user_id),
                reason_code: "older_source_projection_redirected",
            },
        )
        .await
        .map_err(ApiError::internal)?;
        let current_projection = load_projection(&mut tx, &current)
            .await
            .map_err(ApiError::internal)?;
        insert_request_link(
            &mut tx,
            &request_id,
            &current.id,
            user_id,
            mode,
            &item.producer_ref,
        )
        .await
        .map_err(ApiError::internal)?;
        tx.commit().await.map_err(ApiError::internal)?;
        let mut result = request_result(&current, current_projection);
        result["producer_ref"] = Value::String(item.producer_ref.clone());
        result["kind"] = Value::String(item.kind.clone());
        result["variant"] = Value::String(item.variant.clone());
        return Ok((
            StatusCode::CONFLICT,
            GlobalSubmissionResponse {
                request_id: request_id.clone(),
                work_item_id: current.id.clone(),
                status: current.status.clone(),
                poll_url: format!("/api/translate/requests/{request_id}"),
                result,
                error: Some(json!({
                    "code": "content_processing_superseded",
                    "message": "the submitted source version was superseded; polling the current source version",
                    "superseded_work_item_id": existing.id.clone(),
                    "current_work_item_id": current.id,
                })),
            },
        ));
    }
    let work = if let Some(existing) = existing {
        existing
    } else {
        let newer_source = newer_work_for_source_in_transaction(
            &mut tx,
            WorkResourceKey {
                canonical_resource_type: resource_type,
                canonical_resource_id: &item.entity_id,
                pipeline,
                variant: &item.variant,
                target_lang: &item.target_lang,
                protocol_version: GLOBAL_PROTOCOL_VERSION,
            },
            WorkSourceKey {
                snapshot_json: &snapshot,
                work_item_id: &work_id,
            },
        )
        .await
        .map_err(ApiError::internal)?;
        let supersedes_work_item_id = if let Some(newer_source) = newer_source.as_ref() {
            Some(newer_source.id.clone())
        } else {
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM content_work_items WHERE canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = ? AND protocol_version = ? AND source_hash <> ? AND status NOT IN ('cancelled', 'superseded') ORDER BY datetime(updated_at) DESC, id DESC LIMIT 1",
            )
            .bind(resource_type)
            .bind(&item.entity_id)
            .bind(pipeline)
            .bind(&item.variant)
            .bind(&item.target_lang)
            .bind(GLOBAL_PROTOCOL_VERSION)
            .bind(&hash)
            .fetch_optional(&mut *tx)
            .await
            .map_err(ApiError::internal)?
        };
        let status = if newer_source.is_some() {
            "superseded"
        } else if configuration_valid {
            "queued"
        } else {
            "blocked_config"
        };
        sqlx::query(
            "INSERT INTO content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, batch_id, supersedes_work_item_id, attempt_count, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, NULL, ?, 0, ?, ?)",
        )
        .bind(&work_id)
        .bind(resource_type)
        .bind(&item.entity_id)
        .bind(pipeline)
        .bind(&item.variant)
        .bind(&item.target_lang)
        .bind(&hash)
        .bind(GLOBAL_PROTOCOL_VERSION)
        .bind(&model_profile)
        .bind(&snapshot)
        .bind(runtime_configuration_fingerprint(state, &model_profile).await)
        .bind(status)
        .bind(0_i64)
        .bind(i64::try_from(item.source_blocks.iter().map(|block| block.text.len()).sum::<usize>()).unwrap_or(i64::MAX))
        .bind(&supersedes_work_item_id)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
        content_identity_upgrade::ensure_all_identity_members(
            &mut tx,
            &identity,
            &identity_id,
            &now,
        )
        .await
        .map_err(ApiError::internal)?;
        load_work_by_id(&mut tx, &work_id)
            .await
            .map_err(ApiError::internal)?
    };
    if !existing_work {
        supersede_older_work_in_transaction(&mut tx, &work)
            .await
            .map_err(ApiError::internal)?;
    }
    let supersedes_work_item_id = sqlx::query_scalar::<_, Option<String>>(
        "SELECT supersedes_work_item_id FROM content_work_items WHERE id = ?",
    )
    .bind(&work.id)
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    if supersedes_work_item_id.is_some() && work.status != "superseded" {
        // Advance every retained projection for this resource. This keeps the
        // pointer transitive when a second refresh arrives before the first
        // replacement has published.
        sqlx::query("UPDATE content_result_projections SET active_work_item_id = ?, updated_at = ? WHERE canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = ? AND protocol_version = ? AND source_hash <> ? AND (active_work_item_id IS NULL OR active_work_item_id <> ?)")
            .bind(&work.id)
            .bind(&now)
            .bind(resource_type)
            .bind(&item.entity_id)
            .bind(pipeline)
            .bind(&item.variant)
            .bind(&item.target_lang)
            .bind(GLOBAL_PROTOCOL_VERSION)
            .bind(&hash)
            .bind(&work.id)
            .execute(&mut *tx)
            .await
            .map_err(ApiError::internal)?;
        sqlx::query("UPDATE content_current_result_projections SET active_work_item_id = ?, updated_at = ? WHERE identity_id IN (SELECT id FROM content_work_identities WHERE canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = ? AND protocol_version = ? AND source_hash <> ?) AND (active_work_item_id IS NULL OR active_work_item_id <> ?)")
            .bind(&work.id)
            .bind(&now)
            .bind(resource_type)
            .bind(&item.entity_id)
            .bind(pipeline)
            .bind(&item.variant)
            .bind(&item.target_lang)
            .bind(GLOBAL_PROTOCOL_VERSION)
            .bind(&hash)
            .bind(&work.id)
            .execute(&mut *tx)
            .await
            .map_err(ApiError::internal)?;
    }
    let projection = if let Some(projection) = current_projection {
        serde_json::from_str(&projection.payload_json).ok()
    } else {
        load_projection(&mut tx, &work)
            .await
            .map_err(ApiError::internal)?
    };
    let superseded_replacement_id = if work.status == "superseded" {
        match supersedes_work_item_id.clone() {
            Some(work_item_id) => Some(work_item_id),
            None => newer_work_for_resource_in_transaction(&mut tx, &work)
                .await
                .map_err(ApiError::internal)?
                .map(|current| current.id),
        }
    } else {
        None
    };

    let admission_event = if work.status == "superseded" {
        "admission_rejected_superseded"
    } else if existing_work {
        "admission_noop"
    } else {
        "admission_accepted"
    };
    record_work_admission_event(
        &mut tx,
        WorkAdmissionEvent {
            work_item_id: &work.id,
            event_type: admission_event,
            replaced_by_work_item_id: superseded_replacement_id.as_deref(),
            source_hash: &work.source_hash,
            source_snapshot_json: &work.source_snapshot_json,
            producer_ref: &item.producer_ref,
            requester_id: Some(user_id),
            reason_code: if work.status == "superseded" {
                "older_source_rejected_at_admission"
            } else {
                "source_current"
            },
        },
    )
    .await
    .map_err(ApiError::internal)?;

    if let Some(current_work_item_id) = superseded_replacement_id.as_deref() {
        let current = load_work_by_id(&mut tx, current_work_item_id)
            .await
            .map_err(ApiError::internal)?;
        let current_projection = load_projection(&mut tx, &current)
            .await
            .map_err(ApiError::internal)?;
        insert_request_link(
            &mut tx,
            &request_id,
            &current.id,
            user_id,
            mode,
            &item.producer_ref,
        )
        .await
        .map_err(ApiError::internal)?;
        tx.commit().await.map_err(ApiError::internal)?;
        let mut result = request_result(&current, current_projection);
        result["producer_ref"] = Value::String(item.producer_ref.clone());
        result["kind"] = Value::String(item.kind.clone());
        result["variant"] = Value::String(item.variant.clone());
        return Ok((
            StatusCode::CONFLICT,
            GlobalSubmissionResponse {
                request_id: request_id.clone(),
                work_item_id: current.id,
                status: current.status,
                poll_url: format!("/api/translate/requests/{request_id}"),
                result,
                error: Some(json!({
                    "code": "content_processing_superseded",
                    "message": "the submitted source version was superseded; polling the current source version",
                    "superseded_work_item_id": work.id,
                    "current_work_item_id": current_work_item_id,
                })),
            },
        ));
    }

    if let Some(existing_request_id) = sqlx::query_scalar::<_, String>(
        "SELECT request_id FROM content_request_links WHERE work_item_id = ? AND requester_id = ? AND producer_ref = ? ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&work.id)
    .bind(user_id)
    .bind(&item.producer_ref)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?
    {
        tx.commit().await.map_err(ApiError::internal)?;
        let mut result = request_result(&work, projection);
        result["producer_ref"] = Value::String(item.producer_ref.clone());
        result["kind"] = Value::String(item.kind.clone());
        result["variant"] = Value::String(item.variant.clone());
        let poll_url = format!("/api/translate/requests/{existing_request_id}");
        return Ok((
            StatusCode::ACCEPTED,
            GlobalSubmissionResponse {
                request_id: existing_request_id,
                work_item_id: work.id.clone(),
                status: work.status.clone(),
                poll_url,
                result,
                error: None,
            },
        ));
    }
    insert_request_link(
        &mut tx,
        &request_id,
        &work.id,
        user_id,
        mode,
        &item.producer_ref,
    )
    .await
    .map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::internal)?;
    let mut status_code = if existing_work && matches!(work.status.as_str(), "queued" | "running") {
        StatusCode::CONFLICT
    } else {
        StatusCode::ACCEPTED
    };
    let mut result = request_result(&work, projection);
    result["producer_ref"] = Value::String(item.producer_ref.clone());
    result["kind"] = Value::String(item.kind.clone());
    result["variant"] = Value::String(item.variant.clone());
    let mut body = GlobalSubmissionResponse {
        request_id: request_id.clone(),
        work_item_id: work.id.clone(),
        status: work.status.clone(),
        poll_url: format!("/api/translate/requests/{request_id}"),
        result,
        error: None,
    };
    if status_code == StatusCode::CONFLICT {
        body.error = Some(json!({
            "code": "content_processing_active",
            "message": "content processing is already queued or running",
        }));
    }
    if mode == "wait" {
        let deadline = std::time::Instant::now()
            + Duration::from_millis(
                u64::try_from(item.max_wait_ms.max(0))
                    .unwrap_or(0)
                    .min(60_000),
            );
        loop {
            if let Some(response) = get_request(state, user_id, &body.request_id).await? {
                if let Some(result) = response.get("result") {
                    body.result = result.clone();
                }
                if let Some(status) = response.get("status").and_then(Value::as_str) {
                    body.status = status.to_owned();
                }
                if !matches!(
                    body.status.as_str(),
                    "queued" | "running" | "deferred_provider" | "blocked_config"
                ) {
                    status_code = StatusCode::OK;
                    break;
                }
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
    }
    Ok((status_code, body))
}

pub async fn get_request(
    state: &AppState,
    user_id: &str,
    request_id: &str,
) -> Result<Option<Value>, ApiError> {
    let row = sqlx::query(
        "SELECT l.request_id, l.requester_id, l.producer_ref, w.id, w.canonical_resource_type, w.canonical_resource_id, w.pipeline, w.variant, w.target_lang, w.source_hash, w.protocol_version, w.model_profile, w.source_snapshot_json, w.configuration_fingerprint, w.status, w.priority, w.cache_hit, w.token_estimate, w.batch_id, w.attempt_count, w.next_retry_at, w.retry_expires_at, w.retry_after_at, w.created_at FROM content_request_links l JOIN content_work_items w ON w.id = l.work_item_id WHERE l.request_id = ? AND l.requester_id = ? LIMIT 1",
    )
    .bind(request_id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let work = WorkRow {
        id: row.get("id"),
        canonical_resource_type: row.get("canonical_resource_type"),
        canonical_resource_id: row.get("canonical_resource_id"),
        pipeline: row.get("pipeline"),
        variant: row.get("variant"),
        target_lang: row.get("target_lang"),
        source_hash: row.get("source_hash"),
        protocol_version: row.get("protocol_version"),
        model_profile: row.get("model_profile"),
        source_snapshot_json: row.get("source_snapshot_json"),
        configuration_fingerprint: row.get("configuration_fingerprint"),
        status: row.get("status"),
        priority: row.get("priority"),
        cache_hit: row.get("cache_hit"),
        token_estimate: row.get("token_estimate"),
        batch_id: row.get("batch_id"),
        attempt_count: row.get("attempt_count"),
        next_retry_at: row.get("next_retry_at"),
        retry_expires_at: row.get("retry_expires_at"),
        retry_after_at: row.get("retry_after_at"),
        created_at: row.get("created_at"),
        lease_owner: row.try_get("lease_owner").ok(),
        lease_expires_at: row.try_get("lease_expires_at").ok(),
        attempt_configuration_snapshot_json: None,
        attempt_route_snapshot_json: None,
        attempt_configuration_fingerprint: None,
    };
    let authorization_probe = translations::TranslationRequestItemInput {
        producer_ref: row.get("producer_ref"),
        kind: kind_for_work(&work),
        variant: work.variant.clone(),
        entity_id: work.canonical_resource_id.clone(),
        target_lang: work.target_lang.clone(),
        max_wait_ms: 0,
        source_blocks: Vec::new(),
        target_slots: Vec::new(),
    };
    api::canonical_global_translation_item(state, user_id, &authorization_probe).await?;
    let identity_upgrade_complete = content_identity_upgrade::is_complete(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    let response_work = if identity_upgrade_complete {
        work_for_identity(&state.pool, &identity_from_work(&work))
            .await?
            .unwrap_or_else(|| work.clone())
    } else {
        work.clone()
    };
    let (visible_status, projection) = if identity_upgrade_complete {
        read_global_resource(
            state,
            &work.canonical_resource_type,
            &work.canonical_resource_id,
            &work.pipeline,
            &work.variant,
            &work.source_hash,
        )
        .await?
        .unwrap_or_else(|| (response_work.status.clone(), json!({})))
    } else {
        let projection = sqlx::query_scalar::<_, String>(
            "SELECT payload_json FROM content_result_projections WHERE canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = ? AND protocol_version = ? AND model_profile = ? AND source_hash = ? ORDER BY datetime(updated_at) DESC, id DESC LIMIT 1",
        )
        .bind(&work.canonical_resource_type)
        .bind(&work.canonical_resource_id)
        .bind(&work.pipeline)
        .bind(&work.variant)
        .bind(&work.target_lang)
        .bind(&work.protocol_version)
        .bind(&work.model_profile)
        .bind(&work.source_hash)
        .fetch_optional(&state.pool)
        .await
        .map_err(ApiError::internal)?
        .and_then(|raw| serde_json::from_str(&raw).ok());
        (work.status.clone(), projection.unwrap_or_else(|| json!({})))
    };
    let mut response = public_response(&response_work, request_id, Some(projection));
    response["status"] = Value::String(visible_status.clone());
    response["result"]["status"] = Value::String(visible_status);
    response["result"]["producer_ref"] = authorization_probe.producer_ref.into();
    response["result"]["kind"] = Value::String(kind_for_work(&response_work));
    Ok(Some(response))
}

pub async fn read_global_resource(
    state: &AppState,
    resource_type: &str,
    resource_id: &str,
    pipeline: &str,
    variant: &str,
    expected_source_hash: &str,
) -> Result<Option<(String, Value)>, ApiError> {
    if content_identity_upgrade::is_complete(&state.pool)
        .await
        .map_err(ApiError::internal)?
    {
        let key = content_identity_upgrade::ContentWorkIdentity {
            canonical_resource_type: resource_type.to_owned(),
            canonical_resource_id: resource_id.to_owned(),
            pipeline: pipeline.to_owned(),
            variant: variant.to_owned(),
            target_lang: "zh-CN".to_owned(),
            source_hash: expected_source_hash.to_owned(),
            protocol_version: GLOBAL_PROTOCOL_VERSION.to_owned(),
        };
        let identity_id =
            content_identity_upgrade::identity_id_for(&key).map_err(ApiError::internal)?;
        let exact_payload = sqlx::query_scalar::<_, String>(
            "SELECT payload_json FROM content_current_result_projections WHERE identity_id = ?",
        )
        .bind(&identity_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(ApiError::internal)?;
        if let Some(raw) = exact_payload {
            let payload = serde_json::from_str(&raw).map_err(ApiError::internal)?;
            return Ok(Some(("ready".to_owned(), payload)));
        }
        let Some(current_work) = work_for_identity(&state.pool, &key).await? else {
            return Ok(None);
        };
        let retained_payload: Option<Value> = sqlx::query_scalar::<_, String>(
            "SELECT p.payload_json FROM content_current_result_projections p JOIN content_work_identities i ON i.id = p.identity_id WHERE p.active_work_item_id = ? AND i.canonical_resource_type = ? AND i.canonical_resource_id = ? AND i.pipeline = ? AND i.variant = ? AND i.target_lang = ? AND i.protocol_version = ? AND i.source_hash <> ? ORDER BY julianday(p.published_at) DESC, p.published_at DESC, i.source_hash DESC LIMIT 1",
        )
        .bind(&current_work.id)
        .bind(resource_type)
        .bind(resource_id)
        .bind(pipeline)
        .bind(variant)
        .bind("zh-CN")
        .bind(GLOBAL_PROTOCOL_VERSION)
        .bind(expected_source_hash)
        .fetch_optional(&state.pool)
        .await
        .map_err(ApiError::internal)?
        .and_then(|raw| serde_json::from_str(&raw).ok());
        if current_work.status == "failed"
            && retained_payload
                .as_ref()
                .is_some_and(|payload| payload.is_object())
        {
            return Ok(Some((
                "ready".to_owned(),
                retained_payload.unwrap_or_else(|| json!({})),
            )));
        }
        return Ok(Some((
            current_work.status,
            retained_payload.unwrap_or_else(|| json!({})),
        )));
    }
    let model_profile = current_model_profile(state).await;
    let current = sqlx::query_as::<_, (String, String, Option<String>, String)>(
        "SELECT id, status, supersedes_work_item_id, model_profile FROM content_work_items WHERE canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = 'zh-CN' AND source_hash = ? AND protocol_version = ? AND model_profile = ? ORDER BY datetime(updated_at) DESC, id DESC LIMIT 1",
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(pipeline)
    .bind(variant)
    .bind(expected_source_hash)
    .bind(GLOBAL_PROTOCOL_VERSION)
    .bind(&model_profile)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    let current = if current.is_some() {
        current
    } else {
        sqlx::query_as::<_, (String, String, Option<String>, String)>(
            "SELECT id, status, supersedes_work_item_id, model_profile FROM content_work_items WHERE canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = 'zh-CN' AND source_hash = ? AND protocol_version = ? ORDER BY CASE status WHEN 'ready' THEN 0 WHEN 'queued' THEN 1 WHEN 'running' THEN 2 WHEN 'deferred_provider' THEN 3 WHEN 'blocked_config' THEN 4 ELSE 5 END, datetime(updated_at) DESC, id DESC LIMIT 1",
        )
        .bind(resource_type)
        .bind(resource_id)
        .bind(pipeline)
        .bind(variant)
        .bind(expected_source_hash)
        .bind(GLOBAL_PROTOCOL_VERSION)
        .fetch_optional(&state.pool)
        .await
        .map_err(ApiError::internal)?
    };
    let status = if let Some((_, status, _, _)) = &current {
        status.clone()
    } else {
        sqlx::query_scalar::<_, String>(
            "SELECT status FROM content_work_items WHERE canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = 'zh-CN' AND source_hash = ? AND protocol_version = ? ORDER BY datetime(updated_at) DESC, id DESC LIMIT 1",
        )
        .bind(resource_type)
        .bind(resource_id)
        .bind(pipeline)
        .bind(variant)
        .bind(expected_source_hash)
        .bind(GLOBAL_PROTOCOL_VERSION)
        .fetch_optional(&state.pool)
        .await
        .map_err(ApiError::internal)?
        .unwrap_or_else(|| "ready".to_owned())
    };
    let exact_projection = sqlx::query_scalar::<_, String>(
        "SELECT payload_json FROM content_result_projections WHERE canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = 'zh-CN' AND protocol_version = ? AND source_hash = ? ORDER BY CASE WHEN model_profile = ? THEN 0 ELSE 1 END, datetime(updated_at) DESC, id DESC LIMIT 1",
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(pipeline)
    .bind(variant)
    .bind(GLOBAL_PROTOCOL_VERSION)
    .bind(expected_source_hash)
    .bind(&model_profile)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    let retained_projection = if exact_projection.is_none() {
        if let Some((current_id, _, _, _)) = &current {
            let active_projection = sqlx::query_scalar::<_, String>(
                "SELECT p.payload_json FROM content_result_projections p JOIN content_work_items w ON w.id = p.work_item_id WHERE p.canonical_resource_type = ? AND p.canonical_resource_id = ? AND p.pipeline = ? AND p.variant = ? AND p.target_lang = 'zh-CN' AND p.protocol_version = ? AND p.active_work_item_id = ? ORDER BY datetime(p.updated_at) DESC, p.id DESC LIMIT 1",
            )
            .bind(resource_type)
            .bind(resource_id)
            .bind(pipeline)
            .bind(variant)
            .bind(GLOBAL_PROTOCOL_VERSION)
            .bind(current_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(ApiError::internal)?;
            if active_projection.is_some() {
                active_projection
            } else {
                sqlx::query_scalar::<_, String>(
                    "SELECT p.payload_json FROM content_result_projections p JOIN content_work_items w ON w.id = p.work_item_id WHERE p.canonical_resource_type = ? AND p.canonical_resource_id = ? AND p.pipeline = ? AND p.variant = ? AND p.target_lang = 'zh-CN' AND p.protocol_version = ? AND p.source_hash <> ? AND w.status = 'ready' ORDER BY datetime(p.updated_at) DESC, p.id DESC LIMIT 1",
                )
                .bind(resource_type)
                .bind(resource_id)
                .bind(pipeline)
                .bind(variant)
                .bind(GLOBAL_PROTOCOL_VERSION)
                .bind(expected_source_hash)
                .fetch_optional(&state.pool)
                .await
                .map_err(ApiError::internal)?
            }
        } else {
            None
        }
    } else {
        None
    };
    let payload_json = exact_projection.or(retained_projection);
    if current.is_none() && payload_json.is_none() {
        return Ok(None);
    }
    let status = if payload_json.is_some()
        && matches!(
            status.as_str(),
            "failed" | "cancelled" | "superseded" | "not_applicable" | "blocked_config"
        ) {
        "ready".to_owned()
    } else {
        status
    };
    let payload = payload_json
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_else(|| json!({}));
    Ok(Some((status, payload)))
}

#[allow(clippy::too_many_arguments)]
pub async fn latest_request_id_for_resource(
    state: &AppState,
    requester_id: &str,
    resource_type: &str,
    resource_id: &str,
    pipeline: &str,
    variant: &str,
    target_lang: &str,
    source_hash: &str,
) -> Result<Option<String>, ApiError> {
    sqlx::query_scalar::<_, String>(
        "SELECT l.request_id FROM content_request_links l JOIN content_work_items w ON w.id = l.work_item_id WHERE l.requester_id = ? AND w.canonical_resource_type = ? AND w.canonical_resource_id = ? AND w.pipeline = ? AND w.variant = ? AND w.target_lang = ? AND w.source_hash = ? AND w.protocol_version = ? ORDER BY datetime(l.created_at) DESC, l.request_id DESC LIMIT 1",
    )
    .bind(requester_id)
    .bind(resource_type)
    .bind(resource_id)
    .bind(pipeline)
    .bind(variant)
    .bind(target_lang)
    .bind(source_hash)
    .bind(GLOBAL_PROTOCOL_VERSION)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)
}

pub async fn retry_request(
    state: &AppState,
    user_id: &str,
    request_id: &str,
) -> Result<(StatusCode, Value), ApiError> {
    // Manual retry is a scheduler command, serialized through the same writer
    // boundary as claims. The API layer delegates authorization and shaping.
    let breaker_open = provider_breaker_open(state).await;
    let (_lock, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "content_processing_retry")
        .await
        .map_err(ApiError::internal)?;
    ensure_global_mode_in_transaction(&mut tx).await?;
    let configuration_valid = has_valid_runtime_configuration_in_transaction(state, &mut tx)
        .await
        .map_err(ApiError::internal)?;
    let mut row = sqlx::query_as::<_, WorkRow>(
        "SELECT w.id, w.canonical_resource_type, w.canonical_resource_id, w.pipeline, w.variant, w.target_lang, w.source_hash, w.protocol_version, w.model_profile, w.source_snapshot_json, w.configuration_fingerprint, w.status, w.priority, w.cache_hit, w.token_estimate, w.batch_id, w.attempt_count, w.next_retry_at, w.retry_expires_at, w.retry_after_at, w.created_at FROM content_request_links l JOIN content_work_items w ON w.id = l.work_item_id WHERE l.request_id = ? AND l.requester_id = ? LIMIT 1",
    )
    .bind(request_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "translation request not found"))?;
    let requested_row = row.clone();
    let key = identity_from_work(&row);
    let now = Utc::now().to_rfc3339();
    let identity_id = content_identity_upgrade::ensure_identity_registered(&mut tx, &key, &now)
        .await
        .map_err(ApiError::internal)?;
    content_identity_upgrade::ensure_all_identity_members(&mut tx, &key, &identity_id, &now)
        .await
        .map_err(ApiError::internal)?;
    content_identity_upgrade::ensure_current_projection_for_key(&mut tx, &identity_id, &now)
        .await
        .map_err(ApiError::internal)?;
    let producer_ref = sqlx::query_scalar::<_, String>(
        "SELECT producer_ref FROM content_request_links WHERE request_id = ? LIMIT 1",
    )
    .bind(request_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    if let Some(current) = newer_work_for_resource_in_transaction(&mut tx, &requested_row)
        .await
        .map_err(ApiError::internal)?
    {
        if requested_row.status != "superseded" {
            supersede_work_in_transaction(
                &mut tx,
                &requested_row,
                Some(&current.id),
                "newer_source_detected_before_manual_retry",
            )
            .await
            .map_err(ApiError::internal)?;
        }
        let new_request_id = local_id::generate_local_id().to_string();
        insert_request_link(
            &mut tx,
            &new_request_id,
            &current.id,
            user_id,
            "async",
            &producer_ref,
        )
        .await
        .map_err(ApiError::internal)?;
        record_work_admission_event(
            &mut tx,
            WorkAdmissionEvent {
                work_item_id: &requested_row.id,
                event_type: "admission_rejected_superseded",
                replaced_by_work_item_id: Some(&current.id),
                source_hash: &requested_row.source_hash,
                source_snapshot_json: &requested_row.source_snapshot_json,
                producer_ref: &producer_ref,
                requester_id: Some(user_id),
                reason_code: "manual_retry_redirected_to_current_source",
            },
        )
        .await
        .map_err(ApiError::internal)?;
        let current_projection = load_projection(&mut tx, &current)
            .await
            .map_err(ApiError::internal)?;
        tx.commit().await.map_err(ApiError::internal)?;
        let mut body = public_response(&current, &new_request_id, current_projection);
        body["status"] = Value::String(current.status.clone());
        body["result"]["status"] = Value::String(current.status.clone());
        body["error"] = json!({
            "code": "content_processing_superseded",
            "message": "the requested source version was superseded; polling the current source version",
            "superseded_work_item_id": requested_row.id,
            "current_work_item_id": current.id,
        });
        return Ok((StatusCode::CONFLICT, body));
    }
    if let Some(current) = work_for_identity_in_transaction(&mut tx, &key)
        .await
        .map_err(ApiError::internal)?
    {
        row = current;
    }
    let projection = load_projection(&mut tx, &row)
        .await
        .map_err(ApiError::internal)?;
    if matches!(
        row.status.as_str(),
        "queued" | "running" | "deferred_provider"
    ) {
        let new_request_id = local_id::generate_local_id().to_string();
        insert_request_link(
            &mut tx,
            &new_request_id,
            &row.id,
            user_id,
            "async",
            &producer_ref,
        )
        .await
        .map_err(ApiError::internal)?;
        tx.commit().await.map_err(ApiError::internal)?;
        let mut body = active_response(&row, &new_request_id, projection);
        body["result"]["producer_ref"] = Value::String(producer_ref.clone());
        return Ok((StatusCode::CONFLICT, body));
    }
    if row.status != "failed" {
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok((
            StatusCode::CONFLICT,
            public_response(&row, request_id, projection),
        ));
    }
    if let Some(retry_after) = row.retry_after_at.as_deref()
        && retry_after > Utc::now().to_rfc3339().as_str()
    {
        let new_request_id = local_id::generate_local_id().to_string();
        insert_request_link(
            &mut tx,
            &new_request_id,
            &row.id,
            user_id,
            "async",
            &producer_ref,
        )
        .await
        .map_err(ApiError::internal)?;
        tx.commit().await.map_err(ApiError::internal)?;
        let mut body = active_response(&row, &new_request_id, projection);
        body["result"]["producer_ref"] = Value::String(producer_ref.clone());
        body["error"]["code"] = Value::String("content_processing_retry_cooldown".to_owned());
        return Ok((StatusCode::TOO_MANY_REQUESTS, body));
    }
    let request_id = local_id::generate_local_id().to_string();
    let retry_status = if !configuration_valid {
        "blocked_config"
    } else if breaker_open {
        "deferred_provider"
    } else {
        "queued"
    };
    let next_retry_at = (configuration_valid && breaker_open)
        .then(|| (Utc::now() + chrono::Duration::seconds(PROVIDER_DEFER_SECS)).to_rfc3339());
    let retry_expires_at = (Utc::now() + chrono::Duration::hours(24)).to_rfc3339();
    let previous_attempt_no = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(attempt_no) FROM content_attempt_events WHERE work_item_id = ?",
    )
    .bind(&row.id)
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?
    .unwrap_or(row.attempt_count)
    .max(row.attempt_count)
    .max(0);
    let next_attempt_no = previous_attempt_no.saturating_add(1);
    sqlx::query("UPDATE content_work_items SET status = ?, priority = 3, failure_class = CASE WHEN ? = 'blocked_config' THEN 'configuration' ELSE NULL END, next_retry_at = ?, retry_expires_at = CASE WHEN ? = 'blocked_config' THEN retry_expires_at WHEN retry_expires_at IS NULL OR julianday(retry_expires_at) <= julianday('now') THEN ? ELSE retry_expires_at END, retry_after_at = NULL, finished_at = NULL, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(retry_status)
        .bind(retry_status)
        .bind(&next_retry_at)
        .bind(retry_status)
        .bind(&retry_expires_at)
        .bind(&row.id)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    insert_request_link(
        &mut tx,
        &request_id,
        &row.id,
        user_id,
        "async",
        &producer_ref,
    )
    .await
    .map_err(ApiError::internal)?;
    if retry_status != "blocked_config" {
        sqlx::query("INSERT INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, retry_eligible, next_retry_at, created_at) VALUES (?, ?, ?, 'manual_retry', 'attempt_queued', ?, 1, ?, CURRENT_TIMESTAMP)")
            .bind(local_id::generate_local_id().to_string())
            .bind(&row.id)
            .bind(next_attempt_no)
            .bind(retry_status)
            .bind(&next_retry_at)
            .execute(&mut *tx)
            .await
            .map_err(ApiError::internal)?;
    }
    tx.commit().await.map_err(ApiError::internal)?;
    let mut body = public_response(&row, &request_id, projection);
    body["status"] = Value::String(retry_status.to_owned());
    body["result"]["status"] = Value::String(retry_status.to_owned());
    body["result"]["producer_ref"] = Value::String(producer_ref);
    Ok((StatusCode::ACCEPTED, body))
}

async fn claim_next(state: &AppState, manual_limit: i64) -> Result<Option<WorkRow>> {
    let (_lock, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "content_processing_claim")
        .await?;
    ensure_global_mode_in_transaction(&mut tx).await?;
    refresh_model_routes_in_transaction(state, &mut tx).await?;
    let attempt_snapshot = current_attempt_route_snapshot(state).await;
    let Some(row) = sqlx::query_as::<_, WorkRow>(
        "SELECT id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, batch_id, attempt_count, next_retry_at, retry_expires_at, retry_after_at, created_at FROM content_work_items WHERE status = 'queued' AND (next_retry_at IS NULL OR datetime(next_retry_at) <= datetime('now')) AND (retry_expires_at IS NULL OR datetime(retry_expires_at) > datetime('now')) AND ((priority < 3 AND datetime(created_at) <= datetime('now', '-60 seconds')) OR (priority >= 3 AND EXISTS (SELECT 1 FROM content_attempt_events pending WHERE pending.work_item_id = content_work_items.id AND pending.event_type = 'attempt_queued' AND pending.trigger = 'manual_retry' AND NOT EXISTS (SELECT 1 FROM content_attempt_events started WHERE started.work_item_id = pending.work_item_id AND started.attempt_no = pending.attempt_no AND started.event_type = 'attempt_started')) AND (SELECT COUNT(*) FROM content_batches WHERE status = 'running' AND trigger_reason = 'manual_retry') < ?)) ORDER BY priority DESC, datetime(created_at) ASC, id ASC LIMIT 1",
    )
    .bind(manual_limit)
    .fetch_optional(&mut *tx)
    .await?
    else {
        tx.commit().await?;
        return Ok(None);
    };
    if let Some(current) = newer_work_for_resource_in_transaction(&mut tx, &row).await? {
        sqlx::query("UPDATE content_work_items SET status = 'superseded', next_retry_at = NULL, retry_after_at = NULL, finished_at = COALESCE(finished_at, CURRENT_TIMESTAMP), updated_at = CURRENT_TIMESTAMP WHERE id = ? AND status = 'queued'")
            .bind(&row.id)
            .execute(&mut *tx)
            .await?;
        record_work_admission_event(
            &mut tx,
            WorkAdmissionEvent {
                work_item_id: &row.id,
                event_type: "reconciliation_superseded",
                replaced_by_work_item_id: Some(&current.id),
                source_hash: &row.source_hash,
                source_snapshot_json: &row.source_snapshot_json,
                producer_ref: "",
                requester_id: None,
                reason_code: "newer_source_exists_at_claim",
            },
        )
        .await?;
        tx.commit().await?;
        return Ok(None);
    }
    let Some(attempt_snapshot) = attempt_snapshot else {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE content_work_items SET status = 'blocked_config', failure_class = 'configuration', next_retry_at = NULL, retry_expires_at = NULL, retry_after_at = NULL, lease_owner = NULL, lease_expires_at = NULL, updated_at = ? WHERE id = ? AND status = 'queued'",
        )
        .bind(&now)
        .bind(&row.id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(None);
    };
    let batch_id = local_id::generate_local_id().to_string();
    let attempt_id = local_id::generate_local_id().to_string();
    let pending_attempt = sqlx::query_as::<_, (i64, String)>(
        "SELECT e.attempt_no, e.trigger FROM content_attempt_events e WHERE e.work_item_id = ? AND e.event_type = 'attempt_queued' AND NOT EXISTS (SELECT 1 FROM content_attempt_events started WHERE started.work_item_id = e.work_item_id AND started.attempt_no = e.attempt_no AND started.event_type = 'attempt_started') ORDER BY e.attempt_no DESC LIMIT 1",
    )
    .bind(&row.id)
    .fetch_optional(&mut *tx)
    .await?;
    let (next_attempt_no, trigger_reason) = pending_attempt
        .map(|(attempt_no, trigger)| (attempt_no.max(1), trigger))
        .unwrap_or_else(|| {
            (
                row.attempt_count.max(0).saturating_add(1),
                if row.priority >= 3 {
                    "manual_retry".to_owned()
                } else {
                    "initial".to_owned()
                },
            )
        });
    let now = Utc::now().to_rfc3339();
    let lease_expires_at =
        (Utc::now() + chrono::Duration::seconds(GLOBAL_WORK_LEASE_SECS)).to_rfc3339();
    let attempt_profile = attempt_snapshot
        .route_models
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("valid attempt route snapshot has no models"))?;
    sqlx::query("INSERT INTO content_batches (id, partition_key, target_lang, protocol_version, model_profile, trigger_reason, worker_id, worker_kind, request_count, item_count, estimated_input_tokens, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 'content-general-1', 'general', (SELECT COUNT(*) FROM content_request_links WHERE work_item_id = ?), 1, ?, 'running', ?, ?)")
        .bind(&batch_id)
        .bind(format!("{}:{}", row.target_lang, attempt_profile))
        .bind(&row.target_lang)
    .bind(&row.protocol_version)
    .bind(&attempt_profile)
    .bind(trigger_reason.as_str())
        .bind(&row.id)
        .bind(row.token_estimate)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO content_batch_items (id, batch_id, work_item_id, item_index, request_count, token_estimate, created_at, updated_at) VALUES (?, ?, ?, 0, (SELECT COUNT(*) FROM content_request_links WHERE work_item_id = ?), ?, ?, ?)")
        .bind(local_id::generate_local_id().to_string())
        .bind(&batch_id)
        .bind(&row.id)
        .bind(&row.id)
        .bind(row.token_estimate)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE content_work_items SET status = 'running', batch_id = ?, attempt_count = ?, started_at = ?, lease_owner = 'content-general-1', lease_expires_at = ?, updated_at = ? WHERE id = ?")
        .bind(&batch_id)
        .bind(next_attempt_no)
        .bind(&now)
        .bind(&lease_expires_at)
        .bind(&now)
        .bind(&row.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, configuration_snapshot_json, route_snapshot_json, configuration_fingerprint, created_at) VALUES (?, ?, ?, ?, 'attempt_started', ?, ?, ?, ?)")
        .bind(&attempt_id)
        .bind(&row.id)
        .bind(next_attempt_no)
        .bind(trigger_reason)
        .bind(&attempt_snapshot.configuration_snapshot_json)
        .bind(&attempt_snapshot.route_snapshot_json)
        .bind(&attempt_snapshot.configuration_fingerprint)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some(WorkRow {
        status: "running".to_owned(),
        batch_id: Some(batch_id),
        attempt_count: next_attempt_no,
        attempt_configuration_snapshot_json: Some(attempt_snapshot.configuration_snapshot_json),
        attempt_route_snapshot_json: Some(attempt_snapshot.route_snapshot_json),
        attempt_configuration_fingerprint: Some(attempt_snapshot.configuration_fingerprint),
        ..row
    }))
}

async fn renew_global_work_lease(state: &AppState, work: &WorkRow) -> Result<bool> {
    let now = Utc::now();
    let now_text = now.to_rfc3339();
    let lease_expires_at = (now + chrono::Duration::seconds(GLOBAL_WORK_LEASE_SECS)).to_rfc3339();
    let (_lock, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "content_processing_lease_heartbeat")
        .await?;
    let mode =
        sqlx::query_scalar::<_, String>("SELECT mode FROM content_processing_control WHERE id = 1")
            .fetch_optional(&mut *tx)
            .await?;
    if mode.as_deref() != Some(ContentProcessingMode::Global.as_str()) {
        tx.rollback().await?;
        return Ok(false);
    }
    let updated = sqlx::query("UPDATE content_work_items SET lease_expires_at = ?, updated_at = ? WHERE id = ? AND status = 'running' AND attempt_count = ? AND lease_owner = 'content-general-1' AND lease_expires_at IS NOT NULL AND julianday(lease_expires_at) > julianday(?)")
        .bind(&lease_expires_at)
        .bind(&now_text)
        .bind(&work.id)
        .bind(work.attempt_count)
        .bind(&now_text)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(updated.rows_affected() == 1)
}

async fn recover_due(state: &AppState) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let (_lock, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "content_processing_recover")
        .await?;
    ensure_global_mode_in_transaction(&mut tx).await?;
    supersede_stale_work_in_transaction(&mut tx).await?;
    sqlx::query(
        "INSERT OR IGNORE INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, retry_eligible, created_at) SELECT lower(hex(randomblob(16))), id, CASE WHEN attempt_count < 1 THEN 1 ELSE attempt_count + 1 END, 'automatic_recovery', 'attempt_queued', 'queued', 1, ? FROM content_work_items WHERE status IN ('failed', 'deferred_provider') AND next_retry_at IS NOT NULL AND datetime(next_retry_at) <= datetime(?) AND (retry_expires_at IS NULL OR datetime(retry_expires_at) > datetime(?))",
    )
    .bind(&now)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE content_work_items SET status = 'queued', priority = 0, next_retry_at = NULL, lease_owner = NULL, lease_expires_at = NULL, updated_at = ? WHERE status IN ('failed', 'deferred_provider') AND next_retry_at IS NOT NULL AND datetime(next_retry_at) <= datetime(?) AND (retry_expires_at IS NULL OR datetime(retry_expires_at) > datetime(?))",
    )
    .bind(&now)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE content_work_items SET status = 'failed', failure_class = 'provider_unavailable', next_retry_at = NULL, retry_expires_at = NULL, retry_after_at = NULL, updated_at = ? WHERE status = 'deferred_provider' AND retry_expires_at IS NOT NULL AND datetime(retry_expires_at) <= datetime(?)",
    )
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE content_attempt_events SET result_status = 'failed', error_code = 'provider_unavailable', error_summary = 'provider cooldown expired', failure_class = 'provider_unavailable', retry_eligible = 0, next_retry_at = NULL WHERE event_type = 'attempt_completed' AND result_status = 'deferred_provider' AND work_item_id IN (SELECT id FROM content_work_items WHERE status = 'failed' AND failure_class = 'provider_unavailable' AND next_retry_at IS NULL AND retry_expires_at IS NULL AND updated_at = ?)",
    )
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, error_code, failure_class, retry_eligible, created_at) SELECT lower(hex(randomblob(16))), w.id, COALESCE((SELECT MAX(pending.attempt_no) FROM content_attempt_events pending WHERE pending.work_item_id = w.id AND pending.event_type = 'attempt_queued'), (SELECT MAX(started.attempt_no) FROM content_attempt_events started WHERE started.work_item_id = w.id AND started.event_type = 'attempt_started'), CASE WHEN w.attempt_count < 1 THEN 1 ELSE w.attempt_count END), 'automatic_recovery', 'attempt_completed', 'failed', 'provider_unavailable', 'provider_unavailable', 0, ? FROM content_work_items w WHERE w.status = 'failed' AND w.failure_class = 'provider_unavailable' AND w.next_retry_at IS NULL AND w.retry_expires_at IS NULL AND w.updated_at = ? AND NOT EXISTS (SELECT 1 FROM content_attempt_events completed WHERE completed.work_item_id = w.id AND completed.event_type = 'attempt_completed')",
    )
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE content_batch_items SET result_status = 'failed', error_code = 'provider_unavailable', error_summary = 'provider cooldown expired', updated_at = ? WHERE result_status = 'deferred_provider' AND work_item_id IN (SELECT id FROM content_work_items WHERE status = 'failed' AND failure_class = 'provider_unavailable' AND next_retry_at IS NULL AND retry_expires_at IS NULL AND updated_at = ?)",
    )
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let expired_running = "status = 'running' AND lease_expires_at IS NOT NULL AND julianday(lease_expires_at) <= julianday(?)";
    sqlx::query(&format!(
        "UPDATE content_batch_items SET result_status = 'failed', error_code = 'runtime_lease_expired', updated_at = ? WHERE work_item_id IN (SELECT id FROM content_work_items WHERE {expired_running}) AND batch_id IN (SELECT batch_id FROM content_work_items WHERE {expired_running} AND batch_id IS NOT NULL)"
    ))
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(&format!(
        "UPDATE content_batches SET status = 'failed', error_code = 'runtime_lease_expired', error_summary = 'worker lease expired', finished_at = ?, updated_at = ? WHERE id IN (SELECT batch_id FROM content_work_items WHERE {expired_running} AND batch_id IS NOT NULL) AND status = 'running'"
    ))
    .bind(&now)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(&format!(
        "INSERT OR IGNORE INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, error_code, error_summary, failure_class, retry_eligible, created_at) SELECT lower(hex(randomblob(16))), id, attempt_count, 'automatic_recovery', 'attempt_completed', 'failed', 'runtime_lease_expired', 'worker lease expired', 'runtime_lease_expired', 1, ? FROM content_work_items WHERE {expired_running}"
    ))
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let recovery_retry_at = (Utc::now() + chrono::Duration::seconds(60)).to_rfc3339();
    let recovery_expires_at = (Utc::now() + chrono::Duration::hours(24)).to_rfc3339();
    sqlx::query(
        "UPDATE content_work_items SET status = 'queued', priority = 0, lease_owner = NULL, lease_expires_at = NULL, next_retry_at = ?, retry_expires_at = COALESCE(retry_expires_at, ?), retry_after_at = ?, updated_at = ? WHERE status = 'running' AND lease_expires_at IS NOT NULL AND julianday(lease_expires_at) <= julianday(?)",
    )
    .bind(&recovery_retry_at)
    .bind(&recovery_expires_at)
    .bind(&recovery_retry_at)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, retry_eligible, next_retry_at, created_at) SELECT lower(hex(randomblob(16))), id, attempt_count + 1, 'automatic_recovery', 'attempt_queued', 1, ?, ? FROM content_work_items WHERE status = 'queued' AND next_retry_at = ?",
    )
    .bind(&recovery_retry_at)
    .bind(&now)
    .bind(&recovery_retry_at)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn supersede_stale_work_in_transaction(tx: &mut Transaction<'_, Sqlite>) -> Result<u64> {
    let rows = sqlx::query_as::<_, WorkRow>(
        "SELECT id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, batch_id, attempt_count, next_retry_at, retry_expires_at, retry_after_at, created_at FROM content_work_items WHERE status IN ('queued', 'failed', 'deferred_provider', 'blocked_config', 'ready')",
    )
    .fetch_all(&mut **tx)
    .await?;
    let now = Utc::now().to_rfc3339();
    let mut changed = 0;
    for work in rows {
        let Some(current) = newer_work_for_resource_in_transaction(tx, &work).await? else {
            continue;
        };
        sqlx::query("UPDATE content_work_items SET status = 'superseded', next_retry_at = NULL, retry_after_at = NULL, finished_at = COALESCE(finished_at, ?), updated_at = ? WHERE id = ? AND status NOT IN ('cancelled', 'superseded', 'running')")
            .bind(&now)
            .bind(&now)
            .bind(&work.id)
            .execute(&mut **tx)
            .await?;
        record_work_admission_event(
            tx,
            WorkAdmissionEvent {
                work_item_id: &work.id,
                event_type: "reconciliation_superseded",
                replaced_by_work_item_id: Some(&current.id),
                source_hash: &work.source_hash,
                source_snapshot_json: &work.source_snapshot_json,
                producer_ref: "",
                requester_id: None,
                reason_code: "newer_source_exists",
            },
        )
        .await?;
        changed += 1;
    }
    Ok(changed)
}

async fn defer_queued_for_provider(state: &AppState) -> Result<()> {
    let routing = state
        .llm_scheduler
        .routing_status(state.config.ai.as_ref().map(|config| config.model.as_str()))
        .await;
    let retry_at = routing
        .model_statuses
        .iter()
        .filter_map(|model| model.cooldown_until.as_deref())
        .min()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            (Utc::now() + chrono::Duration::seconds(PROVIDER_DEFER_SECS)).to_rfc3339()
        });
    let (_lock, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "content_processing_defer_provider")
        .await?;
    ensure_global_mode_in_transaction(&mut tx).await?;
    sqlx::query("UPDATE content_work_items SET status = 'deferred_provider', priority = 0, next_retry_at = ?, retry_expires_at = COALESCE(retry_expires_at, ?), updated_at = CURRENT_TIMESTAMP WHERE status = 'queued'")
        .bind(&retry_at)
        .bind((Utc::now() + chrono::Duration::hours(24)).to_rfc3339())
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, retry_eligible, next_retry_at, created_at) SELECT lower(hex(randomblob(16))), id, CASE WHEN attempt_count < 1 THEN 1 ELSE attempt_count + 1 END, 'system_requeue', 'attempt_queued', 'deferred_provider', 1, ?, CURRENT_TIMESTAMP FROM content_work_items WHERE status = 'deferred_provider' AND next_retry_at = ?")
        .bind(&retry_at)
        .bind(&retry_at)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

fn build_prompt(snapshot: &SourceSnapshot, pipeline: &str) -> (String, String) {
    let system = if pipeline == "polishing" {
        "你是严谨的技术内容润色助手。只输出一个 JSON 对象，不要解释。JSON 顶层必须直接包含 target_slots 列出的字段；不要使用 output、result 或 data 包装，也不要输出 Markdown 代码围栏。保留事实、链接、代码和 Markdown 结构。"
    } else {
        "你是严谨的技术文档翻译助手。只输出一个 JSON 对象，不要解释。JSON 顶层必须直接包含 target_slots 列出的字段；不要使用 output、result 或 data 包装，也不要输出 Markdown 代码围栏。保留事实、链接、代码和 Markdown 结构。"
    };
    let user = json!({
        "source_blocks": snapshot
            .source_blocks
            .iter()
            .filter(|block| {
                block.slot != "source_observed_at" && block.slot != "source_revision_tiebreak"
            })
            .collect::<Vec<_>>(),
        "target_slots": snapshot.target_slots,
        "response_contract": "Return one JSON object with every declared target slot directly at the top level. Do not wrap it in output, result, or data. Do not use a Markdown code fence. Only declared target slots are persisted; extra scalar metadata is ignored."
    })
        .to_string();
    (system.to_owned(), user)
}

fn source_observed_at(raw_snapshot: &str) -> Option<DateTime<Utc>> {
    serde_json::from_str::<SourceSnapshot>(raw_snapshot)
        .ok()?
        .source_blocks
        .into_iter()
        .find(|block| block.slot == "source_observed_at")
        .and_then(|block| parse_storage_timestamp(&block.text))
}

fn source_revision_tiebreak(raw_snapshot: &str) -> Option<String> {
    serde_json::from_str::<SourceSnapshot>(raw_snapshot)
        .ok()?
        .source_blocks
        .into_iter()
        .find(|block| block.slot == "source_revision_tiebreak")
        .map(|block| block.text)
}

fn source_revision_json(item: &translations::TranslationRequestItemInput) -> Value {
    json!({
        "source_observed_at": item
            .source_blocks
            .iter()
            .find(|block| block.slot == "source_observed_at")
            .map(|block| block.text.clone()),
        "source_revision_tiebreak": item
            .source_blocks
            .iter()
            .find(|block| block.slot == "source_revision_tiebreak")
            .map(|block| block.text.clone()),
    })
}

fn source_revision_json_from_snapshot(raw_snapshot: &str) -> String {
    serde_json::to_string(&json!({
        "source_observed_at": source_observed_at(raw_snapshot).map(|value| value.to_rfc3339()),
        "source_revision_tiebreak": source_revision_tiebreak(raw_snapshot),
    }))
    .unwrap_or_else(|_| "{}".to_owned())
}

fn compare_source_revision_tiebreak(
    candidate: Option<String>,
    current: Option<String>,
) -> std::cmp::Ordering {
    match (candidate, current) {
        (Some(candidate), Some(current)) => {
            match (candidate.parse::<u128>(), current.parse::<u128>()) {
                (Ok(candidate), Ok(current)) => candidate.cmp(&current),
                _ => candidate.cmp(&current),
            }
        }
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn source_version_is_newer(candidate_snapshot: &str, current_snapshot: &str) -> bool {
    match (
        source_observed_at(candidate_snapshot),
        source_observed_at(current_snapshot),
    ) {
        (Some(candidate), Some(current)) if candidate != current => candidate > current,
        (Some(_), Some(_)) => match (
            source_revision_tiebreak(candidate_snapshot),
            source_revision_tiebreak(current_snapshot),
        ) {
            (Some(candidate), Some(current)) => {
                compare_source_revision_tiebreak(Some(candidate), Some(current)).is_gt()
            }
            _ => false,
        },
        (Some(_), None) => true,
        (None, Some(_)) => false,
        _ => false,
    }
}

struct WorkAdmissionEvent<'a> {
    work_item_id: &'a str,
    event_type: &'a str,
    replaced_by_work_item_id: Option<&'a str>,
    source_hash: &'a str,
    source_snapshot_json: &'a str,
    producer_ref: &'a str,
    requester_id: Option<&'a str>,
    reason_code: &'a str,
}

async fn record_work_admission_event(
    tx: &mut Transaction<'_, Sqlite>,
    event: WorkAdmissionEvent<'_>,
) -> Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO content_work_admission_events (id, work_item_id, event_type, replaced_by_work_item_id, source_hash, source_revision_json, producer_ref, requester_id, reason_code, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(local_id::generate_local_id().to_string())
    .bind(event.work_item_id)
    .bind(event.event_type)
    .bind(event.replaced_by_work_item_id)
    .bind(event.source_hash)
    .bind(source_revision_json_from_snapshot(event.source_snapshot_json))
    .bind(event.producer_ref)
    .bind(event.requester_id)
    .bind(event.reason_code)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn supersede_older_work_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    current: &WorkRow,
) -> Result<()> {
    let candidates = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT id, source_snapshot_json, source_hash, status FROM content_work_items WHERE id <> ? AND canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = ? AND protocol_version = ? AND source_hash <> ? AND status NOT IN ('cancelled', 'superseded')",
    )
    .bind(&current.id)
    .bind(&current.canonical_resource_type)
    .bind(&current.canonical_resource_id)
    .bind(&current.pipeline)
    .bind(&current.variant)
    .bind(&current.target_lang)
    .bind(&current.protocol_version)
    .bind(&current.source_hash)
    .fetch_all(&mut **tx)
    .await?;
    let now = Utc::now().to_rfc3339();
    for (id, snapshot, source_hash, status) in candidates {
        if !source_version_is_newer(&current.source_snapshot_json, &snapshot) {
            continue;
        }
        record_work_admission_event(
            tx,
            WorkAdmissionEvent {
                work_item_id: &id,
                event_type: "source_superseded",
                replaced_by_work_item_id: Some(&current.id),
                source_hash: &source_hash,
                source_snapshot_json: &snapshot,
                producer_ref: "",
                requester_id: None,
                reason_code: "newer_source_admitted",
            },
        )
        .await?;
        if status == "running" {
            continue;
        }
        sqlx::query("UPDATE content_work_items SET status = 'superseded', next_retry_at = NULL, retry_after_at = NULL, finished_at = COALESCE(finished_at, ?), updated_at = ? WHERE id = ? AND status NOT IN ('cancelled', 'superseded', 'running')")
            .bind(&now)
            .bind(&now)
            .bind(&id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

fn parse_storage_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|parsed| parsed.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f")
                .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S"))
                .ok()
                .map(|parsed| parsed.and_utc())
        })
}

fn next_retry_at_for_failure(
    attempt_count: i64,
    retry_expires_at: Option<&str>,
    retryable: bool,
    now: DateTime<Utc>,
) -> Option<String> {
    let expires_at = retry_expires_at.and_then(parse_storage_timestamp);
    if !retryable || expires_at.is_some_and(|expires_at| expires_at <= now) {
        return None;
    }
    let delay_index = attempt_count
        .saturating_sub(1)
        .min(i64::try_from(RETRY_DELAYS_SECS.len() - 1).unwrap_or(0))
        as usize;
    let candidate = now + chrono::Duration::seconds(RETRY_DELAYS_SECS[delay_index]);
    if expires_at.is_some_and(|expires_at| candidate >= expires_at) {
        return None;
    }
    Some(candidate.to_rfc3339())
}

fn strip_single_json_code_fence(raw: &str) -> Result<(String, bool)> {
    let trimmed = raw.trim();
    if !trimmed.starts_with("```") {
        return Ok((trimmed.to_owned(), false));
    }

    let mut lines = trimmed.lines();
    let opening = lines.next().unwrap_or_default().trim();
    if opening != "```" && !opening.eq_ignore_ascii_case("```json") {
        anyhow::bail!("global content output uses an unsupported code fence");
    }
    let mut body = lines.collect::<Vec<_>>();
    if body.pop().map(str::trim) != Some("```") {
        anyhow::bail!("global content output has an unterminated code fence");
    }
    Ok((body.join("\n").trim().to_owned(), true))
}

struct UniqueJsonValue(Value);

impl<'de> Deserialize<'de> for UniqueJsonValue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueJsonValueVisitor)
    }
}

struct UniqueJsonValueVisitor;

impl<'de> Visitor<'de> for UniqueJsonValueVisitor {
    type Value = UniqueJsonValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value with unique object keys")
    }

    fn visit_bool<E>(self, value: bool) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJsonValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJsonValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJsonValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> std::result::Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(|number| UniqueJsonValue(Value::Number(number)))
            .ok_or_else(|| E::custom("invalid JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJsonValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJsonValue(Value::String(value)))
    }

    fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJsonValue(Value::Null))
    }

    fn visit_seq<A>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<UniqueJsonValue>()? {
            values.push(value.0);
        }
        Ok(UniqueJsonValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = serde_json::Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(A::Error::custom(
                    "global content output has duplicate JSON keys",
                ));
            }
            let value = object.next_value::<UniqueJsonValue>()?;
            values.insert(key, value.0);
        }
        Ok(UniqueJsonValue(Value::Object(values)))
    }
}

fn normalize_output(raw: &str, target_slots: &[String]) -> Result<Value> {
    let (json_text, fenced) = strip_single_json_code_fence(raw)?;
    let output = serde_json::from_str::<UniqueJsonValue>(&json_text)
        .context("global content output is not JSON")?
        .0;
    let object = output
        .as_object()
        .ok_or_else(|| anyhow!("global content output is not an object"))?;
    let has_direct_target = target_slots.iter().any(|slot| object.contains_key(slot));
    let has_all_direct_targets = target_slots.iter().all(|slot| object.contains_key(slot));

    if object.iter().any(|(key, value)| {
        !target_slots.contains(key) && key != "output" && (value.is_object() || value.is_array())
    }) {
        anyhow::bail!("global content output has an unknown wrapper");
    }

    if object.contains_key("output") {
        if fenced {
            anyhow::bail!("global content output has multiple wrappers");
        }
        if has_direct_target {
            anyhow::bail!("global content output has ambiguous direct and nested targets");
        }
        let nested = object
            .get("output")
            .and_then(Value::as_object)
            .ok_or_else(|| anyhow!("global content output envelope is not an object"))?;
        if nested.contains_key("output") {
            anyhow::bail!("global content output has nested output envelopes");
        }
        if nested.iter().any(|(key, value)| {
            !target_slots.contains(key) && (value.is_object() || value.is_array())
        }) {
            anyhow::bail!("global content output envelope has an unknown wrapper");
        }
        if !target_slots.iter().all(|slot| nested.contains_key(slot)) {
            anyhow::bail!("global content output envelope is missing target slots");
        }
        return Ok(Value::Object(nested.clone()));
    }

    if !has_all_direct_targets {
        anyhow::bail!("global content output is missing target slots");
    }
    Ok(output)
}

fn validate_output(
    raw: &str,
    target_slots: &[String],
    source_blocks: &[translations::TranslationSourceBlock],
) -> Result<Value> {
    let output = normalize_output(raw, target_slots)?;
    let object = output
        .as_object()
        .ok_or_else(|| anyhow!("global content output is not an object"))?;
    for slot in target_slots {
        let Some(value) = object.get(slot) else {
            return Err(anyhow!(
                "global content output is missing target slot: {slot}"
            ));
        };
        let body_without_source = slot == "body_md"
            && !source_blocks
                .iter()
                .any(|block| block.slot == "body_markdown" && !block.text.trim().is_empty());
        if body_without_source {
            if !value.is_null() && value.as_str().is_none_or(|text| text.trim().is_empty()) {
                return Err(anyhow!(
                    "global content output target slot must be null or non-empty text: {slot}"
                ));
            }
        } else if value.as_str().is_none_or(|text| text.trim().is_empty()) {
            return Err(anyhow!(
                "global content output target slot is missing text: {slot}"
            ));
        }
        if slot == "body_md"
            && let Some(source) = source_blocks
                .iter()
                .find(|block| block.slot == "body_markdown")
                .map(|block| block.text.as_str())
        {
            let translated = value
                .as_str()
                .ok_or_else(|| anyhow!("global content output target slot is not text: {slot}"))?;
            if !crate::api::markdown_structure_preserved(source, translated) {
                return Err(anyhow!(
                    "global content output failed to preserve markdown structure: {slot}"
                ));
            }
        }
    }
    // Persist only declared content slots. The model is not allowed to inject
    // lifecycle, identity, or other control fields into a result projection.
    let projection = target_slots
        .iter()
        .filter_map(|slot| object.get(slot).map(|value| (slot.clone(), value.clone())))
        .collect();
    Ok(Value::Object(projection))
}

#[derive(Debug)]
struct OutputValidationFailure {
    call_ids: Vec<String>,
    code: &'static str,
    message: String,
}

impl std::fmt::Display for OutputValidationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for OutputValidationFailure {}

fn output_validation_call_ids(error: &anyhow::Error) -> Vec<String> {
    error
        .downcast_ref::<OutputValidationFailure>()
        .map(|failure| failure.call_ids.clone())
        .unwrap_or_default()
}

fn output_validation_error_code(error: &anyhow::Error) -> Option<&'static str> {
    error
        .downcast_ref::<OutputValidationFailure>()
        .map(|failure| failure.code)
}

#[derive(Debug)]
struct GlobalExecutionFailure {
    call_ids: Vec<String>,
    class: ai::LlmFailureClass,
    message: String,
}

impl std::fmt::Display for GlobalExecutionFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for GlobalExecutionFailure {}

fn global_execution_failure_class(error: &anyhow::Error) -> Option<ai::LlmFailureClass> {
    error
        .downcast_ref::<GlobalExecutionFailure>()
        .map(|failure| failure.class)
}

fn global_execution_call_ids(error: &anyhow::Error) -> Vec<String> {
    error
        .downcast_ref::<GlobalExecutionFailure>()
        .map(|failure| failure.call_ids.clone())
        .unwrap_or_default()
}

struct GlobalCompletion {
    diagnostic: ai::ChatCompletionDiagnostic,
    output: Value,
    call_ids: Vec<String>,
}

struct GlobalCallSpec<'a> {
    work: &'a WorkRow,
    ai_config: &'a crate::config::AiConfig,
    system: &'a str,
    user: &'a str,
    max_tokens: u32,
    route_snapshot: &'a [String],
    role: &'a str,
    call_ordinal_counter: Arc<AtomicI64>,
}

#[derive(Debug, Clone)]
struct LlmCallAudit {
    provider_request_id: Option<String>,
    model: String,
    status: String,
    duration_ms: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
}

async fn load_llm_call_audit(
    tx: &mut Transaction<'_, Sqlite>,
    call_id: &str,
) -> Result<Option<LlmCallAudit>> {
    sqlx::query_as::<_, (Option<String>, String, String, Option<i64>, Option<i64>, Option<i64>)>(
        "SELECT provider_request_id, COALESCE(final_model, model), status, duration_ms, input_tokens, output_tokens FROM llm_calls WHERE id = ? LIMIT 1",
    )
    .bind(call_id)
    .fetch_optional(&mut **tx)
    .await
    .map(|row| {
        row.map(
            |(provider_request_id, model, status, duration_ms, input_tokens, output_tokens)| {
                LlmCallAudit {
                    provider_request_id,
                    model,
                    status,
                    duration_ms,
                    input_tokens,
                    output_tokens,
                }
            },
        )
    })
    .map_err(Into::into)
}

fn unique_call_ids(mut call_ids: Vec<String>) -> Vec<String> {
    let mut unique = Vec::with_capacity(call_ids.len());
    call_ids.retain(|call_id| {
        if unique.iter().any(|existing| existing == call_id) {
            false
        } else {
            unique.push(call_id.clone());
            true
        }
    });
    call_ids
}

fn unique_provider_call_id(
    call_id: &str,
    provider_request_id: Option<&str>,
    used: &mut HashSet<String>,
) -> String {
    let base = provider_request_id
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("llm-call:{call_id}"));
    if used.insert(base.clone()) {
        base
    } else {
        let disambiguated = format!("{base}:{call_id}");
        used.insert(disambiguated.clone());
        disambiguated
    }
}

async fn request_global_completion(
    state: &AppState,
    spec: GlobalCallSpec<'_>,
) -> Result<ai::ChatCompletionDiagnostic> {
    let admission_state = Arc::new(state.clone());
    let admission_work = spec.work.clone();
    let admission_role = spec.role.to_owned();
    let admission_ordinals = spec.call_ordinal_counter.clone();
    let provider_admission: ai::ProviderAdmissionGuard = Arc::new(move |candidate_index| {
        let state = admission_state.clone();
        let work = admission_work.clone();
        let role = if candidate_index > 0 {
            "fallback".to_owned()
        } else {
            admission_role.clone()
        };
        let call_ordinal = admission_ordinals.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if !renew_global_work_lease(&state, &work).await? {
                return Ok(false);
            }
            admit_provider_call(&state, &work, call_ordinal, &role).await
        })
    });
    let call_context = ai::LlmCallContext {
        source: format!(
            "content_processing.global.{}.stage.content_output.role.{}",
            spec.work.pipeline, spec.role
        ),
        requested_by: None,
        parent_task_id: None,
        parent_task_type: None,
        parent_translation_batch_id: spec.work.batch_id.clone(),
        parent_brief_id: None,
    };
    tokio::time::timeout(
        Duration::from_secs(4 * 60),
        ai::with_llm_call_context(
            call_context,
            ai::chat_completion_with_diagnostics_for_config_and_route_with_admission(
                state,
                spec.ai_config,
                spec.system,
                spec.user,
                spec.max_tokens,
                Some(spec.route_snapshot),
                Some(provider_admission),
            ),
        ),
    )
    .await
    .map_err(|_| {
        anyhow::Error::new(ai::LlmCallFailure {
            class: ai::LlmFailureClass::Transient,
            call_id: None,
        })
    })
    .and_then(|result| result)
}

async fn admit_provider_call(
    state: &AppState,
    work: &WorkRow,
    call_ordinal: i64,
    relation_role: &str,
) -> Result<bool> {
    let (_lock, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "content_processing_provider_admission")
        .await?;
    if ensure_global_mode_in_transaction(&mut tx).await.is_err() {
        tx.rollback().await?;
        return Ok(false);
    }
    let now = Utc::now().to_rfc3339();
    let claim_is_current = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM content_work_items WHERE id = ? AND status = 'running' AND attempt_count = ? AND lease_owner = 'content-general-1' AND lease_expires_at IS NOT NULL AND julianday(lease_expires_at) > julianday(?)",
    )
    .bind(&work.id)
    .bind(work.attempt_count)
    .bind(&now)
    .fetch_one(&mut *tx)
    .await?;
    if claim_is_current == 0 {
        tx.rollback().await?;
        return Ok(false);
    }
    if !source_exists_in_transaction(&mut tx, work).await? {
        cancel_deleted_work_in_transaction(&mut tx, work).await?;
        tx.commit().await?;
        return Ok(false);
    }
    if !source_revision_is_current_in_transaction(&mut tx, work).await? {
        supersede_work_in_transaction(
            &mut tx,
            work,
            None,
            "source_revision_changed_before_provider",
        )
        .await?;
        tx.commit().await?;
        return Ok(false);
    }
    if supersede_replaced_work_in_transaction(&mut tx, work).await? {
        tx.commit().await?;
        return Ok(false);
    }
    let attempt_event_id = sqlx::query_scalar::<_, String>(
        "SELECT id FROM content_attempt_events WHERE work_item_id = ? AND attempt_no = ? AND event_type = 'attempt_started' LIMIT 1",
    )
    .bind(&work.id)
    .bind(work.attempt_count)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("INSERT OR IGNORE INTO content_attempt_provider_admissions (id, attempt_event_id, work_item_id, attempt_no, call_ordinal, relation_role, source_hash, source_revision_json, admitted_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(local_id::generate_local_id().to_string())
        .bind(&attempt_event_id)
        .bind(&work.id)
        .bind(work.attempt_count)
        .bind(call_ordinal)
        .bind(relation_role)
        .bind(&work.source_hash)
        .bind(source_revision_json_from_snapshot(&work.source_snapshot_json))
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

async fn complete_global_output(
    state: &AppState,
    work: &WorkRow,
    snapshot: &SourceSnapshot,
    system: &str,
    user: &str,
    route_snapshot: &[String],
) -> Result<GlobalCompletion> {
    let Some(ai_config) = state.config.ai.clone() else {
        return Err(anyhow::Error::new(GlobalExecutionFailure {
            call_ids: Vec::new(),
            class: ai::LlmFailureClass::Configuration,
            message: "AI configuration is missing".to_owned(),
        }));
    };
    let mut call_ids = Vec::new();
    let call_ordinal_counter = Arc::new(AtomicI64::new(0));
    let mut diagnostic = match request_global_completion(
        state,
        GlobalCallSpec {
            work,
            ai_config: &ai_config,
            system,
            user,
            max_tokens: GLOBAL_MAX_TOKENS,
            route_snapshot,
            role: "primary",
            call_ordinal_counter: call_ordinal_counter.clone(),
        },
    )
    .await
    {
        Ok(diagnostic) => diagnostic,
        Err(error) => {
            if let Some(call_id) = ai::llm_call_id(&error) {
                call_ids.push(call_id);
            }
            let class = ai::llm_failure_class(&error).unwrap_or(ai::LlmFailureClass::Transient);
            return Err(anyhow::Error::new(GlobalExecutionFailure {
                call_ids,
                class,
                message: error.to_string(),
            }));
        }
    };
    if let Some(call_id) = diagnostic.call_id.clone() {
        call_ids.push(call_id);
    }

    if diagnostic.finish_reason.as_deref() == Some("length") {
        let recovery_route_snapshot = [diagnostic.model.clone()];
        diagnostic = match request_global_completion(
            state,
            GlobalCallSpec {
                work,
                ai_config: &ai_config,
                system,
                user,
                max_tokens: GLOBAL_LENGTH_RECOVERY_MAX_TOKENS,
                route_snapshot: &recovery_route_snapshot,
                role: "length_recovery",
                call_ordinal_counter: call_ordinal_counter.clone(),
            },
        )
        .await
        {
            Ok(diagnostic) => diagnostic,
            Err(error) => {
                if let Some(call_id) = ai::llm_call_id(&error) {
                    call_ids.push(call_id);
                }
                let class = ai::llm_failure_class(&error).unwrap_or(ai::LlmFailureClass::Transient);
                return Err(anyhow::Error::new(GlobalExecutionFailure {
                    call_ids,
                    class,
                    message: error.to_string(),
                }));
            }
        };
        if let Some(call_id) = diagnostic.call_id.clone() {
            call_ids.push(call_id);
        }
    }

    if diagnostic.finish_reason.as_deref() == Some("length") {
        return Err(anyhow::Error::new(OutputValidationFailure {
            call_ids,
            code: "output_truncated",
            message: "global content output was truncated".to_owned(),
        }));
    }

    let output = match validate_output(
        &diagnostic.content,
        &snapshot.target_slots,
        &snapshot.source_blocks,
    ) {
        Ok(output) => output,
        Err(error) => {
            return Err(anyhow::Error::new(OutputValidationFailure {
                call_ids,
                code: "output_contract_invalid",
                message: error.to_string(),
            }));
        }
    };

    Ok(GlobalCompletion {
        diagnostic,
        output,
        call_ids,
    })
}

async fn source_exists(state: &AppState, work: &WorkRow) -> Result<bool> {
    let count = match work.canonical_resource_type.as_str() {
        "release" => {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM repo_releases WHERE release_id = ?")
                .bind(&work.canonical_resource_id)
                .fetch_one(&state.pool)
                .await?
        }
        "notification" => {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM notifications WHERE thread_id = ?")
                .bind(&work.canonical_resource_id)
                .fetch_one(&state.pool)
                .await?
        }
        "announcement" => {
            let Some((repo, number)) = work.canonical_resource_id.rsplit_once('#') else {
                return Ok(false);
            };
            let number = number.parse::<i64>().unwrap_or_default();
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM social_activity_events WHERE kind = 'announcement' AND lower(repo_full_name) = lower(?) AND discussion_number = ?",
            )
            .bind(repo)
            .bind(number)
            .fetch_one(&state.pool)
            .await?
        }
        _ => 0,
    };
    Ok(count > 0)
}

async fn source_exists_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    work: &WorkRow,
) -> Result<bool> {
    let count = match work.canonical_resource_type.as_str() {
        "release" => {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM repo_releases WHERE release_id = ?")
                .bind(&work.canonical_resource_id)
                .fetch_one(&mut **tx)
                .await?
        }
        "notification" => {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM notifications WHERE thread_id = ?")
                .bind(&work.canonical_resource_id)
                .fetch_one(&mut **tx)
                .await?
        }
        "announcement" => {
            let Some((repo, number)) = work.canonical_resource_id.rsplit_once('#') else {
                return Ok(false);
            };
            let number = number.parse::<i64>().unwrap_or_default();
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM social_activity_events WHERE kind = 'announcement' AND lower(repo_full_name) = lower(?) AND discussion_number = ?",
            )
            .bind(repo)
            .bind(number)
            .fetch_one(&mut **tx)
            .await?
        }
        _ => 0,
    };
    Ok(count > 0)
}

fn source_revision_snapshot_json(observed_at: Option<String>, tiebreak: String) -> String {
    let mut blocks = Vec::new();
    if let Some(observed_at) = observed_at {
        blocks.push(json!({"slot": "source_observed_at", "text": observed_at}));
        blocks.push(json!({"slot": "source_revision_tiebreak", "text": tiebreak}));
    }
    json!({"source_blocks": blocks}).to_string()
}

pub(crate) fn notification_source_revision_tiebreak(
    thread_id: &str,
    repo_full_name: Option<&str>,
    subject_title: Option<&str>,
    reason: Option<&str>,
    subject_type: Option<&str>,
) -> String {
    format!(
        "thread={thread_id}\nrepo={}\ntitle={}\nreason={}\nsubject_type={}",
        repo_full_name.unwrap_or_default(),
        subject_title.unwrap_or_default(),
        reason.unwrap_or_default(),
        subject_type.unwrap_or_default(),
    )
}

pub(crate) fn source_revision_content_tiebreak(parts: &[&str]) -> String {
    parts.join("\n")
}

async fn current_source_revision_snapshot_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    work: &WorkRow,
) -> Result<Option<String>> {
    let revision = match work.canonical_resource_type.as_str() {
        "release" => sqlx::query_as::<_, (
            String,
            String,
            i64,
            String,
            Option<String>,
            Option<String>,
        )>(
            "SELECT updated_at, html_url, repo_id, tag_name, name, body FROM repo_releases WHERE release_id = ? LIMIT 1",
        )
        .bind(&work.canonical_resource_id)
        .fetch_optional(&mut **tx)
        .await?
        .map(|(updated_at, html_url, repo_id, tag_name, name, body)| {
            let repo_full_name = parse_repo_full_name_from_release_url(&html_url)
                .unwrap_or_else(|| format!("unknown/{repo_id}"));
            let title = name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(tag_name.as_str());
            let body = body.unwrap_or_default().replace("\r\n", "\n");
            (
                if updated_at.trim().is_empty() {
                    None
                } else {
                    Some(updated_at)
                },
                source_revision_content_tiebreak(&[
                    repo_full_name.as_str(),
                    tag_name.as_str(),
                    title,
                    body.as_str(),
                ]),
            )
        }),
        "notification" => sqlx::query_as::<_, (
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )>(
            "SELECT updated_at, thread_id, repo_full_name, subject_title, reason, subject_type FROM notifications WHERE thread_id = ? ORDER BY updated_at DESC, COALESCE(repo_full_name, '') DESC, COALESCE(subject_title, '') DESC, COALESCE(reason, '') DESC, COALESCE(subject_type, '') DESC, thread_id DESC LIMIT 1",
        )
        .bind(&work.canonical_resource_id)
        .fetch_optional(&mut **tx)
        .await?
        .map(|(updated_at, thread_id, repo, title, reason, subject_type)| {
            (
                updated_at,
                notification_source_revision_tiebreak(
                    &thread_id,
                    repo.as_deref(),
                    title.as_deref(),
                    reason.as_deref(),
                    subject_type.as_deref(),
                ),
            )
        }),
        "announcement" => {
            let Some((repo, number)) = work.canonical_resource_id.rsplit_once('#') else {
                return Ok(None);
            };
            let number = number.parse::<i64>().unwrap_or_default();
            sqlx::query_as::<_, (Option<String>, String, i64, Option<String>, Option<String>)>(
                "SELECT occurred_at, lower(repo_full_name), discussion_number, title, body FROM social_activity_events WHERE kind = 'announcement' AND lower(repo_full_name) = lower(?) AND discussion_number = ? ORDER BY occurred_at DESC, COALESCE(title, '') DESC, COALESCE(body, '') DESC, id DESC LIMIT 1",
            )
            .bind(repo)
            .bind(number)
            .fetch_optional(&mut **tx)
            .await?
            .map(|(occurred_at, repo_full_name, discussion_number, title, body)| {
                let discussion_key = format!("{repo_full_name}#{discussion_number}");
                let title = title.unwrap_or_else(|| format!("Discussion #{discussion_number}"));
                let body = body.unwrap_or_default();
                (
                    occurred_at,
                    source_revision_content_tiebreak(&[
                        discussion_key.as_str(),
                        title.as_str(),
                        body.as_str(),
                    ]),
                )
            })
        }
        _ => None,
    };
    let Some((observed_at, tiebreak)) = revision else {
        return Ok(None);
    };
    let Some(observed_at) = observed_at else {
        return Ok(None);
    };
    Ok(Some(source_revision_snapshot_json(
        Some(observed_at),
        tiebreak,
    )))
}

async fn source_revision_is_current_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    work: &WorkRow,
) -> Result<bool> {
    if source_observed_at(&work.source_snapshot_json).is_none() {
        return Ok(true);
    }
    let Some(current_snapshot) = current_source_revision_snapshot_in_transaction(tx, work).await?
    else {
        return Ok(false);
    };
    Ok(!source_version_is_newer(
        &current_snapshot,
        &work.source_snapshot_json,
    ))
}

async fn supersede_work_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    work: &WorkRow,
    replaced_by_work_item_id: Option<&str>,
    reason_code: &str,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE content_work_items SET status = 'superseded', finished_at = ?, lease_owner = NULL, lease_expires_at = NULL, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&now)
        .bind(&work.id)
        .execute(&mut **tx)
        .await?;
    record_work_admission_event(
        tx,
        WorkAdmissionEvent {
            work_item_id: &work.id,
            event_type: "source_superseded",
            replaced_by_work_item_id,
            source_hash: &work.source_hash,
            source_snapshot_json: &work.source_snapshot_json,
            producer_ref: "",
            requester_id: None,
            reason_code,
        },
    )
    .await?;
    sqlx::query("INSERT OR IGNORE INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, retry_eligible, created_at) SELECT ?, work_item_id, attempt_no, trigger, 'attempt_completed', 'superseded', 0, ? FROM content_attempt_events WHERE work_item_id = ? AND attempt_no = ? AND event_type = 'attempt_started'")
        .bind(local_id::generate_local_id().to_string())
        .bind(&now)
        .bind(&work.id)
        .bind(work.attempt_count)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE content_batch_items SET result_status = 'superseded', updated_at = ? WHERE work_item_id = ? AND batch_id = ?")
        .bind(&now)
        .bind(&work.id)
        .bind(work.batch_id.as_deref().unwrap_or_default())
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE content_batches SET status = 'completed', finished_at = ?, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&now)
        .bind(work.batch_id.as_deref().unwrap_or_default())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn supersede_replaced_work_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    work: &WorkRow,
) -> Result<bool> {
    let candidates = sqlx::query_as::<_, (String, String)>(
        "SELECT id, source_snapshot_json FROM content_work_items WHERE id <> ? AND canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = ? AND protocol_version = ? AND status NOT IN ('cancelled', 'superseded')",
    )
    .bind(&work.id)
    .bind(&work.canonical_resource_type)
    .bind(&work.canonical_resource_id)
    .bind(&work.pipeline)
    .bind(&work.variant)
    .bind(&work.target_lang)
    .bind(&work.protocol_version)
    .fetch_all(&mut **tx)
    .await?;
    let Some((replaced_by_work_item_id, _)) = candidates
        .into_iter()
        .filter(|(_, snapshot)| source_version_is_newer(snapshot, &work.source_snapshot_json))
        .reduce(|current, candidate| {
            if source_version_is_newer(&candidate.1, &current.1) {
                candidate
            } else {
                current
            }
        })
    else {
        return Ok(false);
    };
    supersede_work_in_transaction(
        tx,
        work,
        Some(&replaced_by_work_item_id),
        "newer_source_detected_before_provider",
    )
    .await?;
    Ok(true)
}

async fn cancel_deleted_work(state: &AppState, work: &WorkRow) -> Result<()> {
    let (_lock, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "content_processing_cancel_deleted")
        .await?;
    ensure_global_mode_in_transaction(&mut tx)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;
    cancel_deleted_work_in_transaction(&mut tx, work).await?;
    tx.commit().await.map_err(Into::into)
}

async fn cancel_deleted_work_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    work: &WorkRow,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let updated = sqlx::query("UPDATE content_work_items SET status = 'cancelled', cancelled_at = ?, finished_at = ?, lease_owner = NULL, lease_expires_at = NULL, updated_at = ? WHERE id = ? AND status = 'running' AND attempt_count = ? AND lease_owner = 'content-general-1' AND lease_expires_at IS NOT NULL AND julianday(lease_expires_at) > julianday(?)")
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .bind(&work.id)
        .bind(work.attempt_count)
        .bind(&now)
        .execute(&mut **tx)
        .await?;
    if updated.rows_affected() == 0 {
        return Ok(());
    }
    sqlx::query("INSERT OR IGNORE INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, retry_eligible, created_at) SELECT ?, work_item_id, attempt_no, trigger, 'attempt_completed', 'cancelled', 0, ? FROM content_attempt_events WHERE work_item_id = ? AND attempt_no = ? AND event_type = 'attempt_started'")
        .bind(local_id::generate_local_id().to_string())
        .bind(&now)
        .bind(&work.id)
        .bind(work.attempt_count)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE content_batch_items SET result_status = 'cancelled', updated_at = ? WHERE work_item_id = ? AND batch_id = ?")
        .bind(&now)
        .bind(&work.id)
        .bind(work.batch_id.as_deref().unwrap_or_default())
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE content_batches SET status = 'completed', finished_at = ?, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&now)
        .bind(work.batch_id.as_deref().unwrap_or_default())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

struct AttemptLlmCallAudit<'a> {
    audit_call_id: &'a str,
    attempt_event_id: &'a str,
    provider_call_id: &'a str,
    model: &'a str,
    status: &'a str,
    duration_ms: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    error_code: &'a str,
    error_summary: Option<&'a str>,
    created_at: &'a str,
}

async fn persist_attempt_llm_call_audit(
    tx: &mut Transaction<'_, Sqlite>,
    audit: AttemptLlmCallAudit<'_>,
) -> std::result::Result<(), SqlxError> {
    sqlx::query("INSERT INTO content_attempt_llm_calls (id, attempt_event_id, provider_call_id, model, status, duration_ms, input_tokens, output_tokens, cost_microunits, error_code, error_summary, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(audit.audit_call_id)
        .bind(audit.attempt_event_id)
        .bind(audit.provider_call_id)
        .bind(audit.model)
        .bind(audit.status)
        .bind(audit.duration_ms)
        .bind(audit.input_tokens)
        .bind(audit.output_tokens)
        .bind(Option::<i64>::None)
        .bind(audit.error_code)
        .bind(audit.error_summary)
        .bind(audit.created_at)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn persist_superseded_provider_call_audits(
    tx: &mut Transaction<'_, Sqlite>,
    work: &WorkRow,
    result: &Result<GlobalCompletion>,
    now: &str,
) -> Result<()> {
    let attempt_event_id = sqlx::query_scalar::<_, String>(
        "SELECT id FROM content_attempt_events WHERE work_item_id = ? AND attempt_no = ? AND event_type = 'attempt_started' LIMIT 1",
    )
    .bind(&work.id)
    .bind(work.attempt_count)
    .fetch_one(&mut **tx)
    .await?;
    let linked_call_ids = match result {
        Ok(completion) => unique_call_ids(
            completion
                .call_ids
                .iter()
                .cloned()
                .chain(completion.diagnostic.call_id.clone())
                .collect(),
        ),
        Err(error) => unique_call_ids(
            output_validation_call_ids(error)
                .into_iter()
                .chain(global_execution_call_ids(error))
                .chain(ai::llm_call_id(error))
                .collect(),
        ),
    };
    let mut used_provider_call_ids = HashSet::new();
    for call_id in linked_call_ids {
        let audit = load_llm_call_audit(tx, &call_id)
            .await?
            .unwrap_or_else(|| LlmCallAudit {
                provider_request_id: None,
                model: "unknown".to_owned(),
                status: "succeeded".to_owned(),
                duration_ms: None,
                input_tokens: None,
                output_tokens: None,
            });
        let provider_call_id = unique_provider_call_id(
            &call_id,
            audit.provider_request_id.as_deref(),
            &mut used_provider_call_ids,
        );
        persist_attempt_llm_call_audit(
            tx,
            AttemptLlmCallAudit {
                audit_call_id: &call_id,
                attempt_event_id: &attempt_event_id,
                provider_call_id: &provider_call_id,
                model: &audit.model,
                status: &audit.status,
                duration_ms: audit.duration_ms,
                input_tokens: audit.input_tokens,
                output_tokens: audit.output_tokens,
                error_code: "superseded",
                error_summary: Some("source superseded after provider admission"),
                created_at: now,
            },
        )
        .await?;
    }
    Ok(())
}

async fn execute(state: &AppState, work: WorkRow) -> Result<()> {
    if !source_exists(state, &work).await? {
        cancel_deleted_work(state, &work).await?;
        return Ok(());
    }
    let snapshot = serde_json::from_str::<SourceSnapshot>(&work.source_snapshot_json)
        .context("invalid global source snapshot")?;
    let _configuration_snapshot = serde_json::from_str::<Value>(
        work.attempt_configuration_snapshot_json
            .as_deref()
            .ok_or_else(|| anyhow!("attempt configuration snapshot is missing"))?,
    )
    .context("invalid attempt configuration snapshot")?;
    let attempt_configuration_fingerprint = work
        .attempt_configuration_fingerprint
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("attempt configuration fingerprint is missing"))?;
    let route_snapshot = serde_json::from_str::<Vec<String>>(
        work.attempt_route_snapshot_json
            .as_deref()
            .ok_or_else(|| anyhow!("attempt route snapshot is missing"))?,
    )
    .context("invalid attempt route snapshot")?;
    let _ = attempt_configuration_fingerprint;
    if let Some(next_retry_at) = state
        .llm_scheduler
        .all_routes_cooldown_until(&route_snapshot)
        .await
    {
        let now = Utc::now();
        let retry_expires_at = (now + chrono::Duration::hours(24)).to_rfc3339();
        let (_lock, mut tx) = state
            .sqlite_writer
            .begin_immediate(&state.pool, "content_processing_defer_model")
            .await?;
        ensure_global_mode_in_transaction(&mut tx)
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        let updated = sqlx::query("UPDATE content_work_items SET status = 'deferred_provider', next_retry_at = ?, retry_expires_at = COALESCE(retry_expires_at, ?), retry_after_at = ?, lease_owner = NULL, lease_expires_at = NULL, updated_at = ? WHERE id = ? AND status = 'running' AND attempt_count = ?")
                .bind(&next_retry_at)
                .bind(&retry_expires_at)
                .bind(&next_retry_at)
                .bind(now.to_rfc3339())
                .bind(&work.id)
                .bind(work.attempt_count)
                .execute(&mut *tx)
                .await?;
        if updated.rows_affected() == 0 {
            tx.commit().await?;
            return Ok(());
        }
        sqlx::query("INSERT OR IGNORE INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, error_code, error_summary, retry_eligible, next_retry_at, created_at) SELECT ?, work_item_id, attempt_no, trigger, 'attempt_completed', 'deferred_provider', 'provider_cooldown', 'model profile is cooling down', 1, ?, ? FROM content_attempt_events WHERE work_item_id = ? AND attempt_no = ? AND event_type = 'attempt_started'")
                .bind(local_id::generate_local_id().to_string())
                .bind(&next_retry_at)
                .bind(now.to_rfc3339())
                .bind(&work.id)
                .bind(work.attempt_count)
                .execute(&mut *tx)
                .await?;
        sqlx::query("UPDATE content_batch_items SET result_status = 'deferred_provider', error_code = 'provider_cooldown', error_summary = 'model profile is cooling down', updated_at = ? WHERE work_item_id = ? AND batch_id = ?")
                .bind(now.to_rfc3339())
                .bind(&work.id)
                .bind(work.batch_id.as_deref().unwrap_or_default())
                .execute(&mut *tx)
                .await?;
        sqlx::query("UPDATE content_batches SET status = 'completed', finished_at = ?, updated_at = ?, error_code = 'provider_cooldown', error_summary = 'model profile is cooling down' WHERE id = ? AND status = 'running'")
                .bind(now.to_rfc3339())
                .bind(now.to_rfc3339())
                .bind(work.batch_id.as_deref().unwrap_or_default())
                .execute(&mut *tx)
                .await?;
        tx.commit().await?;
        return Ok(());
    }
    let (system, user) = build_prompt(&snapshot, &work.pipeline);
    let result =
        complete_global_output(state, &work, &snapshot, &system, &user, &route_snapshot).await;
    let now = Utc::now().to_rfc3339();
    let (_lock, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "content_processing_execute")
        .await?;
    let mode =
        sqlx::query_scalar::<_, String>("SELECT mode FROM content_processing_control WHERE id = 1")
            .fetch_optional(&mut *tx)
            .await?;
    if mode.as_deref() != Some(ContentProcessingMode::Global.as_str()) {
        tx.rollback().await?;
        return Ok(());
    }
    let now_for_lease = Utc::now().to_rfc3339();
    let claim_is_current = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM content_work_items WHERE id = ? AND status = 'running' AND attempt_count = ? AND lease_owner = 'content-general-1' AND lease_expires_at IS NOT NULL AND julianday(lease_expires_at) > julianday(?)",
    )
    .bind(&work.id)
    .bind(work.attempt_count)
    .bind(&now_for_lease)
    .fetch_one(&mut *tx)
    .await?;
    if claim_is_current == 0 {
        persist_superseded_provider_call_audits(&mut tx, &work, &result, &now).await?;
        tx.commit().await?;
        return Ok(());
    }
    if !source_exists_in_transaction(&mut tx, &work).await? {
        cancel_deleted_work_in_transaction(&mut tx, &work).await?;
        tx.commit().await?;
        return Ok(());
    }
    if !source_revision_is_current_in_transaction(&mut tx, &work).await? {
        supersede_work_in_transaction(
            &mut tx,
            &work,
            None,
            "source_revision_changed_before_publication",
        )
        .await?;
        persist_superseded_provider_call_audits(&mut tx, &work, &result, &now).await?;
        tx.commit().await?;
        return Ok(());
    }
    if supersede_replaced_work_in_transaction(&mut tx, &work).await? {
        persist_superseded_provider_call_audits(&mut tx, &work, &result, &now).await?;
        tx.commit().await?;
        return Ok(());
    }
    match result {
        Ok(completion) => {
            let diagnostic = completion.diagnostic;
            let output = completion.output;
            let attempt_event_id = sqlx::query_scalar::<_, String>("SELECT id FROM content_attempt_events WHERE work_item_id = ? AND attempt_no = ? AND event_type = 'attempt_started' LIMIT 1")
                .bind(&work.id)
                .bind(work.attempt_count)
                .fetch_one(&mut *tx)
                .await?;
            let call_ids = if completion.call_ids.is_empty() {
                diagnostic
                    .call_id
                    .clone()
                    .map(|call_id| vec![call_id])
                    .unwrap_or_default()
            } else {
                completion.call_ids
            };
            let mut call_audits = Vec::with_capacity(call_ids.len());
            for call_id in &call_ids {
                call_audits.push((
                    call_id.clone(),
                    load_llm_call_audit(&mut tx, call_id).await?,
                ));
            }
            let final_audit = call_audits
                .last()
                .and_then(|(_, audit)| audit.as_ref())
                .cloned();
            let provider_call_id = final_audit
                .as_ref()
                .and_then(|audit| audit.provider_request_id.as_deref())
                .or(diagnostic.provider_request_id.as_deref())
                .unwrap_or("unknown");
            let model = final_audit
                .as_ref()
                .map_or(diagnostic.model.as_str(), |audit| audit.model.as_str());
            let duration_ms = call_audits
                .iter()
                .filter_map(|(_, audit)| audit.as_ref().and_then(|audit| audit.duration_ms))
                .reduce(i64::saturating_add);
            let input_tokens = call_audits
                .iter()
                .filter_map(|(_, audit)| audit.as_ref().and_then(|audit| audit.input_tokens))
                .reduce(i64::saturating_add);
            let output_tokens = call_audits
                .iter()
                .filter_map(|(_, audit)| audit.as_ref().and_then(|audit| audit.output_tokens))
                .reduce(i64::saturating_add)
                .or(diagnostic.output_tokens);
            // Provider pricing is not part of the existing llm_calls contract; persist an explicit
            // unknown cost instead of deriving a value from model names or token counts.
            if call_audits.is_empty() {
                sqlx::query("INSERT INTO content_attempt_llm_calls (id, attempt_event_id, provider_call_id, model, status, duration_ms, input_tokens, output_tokens, cost_microunits, created_at) VALUES (?, ?, ?, ?, 'succeeded', ?, ?, ?, ?, ?)")
                    .bind(local_id::generate_local_id().to_string())
                    .bind(&attempt_event_id)
                    .bind(provider_call_id)
                    .bind(model)
                    .bind(duration_ms)
                    .bind(input_tokens)
                    .bind(output_tokens)
                    .bind(Option::<i64>::None)
                    .bind(&now)
                    .execute(&mut *tx)
                    .await?;
            } else {
                let mut used_provider_call_ids = HashSet::new();
                for (call_id, audit) in &call_audits {
                    let audit = audit.clone().unwrap_or_else(|| LlmCallAudit {
                        provider_request_id: None,
                        model: model.to_owned(),
                        status: "succeeded".to_owned(),
                        duration_ms: None,
                        input_tokens: None,
                        output_tokens: None,
                    });
                    let provider_call_id = unique_provider_call_id(
                        call_id,
                        audit.provider_request_id.as_deref(),
                        &mut used_provider_call_ids,
                    );
                    sqlx::query("INSERT INTO content_attempt_llm_calls (id, attempt_event_id, provider_call_id, model, status, duration_ms, input_tokens, output_tokens, cost_microunits, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                        .bind(call_id)
                        .bind(&attempt_event_id)
                        .bind(provider_call_id)
                        .bind(audit.model)
                        .bind(audit.status)
                        .bind(audit.duration_ms)
                        .bind(audit.input_tokens)
                        .bind(audit.output_tokens)
                        .bind(Option::<i64>::None)
                        .bind(&now)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            let token_count = match (input_tokens, output_tokens) {
                (Some(input), Some(output)) => Some(input.saturating_add(output)),
                _ => None,
            };
            sqlx::query("INSERT OR IGNORE INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, retry_eligible, duration_ms, token_count, cost_microunits, created_at) SELECT ?, work_item_id, attempt_no, trigger, 'attempt_completed', 'ready', 0, ?, ?, ?, ? FROM content_attempt_events WHERE id = ?")
                .bind(local_id::generate_local_id().to_string())
                .bind(duration_ms)
                .bind(token_count)
                .bind(Option::<i64>::None)
                .bind(&now)
                .bind(&attempt_event_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE content_work_items SET status = 'ready', finished_at = ?, lease_owner = NULL, lease_expires_at = NULL, updated_at = ?, retry_after_at = NULL WHERE id = ?")
                .bind(&now)
                .bind(&now)
                .bind(&work.id)
                .execute(&mut *tx)
                .await?;
            let projection_id = sqlx::query_scalar::<_, String>("INSERT INTO content_result_projections (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash, work_item_id, active_work_item_id, payload_json, published_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash) DO UPDATE SET work_item_id = excluded.work_item_id, active_work_item_id = excluded.active_work_item_id, payload_json = excluded.payload_json, published_at = excluded.published_at, updated_at = excluded.updated_at RETURNING id")
                .bind(local_id::generate_local_id().to_string())
                .bind(&work.canonical_resource_type)
                .bind(&work.canonical_resource_id)
                .bind(&work.pipeline)
                .bind(&work.variant)
                .bind(&work.target_lang)
                .bind(&work.protocol_version)
                .bind(model)
                .bind(&work.source_hash)
                .bind(&work.id)
                .bind(&work.id)
                .bind(output.to_string())
                .bind(&now)
                .bind(&now)
                .fetch_one(&mut *tx)
                .await?;
            let identity_id =
                content_identity_upgrade::ensure_identity_for_work(&mut tx, &work.id, &now).await?;
            sqlx::query("INSERT INTO content_current_result_projections (identity_id, work_item_id, active_work_item_id, source_projection_id, payload_json, published_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(identity_id) DO UPDATE SET work_item_id = excluded.work_item_id, active_work_item_id = excluded.active_work_item_id, source_projection_id = excluded.source_projection_id, payload_json = excluded.payload_json, published_at = excluded.published_at, updated_at = excluded.updated_at")
                .bind(identity_id)
                .bind(&work.id)
                .bind(&work.id)
                .bind(projection_id)
                .bind(output.to_string())
                .bind(&now)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE content_batch_items SET result_status = 'ready', updated_at = ? WHERE work_item_id = ? AND batch_id = ?")
                .bind(&now)
                .bind(&work.id)
                .bind(work.batch_id.as_deref().unwrap_or_default())
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE content_batches SET status = 'completed', finished_at = ?, updated_at = ? WHERE id = ?")
                .bind(&now)
                .bind(&now)
                .bind(work.batch_id.as_deref().unwrap_or_default())
                .execute(&mut *tx)
                .await?;
        }
        Err(error) => {
            let error_text = error.to_string();
            let llm_class =
                ai::llm_failure_class(&error).or_else(|| global_execution_failure_class(&error));
            let class = output_validation_error_code(&error)
                .map(str::to_owned)
                .or_else(|| {
                    llm_class
                        .map(|value| value.as_str().to_owned())
                        .or_else(|| {
                            translations::classify_translation_error(Some(error_text.as_str()))
                                .map(|value| value.code.to_owned())
                        })
                })
                .unwrap_or_else(|| "unknown_internal_error".to_owned());
            let retryable = llm_class.is_some_and(ai::LlmFailureClass::is_recoverable)
                || matches!(
                    class.as_str(),
                    "output_contract_invalid" | "output_truncated"
                );
            let now = Utc::now();
            let next_retry = next_retry_at_for_failure(
                work.attempt_count,
                work.retry_expires_at.as_deref(),
                retryable,
                now,
            );
            let now_text = now.to_rfc3339();
            let retry_after =
                (Utc::now() + chrono::Duration::seconds(RETRY_COOLDOWN_SECS)).to_rfc3339();
            let retry_expires = (Utc::now() + chrono::Duration::hours(24)).to_rfc3339();
            let attempt_event_id = sqlx::query_scalar::<_, String>(
                "SELECT id FROM content_attempt_events WHERE work_item_id = ? AND attempt_no = ? AND event_type = 'attempt_started' LIMIT 1",
            )
            .bind(&work.id)
            .bind(work.attempt_count)
            .fetch_one(&mut *tx)
            .await?;
            let error_summary = translations::translation_error_summary(Some(error_text.as_str()));
            let linked_call_ids = unique_call_ids(
                output_validation_call_ids(&error)
                    .into_iter()
                    .chain(global_execution_call_ids(&error))
                    .chain(ai::llm_call_id(&error))
                    .collect(),
            );
            let mut call_audits = Vec::with_capacity(linked_call_ids.len());
            for call_id in &linked_call_ids {
                call_audits.push((
                    call_id.clone(),
                    load_llm_call_audit(&mut tx, call_id).await?,
                ));
            }
            let link_status = if output_validation_error_code(&error).is_some() {
                "succeeded"
            } else {
                "failed"
            };
            if call_audits.is_empty() {
                let audit_call_id = local_id::generate_local_id().to_string();
                if let Err(audit_error) = persist_attempt_llm_call_audit(
                    &mut tx,
                    AttemptLlmCallAudit {
                        audit_call_id: &audit_call_id,
                        attempt_event_id: &attempt_event_id,
                        provider_call_id: "unknown",
                        model: route_snapshot
                            .first()
                            .map(String::as_str)
                            .unwrap_or("unknown"),
                        status: link_status,
                        duration_ms: None,
                        input_tokens: None,
                        output_tokens: None,
                        error_code: &class,
                        error_summary: error_summary.as_deref(),
                        created_at: now_text.as_str(),
                    },
                )
                .await
                {
                    warn!(
                        ?audit_error,
                        work_item_id = %work.id,
                        attempt_event_id = %attempt_event_id,
                        "failed to persist content processing call audit"
                    );
                }
            } else {
                let mut used_provider_call_ids = HashSet::new();
                for (call_id, audit) in &call_audits {
                    let audit = audit.clone().unwrap_or_else(|| LlmCallAudit {
                        provider_request_id: None,
                        model: route_snapshot
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "unknown".to_owned()),
                        status: link_status.to_owned(),
                        duration_ms: None,
                        input_tokens: None,
                        output_tokens: None,
                    });
                    let provider_call_id = unique_provider_call_id(
                        call_id,
                        audit.provider_request_id.as_deref(),
                        &mut used_provider_call_ids,
                    );
                    if let Err(audit_error) = persist_attempt_llm_call_audit(
                        &mut tx,
                        AttemptLlmCallAudit {
                            audit_call_id: call_id,
                            attempt_event_id: &attempt_event_id,
                            provider_call_id: &provider_call_id,
                            model: &audit.model,
                            status: &audit.status,
                            duration_ms: audit.duration_ms,
                            input_tokens: audit.input_tokens,
                            output_tokens: audit.output_tokens,
                            error_code: &class,
                            error_summary: error_summary.as_deref(),
                            created_at: now_text.as_str(),
                        },
                    )
                    .await
                    {
                        warn!(
                            ?audit_error,
                            work_item_id = %work.id,
                            attempt_event_id = %attempt_event_id,
                            call_id = %call_id,
                            "failed to persist content processing call audit"
                        );
                    }
                }
            }
            sqlx::query("UPDATE content_work_items SET status = 'failed', failure_class = ?, next_retry_at = ?, retry_expires_at = COALESCE(retry_expires_at, ?), retry_after_at = ?, finished_at = ?, lease_owner = NULL, lease_expires_at = NULL, updated_at = ? WHERE id = ?")
                .bind(&class)
                .bind(&next_retry)
                .bind(&retry_expires)
                .bind(&retry_after)
                .bind(now_text.as_str())
                .bind(now_text.as_str())
                .bind(&work.id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE content_batch_items SET result_status = 'failed', error_code = ?, error_summary = ?, updated_at = ? WHERE work_item_id = ? AND batch_id = ?")
                .bind(&class)
                .bind(error_summary.as_deref())
                .bind(now_text.as_str())
                .bind(&work.id)
                .bind(work.batch_id.as_deref().unwrap_or_default())
                .execute(&mut *tx)
                .await?;
            let token_count = match (
                call_audits
                    .iter()
                    .filter_map(|(_, audit)| audit.as_ref().and_then(|audit| audit.input_tokens))
                    .reduce(i64::saturating_add),
                call_audits
                    .iter()
                    .filter_map(|(_, audit)| audit.as_ref().and_then(|audit| audit.output_tokens))
                    .reduce(i64::saturating_add),
            ) {
                (Some(input), Some(output)) => Some(input.saturating_add(output)),
                _ => None,
            };
            sqlx::query("INSERT OR IGNORE INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, error_code, error_summary, failure_class, retry_eligible, next_retry_at, duration_ms, token_count, cost_microunits, created_at) SELECT ?, work_item_id, attempt_no, trigger, 'attempt_completed', 'failed', ?, ?, ?, ?, ?, ?, ?, ?, ? FROM content_attempt_events WHERE work_item_id = ? AND attempt_no = ? AND event_type = 'attempt_started'")
                .bind(local_id::generate_local_id().to_string())
                .bind(&class)
                .bind(error_summary)
                .bind(&class)
                .bind(i64::from(next_retry.is_some()))
                .bind(&next_retry)
                .bind(
                    call_audits
                        .iter()
                        .filter_map(|(_, audit)| audit.as_ref().and_then(|audit| audit.duration_ms))
                        .reduce(i64::saturating_add),
                )
                .bind(token_count)
                .bind(Option::<i64>::None)
                .bind(now_text.as_str())
                .bind(&work.id)
                .bind(work.attempt_count)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE content_batches SET status = 'failed', finished_at = ?, updated_at = ?, error_code = ?, error_summary = ? WHERE id = ?")
                .bind(now_text.as_str())
                .bind(now_text.as_str())
                .bind(&class)
                .bind(&class)
                .bind(work.batch_id.as_deref().unwrap_or_default())
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

pub async fn run_once(state: &AppState) -> Result<()> {
    if current_mode(&state.pool).await? != ContentProcessingMode::Global {
        return Ok(());
    }
    if !content_identity_upgrade::is_complete(&state.pool).await? {
        return Ok(());
    }
    recover_due(state).await?;
    if provider_breaker_open(state).await {
        defer_queued_for_provider(state).await?;
        return Ok(());
    }
    let worker_count = state
        .translation_scheduler
        .desired_config()
        .await
        .general_worker_concurrency
        .max(1);
    // With a single general worker there is no separate background slot to
    // reserve; allowing one manual slot prevents accepted retries starving
    // forever. With two or more workers, retain one slot for background work.
    let manual_limit = if worker_count > 1 {
        i64::try_from(worker_count.saturating_sub(1)).unwrap_or(i64::MAX)
    } else {
        1
    };
    if let Some(work) = claim_next(state, manual_limit).await?
        && let Err(error) = execute(state, work).await
    {
        warn!(?error, "global content processing execution failed");
    }
    Ok(())
}

pub fn spawn_global_scheduler(state: Arc<AppState>) -> tokio::task::AbortHandle {
    tokio::spawn(async move {
        let mut workers = JoinSet::new();
        let mut worker_count = 0usize;
        let desired_workers = Arc::new(AtomicUsize::new(0));
        loop {
            let desired = state
                .translation_scheduler
                .desired_config()
                .await
                .general_worker_concurrency
                .max(1);
            desired_workers.store(desired, Ordering::Release);
            while workers.try_join_next().is_some() {
                worker_count = worker_count.saturating_sub(1);
            }
            if desired > worker_count {
                let first_new_worker = worker_count;
                for worker_index in first_new_worker..desired {
                    let worker_state = state.clone();
                    let worker_target = desired_workers.clone();
                    workers.spawn(async move {
                        loop {
                            if worker_index >= worker_target.load(Ordering::Acquire) {
                                break;
                            }
                            if let Err(error) = run_once(worker_state.as_ref()).await {
                                warn!(?error, "global content processing scheduler failed");
                            }
                            sleep(Duration::from_millis(250)).await;
                        }
                    });
                }
                worker_count = desired;
            }
            sleep(Duration::from_millis(250)).await;
        }
    })
    .abort_handle()
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::{
        borrow::Cow,
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use super::*;
    use crate::config::AppConfig;
    use crate::crypto::EncryptionKey;
    use crate::observability::LoggingThresholds;
    use crate::state::{build_oauth_client, build_webauthn};
    use crate::translations::{TranslationRuntimeConfig, TranslationSchedulerController};
    use axum::{Router, routing::post};
    use sqlx::sqlite::SqlitePoolOptions;
    use tokio::sync::Notify;
    use url::Url;

    async fn pool(mode: &str) -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE content_processing_control (id INTEGER PRIMARY KEY, mode TEXT NOT NULL, switch_token TEXT, updated_at TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO content_processing_control (id, mode, updated_at) VALUES (1, ?, CURRENT_TIMESTAMP)")
            .bind(mode)
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    async fn global_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
        "CREATE TABLE notifications (id TEXT PRIMARY KEY, user_id TEXT NOT NULL, thread_id TEXT NOT NULL)",
    )
    .execute(&pool)
    .await
    .unwrap();
        sqlx::raw_sql(include_str!(
            "../migrations/0078_content_processing_global_model.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../migrations/0084_content_processing_model_independent_identity.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../migrations/0086_content_work_admission_events.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("UPDATE content_processing_control SET mode = 'global' WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE content_identity_upgrade_control SET status = 'completed', phase = 'complete' WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    async fn global_execution_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::database_migrations::run(&pool).await.unwrap();
        sqlx::query("UPDATE content_processing_control SET mode = 'global' WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE content_identity_upgrade_control SET status = 'completed', phase = 'complete' WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn failed_llm_call_audit_persists_classification_and_usage() {
        let pool = global_pool().await;
        insert_test_work(&pool, "audit-work", "running", 1, None).await;
        sqlx::query(
            "INSERT INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, created_at) VALUES ('audit-event', 'audit-work', 1, 'initial', 'attempt_started', CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut transaction = pool.begin().await.unwrap();
        persist_attempt_llm_call_audit(
            &mut transaction,
            AttemptLlmCallAudit {
                audit_call_id: "audit-call",
                attempt_event_id: "audit-event",
                provider_call_id: "provider-request-1",
                model: "test-model",
                status: "failed",
                duration_ms: Some(125),
                input_tokens: Some(12),
                output_tokens: Some(8),
                error_code: "provider_unavailable",
                error_summary: Some("provider request failed"),
                created_at: "2026-09-20T12:00:00Z",
            },
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();

        let audit = sqlx::query_as::<_, (String, String, String, i64, i64, String)>(
            "SELECT status, provider_call_id, error_code, duration_ms, input_tokens, error_summary FROM content_attempt_llm_calls WHERE id = 'audit-call'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(audit.0, "failed");
        assert_eq!(audit.1, "provider-request-1");
        assert_eq!(audit.2, "provider_unavailable");
        assert_eq!(audit.3, 125);
        assert_eq!(audit.4, 12);
        assert_eq!(audit.5, "provider request failed");
    }

    fn global_state(pool: SqlitePool) -> Arc<AppState> {
        let encryption_key =
            EncryptionKey::from_base64("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").unwrap();
        let config = AppConfig {
            bind_addr: "127.0.0.1:58090".parse::<SocketAddr>().unwrap(),
            public_base_url: Url::parse("http://127.0.0.1:58090").unwrap(),
            database_url: "sqlite::memory:".to_owned(),
            sqlite_pool_max_connections: 1,
            static_dir: None,
            task_log_dir: std::env::temp_dir().join("octo-rill-content-processing-tests"),
            job_worker_concurrency: 1,
            encryption_key: encryption_key.clone(),
            github: crate::config::GitHubOAuthConfig {
                client_id: "test-client-id".to_owned(),
                client_secret: "test-client-secret".to_owned(),
                redirect_url: Url::parse("http://127.0.0.1:58090/auth/callback").unwrap(),
            },
            linuxdo: None,
            ai: Some(crate::config::AiConfig {
                base_url: Url::parse("https://ai.example.test/v1/").unwrap(),
                model: "test-model".to_owned(),
                api_key: "test-api-key".to_owned(),
            }),
            ai_max_concurrency: 1,
            ai_daily_at_local: None,
            app_default_time_zone: "UTC".to_owned(),
            logging: LoggingThresholds::default(),
        };
        let github_oauth = build_oauth_client(&config).unwrap();
        let webauthn = build_webauthn(&config).unwrap();
        Arc::new(AppState {
            config,
            pool,
            sqlite_writer: crate::sqlite_write::SqliteWriteCoordinator::new(),
            api_key_last_used_touches: crate::api_keys::ApiKeyLastUsedTouchQueue::new(),
            http: reqwest::Client::new(),
            github_rest_http: reqwest::Client::new(),
            github_rest_api_base: Url::parse("https://api.github.com/").unwrap(),
            github_graphql_url: Url::parse("https://api.github.com/graphql").unwrap(),
            github_oauth,
            linuxdo_oauth: None,
            webauthn,
            encryption_key,
            llm_scheduler: Arc::new(ai::LlmScheduler::new(1)),
            translation_scheduler: Arc::new(TranslationSchedulerController::new(
                TranslationRuntimeConfig::default(),
            )),
            runtime_owner_id: "content-processing-test-owner".to_owned(),
        })
    }

    async fn spawn_test_ai_server(app: Router) -> Url {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test ai server");
        let addr = listener.local_addr().expect("resolve test ai server addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test ai app");
        });
        Url::parse(&format!("http://{addr}/v1/")).expect("parse test ai base url")
    }

    async fn spawn_sequenced_test_ai_server(
        responses: Vec<(&str, &str)>,
    ) -> (Url, Arc<Mutex<Vec<u32>>>, Arc<Mutex<Vec<String>>>) {
        let responses = Arc::new(Mutex::new(
            responses
                .into_iter()
                .map(|(content, finish_reason)| (content.to_owned(), finish_reason.to_owned()))
                .collect::<VecDeque<_>>(),
        ));
        let requested_tokens = Arc::new(Mutex::new(Vec::new()));
        let requested_models = Arc::new(Mutex::new(Vec::new()));
        let response_state = responses.clone();
        let token_state = requested_tokens.clone();
        let model_state = requested_models.clone();
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move |Json(request): Json<Value>| {
                let response_state = response_state.clone();
                let token_state = token_state.clone();
                let model_state = model_state.clone();
                async move {
                    token_state
                        .lock()
                        .expect("token state lock")
                        .push(request["max_tokens"].as_u64().unwrap_or_default() as u32);
                    model_state
                        .lock()
                        .expect("model state lock")
                        .push(request["model"].as_str().unwrap_or_default().to_owned());
                    let (content, finish_reason) = response_state
                        .lock()
                        .expect("response state lock")
                        .pop_front()
                        .expect("mock response remains");
                    (
                        StatusCode::OK,
                        Json(json!({
                            "id": "mock-provider-response",
                            "choices": [{
                                "message": {"content": content},
                                "finish_reason": finish_reason
                            }],
                            "usage": {
                                "prompt_tokens": 10,
                                "completion_tokens": 5,
                                "total_tokens": 15
                            }
                        })),
                    )
                }
            }),
        );
        (
            spawn_test_ai_server(app).await,
            requested_tokens,
            requested_models,
        )
    }

    async fn seed_executable_work(pool: &SqlitePool, id: &str, target_slots: &[&str]) {
        sqlx::query("INSERT INTO repo_releases (id, repo_id, release_id, tag_name, html_url, updated_at) VALUES (?, 1, 12345, 'v1', 'https://example.test/releases/12345', CURRENT_TIMESTAMP)")
            .bind(format!("release-row-{id}"))
            .execute(pool)
            .await
            .unwrap();
        let source_snapshot = json!({
            "source_blocks": [{"slot": "title", "text": "A release title"}],
            "target_slots": target_slots
        });
        sqlx::query("INSERT INTO content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, attempt_count, created_at, updated_at) VALUES (?, 'release', '12345', 'translation', 'summary', 'zh-CN', ?, ?, 'test-model', ?, 'test-fingerprint', 'queued', 0, 0, 1, 0, '2000-01-01T00:00:00Z', '2000-01-01T00:00:00Z')")
            .bind(id)
            .bind(format!("source-hash-{id}"))
            .bind(GLOBAL_PROTOCOL_VERSION)
            .bind(source_snapshot.to_string())
            .execute(pool)
            .await
            .unwrap();
    }

    async fn assert_retryable_failure(pool: &SqlitePool, work_id: &str, attempt_no: i64) {
        let (status, failure_class, work_next_retry, lease_owner): (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = sqlx::query_as(
            "SELECT status, failure_class, next_retry_at, lease_owner FROM content_work_items WHERE id = ?",
        )
        .bind(work_id)
        .fetch_one(pool)
        .await
        .unwrap();
        let (error_code, retry_eligible, event_next_retry): (Option<String>, i64, Option<String>) =
            sqlx::query_as(
                "SELECT error_code, retry_eligible, next_retry_at FROM content_attempt_events WHERE work_item_id = ? AND attempt_no = ? AND event_type = 'attempt_completed'",
            )
            .bind(work_id)
            .bind(attempt_no)
            .fetch_one(pool)
            .await
            .unwrap();

        assert_eq!(status, "failed");
        assert_eq!(failure_class.as_deref(), error_code.as_deref());
        assert!(work_next_retry.is_some());
        assert_eq!(work_next_retry, event_next_retry);
        assert_eq!(lease_owner, None);
        assert_eq!(retry_eligible, 1);
    }

    async fn insert_test_work(
        pool: &SqlitePool,
        id: &str,
        status: &str,
        attempt_count: i64,
        next_retry_at: Option<&str>,
    ) {
        sqlx::query(
            "INSERT INTO content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, attempt_count, next_retry_at, created_at, updated_at) VALUES (?, 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'hash-1', ?, 'test-model', '{}', 'config-1', ?, 0, 0, 1, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .bind(id)
        .bind(GLOBAL_PROTOCOL_VERSION)
        .bind(status)
        .bind(attempt_count)
        .bind(next_retry_at)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn execute_persists_failure_finalization_atomically() {
        let pool = global_execution_pool().await;
        sqlx::query("INSERT INTO repo_releases (id, repo_id, release_id, tag_name, html_url, updated_at) VALUES ('test-release', 1, 12345, 'v1', 'https://example.test/releases/12345', CURRENT_TIMESTAMP)")
            .execute(&pool)
            .await
            .unwrap();
        let source_snapshot = json!({
            "source_blocks": [{"slot": "title", "text": "A release title"}],
            "target_slots": ["title_zh"]
        });
        sqlx::query("INSERT INTO content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, attempt_count, created_at, updated_at) VALUES ('execute-failure-work', 'release', '12345', 'translation', 'summary', 'zh-CN', 'source-hash', ?, 'test-model', ?, 'test-fingerprint', 'queued', 0, 0, 1, 0, '2000-01-01T00:00:00Z', '2000-01-01T00:00:00Z')")
            .bind(GLOBAL_PROTOCOL_VERSION)
            .bind(source_snapshot.to_string())
            .execute(&pool)
            .await
            .unwrap();

        let base_url = spawn_test_ai_server(Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": {"message": "synthetic provider failure"}})),
                )
            }),
        ))
        .await;
        let mut state = global_state(pool.clone());
        Arc::get_mut(&mut state)
            .expect("state has a single owner")
            .config
            .ai
            .as_mut()
            .expect("AI configuration")
            .base_url = base_url;

        let work = claim_next(&state, 1)
            .await
            .unwrap()
            .expect("queued work should be claimed");
        let attempt_no = work.attempt_count;
        let batch_id = work.batch_id.clone().expect("claimed work has a batch");
        let execution = execute(&state, work).await;
        assert!(
            execution.is_ok(),
            "failure finalizer returned: {execution:?}"
        );

        let (work_status, failure_class, work_retry, lease_owner, lease_expires_at): (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = sqlx::query_as("SELECT status, failure_class, next_retry_at, lease_owner, lease_expires_at FROM content_work_items WHERE id = 'execute-failure-work'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(work_status, "failed");
        assert!(failure_class.is_some());
        assert_eq!(lease_owner, None);
        assert_eq!(lease_expires_at, None);

        let (result_status, event_class, retry_eligible, event_retry): (
            String,
            Option<String>,
            i64,
            Option<String>,
        ) = sqlx::query_as("SELECT result_status, failure_class, retry_eligible, next_retry_at FROM content_attempt_events WHERE work_item_id = 'execute-failure-work' AND attempt_no = ? AND event_type = 'attempt_completed'")
            .bind(attempt_no)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(result_status, "failed");
        assert_eq!(event_class, failure_class);
        assert_eq!(work_retry, event_retry);
        assert_eq!(retry_eligible, 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM content_attempt_events WHERE work_item_id = 'execute-failure-work' AND attempt_no = ? AND event_type = 'attempt_completed'")
                .bind(attempt_no)
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );

        let started_event_id: String = sqlx::query_scalar(
            "SELECT id FROM content_attempt_events WHERE work_item_id = 'execute-failure-work' AND attempt_no = ? AND event_type = 'attempt_started'",
        )
        .bind(attempt_no)
        .fetch_one(&pool)
        .await
        .unwrap();
        let (call_status, linked_event_id): (String, String) = sqlx::query_as(
            "SELECT status, attempt_event_id FROM content_attempt_llm_calls WHERE attempt_event_id = ?",
        )
        .bind(&started_event_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(call_status, "failed");
        assert_eq!(linked_event_id, started_event_id);

        let batch_item_status: String = sqlx::query_scalar(
            "SELECT result_status FROM content_batch_items WHERE work_item_id = 'execute-failure-work' AND batch_id = ?",
        )
        .bind(&batch_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let batch_status: String =
            sqlx::query_scalar("SELECT status FROM content_batches WHERE id = ?")
                .bind(&batch_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(batch_item_status, "failed");
        assert_eq!(batch_status, "failed");
    }

    #[tokio::test]
    async fn execute_normalizes_wrapped_output_and_persists_declared_projection() {
        let pool = global_execution_pool().await;
        seed_executable_work(&pool, "wrapped-output-work", &["title_zh"]).await;
        let base_url = spawn_sequenced_test_ai_server(vec![(
            r#"{"output":{"title_zh":"标题","status":"ready"}}"#,
            "stop",
        )])
        .await
        .0;
        let mut state = global_state(pool.clone());
        Arc::get_mut(&mut state)
            .expect("state has a single owner")
            .config
            .ai
            .as_mut()
            .expect("AI configuration")
            .base_url = base_url;

        let work = claim_next(&state, 1)
            .await
            .unwrap()
            .expect("queued work should be claimed");
        let attempt_no = work.attempt_count;
        execute(&state, work).await.unwrap();

        let status: String = sqlx::query_scalar(
            "SELECT status FROM content_work_items WHERE id = 'wrapped-output-work'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "ready");
        let payload: String = sqlx::query_scalar(
            "SELECT payload_json FROM content_result_projections WHERE work_item_id = 'wrapped-output-work'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&payload).unwrap(),
            json!({"title_zh": "标题"})
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_llm_calls WHERE attempt_event_id = (SELECT id FROM content_attempt_events WHERE work_item_id = 'wrapped-output-work' AND attempt_no = ? AND event_type = 'attempt_started') AND status = 'succeeded'",
            )
            .bind(attempt_no)
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn execute_rejects_unknown_output_wrapper_after_provider_success() {
        let pool = global_execution_pool().await;
        seed_executable_work(&pool, "unknown-wrapper-work", &["title_zh"]).await;
        let base_url = spawn_sequenced_test_ai_server(vec![(
            r#"{"title_zh":"标题","result":{"title_zh":"错误"}}"#,
            "stop",
        )])
        .await
        .0;
        let mut state = global_state(pool.clone());
        Arc::get_mut(&mut state)
            .expect("state has a single owner")
            .config
            .ai
            .as_mut()
            .expect("AI configuration")
            .base_url = base_url;

        let work = claim_next(&state, 1)
            .await
            .unwrap()
            .expect("queued work should be claimed");
        let attempt_no = work.attempt_count;
        execute(&state, work).await.unwrap();

        let (status, failure_class, error_code): (String, String, String) = sqlx::query_as(
            "SELECT w.status, w.failure_class, e.error_code FROM content_work_items w JOIN content_attempt_events e ON e.work_item_id = w.id AND e.attempt_no = ? AND e.event_type = 'attempt_completed' WHERE w.id = 'unknown-wrapper-work'",
        )
        .bind(attempt_no)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "failed");
        assert_eq!(failure_class, "output_contract_invalid");
        assert_eq!(error_code, "output_contract_invalid");
        assert_retryable_failure(&pool, "unknown-wrapper-work", attempt_no).await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_result_projections WHERE work_item_id = 'unknown-wrapper-work'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_llm_calls WHERE attempt_event_id = (SELECT id FROM content_attempt_events WHERE work_item_id = 'unknown-wrapper-work' AND attempt_no = ? AND event_type = 'attempt_started') AND status = 'succeeded'",
            )
            .bind(attempt_no)
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn global_work_lease_renews_only_for_the_current_live_claim() {
        let pool = global_execution_pool().await;
        seed_executable_work(&pool, "lease-renewal-work", &["title_zh"]).await;
        let state = global_state(pool.clone());
        let work = claim_next(&state, 1)
            .await
            .unwrap()
            .expect("queued work should be claimed");
        let short_expiry = (Utc::now() + chrono::Duration::seconds(30)).to_rfc3339();
        sqlx::query("UPDATE content_work_items SET lease_expires_at = ? WHERE id = ?")
            .bind(&short_expiry)
            .bind(&work.id)
            .execute(&pool)
            .await
            .unwrap();

        assert!(renew_global_work_lease(&state, &work).await.unwrap());
        let renewed_expiry: String =
            sqlx::query_scalar("SELECT lease_expires_at FROM content_work_items WHERE id = ?")
                .bind(&work.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            parse_storage_timestamp(&renewed_expiry).unwrap()
                > Utc::now() + chrono::Duration::minutes(4)
        );

        let expired = (Utc::now() - chrono::Duration::seconds(1)).to_rfc3339();
        sqlx::query("UPDATE content_work_items SET lease_expires_at = ? WHERE id = ?")
            .bind(expired)
            .bind(&work.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(!renew_global_work_lease(&state, &work).await.unwrap());
    }

    #[tokio::test]
    async fn execute_performs_one_length_recovery_and_links_both_calls() {
        let pool = global_execution_pool().await;
        seed_executable_work(&pool, "length-recovery-work", &["title_zh"]).await;
        let (base_url, requested_tokens, requested_models) = spawn_sequenced_test_ai_server(vec![
            (r#"{"title_zh":"截断但仍是 JSON"}"#, "length"),
            (r#"{"title_zh":"完整标题"}"#, "stop"),
        ])
        .await;
        let mut state = global_state(pool.clone());
        Arc::get_mut(&mut state)
            .expect("state has a single owner")
            .config
            .ai
            .as_mut()
            .expect("AI configuration")
            .base_url = base_url;

        let work = claim_next(&state, 1)
            .await
            .unwrap()
            .expect("queued work should be claimed");
        let attempt_no = work.attempt_count;
        execute(&state, work).await.unwrap();

        assert_eq!(
            *requested_tokens.lock().expect("token state lock"),
            vec![GLOBAL_MAX_TOKENS, GLOBAL_LENGTH_RECOVERY_MAX_TOKENS]
        );
        let requested_models = requested_models.lock().expect("model state lock").clone();
        assert_eq!(requested_models.len(), 2);
        assert_eq!(requested_models[0], requested_models[1]);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_llm_calls WHERE attempt_event_id = (SELECT id FROM content_attempt_events WHERE work_item_id = 'length-recovery-work' AND attempt_no = ? AND event_type = 'attempt_started')",
            )
            .bind(attempt_no)
            .fetch_one(&pool)
            .await
            .unwrap(),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(DISTINCT provider_call_id) FROM content_attempt_llm_calls WHERE attempt_event_id = (SELECT id FROM content_attempt_events WHERE work_item_id = 'length-recovery-work' AND attempt_no = ? AND event_type = 'attempt_started')",
            )
            .bind(attempt_no)
            .fetch_one(&pool)
            .await
            .unwrap(),
            2
        );
        let status: String = sqlx::query_scalar(
            "SELECT status FROM content_work_items WHERE id = 'length-recovery-work'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "ready");
    }

    #[tokio::test]
    async fn execute_marks_second_length_response_as_bounded_truncation_failure() {
        let pool = global_execution_pool().await;
        seed_executable_work(&pool, "length-failure-work", &["title_zh"]).await;
        let (base_url, requested_tokens, requested_models) = spawn_sequenced_test_ai_server(vec![
            (r#"{"title_zh":"第一次截断"}"#, "length"),
            (r#"{"title_zh":"第二次截断"}"#, "length"),
        ])
        .await;
        let mut state = global_state(pool.clone());
        Arc::get_mut(&mut state)
            .expect("state has a single owner")
            .config
            .ai
            .as_mut()
            .expect("AI configuration")
            .base_url = base_url;

        let work = claim_next(&state, 1)
            .await
            .unwrap()
            .expect("queued work should be claimed");
        let attempt_no = work.attempt_count;
        execute(&state, work).await.unwrap();

        assert_eq!(
            *requested_tokens.lock().expect("token state lock"),
            vec![GLOBAL_MAX_TOKENS, GLOBAL_LENGTH_RECOVERY_MAX_TOKENS]
        );
        let requested_models = requested_models.lock().expect("model state lock").clone();
        assert_eq!(requested_models.len(), 2);
        assert_eq!(requested_models[0], requested_models[1]);
        let (status, failure_class, error_code): (String, String, String) = sqlx::query_as(
            "SELECT w.status, w.failure_class, e.error_code FROM content_work_items w JOIN content_attempt_events e ON e.work_item_id = w.id AND e.attempt_no = ? AND e.event_type = 'attempt_completed' WHERE w.id = 'length-failure-work'",
        )
        .bind(attempt_no)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "failed");
        assert_eq!(failure_class, "output_truncated");
        assert_eq!(error_code, "output_truncated");
        assert_retryable_failure(&pool, "length-failure-work", attempt_no).await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_llm_calls WHERE attempt_event_id = (SELECT id FROM content_attempt_events WHERE work_item_id = 'length-failure-work' AND attempt_no = ? AND event_type = 'attempt_started')",
            )
            .bind(attempt_no)
            .fetch_one(&pool)
            .await
            .unwrap(),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_llm_calls WHERE attempt_event_id = (SELECT id FROM content_attempt_events WHERE work_item_id = 'length-failure-work' AND attempt_no = ? AND event_type = 'attempt_started') AND status = 'succeeded'",
            )
            .bind(attempt_no)
            .fetch_one(&pool)
            .await
            .unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn reads_modes_and_transitions_only_from_freeze() {
        let pool = pool("rollback_freeze").await;
        assert_eq!(
            current_mode(&pool).await.unwrap(),
            ContentProcessingMode::RollbackFreeze
        );
        assert!(transition_to_global(&pool, "cutover-1").await.unwrap());
        assert_eq!(
            current_mode(&pool).await.unwrap(),
            ContentProcessingMode::Global
        );
        assert!(!transition_to_global(&pool, "cutover-2").await.unwrap());
    }

    #[tokio::test]
    async fn legacy_mode_can_enter_the_controlled_freeze_window() {
        let pool = pool("legacy").await;
        let mut tx = pool.begin().await.unwrap();
        assert!(
            transition_to_rollback_freeze_in_transaction(&mut tx, "freeze-1")
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        assert_eq!(
            current_mode(&pool).await.unwrap(),
            ContentProcessingMode::RollbackFreeze
        );
        let mut tx = pool.begin().await.unwrap();
        assert!(
            !transition_to_rollback_freeze_in_transaction(&mut tx, "freeze-2")
                .await
                .unwrap()
        );
        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn migration_preserves_legacy_rows_and_creates_only_global_tables() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(
            "CREATE TABLE translation_work_items (id TEXT PRIMARY KEY, scope_user_id TEXT NOT NULL, kind TEXT NOT NULL, entity_id TEXT NOT NULL, target_lang TEXT NOT NULL, source_hash TEXT NOT NULL, result_status TEXT, status TEXT NOT NULL, attempt_count INTEGER NOT NULL DEFAULT 0, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP); CREATE TABLE ai_translations (id TEXT PRIMARY KEY, user_id TEXT NOT NULL, entity_type TEXT NOT NULL, entity_id TEXT NOT NULL, lang TEXT NOT NULL, source_hash TEXT NOT NULL, status TEXT NOT NULL, title TEXT, summary TEXT, value TEXT NOT NULL); CREATE TABLE notifications (id TEXT PRIMARY KEY, user_id TEXT NOT NULL, thread_id TEXT NOT NULL, updated_at TEXT); INSERT INTO translation_work_items (id, scope_user_id, kind, entity_id, target_lang, source_hash, result_status, status) VALUES ('legacy-1', 'user-1', 'release_summary', 'release-1', 'zh-CN', 'hash-1', 'ready', 'completed'); INSERT INTO ai_translations VALUES ('cache-1', 'user-1', 'release', 'release-1', 'zh-CN', 'hash-1', 'ready', 'Cached title', 'Cached summary', 'cached');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let before_work: (String, String, String, String) =
            sqlx::query_as("SELECT id, kind, entity_id, status FROM translation_work_items")
                .fetch_one(&pool)
                .await
                .unwrap();
        let before_cache: (String, String, String, String, String, String, String, String) =
            sqlx::query_as("SELECT id, user_id, entity_type, entity_id, status, title, summary, value FROM ai_translations")
                .fetch_one(&pool)
                .await
                .unwrap();

        sqlx::raw_sql(include_str!(
            "../migrations/0078_content_processing_global_model.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../migrations/0079_admin_collection_read_budget_indexes.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, attempt_count, created_at, updated_at) VALUES ('global-model-a', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'hash-1', 'protocol-1', 'model-a', '{}', 'config-a', 'blocked_config', 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP), ('global-model-b', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'hash-1', 'protocol-1', 'model-b', '{}', 'config-b', 'ready', 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP), ('global-model-c', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'hash-2', 'protocol-1', 'model-a', '{}', 'config-a', 'ready', 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP), ('global-model-d', 'release', 'release-2', 'translation', 'summary', 'zh-CN', 'hash-3', 'protocol-1', 'model-c', '{}', 'config-c', 'ready', 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP), ('global-model-e', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'hash-2', 'protocol-1', 'model-b', '{}', 'config-b', 'ready', 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO content_request_links (id, request_id, work_item_id, requester_type, requester_id, authorization_snapshot_json, producer_ref, request_source, delivery_mode, created_at, updated_at) VALUES ('request-link-a', 'request-a', 'global-model-a', 'user', 'user-1', '{}', 'test', 'test', 'async', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, retry_eligible, created_at) VALUES ('attempt-start-a', 'global-model-a', 1, 'initial', 'attempt_started', 0, CURRENT_TIMESTAMP), ('attempt-end-a', 'global-model-a', 1, 'initial', 'attempt_completed', 0, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO content_attempt_llm_calls (id, attempt_event_id, provider_call_id, model, status, created_at) VALUES ('call-a', 'attempt-start-a', 'provider-call-a', 'model-a', 'failed', CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO content_result_projections (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash, work_item_id, payload_json, published_at, updated_at) VALUES ('projection-a', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'protocol-1', 'model-a', 'hash-1', 'global-model-a', '{\"title_zh\":\"A\"}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'), ('projection-b', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'protocol-1', 'model-b', 'hash-1', 'global-model-b', '{\"title_zh\":\"B\"}', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z'), ('projection-c', 'release', 'release-2', 'translation', 'summary', 'zh-CN', 'protocol-1', 'model-c', 'hash-3', 'global-model-d', '{\"title_zh\":\"C\"}', '2026-01-03T00:00:00Z', '2026-01-03T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::raw_sql(include_str!(
            "../migrations/0084_content_processing_model_independent_identity.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();

        let after_work: (String, String, String, String) =
            sqlx::query_as("SELECT id, kind, entity_id, status FROM translation_work_items")
                .fetch_one(&pool)
                .await
                .unwrap();
        let after_cache: (String, String, String, String, String, String, String, String) = sqlx::query_as("SELECT id, user_id, entity_type, entity_id, status, title, summary, value FROM ai_translations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(before_work, after_work);
        assert_eq!(before_cache, after_cache);
        let new_table_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('content_processing_control', 'content_work_items', 'content_batches', 'content_batch_items', 'content_result_projections', 'content_request_links', 'content_attempt_events', 'content_attempt_llm_calls', 'content_legacy_observations', 'content_work_identities', 'content_work_identity_members', 'content_current_result_projections', 'content_identity_upgrade_control')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(new_table_count, 13);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_work_items WHERE id IN ('global-model-a', 'global-model-b') AND source_hash = 'hash-1'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_result_projections WHERE id IN ('projection-a', 'projection-b')",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_request_links WHERE id = 'request-link-a'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_events WHERE work_item_id = 'global-model-a'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_llm_calls WHERE id = 'call-a'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
        for table in [
            "content_work_identities",
            "content_work_identity_members",
            "content_current_result_projections",
        ] {
            let row_count = sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(row_count, 0, "compatibility migration backfilled {table}");
        }
        let upgrade_state: (String, String, Option<String>) = sqlx::query_as(
            "SELECT status, phase, cursor FROM content_identity_upgrade_control WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            upgrade_state,
            (
                "pending".to_owned(),
                "work_identity_backfill".to_owned(),
                None
            )
        );
        let attempt_snapshot: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT configuration_snapshot_json, route_snapshot_json, configuration_fingerprint FROM content_attempt_events WHERE id = 'attempt-start-a'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(attempt_snapshot, (None, None, None));
        let model_independent_columns: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM pragma_table_info('content_work_identities') ORDER BY cid",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(
            !model_independent_columns
                .iter()
                .any(|column| column == "model_profile")
        );

        for (id, resource_id, source_hash) in [
            ("identity-a", "release-1", "hash-1"),
            ("identity-b", "release-1", "hash-2"),
            ("identity-c", "release-2", "hash-3"),
        ] {
            sqlx::query(
                "INSERT INTO content_work_identities (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, created_at, updated_at) VALUES (?, 'release', ?, 'translation', 'summary', 'zh-CN', ?, 'protocol-1', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            )
            .bind(id)
            .bind(resource_id)
            .bind(source_hash)
            .execute(&pool)
            .await
            .unwrap();
        }
        let duplicate_identity = sqlx::query(
            "INSERT INTO content_work_identities (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, created_at, updated_at) VALUES ('identity-duplicate', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'hash-1', 'protocol-1', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await;
        assert!(duplicate_identity.is_err());
        for (work_item_id, identity_id, resource_id, source_hash) in [
            ("global-model-a", "identity-a", "release-1", "hash-1"),
            ("global-model-b", "identity-a", "release-1", "hash-1"),
            ("global-model-c", "identity-b", "release-1", "hash-2"),
            ("global-model-d", "identity-c", "release-2", "hash-3"),
        ] {
            sqlx::query(
                "INSERT INTO content_work_identity_members (work_item_id, identity_id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, linked_at) VALUES (?, ?, 'release', ?, 'translation', 'summary', 'zh-CN', ?, 'protocol-1', CURRENT_TIMESTAMP)",
            )
            .bind(work_item_id)
            .bind(identity_id)
            .bind(resource_id)
            .bind(source_hash)
            .execute(&pool)
            .await
            .unwrap();
        }
        let mismatched_work_member = sqlx::query(
            "INSERT INTO content_work_identity_members (work_item_id, identity_id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, linked_at) VALUES ('global-model-e', 'identity-a', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'hash-1', 'protocol-1', CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await;
        assert!(mismatched_work_member.is_err());
        let mismatched_identity_member = sqlx::query(
            "INSERT INTO content_work_identity_members (work_item_id, identity_id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, linked_at) VALUES ('global-model-e', 'identity-a', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'hash-2', 'protocol-1', CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await;
        assert!(mismatched_identity_member.is_err());

        sqlx::query(
            "INSERT INTO content_current_result_projections (identity_id, work_item_id, active_work_item_id, source_projection_id, payload_json, published_at, updated_at) VALUES ('identity-a', 'global-model-a', 'global-model-c', 'projection-b', '{}', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mismatched_projection_provenance = sqlx::query(
            "INSERT INTO content_current_result_projections (identity_id, work_item_id, active_work_item_id, source_projection_id, payload_json, published_at, updated_at) VALUES ('identity-b', 'global-model-c', NULL, 'projection-a', '{}', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await;
        assert!(mismatched_projection_provenance.is_err());
        let mismatched_projection_resource = sqlx::query(
            "INSERT INTO content_current_result_projections (identity_id, work_item_id, active_work_item_id, source_projection_id, payload_json, published_at, updated_at) VALUES ('identity-c', 'global-model-d', NULL, 'projection-a', '{}', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await;
        assert!(mismatched_projection_resource.is_err());
        let mismatched_projection_work = sqlx::query(
            "INSERT INTO content_current_result_projections (identity_id, work_item_id, active_work_item_id, source_projection_id, payload_json, published_at, updated_at) VALUES ('identity-a', 'global-model-c', NULL, NULL, '{}', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await;
        assert!(mismatched_projection_work.is_err());
        let mismatched_active_work = sqlx::query(
            "INSERT INTO content_current_result_projections (identity_id, work_item_id, active_work_item_id, source_projection_id, payload_json, published_at, updated_at) VALUES ('identity-a', 'global-model-a', 'global-model-d', NULL, '{}', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await;
        assert!(mismatched_active_work.is_err());
        let invalid_active_work_update = sqlx::query(
            "UPDATE content_current_result_projections SET active_work_item_id = 'global-model-d' WHERE identity_id = 'identity-a'",
        )
        .execute(&pool)
        .await;
        assert!(invalid_active_work_update.is_err());
        let valid_active_work_update = sqlx::query(
            "UPDATE content_current_result_projections SET active_work_item_id = 'global-model-b' WHERE identity_id = 'identity-a'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(valid_active_work_update.rows_affected(), 1);
        let invalid_provenance_update = sqlx::query(
            "UPDATE content_current_result_projections SET source_projection_id = 'projection-c' WHERE identity_id = 'identity-a'",
        )
        .execute(&pool)
        .await;
        assert!(invalid_provenance_update.is_err());
        let valid_provenance_update = sqlx::query(
            "UPDATE content_current_result_projections SET source_projection_id = 'projection-a' WHERE identity_id = 'identity-a'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(valid_provenance_update.rows_affected(), 1);
        sqlx::query(
            "INSERT INTO content_current_result_projections (identity_id, work_item_id, active_work_item_id, source_projection_id, payload_json, published_at, updated_at) VALUES ('identity-b', 'global-model-c', NULL, NULL, '{}', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN ('idx_notifications_admin_canonical_source', 'idx_translation_work_items_admin_entity_kind_attempt')",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            2
        );
        let notification_index_sql: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'idx_notifications_admin_canonical_source'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(notification_index_sql.contains("(thread_id, updated_at DESC, id DESC)"));
        let work_item_index_sql: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'idx_translation_work_items_admin_entity_kind_attempt'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(work_item_index_sql.contains("(entity_id, kind, attempt_count DESC)"));
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT mode FROM content_processing_control WHERE id = 1",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            "legacy"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM content_legacy_observations")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        let mut tx = pool.begin().await.unwrap();
        assert!(
            transition_to_rollback_freeze_in_transaction(&mut tx, "freeze-1")
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        assert!(transition_to_global(&pool, "global-1").await.unwrap());
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM content_legacy_observations")
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT classification FROM content_legacy_observations WHERE legacy_table = 'translation_work_items'")
                .fetch_one(&pool)
                .await
                .unwrap(),
            "legacy_cached"
        );
    }

    #[tokio::test]
    async fn legacy_writer_is_frozen_in_rollback_and_global_modes() {
        for mode in ["rollback_freeze", "global"] {
            let pool = pool(mode).await;
            let error = ensure_legacy_writer(&pool)
                .await
                .expect_err("writer must be blocked");
            assert_eq!(error.status(), StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(error.code(), "content_processing_transition");
        }
    }

    #[tokio::test]
    async fn migration_preceding_migrator_is_rejected_after_global_schema() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::database_migrations::run(&pool).await.unwrap();

        let current_migrations = sqlx::migrate!("./migrations");
        let pre_cutover_migrator = sqlx::migrate::Migrator {
            migrations: Cow::Owned(
                current_migrations
                    .iter()
                    .filter(|migration| migration.version < CONTENT_PROCESSING_MIGRATION_VERSION)
                    .cloned()
                    .collect(),
            ),
            ignore_missing: false,
            locking: true,
            no_tx: false,
        };
        let error = pre_cutover_migrator
            .run(&pool)
            .await
            .expect_err("a migrator without 0078 must be rejected");
        assert!(matches!(
            error,
            sqlx::migrate::MigrateError::VersionMissing(CONTENT_PROCESSING_MIGRATION_VERSION)
        ));
    }

    #[tokio::test]
    async fn ready_projection_remains_readable_after_model_profile_changes() {
        let pool = global_pool().await;
        insert_test_work(&pool, "model-a-work", "ready", 0, None).await;
        sqlx::query(
            "INSERT INTO content_result_projections (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash, work_item_id, active_work_item_id, payload_json, published_at, updated_at) VALUES (?, 'release', 'release-1', 'translation', 'summary', 'zh-CN', ?, 'test-model', 'hash-1', ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .bind("model-a-projection")
        .bind(GLOBAL_PROTOCOL_VERSION)
        .bind("model-a-work")
        .bind("model-a-work")
        .bind(r#"{"title_zh":"保留标题","body_md":"保留摘要"}"#)
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let now = Utc::now().to_rfc3339();
        let identity_id =
            content_identity_upgrade::ensure_identity_for_work(&mut tx, "model-a-work", &now)
                .await
                .unwrap();
        content_identity_upgrade::ensure_current_projection_for_key(&mut tx, &identity_id, &now)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let state = global_state(pool);
        let (status, payload) = read_global_resource(
            &state,
            "release",
            "release-1",
            "translation",
            "summary",
            "hash-1",
        )
        .await
        .unwrap()
        .expect("ready projection should survive a model-profile change");
        assert_eq!(status, "ready");
        assert_eq!(payload["title_zh"], "保留标题");
        assert_eq!(payload["body_md"], "保留摘要");
    }

    #[tokio::test]
    async fn submission_reuses_a_ready_projection_created_by_another_model() {
        let pool = global_pool().await;
        let item = translations::TranslationRequestItemInput {
            producer_ref: "feed.auto_translate:release:release-1".to_owned(),
            kind: "release_summary".to_owned(),
            variant: "summary".to_owned(),
            entity_id: "release-1".to_owned(),
            target_lang: "zh-CN".to_owned(),
            max_wait_ms: 0,
            source_blocks: vec![translations::TranslationSourceBlock {
                slot: "title".to_owned(),
                text: "A release title".to_owned(),
            }],
            target_slots: vec!["title_zh".to_owned()],
        };
        let hash = source_hash(&item).unwrap();
        sqlx::query(
            "INSERT INTO content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, attempt_count, created_at, updated_at) VALUES ('old-model-work', 'release', 'release-1', 'translation', 'summary', 'zh-CN', ?, ?, 'retired-model', '{}', 'historical-config', 'ready', 0, 0, 1, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .bind(&hash)
        .bind(GLOBAL_PROTOCOL_VERSION)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO content_result_projections (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash, work_item_id, active_work_item_id, payload_json, published_at, updated_at) VALUES ('old-model-projection', 'release', 'release-1', 'translation', 'summary', 'zh-CN', ?, 'retired-model', ?, 'old-model-work', 'old-model-work', '{\"title_zh\":\"保留译文\",\"body_md\":\"保留正文\"}', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .bind(GLOBAL_PROTOCOL_VERSION)
        .bind(&hash)
        .execute(&pool)
        .await
        .unwrap();
        let state = global_state(pool.clone());

        let (status, response) = submit_item(&state, "user-1", "async", &item).await.unwrap();

        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(response.work_item_id, "old-model-work");
        assert_eq!(response.status, "ready");
        assert_eq!(response.result["title_zh"], "保留译文");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM content_work_items")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM content_attempt_events")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn older_source_cannot_displace_newer_source() {
        let pool = global_pool().await;
        let make_item = |producer_ref: &str, revision: &str, title: &str| {
            translations::TranslationRequestItemInput {
                producer_ref: producer_ref.to_owned(),
                kind: "release_summary".to_owned(),
                variant: "summary".to_owned(),
                entity_id: "ordered-release".to_owned(),
                target_lang: "zh-CN".to_owned(),
                max_wait_ms: 0,
                source_blocks: vec![
                    translations::TranslationSourceBlock {
                        slot: "source_observed_at".to_owned(),
                        text: revision.to_owned(),
                    },
                    translations::TranslationSourceBlock {
                        slot: "title".to_owned(),
                        text: title.to_owned(),
                    },
                ],
                target_slots: vec!["title_zh".to_owned()],
            }
        };
        let newer = make_item("feed.newer", "2026-01-02T00:00:00Z", "New title");
        let older = make_item("feed.older", "2026-01-01T00:00:00Z", "Old title");
        let state = global_state(pool.clone());
        let (newer_status, newer_response) = submit_item(&state, "user-1", "async", &newer)
            .await
            .unwrap();
        assert_eq!(newer_status, StatusCode::ACCEPTED);
        let (older_status, older_response) = submit_item(&state, "user-1", "async", &older)
            .await
            .unwrap();

        assert_eq!(older_status, StatusCode::CONFLICT);
        assert_eq!(older_response.work_item_id, newer_response.work_item_id);
        assert_eq!(older_response.status, "queued");
        assert_eq!(
            older_response.error.as_ref().unwrap()["code"],
            "content_processing_superseded"
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT status FROM content_work_items WHERE source_hash = ?",
            )
            .bind(source_hash(&older).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap(),
            "superseded"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_events e JOIN content_work_items w ON w.id = e.work_item_id WHERE w.source_hash = ? AND e.event_type = 'attempt_queued'",
            )
            .bind(source_hash(&older).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_work_admission_events e JOIN content_work_items w ON w.id = e.work_item_id WHERE w.source_hash = ? AND e.event_type = 'admission_rejected_superseded'",
            )
            .bind(source_hash(&older).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );

        let (repeated_status, repeated_response) = submit_item(&state, "user-1", "async", &older)
            .await
            .unwrap();
        assert_eq!(repeated_status, StatusCode::CONFLICT);
        assert_eq!(repeated_response.work_item_id, newer_response.work_item_id);
        assert_eq!(
            repeated_response.error.as_ref().unwrap()["code"],
            "content_processing_superseded"
        );
    }

    #[tokio::test]
    async fn submission_does_not_requeue_superseded_work() {
        let pool = global_pool().await;
        let old_item = translations::TranslationRequestItemInput {
            producer_ref: "sync.global.release:release-1:summary".to_owned(),
            kind: "release_summary".to_owned(),
            variant: "summary".to_owned(),
            entity_id: "release-1".to_owned(),
            target_lang: "zh-CN".to_owned(),
            max_wait_ms: 0,
            source_blocks: vec![
                translations::TranslationSourceBlock {
                    slot: "source_observed_at".to_owned(),
                    text: "2026-01-01T00:00:00Z".to_owned(),
                },
                translations::TranslationSourceBlock {
                    slot: "title".to_owned(),
                    text: "Old title".to_owned(),
                },
            ],
            target_slots: vec!["title_zh".to_owned()],
        };
        let mut new_item = old_item.clone();
        new_item.source_blocks[1].text = "New title".to_owned();
        new_item.source_blocks[0].text = "2026-01-02T00:00:00Z".to_owned();
        let old_snapshot = serde_json::to_string(&json!({
            "source_blocks": old_item.source_blocks,
            "target_slots": old_item.target_slots,
        }))
        .unwrap();
        let new_snapshot = serde_json::to_string(&json!({
            "source_blocks": new_item.source_blocks,
            "target_slots": new_item.target_slots,
        }))
        .unwrap();
        let old_hash = source_hash(&old_item).unwrap();
        let new_hash = source_hash(&new_item).unwrap();
        for (id, hash, snapshot, status, retry_expires_at, created_at) in [
            (
                "superseded-old",
                old_hash.as_str(),
                old_snapshot.as_str(),
                "superseded",
                Some("2026-01-03T00:00:00Z"),
                "2026-01-01T00:00:00Z",
            ),
            (
                "superseded-new",
                new_hash.as_str(),
                new_snapshot.as_str(),
                "queued",
                None,
                "2026-01-02T00:00:00Z",
            ),
        ] {
            sqlx::query(
                "INSERT INTO content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, attempt_count, retry_expires_at, created_at, updated_at) VALUES (?, 'release', 'release-1', 'translation', 'summary', 'zh-CN', ?, ?, 'test-model', ?, 'config-1', ?, 0, 0, 1, 0, ?, ?, ?)",
            )
            .bind(id)
            .bind(hash)
            .bind(GLOBAL_PROTOCOL_VERSION)
            .bind(snapshot)
            .bind(status)
            .bind(retry_expires_at)
            .bind(created_at)
            .bind(created_at)
            .execute(&pool)
            .await
            .unwrap();
        }
        let mut tx = pool.begin().await.unwrap();
        content_identity_upgrade::ensure_identity_for_work(
            &mut tx,
            "superseded-old",
            "2026-01-02T00:00:00Z",
        )
        .await
        .unwrap();
        content_identity_upgrade::ensure_identity_for_work(
            &mut tx,
            "superseded-new",
            "2026-01-02T00:00:00Z",
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let state = global_state(pool.clone());
        let (status, response) = submit_item(&state, "user-1", "async", &old_item)
            .await
            .unwrap();

        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(response.work_item_id, "superseded-new");
        assert_eq!(response.status, "queued");
        assert_eq!(
            response.error.as_ref().unwrap()["code"],
            "content_processing_superseded"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_events WHERE work_item_id = 'superseded-old' AND event_type = 'attempt_queued'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT retry_expires_at FROM content_work_items WHERE id = 'superseded-old'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            "2026-01-03T00:00:00Z"
        );
    }

    #[tokio::test]
    async fn execute_checks_supersession_before_provider_call() {
        let pool = global_execution_pool().await;
        seed_executable_work(&pool, "stale-execution-old", &["title_zh"]).await;
        sqlx::query(
            "UPDATE content_work_items SET source_snapshot_json = ?, created_at = ?, updated_at = ? WHERE id = ?",
        )
        .bind(
            json!({
                "source_blocks": [{"slot": "source_observed_at", "text": "2026-01-01T00:00:00Z"}, {"slot": "title", "text": "Old title"}],
                "target_slots": ["title_zh"]
            })
            .to_string(),
        )
        .bind("2026-01-01T00:00:00Z")
        .bind("2026-01-01T00:00:00Z")
        .bind("stale-execution-old")
        .execute(&pool)
        .await
        .unwrap();
        let (base_url, requested_tokens, _) = spawn_sequenced_test_ai_server(vec![(
            r#"{"title_zh":"should not be called"}"#,
            "stop",
        )])
        .await;
        let mut state = global_state(pool.clone());
        Arc::get_mut(&mut state)
            .unwrap()
            .config
            .ai
            .as_mut()
            .unwrap()
            .base_url = base_url;

        let work = claim_next(&state, 1).await.unwrap().unwrap();
        assert_eq!(work.id, "stale-execution-old");
        sqlx::query(
            "INSERT INTO content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, attempt_count, supersedes_work_item_id, created_at, updated_at) VALUES ('stale-execution-new', 'release', '12345', 'translation', 'summary', 'zh-CN', 'new-source-hash', ?, 'test-model', ?, 'test-fingerprint', 'queued', 0, 0, 1, 0, 'stale-execution-old', ?, ?)",
        )
        .bind(GLOBAL_PROTOCOL_VERSION)
        .bind(
            json!({
                "source_blocks": [{"slot": "source_observed_at", "text": "2026-01-02T00:00:00Z"}, {"slot": "title", "text": "New title"}],
                "target_slots": ["title_zh"]
            })
            .to_string(),
        )
        .bind("2026-01-02T00:00:00Z")
        .bind("2026-01-02T00:00:00Z")
        .execute(&pool)
        .await
        .unwrap();
        let attempt_no = work.attempt_count;
        execute(&state, work).await.unwrap();

        assert!(requested_tokens.lock().unwrap().is_empty());
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT status FROM content_work_items WHERE id = 'stale-execution-old'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            "superseded"
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT result_status FROM content_attempt_events WHERE work_item_id = 'stale-execution-old' AND attempt_no = ? AND event_type = 'attempt_completed'",
            )
            .bind(attempt_no)
            .fetch_one(&pool)
            .await
            .unwrap(),
            "superseded"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_provider_admissions WHERE work_item_id = 'stale-execution-old'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn retry_of_superseded_work_points_to_current_work() {
        let pool = global_pool().await;
        for (id, hash, revision, status) in [
            (
                "retry-superseded-old",
                "retry-old-hash",
                "2026-01-01T00:00:00Z",
                "superseded",
            ),
            (
                "retry-superseded-new",
                "retry-new-hash",
                "2026-01-02T00:00:00Z",
                "queued",
            ),
        ] {
            sqlx::query(
                "INSERT INTO content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, attempt_count, retry_after_at, retry_expires_at, created_at, updated_at) VALUES (?, 'release', 'retry-release', 'translation', 'summary', 'zh-CN', ?, ?, 'test-model', ?, ?, ?, 0, 0, 1, 0, '2099-01-01T00:00:00Z', '2099-01-02T00:00:00Z', ?, ?)",
            )
            .bind(id)
            .bind(hash)
            .bind(GLOBAL_PROTOCOL_VERSION)
            .bind(
                json!({
                    "source_blocks": [{"slot": "source_observed_at", "text": revision}, {"slot": "title", "text": id}],
                    "target_slots": ["title_zh"]
                })
                .to_string(),
            )
            .bind("test-fingerprint")
            .bind(status)
            .bind(revision)
            .bind(revision)
            .execute(&pool)
            .await
            .unwrap();
        }
        let mut tx = pool.begin().await.unwrap();
        content_identity_upgrade::ensure_identity_for_work(
            &mut tx,
            "retry-superseded-old",
            "2026-01-02T00:00:00Z",
        )
        .await
        .unwrap();
        content_identity_upgrade::ensure_identity_for_work(
            &mut tx,
            "retry-superseded-new",
            "2026-01-02T00:00:00Z",
        )
        .await
        .unwrap();
        insert_request_link(
            &mut tx,
            "retry-superseded-request",
            "retry-superseded-old",
            "user-1",
            "async",
            "global.release.summary",
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let state = global_state(pool.clone());
        let (status, response) = retry_request(&state, "user-1", "retry-superseded-request")
            .await
            .unwrap();

        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(response["error"]["code"], "content_processing_superseded");
        assert_eq!(
            response["error"]["superseded_work_item_id"],
            "retry-superseded-old"
        );
        assert_eq!(
            response["error"]["current_work_item_id"],
            "retry-superseded-new"
        );
        assert_eq!(response["work_item_id"], "retry-superseded-new");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_events WHERE work_item_id = 'retry-superseded-old' AND event_type = 'attempt_queued'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_work_admission_events WHERE work_item_id = 'retry-superseded-old' AND event_type = 'admission_rejected_superseded' AND replaced_by_work_item_id = 'retry-superseded-new'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn admitted_call_finishes_without_publishing_superseded_output() {
        let pool = global_execution_pool().await;
        seed_executable_work(&pool, "admitted-race-old", &["title_zh"]).await;
        sqlx::query(
            "UPDATE repo_releases SET updated_at = '2026-01-01T00:00:00Z' WHERE release_id = 12345",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE content_work_items SET source_snapshot_json = ?, created_at = ?, updated_at = ? WHERE id = ?",
        )
        .bind(
            json!({
                "source_blocks": [{"slot": "source_observed_at", "text": "2026-01-01T00:00:00Z"}, {"slot": "title", "text": "Old title"}],
                "target_slots": ["title_zh"]
            })
            .to_string(),
        )
        .bind("2026-01-01T00:00:00Z")
        .bind("2026-01-01T00:00:00Z")
        .bind("admitted-race-old")
        .execute(&pool)
        .await
        .unwrap();
        let request_seen = Arc::new(Notify::new());
        let server_request_seen = request_seen.clone();
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move |Json(_request): Json<Value>| {
                let request_seen = server_request_seen.clone();
                async move {
                    request_seen.notify_one();
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    (
                        StatusCode::OK,
                        Json(json!({
                            "id": "admitted-race-provider-call",
                            "choices": [{
                                "message": {"content": "{\"title_zh\":\"old result\"}"},
                                "finish_reason": "stop"
                            }],
                            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
                        })),
                    )
                }
            }),
        );
        let base_url = spawn_test_ai_server(app).await;
        let mut state = global_state(pool.clone());
        Arc::get_mut(&mut state)
            .unwrap()
            .config
            .ai
            .as_mut()
            .unwrap()
            .base_url = base_url;
        let work = claim_next(&state, 1).await.unwrap().unwrap();
        let attempt_no = work.attempt_count;
        let execution = tokio::spawn({
            let state = state.clone();
            async move { execute(&state, work).await }
        });
        request_seen.notified().await;
        sqlx::query(
            "INSERT INTO content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, attempt_count, supersedes_work_item_id, created_at, updated_at) VALUES ('admitted-race-new', 'release', '12345', 'translation', 'summary', 'zh-CN', 'admitted-new-hash', ?, 'test-model', ?, 'test-fingerprint', 'queued', 0, 0, 1, 0, 'admitted-race-old', ?, ?)",
        )
        .bind(GLOBAL_PROTOCOL_VERSION)
        .bind(
            json!({
                "source_blocks": [{"slot": "source_observed_at", "text": "2026-01-02T00:00:00Z"}, {"slot": "title", "text": "New title"}],
                "target_slots": ["title_zh"]
            })
            .to_string(),
        )
        .bind("2026-01-02T00:00:00Z")
        .bind("2026-01-02T00:00:00Z")
        .execute(&pool)
        .await
        .unwrap();
        execution.await.unwrap().unwrap();

        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT status FROM content_work_items WHERE id = 'admitted-race-old'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            "superseded"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_provider_admissions WHERE work_item_id = 'admitted-race-old' AND call_ordinal = 0",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_llm_calls c JOIN content_attempt_events e ON e.id = c.attempt_event_id WHERE e.work_item_id = 'admitted-race-old' AND e.attempt_no = ?",
            )
            .bind(attempt_no)
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_result_projections WHERE work_item_id = 'admitted-race-old'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT retry_eligible FROM content_attempt_events WHERE work_item_id = 'admitted-race-old' AND attempt_no = ? AND event_type = 'attempt_completed'",
            )
            .bind(attempt_no)
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn startup_reconciliation_is_idempotent_and_provider_free() {
        let pool = global_pool().await;
        for (id, hash, revision, status, created_at) in [
            (
                "startup-old",
                "startup-old-hash",
                "2026-01-01T00:00:00Z",
                "queued",
                "2026-01-01T00:00:00Z",
            ),
            (
                "startup-new",
                "startup-new-hash",
                "2026-01-02T00:00:00Z",
                "ready",
                "2026-01-02T00:00:00Z",
            ),
        ] {
            sqlx::query(
                "INSERT INTO content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, attempt_count, created_at, updated_at) VALUES (?, 'release', 'startup-release', 'translation', 'summary', 'zh-CN', ?, ?, 'test-model', ?, 'test-fingerprint', ?, 0, 0, 1, 0, ?, ?)",
            )
            .bind(id)
            .bind(hash)
            .bind(GLOBAL_PROTOCOL_VERSION)
            .bind(json!({
                "source_blocks": [{"slot": "source_observed_at", "text": revision}, {"slot": "title", "text": id}],
                "target_slots": ["title_zh"]
            }).to_string())
            .bind(status)
            .bind(created_at)
            .bind(created_at)
            .execute(&pool)
            .await
            .unwrap();
        }
        let mut tx = pool.begin().await.unwrap();
        assert_eq!(
            supersede_stale_work_in_transaction(&mut tx).await.unwrap(),
            1
        );
        tx.commit().await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert_eq!(
            supersede_stale_work_in_transaction(&mut tx).await.unwrap(),
            0
        );
        tx.commit().await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT status FROM content_work_items WHERE id = 'startup-old'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            "superseded"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_work_admission_events WHERE work_item_id = 'startup-old' AND event_type = 'reconciliation_superseded'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn submission_uses_persisted_routes_when_the_local_scheduler_is_stale() {
        let pool = global_pool().await;
        sqlx::query(
            "CREATE TABLE admin_runtime_settings (id INTEGER PRIMARY KEY, llm_models_json TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO admin_runtime_settings (id, llm_models_json) VALUES (1, '[\"persisted-route\"]')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut state = global_state(pool.clone());
        Arc::get_mut(&mut state)
            .expect("state has a single owner")
            .config
            .ai
            .as_mut()
            .expect("AI configuration")
            .model
            .clear();
        let item = translations::TranslationRequestItemInput {
            producer_ref: "feed.auto_translate:release:release-1".to_owned(),
            kind: "release_summary".to_owned(),
            variant: "summary".to_owned(),
            entity_id: "release-1".to_owned(),
            target_lang: "zh-CN".to_owned(),
            max_wait_ms: 0,
            source_blocks: vec![translations::TranslationSourceBlock {
                slot: "title".to_owned(),
                text: "A release title".to_owned(),
            }],
            target_slots: vec!["title_zh".to_owned()],
        };

        let (http_status, response) = submit_item(&state, "user-1", "async", &item).await.unwrap();
        let (stored_status, model_profile): (String, String) =
            sqlx::query_as("SELECT status, model_profile FROM content_work_items WHERE id = ?")
                .bind(&response.work_item_id)
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(http_status, StatusCode::ACCEPTED);
        assert_eq!(response.status, "queued");
        assert_eq!(stored_status, "queued");
        assert_eq!(model_profile, "persisted-route");
    }

    #[tokio::test]
    async fn runtime_configuration_reload_requeues_blocked_identity_once() {
        let pool = global_pool().await;
        insert_test_work(&pool, "blocked-work", "blocked_config", 1, None).await;
        let mut state = global_state(pool.clone());
        let valid_ai = state.config.ai.clone().unwrap();
        Arc::get_mut(&mut state)
            .expect("state has a single owner")
            .config
            .ai = None;
        let now = Utc::now().to_rfc3339();
        let mut tx = pool.begin().await.unwrap();
        content_identity_upgrade::ensure_identity_for_work(&mut tx, "blocked-work", &now)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        on_runtime_configuration_reload(&state).await.unwrap();

        let status: String =
            sqlx::query_scalar("SELECT status FROM content_work_items WHERE id = 'blocked-work'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "blocked_config");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_events WHERE work_item_id = 'blocked-work'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        Arc::get_mut(&mut state)
            .expect("state has a single owner")
            .config
            .ai = Some(valid_ai);
        on_runtime_configuration_reload(&state).await.unwrap();
        let status: String =
            sqlx::query_scalar("SELECT status FROM content_work_items WHERE id = 'blocked-work'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "queued");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_events WHERE work_item_id = 'blocked-work' AND event_type = 'attempt_queued' AND trigger = 'automatic_recovery'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_events WHERE work_item_id = 'blocked-work' AND event_type = 'attempt_started'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        on_runtime_configuration_reload(&state).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_events WHERE work_item_id = 'blocked-work' AND event_type = 'attempt_queued' AND trigger = 'automatic_recovery'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn runtime_configuration_reload_revalidates_persisted_routes_before_requeue() {
        let pool = global_pool().await;
        sqlx::query(
            "CREATE TABLE admin_runtime_settings (id INTEGER PRIMARY KEY, llm_models_json TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO admin_runtime_settings (id, llm_models_json) VALUES (1, '[]')")
            .execute(&pool)
            .await
            .unwrap();
        insert_test_work(&pool, "blocked-work", "blocked_config", 1, None).await;
        let now = Utc::now().to_rfc3339();
        let mut tx = pool.begin().await.unwrap();
        content_identity_upgrade::ensure_identity_for_work(&mut tx, "blocked-work", &now)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let mut state = global_state(pool.clone());
        Arc::get_mut(&mut state)
            .expect("state has a single owner")
            .config
            .ai
            .as_mut()
            .expect("AI configuration")
            .model
            .clear();
        assert!(
            crate::admin_runtime::default_llm_models(&state.config).is_empty(),
            "test requires no legacy environment model override"
        );
        state
            .llm_scheduler
            .set_model_routing(vec!["stale-route".to_owned()])
            .await;
        assert!(has_valid_runtime_configuration(&state).await);

        on_runtime_configuration_reload(&state).await.unwrap();

        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT status FROM content_work_items WHERE id = 'blocked-work'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            "blocked_config"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_events WHERE work_item_id = 'blocked-work' AND event_type = 'attempt_queued' AND trigger = 'automatic_recovery'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn claim_snapshots_the_persisted_route_under_the_claim_transaction() {
        let pool = global_pool().await;
        sqlx::query(
            "CREATE TABLE admin_runtime_settings (id INTEGER PRIMARY KEY, llm_models_json TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO admin_runtime_settings (id, llm_models_json) VALUES (1, '[\"current-route\"]')",
        )
        .execute(&pool)
        .await
        .unwrap();
        insert_test_work(&pool, "route-work", "queued", 0, None).await;
        sqlx::query(
            "UPDATE content_work_items SET created_at = '2000-01-01T00:00:00Z' WHERE id = 'route-work'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let state = global_state(pool.clone());

        let claimed = claim_next(&state, 1).await.unwrap().unwrap();

        assert_eq!(
            claimed.attempt_route_snapshot_json.as_deref(),
            Some("[\"current-route\"]")
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT route_snapshot_json FROM content_attempt_events WHERE work_item_id = 'route-work' AND attempt_no = 1 AND event_type = 'attempt_started'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            r#"["current-route"]"#
        );
    }

    #[tokio::test]
    async fn retained_projection_survives_chained_source_refreshes() {
        let pool = global_pool().await;
        insert_test_work(&pool, "refresh-w1", "ready", 0, None).await;
        for (id, hash, supersedes, status) in [
            ("refresh-w2", "hash-2", Some("refresh-w1"), "queued"),
            ("refresh-w3", "hash-3", Some("refresh-w2"), "queued"),
        ] {
            sqlx::query(
                "INSERT INTO content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, supersedes_work_item_id, attempt_count, created_at, updated_at) VALUES (?, 'release', 'release-1', 'translation', 'summary', 'zh-CN', ?, ?, 'test-model', '{}', 'config-1', ?, 0, 0, 1, ?, 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            )
            .bind(id)
            .bind(hash)
            .bind(GLOBAL_PROTOCOL_VERSION)
            .bind(status)
            .bind(supersedes)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO content_result_projections (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash, work_item_id, active_work_item_id, payload_json, published_at, updated_at) VALUES (?, 'release', 'release-1', 'translation', 'summary', 'zh-CN', ?, 'test-model', 'hash-1', ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .bind("refresh-projection")
        .bind(GLOBAL_PROTOCOL_VERSION)
        .bind("refresh-w1")
        .bind("refresh-w3")
        .bind(r#"{"title_zh":"连续刷新仍可见","body_md":"保留旧摘要"}"#)
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let now = Utc::now().to_rfc3339();
        let identity_id =
            content_identity_upgrade::ensure_identity_for_work(&mut tx, "refresh-w1", &now)
                .await
                .unwrap();
        content_identity_upgrade::ensure_identity_for_work(&mut tx, "refresh-w2", &now)
            .await
            .unwrap();
        content_identity_upgrade::ensure_identity_for_work(&mut tx, "refresh-w3", &now)
            .await
            .unwrap();
        content_identity_upgrade::ensure_current_projection_for_key(&mut tx, &identity_id, &now)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let state = global_state(pool);
        let (status, payload) = read_global_resource(
            &state,
            "release",
            "release-1",
            "translation",
            "summary",
            "hash-3",
        )
        .await
        .unwrap()
        .expect("retained projection should survive chained refreshes");
        assert_eq!(status, "queued");
        assert_eq!(payload["title_zh"], "连续刷新仍可见");
        assert_eq!(payload["body_md"], "保留旧摘要");
    }

    #[test]
    fn transition_error_maps_to_pollable_service_unavailable() {
        let error = api_error_from_anyhow(transition_error(ContentProcessingMode::RollbackFreeze));
        assert_eq!(error.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(error.code(), "content_processing_transition");
    }

    #[tokio::test]
    async fn manual_retry_reuses_next_attempt_and_claims_it_once() {
        let pool = global_pool().await;
        insert_test_work(&pool, "work-1", "failed", 1, None).await;
        sqlx::query("INSERT INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, created_at) VALUES ('attempt-1', 'work-1', 1, 'initial', 'attempt_completed', 'failed', CURRENT_TIMESTAMP)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO content_request_links (id, request_id, work_item_id, requester_type, requester_id, authorization_snapshot_json, producer_ref, request_source, delivery_mode, created_at, updated_at) VALUES ('link-1', 'request-1', 'work-1', 'user', 'user-1', '{}', 'test', 'api', 'async', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)")
            .execute(&pool)
            .await
            .unwrap();
        let state = global_state(pool.clone());

        let (status, body) = retry_request(&state, "user-1", "request-1").await.unwrap();
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(body["status"], "queued");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT attempt_no FROM content_attempt_events WHERE work_item_id = 'work-1' AND event_type = 'attempt_queued'")
                .fetch_one(&pool)
                .await
                .unwrap(),
            2
        );

        let claimed = claim_next(&state, 1).await.unwrap().unwrap();
        assert_eq!(claimed.attempt_count, 2);
        let attempt_snapshot: (String, String, String) = sqlx::query_as(
            "SELECT configuration_snapshot_json, route_snapshot_json, configuration_fingerprint FROM content_attempt_events WHERE work_item_id = 'work-1' AND attempt_no = 2 AND event_type = 'attempt_started'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let configuration: Value = serde_json::from_str(&attempt_snapshot.0).unwrap();
        assert_eq!(configuration["base_url_origin"], "https://ai.example.test");
        assert!(configuration.get("base_url").is_none());
        assert!(configuration["api_key_sha256"].is_string());
        assert!(!attempt_snapshot.0.contains("test-api-key"));
        assert_eq!(attempt_snapshot.1, r#"["test-model"]"#);
        assert_eq!(
            claimed.attempt_configuration_fingerprint.as_deref(),
            Some(attempt_snapshot.2.as_str())
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM content_attempt_events WHERE work_item_id = 'work-1' AND attempt_no = 2 AND event_type = 'attempt_started'")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn automatic_recovery_normalizes_zero_attempts_to_one() {
        let pool = global_pool().await;
        insert_test_work(&pool, "work-2", "failed", 0, Some("2000-01-01T00:00:00Z")).await;
        sqlx::query(
            "UPDATE content_work_items SET created_at = '2000-01-01T00:00:00Z' WHERE id = 'work-2'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let state = global_state(pool.clone());

        recover_due(&state).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT attempt_no FROM content_attempt_events WHERE work_item_id = 'work-2' AND event_type = 'attempt_queued'")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        let claimed = claim_next(&state, 1).await.unwrap().unwrap();
        assert_eq!(claimed.attempt_count, 1);
    }

    #[tokio::test]
    async fn expired_manual_retry_reopens_the_retry_window() {
        let pool = global_pool().await;
        insert_test_work(&pool, "work-expired", "failed", 1, None).await;
        sqlx::query(
            "UPDATE content_work_items SET retry_expires_at = '2000-01-01T00:00:00Z' WHERE id = 'work-expired'",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, created_at) VALUES ('attempt-expired', 'work-expired', 1, 'initial', 'attempt_completed', 'failed', CURRENT_TIMESTAMP)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO content_request_links (id, request_id, work_item_id, requester_type, requester_id, authorization_snapshot_json, producer_ref, request_source, delivery_mode, created_at, updated_at) VALUES ('link-expired', 'request-expired', 'work-expired', 'user', 'user-1', '{}', 'test', 'api', 'async', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)")
            .execute(&pool)
            .await
            .unwrap();
        let state = global_state(pool.clone());

        let (status, _) = retry_request(&state, "user-1", "request-expired")
            .await
            .unwrap();
        assert_eq!(status, StatusCode::ACCEPTED);
        let retry_expires_at: String = sqlx::query_scalar(
            "SELECT retry_expires_at FROM content_work_items WHERE id = 'work-expired'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(parse_storage_timestamp(&retry_expires_at).unwrap() > Utc::now());
        assert_eq!(
            claim_next(&state, 1).await.unwrap().unwrap().attempt_count,
            2
        );
    }

    #[tokio::test]
    async fn provider_cooldown_expiry_closes_the_existing_attempt_audit() {
        let pool = global_pool().await;
        insert_test_work(
            &pool,
            "work-provider-expired",
            "deferred_provider",
            1,
            Some("2000-01-01T00:00:00Z"),
        )
        .await;
        sqlx::query("UPDATE content_work_items SET retry_expires_at = '2000-01-01T00:00:00Z' WHERE id = 'work-provider-expired'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, created_at) VALUES ('provider-started', 'work-provider-expired', 1, 'initial', 'attempt_started', NULL, CURRENT_TIMESTAMP), ('provider-completed', 'work-provider-expired', 1, 'initial', 'attempt_completed', 'deferred_provider', CURRENT_TIMESTAMP)")
            .execute(&pool)
            .await
            .unwrap();
        let state = global_state(pool.clone());

        recover_due(&state).await.unwrap();
        let audit: (String, String, String) = sqlx::query_as("SELECT result_status, error_code, failure_class FROM content_attempt_events WHERE id = 'provider-completed'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            audit,
            (
                "failed".to_owned(),
                "provider_unavailable".to_owned(),
                "provider_unavailable".to_owned()
            )
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM content_attempt_events WHERE work_item_id = 'work-provider-expired' AND event_type = 'attempt_completed'")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    }

    #[test]
    fn bodyless_detail_output_accepts_a_null_body() {
        let output = validate_output(
            r#"{"title_zh":"标题","body_md":null}"#,
            &["title_zh".to_owned(), "body_md".to_owned()],
            &[translations::TranslationSourceBlock {
                slot: "title".to_owned(),
                text: "Title".to_owned(),
            }],
        )
        .expect("bodyless detail output is valid");
        assert_eq!(output["body_md"], Value::Null);
    }

    #[test]
    fn build_prompt_requires_direct_declared_slots() {
        let snapshot = SourceSnapshot {
            source_blocks: vec![],
            target_slots: vec!["title_zh".to_owned()],
        };
        let (system, user) = build_prompt(&snapshot, "translation");
        assert!(system.contains("顶层必须直接包含 target_slots"));
        assert!(user.contains("target_slots"));
        assert!(user.contains("response_contract"));
        assert!(user.contains("extra scalar metadata is ignored"));
        assert!(!user.contains("\"output\": {"));
    }

    #[test]
    fn output_normalization_accepts_one_fence_or_envelope() {
        let target_slots = ["title_zh".to_owned()];
        let source_blocks = [];
        for raw in [
            r#"{"title_zh":"标题"}"#,
            "```json\n{\"title_zh\":\"标题\"}\n```",
            r#"{"output":{"title_zh":"标题"}}"#,
        ] {
            let output = validate_output(raw, &target_slots, &source_blocks)
                .expect("supported output wrapper should normalize");
            assert_eq!(output, json!({"title_zh": "标题"}));
        }
    }

    #[test]
    fn output_normalization_rejects_ambiguous_or_nested_envelopes() {
        let target_slots = ["title_zh".to_owned()];
        let source_blocks = [];
        for raw in [
            r#"{"title_zh":"标题","output":{"title_zh":"另一个标题"}}"#,
            r#"{"title_zh":"第一个标题","title_zh":"第二个标题"}"#,
            r#"{"output":{"output":{"title_zh":"标题"}}}"#,
            r#"{"output":{"title_zh":"第一个标题","title_zh":"第二个标题"}}"#,
            r#"{"output":{"title_zh":"标题"},"output":{"title_zh":"另一个标题"}}"#,
            r#"{"title_zh":"标题","result":{"title_zh":"另一个标题"}}"#,
            r#"{"title_zh":"标题","data":[]}"#,
            r#"{"output":{"title_zh":"标题","result":{"title_zh":"另一个标题"}}}"#,
            "```json\n{\"output\":{\"title_zh\":\"标题\"}}\n```",
            "```json\n{\"title_zh\":\"标题\"}",
        ] {
            assert!(validate_output(raw, &target_slots, &source_blocks).is_err());
        }
    }

    #[test]
    fn bodyless_detail_output_rejects_non_text_body() {
        let error = validate_output(
            r#"{"title_zh":"标题","body_md":{}}"#,
            &["title_zh".to_owned(), "body_md".to_owned()],
            &[translations::TranslationSourceBlock {
                slot: "title".to_owned(),
                text: "Title".to_owned(),
            }],
        )
        .expect_err("bodyless body must remain null or text");
        assert!(error.to_string().contains("null or non-empty text"));
    }

    #[test]
    fn output_validation_drops_untrusted_control_metadata() {
        let output = validate_output(
            r#"{"title_zh":"标题","status":"ready","work_item_id":"attacker","batch_id":"attacker"}"#,
            &["title_zh".to_owned()],
            &[],
        )
        .expect("declared output is valid");
        assert_eq!(output, json!({"title_zh": "标题"}));
    }

    #[test]
    fn output_validation_rejects_markdown_structure_loss() {
        let error = validate_output(
            r#"{"body_md":"plain text"}"#,
            &["body_md".to_owned()],
            &[translations::TranslationSourceBlock {
                slot: "body_markdown".to_owned(),
                text: "- one\n- two".to_owned(),
            }],
        )
        .expect_err("markdown structure must be preserved");
        assert!(error.to_string().contains("preserve markdown structure"));
    }

    #[test]
    fn source_observed_at_is_ordering_metadata_not_content_identity() {
        let item = translations::TranslationRequestItemInput {
            producer_ref: "test".to_owned(),
            kind: "release_summary".to_owned(),
            variant: "summary".to_owned(),
            entity_id: "release-1".to_owned(),
            target_lang: "zh-CN".to_owned(),
            max_wait_ms: 0,
            source_blocks: vec![
                translations::TranslationSourceBlock {
                    slot: "source_observed_at".to_owned(),
                    text: "2026-01-01T00:00:00Z".to_owned(),
                },
                translations::TranslationSourceBlock {
                    slot: "title".to_owned(),
                    text: "Release".to_owned(),
                },
            ],
            target_slots: vec!["title_zh".to_owned()],
        };
        let mut later = item.clone();
        later.source_blocks[0].text = "2026-01-01T00:01:00Z".to_owned();
        assert_eq!(source_hash(&item).unwrap(), source_hash(&later).unwrap());
        assert!(
            source_observed_at(
                &serde_json::to_string(&json!({
                    "source_blocks": item.source_blocks,
                    "target_slots": item.target_slots,
                }))
                .unwrap()
            )
            .is_some()
        );
    }

    #[test]
    fn equal_source_revision_timestamp_uses_authoritative_tiebreak() {
        let older = json!({
            "source_blocks": [
                {"slot": "source_observed_at", "text": "2026-01-01T00:00:00Z"},
                {"slot": "source_revision_tiebreak", "text": "0001"},
            ]
        })
        .to_string();
        let newer = json!({
            "source_blocks": [
                {"slot": "source_observed_at", "text": "2026-01-01T00:00:00Z"},
                {"slot": "source_revision_tiebreak", "text": "0002"},
            ]
        })
        .to_string();
        assert!(source_version_is_newer(&newer, &older,));
        assert!(!source_version_is_newer(&older, &newer,));
        assert!(!source_version_is_newer(&newer, &newer,));
        let legacy = json!({"source_blocks": [{"slot": "title", "text": "legacy"}]}).to_string();
        assert!(source_version_is_newer(&newer, &legacy));
        assert!(!source_version_is_newer(&legacy, &newer));
        let numeric_10 = json!({
            "source_blocks": [
                {"slot": "source_observed_at", "text": "2026-01-01T00:00:00Z"},
                {"slot": "source_revision_tiebreak", "text": "10"},
            ]
        })
        .to_string();
        let numeric_9 = numeric_10.replace("\"10\"", "\"9\"");
        assert!(source_version_is_newer(&numeric_10, &numeric_9));
    }

    #[test]
    fn automatic_retry_uses_the_last_delay_until_expiry() {
        let now = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let next = next_retry_at_for_failure(6, Some("2026-01-02T00:00:00Z"), true, now)
            .expect("sixth attempt remains retryable");
        assert_eq!(next, "2026-01-01T04:00:00+00:00");
        assert!(next_retry_at_for_failure(6, Some("2025-12-31T23:59:59Z"), true, now,).is_none());
        assert!(next_retry_at_for_failure(6, Some("2026-01-01T04:00:00Z"), true, now,).is_none());
        assert_eq!(
            next_retry_at_for_failure(6, Some("2026-01-01T04:00:01Z"), true, now,),
            Some("2026-01-01T04:00:00+00:00".to_owned())
        );
    }
}
