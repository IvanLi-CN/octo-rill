use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow};
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Error as SqlxError, Row, Sqlite, SqlitePool, Transaction};
use tokio::{task::JoinSet, time::sleep};
use tracing::warn;

use crate::{ai, api, error::ApiError, local_id, state::AppState, translations};
use tower_sessions::Session;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentProcessingMode {
    Legacy,
    RollbackFreeze,
    Global,
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
                    "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = 78",
                )
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
    ))
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
                    "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = 78",
                )
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
                "SELECT COUNT(*) FROM _sqlx_migrations WHERE version = 78",
            )
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
            "INSERT OR IGNORE INTO content_legacy_observations (id, legacy_table, legacy_primary_key, canonical_resource_type, canonical_resource_id, pipeline, classification, observation_basis_json, observed_at) SELECT 'legacy-cache-' || id, 'ai_translations', id, CASE WHEN entity_type LIKE 'release%' THEN 'release' WHEN entity_type LIKE 'announcement%' THEN 'announcement' WHEN entity_type IN ('notification', 'notification_smart') THEN 'notification' ELSE NULL END, entity_id, CASE WHEN entity_type LIKE '%smart' THEN 'polishing' ELSE 'translation' END, CASE WHEN status = 'ready' AND (title IS NOT NULL OR summary IS NOT NULL) THEN 'legacy_cached' ELSE 'legacy_conflict' END, '{\"source\":\"ai_translations\",\"status\":\"' || replace(status, '\"', '') || '\"}' , CURRENT_TIMESTAMP FROM ai_translations",
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
        sqlx::query(
            "INSERT OR IGNORE INTO content_legacy_observations (id, legacy_table, legacy_primary_key, canonical_resource_type, canonical_resource_id, pipeline, classification, observation_basis_json, observed_at) SELECT 'legacy-work-' || id, 'translation_work_items', id, CASE WHEN kind LIKE 'release%' THEN 'release' WHEN kind LIKE 'announcement%' THEN 'announcement' WHEN kind IN ('notification', 'notification_smart') THEN 'notification' ELSE NULL END, entity_id, CASE WHEN kind LIKE '%smart' THEN 'polishing' ELSE 'translation' END, 'legacy_conflict', '{\"source\":\"translation_work_items\",\"status\":\"' || replace(COALESCE(status, ''), '\"', '') || '\"}' , CURRENT_TIMESTAMP FROM translation_work_items",
        )
        .execute(&mut **tx)
        .await?;
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
}

const GLOBAL_PROTOCOL_VERSION: &str = "content-processing.v1";
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
    let source = serde_json::to_string(&json!({
        "kind": item.kind,
        "variant": item.variant,
        "entity_id": item.entity_id,
        "target_lang": item.target_lang,
        "source_blocks": item.source_blocks,
        "target_slots": item.target_slots,
    }))?;
    Ok(ai::sha256_hex(&format!(
        "{GLOBAL_PROTOCOL_VERSION}\n{source}"
    )))
}

pub(crate) fn source_hash_for_item(item: &translations::TranslationRequestItemInput) -> String {
    source_hash(item).expect("translation request source fields are serializable")
}

fn runtime_configuration_fingerprint(state: &AppState, model_profile: &str) -> String {
    let (base_url, api_key) = state
        .config
        .ai
        .as_ref()
        .map(|config| (config.base_url.to_string(), ai::sha256_hex(&config.api_key)))
        .unwrap_or_default();
    ai::sha256_hex(&format!(
        "{GLOBAL_PROTOCOL_VERSION}\nmodel={model_profile}\nbase_url={base_url}\napi_key_hash={api_key}"
    ))
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
    let mut result = projection.unwrap_or_else(|| {
        json!({
            "producer_ref": "(global)",
            "entity_id": work.canonical_resource_id,
            "kind": work.canonical_resource_type,
            "variant": work.variant,
            "status": work.status,
            "title_zh": null,
            "summary_md": null,
            "body_md": null,
            "error": null,
            "error_code": null,
            "error_summary": null,
            "error_detail": null,
            "work_item_id": work.id,
            "batch_id": work.batch_id,
        })
    });
    if result.get("status").is_none() {
        result["status"] = Value::String(work.status.clone());
    }
    if result.get("work_item_id").is_none() {
        result["work_item_id"] = Value::String(work.id.clone());
    }
    if result.get("batch_id").is_none() {
        result["batch_id"] = work
            .batch_id
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null);
    }
    if result.get("entity_id").is_none() {
        result["entity_id"] = Value::String(work.canonical_resource_id.clone());
    }
    if result.get("kind").is_none() {
        result["kind"] = Value::String(kind_for_work(work));
    }
    if result.get("variant").is_none() {
        result["variant"] = Value::String(work.variant.clone());
    }
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

pub async fn submit_item(
    state: &AppState,
    user_id: &str,
    mode: &str,
    item: &translations::TranslationRequestItemInput,
) -> Result<(StatusCode, GlobalSubmissionResponse), ApiError> {
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
    let model_profile = current_model_profile(state).await;
    let snapshot = serde_json::to_string(&json!({
        "source_blocks": item.source_blocks,
        "target_slots": item.target_slots,
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
    let existing = sqlx::query_as::<_, WorkRow>(
        "SELECT id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, batch_id, attempt_count, next_retry_at, retry_expires_at, retry_after_at, created_at FROM content_work_items WHERE canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = ? AND source_hash = ? AND protocol_version = ? AND model_profile = ? LIMIT 1",
    )
    .bind(resource_type)
    .bind(&item.entity_id)
    .bind(pipeline)
    .bind(&item.variant)
    .bind(&item.target_lang)
    .bind(&hash)
    .bind(GLOBAL_PROTOCOL_VERSION)
    .bind(&model_profile)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    let existing_work = existing.is_some();
    let work = if let Some(existing) = existing {
        existing
    } else {
        let supersedes_work_item_id = sqlx::query_scalar::<_, String>(
            "SELECT id FROM content_work_items WHERE canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = ? AND protocol_version = ? AND source_hash <> ? ORDER BY datetime(created_at) DESC, id DESC LIMIT 1",
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
        .map_err(ApiError::internal)?;
        let projection_exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM content_result_projections WHERE canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = ? AND protocol_version = ? AND model_profile = ? AND source_hash = ?",
        )
        .bind(resource_type)
        .bind(&item.entity_id)
        .bind(pipeline)
        .bind(&item.variant)
        .bind(&item.target_lang)
        .bind(GLOBAL_PROTOCOL_VERSION)
        .bind(&model_profile)
        .bind(&hash)
        .fetch_one(&mut *tx)
        .await
        .map_err(ApiError::internal)?
            > 0;
        let status = if model_profile == "ai-disabled" {
            "blocked_config"
        } else if projection_exists {
            "ready"
        } else {
            "queued"
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
        .bind(runtime_configuration_fingerprint(state, &model_profile))
        .bind(status)
        .bind(if projection_exists { 1_i64 } else { 0_i64 })
        .bind(i64::try_from(item.source_blocks.iter().map(|block| block.text.len()).sum::<usize>()).unwrap_or(i64::MAX))
        .bind(&supersedes_work_item_id)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
        sqlx::query_as::<_, WorkRow>(
            "SELECT id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, batch_id, attempt_count, next_retry_at, retry_expires_at, retry_after_at, created_at FROM content_work_items WHERE id = ?",
        )
        .bind(&work_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(ApiError::internal)?
    };
    sqlx::query("UPDATE content_result_projections SET active_work_item_id = ?, updated_at = ? WHERE canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = ? AND protocol_version = ? AND model_profile = ? AND (active_work_item_id IS NULL OR active_work_item_id <> ?)")
        .bind(&work.id)
        .bind(&now)
        .bind(resource_type)
        .bind(&item.entity_id)
        .bind(pipeline)
        .bind(&item.variant)
        .bind(&item.target_lang)
        .bind(GLOBAL_PROTOCOL_VERSION)
        .bind(&model_profile)
        .bind(&work.id)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    let projection = load_projection(&mut tx, &work)
        .await
        .map_err(ApiError::internal)?;

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
                    "queued" | "running" | "deferred_provider"
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
    let projection = sqlx::query_scalar::<_, String>(
        "SELECT payload_json FROM content_result_projections WHERE work_item_id = ? LIMIT 1",
    )
    .bind(&work.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::internal)?
    .and_then(|raw| serde_json::from_str(&raw).ok());
    let mut response = public_response(&work, request_id, projection);
    response["result"]["producer_ref"] = authorization_probe.producer_ref.into();
    response["result"]["kind"] = Value::String(kind_for_work(&work));
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
    let model_profile = current_model_profile(state).await;
    let row = sqlx::query(
        "WITH params(resource_type, resource_id, pipeline, variant, source_hash, protocol_version, model_profile) AS (SELECT ?, ?, ?, ?, ?, ?, ?) SELECT COALESCE((SELECT status FROM content_work_items w, params p WHERE w.canonical_resource_type = p.resource_type AND w.canonical_resource_id = p.resource_id AND w.pipeline = p.pipeline AND w.variant = p.variant AND w.target_lang = 'zh-CN' AND w.source_hash = p.source_hash AND w.protocol_version = p.protocol_version AND w.model_profile = p.model_profile ORDER BY datetime(w.updated_at) DESC, w.id DESC LIMIT 1), (SELECT status FROM content_work_items w, params p WHERE w.canonical_resource_type = p.resource_type AND w.canonical_resource_id = p.resource_id AND w.pipeline = p.pipeline AND w.variant = p.variant AND w.target_lang = 'zh-CN' AND w.source_hash = p.source_hash AND w.protocol_version = p.protocol_version ORDER BY datetime(w.updated_at) DESC, w.id DESC LIMIT 1), 'ready') AS status, (SELECT p.payload_json FROM content_result_projections p, params x WHERE p.canonical_resource_type = x.resource_type AND p.canonical_resource_id = x.resource_id AND p.pipeline = x.pipeline AND p.variant = x.variant AND p.target_lang = 'zh-CN' AND p.protocol_version = x.protocol_version AND (p.source_hash = x.source_hash OR EXISTS (SELECT 1 FROM content_work_items w2, params y WHERE w2.canonical_resource_type = y.resource_type AND w2.canonical_resource_id = y.resource_id AND w2.pipeline = y.pipeline AND w2.variant = y.variant AND w2.target_lang = 'zh-CN' AND w2.source_hash = y.source_hash AND w2.protocol_version = y.protocol_version)) ORDER BY CASE WHEN p.model_profile = x.model_profile AND p.source_hash = x.source_hash THEN 0 WHEN p.source_hash = x.source_hash THEN 1 ELSE 2 END, datetime(p.updated_at) DESC, p.id DESC LIMIT 1) AS payload_json, CASE WHEN EXISTS (SELECT 1 FROM content_work_items w, params p WHERE w.canonical_resource_type = p.resource_type AND w.canonical_resource_id = p.resource_id AND w.pipeline = p.pipeline AND w.variant = p.variant AND w.target_lang = 'zh-CN' AND w.source_hash = p.source_hash AND w.protocol_version = p.protocol_version) OR EXISTS (SELECT 1 FROM content_result_projections p, params x WHERE p.canonical_resource_type = x.resource_type AND p.canonical_resource_id = x.resource_id AND p.pipeline = x.pipeline AND p.variant = x.variant AND p.target_lang = 'zh-CN' AND p.protocol_version = x.protocol_version AND p.source_hash = x.source_hash) THEN 1 ELSE 0 END AS present",
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
    let Some(row) = row else {
        return Ok(None);
    };
    if row.get::<i64, _>("present") == 0 {
        return Ok(None);
    }
    let status: String = row.get("status");
    let payload = row
        .get::<Option<String>, _>("payload_json")
        .and_then(|raw| serde_json::from_str(&raw).ok())
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
    let breaker_open = provider_breaker_open(state).await;
    let (_lock, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "content_processing_retry")
        .await
        .map_err(ApiError::internal)?;
    ensure_global_mode_in_transaction(&mut tx).await?;
    let row = sqlx::query_as::<_, WorkRow>(
        "SELECT w.id, w.canonical_resource_type, w.canonical_resource_id, w.pipeline, w.variant, w.target_lang, w.source_hash, w.protocol_version, w.model_profile, w.source_snapshot_json, w.configuration_fingerprint, w.status, w.priority, w.cache_hit, w.token_estimate, w.batch_id, w.attempt_count, w.next_retry_at, w.retry_expires_at, w.retry_after_at, w.created_at FROM content_request_links l JOIN content_work_items w ON w.id = l.work_item_id WHERE l.request_id = ? AND l.requester_id = ? LIMIT 1",
    )
    .bind(request_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "translation request not found"))?;
    let projection = load_projection(&mut tx, &row)
        .await
        .map_err(ApiError::internal)?;
    let producer_ref = sqlx::query_scalar::<_, String>(
        "SELECT producer_ref FROM content_request_links WHERE request_id = ? LIMIT 1",
    )
    .bind(request_id)
    .fetch_one(&mut *tx)
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
    let retry_status = if breaker_open {
        "deferred_provider"
    } else {
        "queued"
    };
    let next_retry_at = breaker_open
        .then(|| (Utc::now() + chrono::Duration::seconds(PROVIDER_DEFER_SECS)).to_rfc3339());
    sqlx::query("UPDATE content_work_items SET status = ?, priority = 3, next_retry_at = ?, retry_after_at = NULL, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(retry_status)
        .bind(&next_retry_at)
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
    let Some(row) = sqlx::query_as::<_, WorkRow>(
        "SELECT id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, priority, cache_hit, token_estimate, batch_id, attempt_count, next_retry_at, retry_expires_at, retry_after_at, created_at FROM content_work_items WHERE status = 'queued' AND (next_retry_at IS NULL OR datetime(next_retry_at) <= datetime('now')) AND (priority >= 3 OR datetime(created_at) <= datetime('now', '-60 seconds')) AND (priority < 3 OR (SELECT COUNT(*) FROM content_batches WHERE status = 'running' AND trigger_reason = 'manual_retry') < ?) ORDER BY priority DESC, datetime(created_at) ASC, id ASC LIMIT 1",
    )
    .bind(manual_limit)
    .fetch_optional(&mut *tx)
    .await?
    else {
        tx.commit().await?;
        return Ok(None);
    };
    let batch_id = local_id::generate_local_id().to_string();
    let attempt_id = local_id::generate_local_id().to_string();
    let now = Utc::now().to_rfc3339();
    let lease_expires_at = (Utc::now() + chrono::Duration::minutes(5)).to_rfc3339();
    let trigger_reason = if row.priority >= 3 {
        "manual_retry"
    } else {
        "initial"
    };
    sqlx::query("INSERT INTO content_batches (id, partition_key, target_lang, protocol_version, model_profile, trigger_reason, worker_id, worker_kind, request_count, item_count, estimated_input_tokens, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 'content-general-1', 'general', (SELECT COUNT(*) FROM content_request_links WHERE work_item_id = ?), 1, ?, 'running', ?, ?)")
        .bind(&batch_id)
        .bind(format!("{}:{}", row.target_lang, row.model_profile))
        .bind(&row.target_lang)
    .bind(&row.protocol_version)
    .bind(&row.model_profile)
    .bind(trigger_reason)
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
    sqlx::query("UPDATE content_work_items SET status = 'running', batch_id = ?, attempt_count = attempt_count + 1, started_at = ?, lease_owner = 'content-general-1', lease_expires_at = ?, updated_at = ? WHERE id = ?")
        .bind(&batch_id)
        .bind(&now)
        .bind(&lease_expires_at)
        .bind(&now)
        .bind(&row.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, created_at) VALUES (?, ?, ?, ?, 'attempt_started', ?)")
        .bind(&attempt_id)
        .bind(&row.id)
        .bind(row.attempt_count + 1)
        .bind(trigger_reason)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some(WorkRow {
        status: "running".to_owned(),
        batch_id: Some(batch_id),
        attempt_count: row.attempt_count + 1,
        ..row
    }))
}

async fn recover_due(state: &AppState) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let (_lock, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "content_processing_recover")
        .await?;
    ensure_global_mode_in_transaction(&mut tx).await?;
    sqlx::query(
        "UPDATE content_work_items SET status = 'queued', next_retry_at = NULL, lease_owner = NULL, lease_expires_at = NULL, updated_at = ? WHERE status IN ('failed', 'deferred_provider') AND next_retry_at IS NOT NULL AND datetime(next_retry_at) <= datetime(?) AND (retry_expires_at IS NULL OR datetime(retry_expires_at) > datetime(?))",
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
    let expired_running = "status = 'running' AND lease_expires_at IS NOT NULL AND julianday(lease_expires_at) <= julianday(?)";
    sqlx::query(&format!(
        "UPDATE content_batch_items SET result_status = 'failed', error_code = 'runtime_lease_expired', updated_at = ? WHERE work_item_id IN (SELECT id FROM content_work_items WHERE {expired_running})"
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
        "UPDATE content_work_items SET status = 'queued', lease_owner = NULL, lease_expires_at = NULL, next_retry_at = ?, retry_expires_at = COALESCE(retry_expires_at, ?), retry_after_at = ?, updated_at = ? WHERE status = 'running' AND lease_expires_at IS NOT NULL AND julianday(lease_expires_at) <= julianday(?)",
    )
    .bind(&recovery_retry_at)
    .bind(&recovery_expires_at)
    .bind(&recovery_retry_at)
    .bind(&now)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, retry_eligible, next_retry_at, created_at) SELECT lower(hex(randomblob(16))), id, attempt_count, 'automatic_recovery', 'attempt_queued', 1, ?, ? FROM content_work_items WHERE status = 'queued' AND next_retry_at = ?",
    )
    .bind(&recovery_retry_at)
    .bind(&now)
    .bind(&recovery_retry_at)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
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
    sqlx::query("UPDATE content_work_items SET status = 'deferred_provider', next_retry_at = ?, retry_expires_at = COALESCE(retry_expires_at, ?), updated_at = CURRENT_TIMESTAMP WHERE status = 'queued'")
        .bind(&retry_at)
        .bind((Utc::now() + chrono::Duration::hours(24)).to_rfc3339())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

fn build_prompt(snapshot: &SourceSnapshot, pipeline: &str) -> (String, String) {
    let system = if pipeline == "polishing" {
        "你是严谨的技术内容润色助手。只输出 JSON，不要解释。保留事实、链接、代码和 Markdown 结构。"
    } else {
        "你是严谨的技术文档翻译助手。只输出 JSON，不要解释。保留事实、链接、代码和 Markdown 结构。"
    };
    let user = json!({
        "source_blocks": snapshot.source_blocks,
        "target_slots": snapshot.target_slots,
        "output": {"title_zh": "string|null", "summary_md": "string|null", "body_md": "string|null"}
    })
    .to_string();
    (system.to_owned(), user)
}

fn validate_output(raw: &str, target_slots: &[String]) -> Result<Value> {
    let output = serde_json::from_str::<Value>(raw).context("global content output is not JSON")?;
    let object = output
        .as_object()
        .ok_or_else(|| anyhow!("global content output is not an object"))?;
    for slot in target_slots {
        let Some(value) = object.get(slot) else {
            return Err(anyhow!(
                "global content output is missing target slot: {slot}"
            ));
        };
        if !value.is_null() && value.as_str().is_none() {
            return Err(anyhow!(
                "global content output target slot is not text: {slot}"
            ));
        }
    }
    if !target_slots.iter().any(|slot| {
        object
            .get(slot)
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
    }) {
        return Err(anyhow!("global content output is empty"));
    }
    Ok(output)
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

async fn supersede_replaced_work_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    work: &WorkRow,
) -> Result<bool> {
    let replaced = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS (SELECT 1 FROM content_work_items newer WHERE newer.id <> ? AND newer.canonical_resource_type = ? AND newer.canonical_resource_id = ? AND newer.pipeline = ? AND newer.variant = ? AND newer.target_lang = ? AND newer.protocol_version = ? AND datetime(newer.created_at) > datetime(?) AND newer.status NOT IN ('cancelled', 'superseded'))",
    )
    .bind(&work.id)
    .bind(&work.canonical_resource_type)
    .bind(&work.canonical_resource_id)
    .bind(&work.pipeline)
    .bind(&work.variant)
    .bind(&work.target_lang)
    .bind(&work.protocol_version)
    .bind(&work.created_at)
    .fetch_one(&mut **tx)
    .await?
        != 0;
    if !replaced {
        return Ok(false);
    }
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE content_work_items SET status = 'superseded', finished_at = ?, lease_owner = NULL, lease_expires_at = NULL, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(&now)
        .bind(&work.id)
        .execute(&mut **tx)
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

async fn block_config_work(state: &AppState, work: &WorkRow) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let (_lock, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "content_processing_block_config")
        .await?;
    ensure_global_mode_in_transaction(&mut tx)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;
    let updated = sqlx::query("UPDATE content_work_items SET status = 'blocked_config', failure_class = 'configuration', finished_at = ?, lease_owner = NULL, lease_expires_at = NULL, updated_at = ? WHERE id = ? AND status = 'running' AND attempt_count = ? AND lease_owner = 'content-general-1' AND lease_expires_at IS NOT NULL AND julianday(lease_expires_at) > julianday(?)")
        .bind(&now)
        .bind(&now)
        .bind(&work.id)
        .bind(work.attempt_count)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    if updated.rows_affected() == 0 {
        tx.commit().await?;
        return Ok(());
    }
    sqlx::query("INSERT OR IGNORE INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, error_code, error_summary, failure_class, retry_eligible, created_at) SELECT ?, work_item_id, attempt_no, trigger, 'attempt_completed', 'blocked_config', 'configuration', 'model configuration changed before execution', 'configuration', 0, ? FROM content_attempt_events WHERE work_item_id = ? AND attempt_no = ? AND event_type = 'attempt_started'")
        .bind(local_id::generate_local_id().to_string())
        .bind(&now)
        .bind(&work.id)
        .bind(work.attempt_count)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE content_batch_items SET result_status = 'blocked_config', error_code = 'configuration', updated_at = ? WHERE work_item_id = ? AND batch_id = ?")
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
    tx.commit().await?;
    Ok(())
}

async fn execute(state: &AppState, work: WorkRow) -> Result<()> {
    if !source_exists(state, &work).await? {
        cancel_deleted_work(state, &work).await?;
        return Ok(());
    }
    let snapshot = serde_json::from_str::<SourceSnapshot>(&work.source_snapshot_json)
        .context("invalid global source snapshot")?;
    let selected_model = ai::select_model_for_new_calls(state).await;
    if selected_model.model != work.model_profile {
        let routing = state
            .llm_scheduler
            .routing_status(state.config.ai.as_ref().map(|config| config.model.as_str()))
            .await;
        let model_is_cooling_down = routing
            .model_statuses
            .iter()
            .find(|status| status.model == work.model_profile)
            .is_some_and(|status| status.status == "cooldown");
        if model_is_cooling_down {
            let now = Utc::now();
            let next_retry_at = routing
                .model_statuses
                .iter()
                .find(|status| status.model == work.model_profile)
                .and_then(|status| status.cooldown_until.clone())
                .unwrap_or_else(|| {
                    (now + chrono::Duration::seconds(PROVIDER_DEFER_SECS)).to_rfc3339()
                });
            let retry_expires_at = (now + chrono::Duration::hours(24)).to_rfc3339();
            let (_lock, mut tx) = state
                .sqlite_writer
                .begin_immediate(&state.pool, "content_processing_defer_model")
                .await?;
            ensure_global_mode_in_transaction(&mut tx)
                .await
                .map_err(|error| anyhow!(error.to_string()))?;
            sqlx::query("UPDATE content_work_items SET status = 'deferred_provider', next_retry_at = ?, retry_expires_at = COALESCE(retry_expires_at, ?), retry_after_at = ?, lease_owner = NULL, lease_expires_at = NULL, updated_at = ? WHERE id = ? AND status = 'running' AND attempt_count = ?")
                .bind(&next_retry_at)
                .bind(&retry_expires_at)
                .bind(&next_retry_at)
                .bind(now.to_rfc3339())
                .bind(&work.id)
                .bind(work.attempt_count)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
        if runtime_configuration_fingerprint(state, &work.model_profile)
            != work.configuration_fingerprint
        {
            block_config_work(state, &work).await?;
            return Ok(());
        }
        block_config_work(state, &work).await?;
        return Ok(());
    }
    if runtime_configuration_fingerprint(state, &work.model_profile)
        != work.configuration_fingerprint
    {
        block_config_work(state, &work).await?;
        return Ok(());
    }
    let (system, user) = build_prompt(&snapshot, &work.pipeline);
    let result = tokio::time::timeout(
        Duration::from_secs(4 * 60),
        ai::chat_completion_with_diagnostics(state, &system, &user, 3_000),
    )
    .await
    .map_err(|_| {
        anyhow::Error::new(ai::LlmCallFailure {
            class: ai::LlmFailureClass::Transient,
            call_id: None,
        })
    })
    .and_then(|result| result)
    .and_then(|diagnostic| {
        validate_output(&diagnostic.content, &snapshot.target_slots)
            .map(|output| (diagnostic, output))
    });
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
        tx.rollback().await?;
        return Ok(());
    }
    if !source_exists_in_transaction(&mut tx, &work).await? {
        cancel_deleted_work_in_transaction(&mut tx, &work).await?;
        tx.commit().await?;
        return Ok(());
    }
    if supersede_replaced_work_in_transaction(&mut tx, &work).await? {
        tx.commit().await?;
        return Ok(());
    }
    match result {
        Ok((diagnostic, output)) => {
            let attempt_event_id = sqlx::query_scalar::<_, String>("SELECT id FROM content_attempt_events WHERE work_item_id = ? AND attempt_no = ? AND event_type = 'attempt_started' LIMIT 1")
                .bind(&work.id)
                .bind(work.attempt_count)
                .fetch_one(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO content_attempt_llm_calls (id, attempt_event_id, provider_call_id, model, status, output_tokens, created_at) VALUES (?, ?, ?, ?, 'succeeded', ?, ?)")
                .bind(local_id::generate_local_id().to_string())
                .bind(&attempt_event_id)
                .bind(diagnostic.provider_request_id.as_deref().unwrap_or("unknown"))
                .bind(&work.model_profile)
                .bind(diagnostic.output_tokens)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT OR IGNORE INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, retry_eligible, created_at) SELECT ?, work_item_id, attempt_no, trigger, 'attempt_completed', 'ready', 0, ? FROM content_attempt_events WHERE id = ?")
                .bind(local_id::generate_local_id().to_string())
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
            sqlx::query("INSERT INTO content_result_projections (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash, work_item_id, active_work_item_id, payload_json, published_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile) DO UPDATE SET source_hash = excluded.source_hash, work_item_id = excluded.work_item_id, active_work_item_id = excluded.active_work_item_id, payload_json = excluded.payload_json, published_at = excluded.published_at, updated_at = excluded.updated_at")
                .bind(local_id::generate_local_id().to_string())
                .bind(&work.canonical_resource_type)
                .bind(&work.canonical_resource_id)
                .bind(&work.pipeline)
                .bind(&work.variant)
                .bind(&work.target_lang)
                .bind(&work.protocol_version)
                .bind(&work.model_profile)
                .bind(&work.source_hash)
                .bind(&work.id)
                .bind(&work.id)
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
            let llm_class = ai::llm_failure_class(&error);
            let class = llm_class
                .map(|value| value.as_str().to_owned())
                .or_else(|| {
                    translations::classify_translation_error(Some(error_text.as_str()))
                        .map(|value| value.code.to_owned())
                })
                .unwrap_or_else(|| "unknown_internal_error".to_owned());
            let retryable = llm_class.is_some_and(ai::LlmFailureClass::is_recoverable)
                || class == "output_contract_invalid";
            let next_retry = if retryable
                && work.attempt_count <= i64::try_from(RETRY_DELAYS_SECS.len()).unwrap_or(i64::MAX)
            {
                Some(
                    (Utc::now()
                        + chrono::Duration::seconds(
                            RETRY_DELAYS_SECS[(work.attempt_count - 1).max(0) as usize],
                        ))
                    .to_rfc3339(),
                )
            } else {
                None
            };
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
            sqlx::query("INSERT INTO content_attempt_llm_calls (id, attempt_event_id, provider_call_id, model, status, error_code, error_summary, created_at) VALUES (?, ?, 'unknown', ?, 'failed', ?, ?, ?)")
                .bind(local_id::generate_local_id().to_string())
                .bind(&attempt_event_id)
                .bind(&work.model_profile)
                .bind(&class)
                .bind(error_summary.as_deref())
                .bind(&now)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE content_work_items SET status = 'failed', failure_class = ?, next_retry_at = ?, retry_expires_at = ?, retry_after_at = ?, finished_at = ?, lease_owner = NULL, lease_expires_at = NULL, updated_at = ? WHERE id = ?")
                .bind(&class)
                .bind(&next_retry)
                .bind(&retry_expires)
                .bind(&retry_after)
                .bind(&now)
                .bind(&now)
                .bind(&work.id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT OR IGNORE INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, error_code, error_summary, failure_class, retry_eligible, next_retry_at, created_at) SELECT ?, work_item_id, attempt_no, trigger, 'attempt_completed', 'failed', ?, ?, ?, ?, ?, ? FROM content_attempt_events WHERE work_item_id = ? AND attempt_no = ? AND event_type = 'attempt_started'")
                .bind(local_id::generate_local_id().to_string())
                .bind(&class)
                .bind(error_summary)
                .bind(&class)
                .bind(i64::from(next_retry.is_some()))
                .bind(&next_retry)
                .bind(&now)
                .bind(&work.id)
                .bind(work.attempt_count)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE content_batches SET status = 'failed', finished_at = ?, updated_at = ?, error_code = ?, error_summary = ? WHERE id = ?")
                .bind(&now)
                .bind(&now)
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
    let manual_limit = i64::try_from(worker_count.saturating_sub(1)).unwrap_or(i64::MAX);
    if let Some(work) = claim_next(state, manual_limit).await?
        && let Err(error) = execute(state, work).await
    {
        warn!(?error, "global content processing execution failed");
    }
    Ok(())
}

pub fn spawn_global_scheduler(state: Arc<AppState>) -> tokio::task::AbortHandle {
    tokio::spawn(async move {
        let worker_count = state
            .translation_scheduler
            .desired_config()
            .await
            .general_worker_concurrency
            .max(1);
        let mut workers = JoinSet::new();
        for _ in 0..worker_count {
            let worker_state = state.clone();
            workers.spawn(async move {
                loop {
                    if let Err(error) = run_once(worker_state.as_ref()).await {
                        warn!(?error, "global content processing scheduler failed");
                    }
                    sleep(Duration::from_millis(250)).await;
                }
            });
        }
        while workers.join_next().await.is_some() {}
    })
    .abort_handle()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

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
    async fn migration_preserves_legacy_rows_and_creates_only_global_tables() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(
            "CREATE TABLE translation_work_items (id TEXT PRIMARY KEY, status TEXT NOT NULL); CREATE TABLE ai_translations (id TEXT PRIMARY KEY, value TEXT NOT NULL); INSERT INTO translation_work_items VALUES ('legacy-1', 'ready'); INSERT INTO ai_translations VALUES ('cache-1', 'cached');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let before_work: (String, String) =
            sqlx::query_as("SELECT id, status FROM translation_work_items")
                .fetch_one(&pool)
                .await
                .unwrap();
        let before_cache: (String, String) =
            sqlx::query_as("SELECT id, value FROM ai_translations")
                .fetch_one(&pool)
                .await
                .unwrap();

        sqlx::raw_sql(include_str!(
            "../migrations/0078_content_processing_global_model.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();

        let after_work: (String, String) =
            sqlx::query_as("SELECT id, status FROM translation_work_items")
                .fetch_one(&pool)
                .await
                .unwrap();
        let after_cache: (String, String) = sqlx::query_as("SELECT id, value FROM ai_translations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(before_work, after_work);
        assert_eq!(before_cache, after_cache);
        let new_table_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('content_processing_control', 'content_work_items', 'content_batches', 'content_batch_items', 'content_result_projections', 'content_request_links', 'content_attempt_events', 'content_attempt_llm_calls', 'content_legacy_observations')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(new_table_count, 9);
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
}
