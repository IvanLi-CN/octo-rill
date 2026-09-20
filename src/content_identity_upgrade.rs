use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow};
use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use tower_sessions::Session;

use crate::{
    ai, api,
    error::ApiError,
    local_id,
    sqlite_write::{SqliteWriteCoordinator, SqliteWritePriority},
    state::AppState,
};

const BATCH_SIZE: i64 = 64;
const POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum UpgradePhase {
    WorkIdentityBackfill,
    ProjectionBackfill,
    BlockedConfigRecovery,
    Complete,
}

impl UpgradePhase {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "work_identity_backfill" => Ok(Self::WorkIdentityBackfill),
            "projection_backfill" => Ok(Self::ProjectionBackfill),
            "blocked_config_recovery" => Ok(Self::BlockedConfigRecovery),
            "complete" => Ok(Self::Complete),
            other => Err(anyhow!("unknown content identity upgrade phase: {other}")),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::WorkIdentityBackfill => "work_identity_backfill",
            Self::ProjectionBackfill => "projection_backfill",
            Self::BlockedConfigRecovery => "blocked_config_recovery",
            Self::Complete => "complete",
        }
    }

    fn next(&self) -> Self {
        match self {
            Self::WorkIdentityBackfill => Self::ProjectionBackfill,
            Self::ProjectionBackfill => Self::BlockedConfigRecovery,
            Self::BlockedConfigRecovery | Self::Complete => Self::Complete,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PhaseProgress {
    last_key: Option<String>,
    processed: i64,
    total: Option<i64>,
    completed_at: Option<String>,
    requeued: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct UpgradeCursor {
    version: u8,
    #[serde(default)]
    search_index_refresh_pending: bool,
    work_identity_backfill: PhaseProgress,
    projection_backfill: PhaseProgress,
    blocked_config_recovery: PhaseProgress,
}

impl UpgradeCursor {
    fn decode(raw: Option<&str>) -> Result<Self> {
        raw.map(serde_json::from_str)
            .transpose()
            .context("invalid content identity upgrade cursor")
            .map(|cursor| cursor.unwrap_or_default())
    }

    fn phase_mut(&mut self, phase: &UpgradePhase) -> Option<&mut PhaseProgress> {
        match phase {
            UpgradePhase::WorkIdentityBackfill => Some(&mut self.work_identity_backfill),
            UpgradePhase::ProjectionBackfill => Some(&mut self.projection_backfill),
            UpgradePhase::BlockedConfigRecovery => Some(&mut self.blocked_config_recovery),
            UpgradePhase::Complete => None,
        }
    }
}

#[derive(Debug, Clone)]
struct UpgradeControl {
    generation: i64,
    status: String,
    phase: UpgradePhase,
    cursor: UpgradeCursor,
    last_error_code: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ContentWorkIdentity {
    pub canonical_resource_type: String,
    pub canonical_resource_id: String,
    pub pipeline: String,
    pub variant: String,
    pub target_lang: String,
    pub source_hash: String,
    pub protocol_version: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct IdentityWorkRow {
    id: String,
    canonical_resource_type: String,
    canonical_resource_id: String,
    pipeline: String,
    variant: String,
    target_lang: String,
    source_hash: String,
    protocol_version: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct IdentityRow {
    id: String,
    canonical_resource_type: String,
    canonical_resource_id: String,
    pipeline: String,
    variant: String,
    target_lang: String,
    source_hash: String,
    protocol_version: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ProjectionRow {
    id: String,
    work_item_id: String,
    active_work_item_id: Option<String>,
    payload_json: String,
    published_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct PhaseProgressResponse {
    processed: i64,
    total: Option<i64>,
    completed_at: Option<String>,
    requeued: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpgradeStatusResponse {
    generation: i64,
    status: String,
    phase: String,
    last_error_code: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
    search_index_refresh_pending: bool,
    work_identity_backfill: PhaseProgressResponse,
    projection_backfill: PhaseProgressResponse,
    blocked_config_recovery: PhaseProgressResponse,
    waiting_blocked_config_identities: i64,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum UpgradeAction {
    Pause,
    Resume,
}

#[derive(Debug, Deserialize)]
pub struct UpgradeControlRequest {
    action: UpgradeAction,
}

#[derive(Debug, Clone)]
pub(crate) struct CurrentProjection {
    pub work_item_id: String,
    pub payload_json: String,
}

pub(crate) fn identity_id_for(key: &ContentWorkIdentity) -> Result<String> {
    let encoded = serde_json::to_string(&(
        key.canonical_resource_type.as_str(),
        key.canonical_resource_id.as_str(),
        key.pipeline.as_str(),
        key.variant.as_str(),
        key.target_lang.as_str(),
        key.source_hash.as_str(),
        key.protocol_version.as_str(),
    ))?;
    Ok(format!(
        "content-work-{}",
        ai::sha256_hex(&format!("content-work-identity.v1\n{encoded}"))
    ))
}

pub(crate) async fn ensure_identity_registered(
    tx: &mut Transaction<'_, Sqlite>,
    key: &ContentWorkIdentity,
    now: &str,
) -> Result<String> {
    let identity_id = identity_id_for(key)?;
    sqlx::query(
        "INSERT OR IGNORE INTO content_work_identities (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&identity_id)
    .bind(&key.canonical_resource_type)
    .bind(&key.canonical_resource_id)
    .bind(&key.pipeline)
    .bind(&key.variant)
    .bind(&key.target_lang)
    .bind(&key.source_hash)
    .bind(&key.protocol_version)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    let registered_id = sqlx::query_scalar::<_, String>(
        "SELECT id FROM content_work_identities WHERE canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = ? AND source_hash = ? AND protocol_version = ? LIMIT 1",
    )
    .bind(&key.canonical_resource_type)
    .bind(&key.canonical_resource_id)
    .bind(&key.pipeline)
    .bind(&key.variant)
    .bind(&key.target_lang)
    .bind(&key.source_hash)
    .bind(&key.protocol_version)
    .fetch_one(&mut **tx)
    .await?;
    if registered_id != identity_id {
        anyhow::bail!("content work identity id does not match its canonical key");
    }
    Ok(identity_id)
}

async fn insert_member(
    tx: &mut Transaction<'_, Sqlite>,
    identity_id: &str,
    work_item_id: &str,
    key: &ContentWorkIdentity,
    now: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO content_work_identity_members (work_item_id, identity_id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, linked_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(work_item_id)
    .bind(identity_id)
    .bind(&key.canonical_resource_type)
    .bind(&key.canonical_resource_id)
    .bind(&key.pipeline)
    .bind(&key.variant)
    .bind(&key.target_lang)
    .bind(&key.source_hash)
    .bind(&key.protocol_version)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    let registered_identity = sqlx::query_scalar::<_, String>(
        "SELECT identity_id FROM content_work_identity_members WHERE work_item_id = ?",
    )
    .bind(work_item_id)
    .fetch_one(&mut **tx)
    .await?;
    if registered_identity != identity_id {
        anyhow::bail!("content work item is already linked to another identity");
    }
    Ok(())
}

pub(crate) async fn ensure_all_identity_members(
    tx: &mut Transaction<'_, Sqlite>,
    key: &ContentWorkIdentity,
    identity_id: &str,
    now: &str,
) -> Result<()> {
    let rows = sqlx::query_as::<_, (String,)>(
        "SELECT id FROM content_work_items WHERE canonical_resource_type = ? AND canonical_resource_id = ? AND pipeline = ? AND variant = ? AND target_lang = ? AND source_hash = ? AND protocol_version = ? ORDER BY id",
    )
    .bind(&key.canonical_resource_type)
    .bind(&key.canonical_resource_id)
    .bind(&key.pipeline)
    .bind(&key.variant)
    .bind(&key.target_lang)
    .bind(&key.source_hash)
    .bind(&key.protocol_version)
    .fetch_all(&mut **tx)
    .await?;
    for (work_item_id,) in rows {
        insert_member(tx, identity_id, &work_item_id, key, now).await?;
    }
    Ok(())
}

pub(crate) async fn ensure_identity_for_work(
    tx: &mut Transaction<'_, Sqlite>,
    work_item_id: &str,
    now: &str,
) -> Result<String> {
    let key = sqlx::query_as::<_, (String, String, String, String, String, String, String)>(
        "SELECT canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version FROM content_work_items WHERE id = ?",
    )
    .bind(work_item_id)
    .fetch_one(&mut **tx)
    .await?;
    let key = ContentWorkIdentity {
        canonical_resource_type: key.0,
        canonical_resource_id: key.1,
        pipeline: key.2,
        variant: key.3,
        target_lang: key.4,
        source_hash: key.5,
        protocol_version: key.6,
    };
    let identity_id = ensure_identity_registered(tx, &key, now).await?;
    ensure_all_identity_members(tx, &key, &identity_id, now).await?;
    Ok(identity_id)
}

async fn ensure_current_projection(
    tx: &mut Transaction<'_, Sqlite>,
    identity: &IdentityRow,
    now: &str,
) -> Result<Option<CurrentProjection>> {
    let projection = sqlx::query_as::<_, ProjectionRow>(
        "SELECT p.id, p.work_item_id, p.active_work_item_id, p.payload_json, p.published_at FROM content_result_projections p JOIN content_work_items w ON w.id = p.work_item_id WHERE p.canonical_resource_type = ? AND p.canonical_resource_id = ? AND p.pipeline = ? AND p.variant = ? AND p.target_lang = ? AND p.protocol_version = ? AND p.source_hash = ? AND w.canonical_resource_type = p.canonical_resource_type AND w.canonical_resource_id = p.canonical_resource_id AND w.pipeline = p.pipeline AND w.variant = p.variant AND w.target_lang = p.target_lang AND w.protocol_version = p.protocol_version AND w.source_hash = p.source_hash AND w.status = 'ready' AND CASE WHEN json_valid(p.payload_json) THEN json_type(p.payload_json) = 'object' ELSE 0 END = 1 ORDER BY julianday(p.published_at) DESC, p.published_at DESC, p.id DESC LIMIT 1",
    )
    .bind(&identity.canonical_resource_type)
    .bind(&identity.canonical_resource_id)
    .bind(&identity.pipeline)
    .bind(&identity.variant)
    .bind(&identity.target_lang)
    .bind(&identity.protocol_version)
    .bind(&identity.source_hash)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(projection) = projection else {
        return Ok(None);
    };
    if sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM content_work_identity_members WHERE identity_id = ? AND work_item_id = ?",
    )
    .bind(&identity.id)
    .bind(&projection.work_item_id)
    .fetch_one(&mut **tx)
    .await?
        == 0
    {
        anyhow::bail!("current projection work item is missing its identity membership");
    }
    let active_work_item_id = match projection.active_work_item_id.as_deref() {
        Some(active_id) => {
            let active = sqlx::query_as::<_, (String, String, String, String, String, String, String)>(
                "SELECT canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, source_hash FROM content_work_items WHERE id = ?",
            )
            .bind(active_id)
            .fetch_optional(&mut **tx)
            .await?;
            active
                .filter(|active| {
                    active.0 == identity.canonical_resource_type
                        && active.1 == identity.canonical_resource_id
                        && active.2 == identity.pipeline
                        && active.3 == identity.variant
                        && active.4 == identity.target_lang
                        && active.5 == identity.protocol_version
                })
                .map(|active| {
                    if active.6 == identity.source_hash {
                        projection.work_item_id.clone()
                    } else {
                        active_id.to_owned()
                    }
                })
        }
        None => None,
    };
    sqlx::query(
        "INSERT INTO content_current_result_projections (identity_id, work_item_id, active_work_item_id, source_projection_id, payload_json, published_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(identity_id) DO UPDATE SET work_item_id = excluded.work_item_id, active_work_item_id = excluded.active_work_item_id, source_projection_id = excluded.source_projection_id, payload_json = excluded.payload_json, published_at = excluded.published_at, updated_at = excluded.updated_at",
    )
    .bind(&identity.id)
    .bind(&projection.work_item_id)
    .bind(active_work_item_id.as_deref())
    .bind(&projection.id)
    .bind(&projection.payload_json)
    .bind(&projection.published_at)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(Some(CurrentProjection {
        work_item_id: projection.work_item_id,
        payload_json: projection.payload_json,
    }))
}

pub(crate) async fn ensure_current_projection_for_key(
    tx: &mut Transaction<'_, Sqlite>,
    identity_id: &str,
    now: &str,
) -> Result<Option<CurrentProjection>> {
    let identity = load_identity(tx, identity_id).await?;
    ensure_current_projection(tx, &identity, now).await
}

async fn load_identity(tx: &mut Transaction<'_, Sqlite>, identity_id: &str) -> Result<IdentityRow> {
    sqlx::query_as::<_, IdentityRow>(
        "SELECT id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version FROM content_work_identities WHERE id = ?",
    )
    .bind(identity_id)
    .fetch_one(&mut **tx)
    .await
    .context("load content work identity")
}

async fn load_control(tx: &mut Transaction<'_, Sqlite>) -> Result<UpgradeControl> {
    let row = sqlx::query(
        "SELECT generation, status, phase, cursor, last_error_code, started_at, completed_at FROM content_identity_upgrade_control WHERE id = 1",
    )
    .fetch_one(&mut **tx)
    .await
    .context("load content identity upgrade control")?;
    Ok(UpgradeControl {
        generation: row.get("generation"),
        status: row.get("status"),
        phase: UpgradePhase::parse(row.get::<&str, _>("phase"))?,
        cursor: UpgradeCursor::decode(row.try_get::<Option<String>, _>("cursor")?.as_deref())?,
        last_error_code: row.get("last_error_code"),
        started_at: row.get("started_at"),
        completed_at: row.get("completed_at"),
    })
}

fn encode_cursor(cursor: &UpgradeCursor) -> Result<String> {
    Ok(serde_json::to_string(cursor)?)
}

async fn phase_total(tx: &mut Transaction<'_, Sqlite>, phase: &UpgradePhase) -> Result<i64> {
    let query = match phase {
        UpgradePhase::WorkIdentityBackfill => "SELECT COUNT(*) FROM content_work_items",
        UpgradePhase::ProjectionBackfill => "SELECT COUNT(*) FROM content_work_identities",
        UpgradePhase::BlockedConfigRecovery => {
            "SELECT COUNT(DISTINCT m.identity_id) FROM content_work_identity_members m JOIN content_work_items w ON w.id = m.work_item_id WHERE w.status = 'blocked_config'"
        }
        UpgradePhase::Complete => return Ok(0),
    };
    sqlx::query_scalar::<_, i64>(query)
        .fetch_one(&mut **tx)
        .await
        .map_err(Into::into)
}

async fn process_work_identity_batch(
    tx: &mut Transaction<'_, Sqlite>,
    cursor: &mut UpgradeCursor,
    now: &str,
    limit: i64,
) -> Result<bool> {
    let phase = &UpgradePhase::WorkIdentityBackfill;
    let progress = cursor.phase_mut(phase).expect("phase has progress");
    if progress.total.is_none() {
        progress.total = Some(phase_total(tx, phase).await?);
    }
    let rows = sqlx::query_as::<_, IdentityWorkRow>(
        "SELECT w.id, w.canonical_resource_type, w.canonical_resource_id, w.pipeline, w.variant, w.target_lang, w.source_hash, w.protocol_version FROM content_work_items w WHERE NOT EXISTS (SELECT 1 FROM content_work_identity_members m WHERE m.work_item_id = w.id) ORDER BY w.id LIMIT ?",
    )
    .bind(limit)
    .fetch_all(&mut **tx)
    .await?;
    if rows.is_empty() {
        progress.completed_at = Some(now.to_owned());
        return Ok(true);
    }
    for row in &rows {
        let key = key_from_work(row);
        let identity_id = ensure_identity_registered(tx, &key, now).await?;
        insert_member(tx, &identity_id, &row.id, &key, now).await?;
    }
    progress.last_key = rows.last().map(|row| row.id.clone());
    progress.processed = progress
        .processed
        .saturating_add(i64::try_from(rows.len()).unwrap_or(i64::MAX));
    Ok(false)
}

fn key_from_work(row: &IdentityWorkRow) -> ContentWorkIdentity {
    ContentWorkIdentity {
        canonical_resource_type: row.canonical_resource_type.clone(),
        canonical_resource_id: row.canonical_resource_id.clone(),
        pipeline: row.pipeline.clone(),
        variant: row.variant.clone(),
        target_lang: row.target_lang.clone(),
        source_hash: row.source_hash.clone(),
        protocol_version: row.protocol_version.clone(),
    }
}

async fn process_projection_batch(
    tx: &mut Transaction<'_, Sqlite>,
    cursor: &mut UpgradeCursor,
    now: &str,
    limit: i64,
) -> Result<bool> {
    let phase = &UpgradePhase::ProjectionBackfill;
    let progress = cursor.phase_mut(phase).expect("phase has progress");
    if progress.total.is_none() {
        progress.total = Some(phase_total(tx, phase).await?);
    }
    let identities = sqlx::query_scalar::<_, String>(
        "SELECT id FROM content_work_identities WHERE (? IS NULL OR id > ?) ORDER BY id LIMIT ?",
    )
    .bind(progress.last_key.as_deref())
    .bind(progress.last_key.as_deref())
    .bind(limit)
    .fetch_all(&mut **tx)
    .await?;
    if identities.is_empty() {
        progress.completed_at = Some(now.to_owned());
        return Ok(true);
    }
    for identity_id in &identities {
        let identity = load_identity(tx, identity_id).await?;
        let projection = ensure_current_projection(tx, &identity, now).await?;
        reconcile_active_members(tx, identity_id, projection.as_ref(), now).await?;
    }
    progress.last_key = identities.last().cloned();
    progress.processed = progress
        .processed
        .saturating_add(i64::try_from(identities.len()).unwrap_or(i64::MAX));
    Ok(false)
}

async fn reconcile_active_members(
    tx: &mut Transaction<'_, Sqlite>,
    identity_id: &str,
    projection: Option<&CurrentProjection>,
    now: &str,
) -> Result<()> {
    let keep_id_storage = if let Some(projection) = projection {
        Some(projection.work_item_id.clone())
    } else {
        sqlx::query_scalar::<_, String>(
            "SELECT w.id FROM content_work_identity_members m JOIN content_work_items w ON w.id = m.work_item_id WHERE m.identity_id = ? AND (w.status IN ('running', 'queued', 'deferred_provider', 'blocked_config') OR (w.status = 'failed' AND w.next_retry_at IS NOT NULL)) ORDER BY CASE w.status WHEN 'running' THEN 0 WHEN 'queued' THEN 1 WHEN 'deferred_provider' THEN 2 WHEN 'blocked_config' THEN 3 ELSE 4 END, w.attempt_count DESC, julianday(w.updated_at) DESC, w.updated_at DESC, w.id DESC LIMIT 1",
        )
        .bind(identity_id)
        .fetch_optional(&mut **tx)
        .await?
    };
    let keep_id = keep_id_storage.as_deref();
    let active_rows = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT w.id, w.batch_id FROM content_work_identity_members m JOIN content_work_items w ON w.id = m.work_item_id WHERE m.identity_id = ? AND (w.status IN ('running', 'queued', 'deferred_provider', 'blocked_config') OR (w.status = 'failed' AND w.next_retry_at IS NOT NULL)) ORDER BY w.id",
    )
    .bind(identity_id)
    .fetch_all(&mut **tx)
    .await?;
    for (work_item_id, batch_id) in active_rows {
        if keep_id == Some(work_item_id.as_str()) {
            continue;
        }
        sqlx::query(
            "UPDATE content_work_items SET status = 'superseded', failure_class = 'model_identity_merged', lease_owner = NULL, lease_expires_at = NULL, next_retry_at = NULL, retry_expires_at = NULL, retry_after_at = NULL, finished_at = ?, updated_at = ? WHERE id = ? AND (status IN ('running', 'queued', 'deferred_provider', 'blocked_config') OR (status = 'failed' AND next_retry_at IS NOT NULL))",
        )
        .bind(now)
        .bind(now)
        .bind(&work_item_id)
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "UPDATE content_batch_items SET result_status = 'superseded', error_code = 'model_identity_merged', error_summary = 'duplicate model-specific work was reconciled', updated_at = ? WHERE work_item_id = ? AND result_status IS NULL",
        )
        .bind(now)
        .bind(&work_item_id)
        .execute(&mut **tx)
        .await?;
        if let Some(batch_id) = batch_id {
            sqlx::query(
                "UPDATE content_batches SET status = 'failed', error_code = 'model_identity_merged', error_summary = 'duplicate model-specific work was reconciled', finished_at = ?, updated_at = ? WHERE id = ? AND status = 'running'",
            )
            .bind(now)
            .bind(now)
            .bind(batch_id)
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

async fn process_blocked_config_batch(
    tx: &mut Transaction<'_, Sqlite>,
    cursor: &mut UpgradeCursor,
    now: &str,
    limit: i64,
    configuration_valid: bool,
) -> Result<bool> {
    let phase = &UpgradePhase::BlockedConfigRecovery;
    let progress = cursor.phase_mut(phase).expect("phase has progress");
    if progress.total.is_none() {
        progress.total = Some(phase_total(tx, phase).await?);
    }
    let identities = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT m.identity_id FROM content_work_identity_members m JOIN content_work_items w ON w.id = m.work_item_id WHERE w.status = 'blocked_config' AND (? IS NULL OR m.identity_id > ?) ORDER BY m.identity_id LIMIT ?",
    )
    .bind(progress.last_key.as_deref())
    .bind(progress.last_key.as_deref())
    .bind(limit)
    .fetch_all(&mut **tx)
    .await?;
    if identities.is_empty() {
        progress.completed_at = Some(now.to_owned());
        return Ok(true);
    }
    for identity_id in &identities {
        if configuration_valid && requeue_blocked_identity(tx, identity_id, now).await? {
            progress.requeued = progress.requeued.saturating_add(1);
        }
    }
    progress.last_key = identities.last().cloned();
    progress.processed = progress
        .processed
        .saturating_add(i64::try_from(identities.len()).unwrap_or(i64::MAX));
    Ok(false)
}

async fn requeue_blocked_identity(
    tx: &mut Transaction<'_, Sqlite>,
    identity_id: &str,
    now: &str,
) -> Result<bool> {
    if sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM content_current_result_projections WHERE identity_id = ?",
    )
    .bind(identity_id)
    .fetch_one(&mut **tx)
    .await?
        > 0
    {
        return Ok(false);
    }
    if sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM content_work_identity_members m JOIN content_work_items w ON w.id = m.work_item_id WHERE m.identity_id = ? AND w.status IN ('queued', 'running', 'deferred_provider')",
    )
    .bind(identity_id)
    .fetch_one(&mut **tx)
    .await?
        > 0
    {
        return Ok(false);
    }
    let Some(work_item_id) = sqlx::query_scalar::<_, String>(
        "SELECT w.id FROM content_work_identity_members m JOIN content_work_items w ON w.id = m.work_item_id WHERE m.identity_id = ? AND w.status = 'blocked_config' ORDER BY w.attempt_count DESC, julianday(w.updated_at) DESC, w.updated_at DESC, w.id DESC LIMIT 1",
    )
    .bind(identity_id)
    .fetch_optional(&mut **tx)
    .await?
    else {
        return Ok(false);
    };
    let pending_attempt = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(pending.attempt_no) FROM content_attempt_events pending WHERE pending.work_item_id = ? AND pending.event_type = 'attempt_queued' AND NOT EXISTS (SELECT 1 FROM content_attempt_events started WHERE started.work_item_id = pending.work_item_id AND started.attempt_no = pending.attempt_no AND started.event_type = 'attempt_started')",
    )
    .bind(&work_item_id)
    .fetch_one(&mut **tx)
    .await?;
    let attempt_no = match pending_attempt {
        Some(attempt_no) => attempt_no,
        None => {
            let max_attempt = sqlx::query_scalar::<_, Option<i64>>(
                "SELECT MAX(attempt_no) FROM content_attempt_events WHERE work_item_id = ?",
            )
            .bind(&work_item_id)
            .fetch_one(&mut **tx)
            .await?
            .unwrap_or(0);
            let work_attempt = sqlx::query_scalar::<_, i64>(
                "SELECT attempt_count FROM content_work_items WHERE id = ?",
            )
            .bind(&work_item_id)
            .fetch_one(&mut **tx)
            .await?;
            max_attempt.max(work_attempt).max(0).saturating_add(1)
        }
    };
    sqlx::query(
        "UPDATE content_work_items SET status = 'queued', priority = 0, failure_class = NULL, next_retry_at = NULL, retry_expires_at = NULL, retry_after_at = NULL, lease_owner = NULL, lease_expires_at = NULL, finished_at = NULL, updated_at = ? WHERE id = ? AND status = 'blocked_config'",
    )
    .bind(now)
    .bind(&work_item_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, retry_eligible, created_at) VALUES (?, ?, ?, 'automatic_recovery', 'attempt_queued', 'queued', 1, ?)",
    )
    .bind(local_id::generate_local_id().to_string())
    .bind(work_item_id)
    .bind(attempt_no)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(true)
}

async fn process_upgrade_batch(
    tx: &mut Transaction<'_, Sqlite>,
    configuration_valid: bool,
    batch_size: i64,
) -> Result<bool> {
    let control = load_control(tx).await?;
    if control.status == "completed" || control.status == "paused" || control.status == "failed" {
        return Ok(false);
    }
    if control.status == "pending" {
        sqlx::query(
            "UPDATE content_identity_upgrade_control SET status = 'running', started_at = COALESCE(started_at, CURRENT_TIMESTAMP), last_error_code = NULL, updated_at = CURRENT_TIMESTAMP WHERE id = 1 AND status = 'pending'",
        )
        .execute(&mut **tx)
        .await?;
    }
    let mut cursor = control.cursor;
    if cursor.version == 0 {
        cursor.version = 1;
    }
    let now = Utc::now().to_rfc3339();
    let phase_finished = match control.phase {
        UpgradePhase::WorkIdentityBackfill => {
            process_work_identity_batch(tx, &mut cursor, &now, batch_size).await?
        }
        UpgradePhase::ProjectionBackfill => {
            process_projection_batch(tx, &mut cursor, &now, batch_size).await?
        }
        UpgradePhase::BlockedConfigRecovery => {
            process_blocked_config_batch(tx, &mut cursor, &now, batch_size, configuration_valid)
                .await?
        }
        UpgradePhase::Complete => {
            sqlx::query(
                "UPDATE content_identity_upgrade_control SET status = 'completed', completed_at = COALESCE(completed_at, ?), updated_at = ? WHERE id = 1",
            )
            .bind(&now)
            .bind(&now)
            .execute(&mut **tx)
            .await?;
            return Ok(false);
        }
    };
    let next_phase = if phase_finished {
        control.phase.next()
    } else {
        control.phase
    };
    let is_complete = next_phase == UpgradePhase::Complete;
    if is_complete {
        cursor.search_index_refresh_pending = true;
        sqlx::query(
            "UPDATE content_identity_upgrade_control SET status = 'completed', phase = 'complete', cursor = ?, last_error_code = NULL, completed_at = ?, updated_at = ? WHERE id = 1",
        )
        .bind(encode_cursor(&cursor)?)
        .bind(&now)
        .bind(&now)
        .execute(&mut **tx)
        .await?;
    } else {
        sqlx::query(
            "UPDATE content_identity_upgrade_control SET status = 'running', phase = ?, cursor = ?, last_error_code = NULL, updated_at = ? WHERE id = 1",
        )
        .bind(next_phase.as_str())
        .bind(encode_cursor(&cursor)?)
        .bind(&now)
        .execute(&mut **tx)
        .await?;
    }
    Ok(true)
}

pub(crate) async fn run_one_batch(
    pool: &SqlitePool,
    writer: &SqliteWriteCoordinator,
    configuration_valid: bool,
    batch_size: i64,
) -> Result<bool> {
    let (_permit, mut tx) = writer
        .begin_immediate_with_priority(
            pool,
            "content_identity_upgrade_batch",
            SqliteWritePriority::Background,
        )
        .await?;
    let progressed = process_upgrade_batch(&mut tx, configuration_valid, batch_size).await?;
    tx.commit().await?;
    Ok(progressed)
}

pub(crate) async fn is_complete(pool: &SqlitePool) -> Result<bool> {
    let state = sqlx::query_as::<_, (String, String)>(
        "SELECT status, phase FROM content_identity_upgrade_control WHERE id = 1",
    )
    .fetch_one(pool)
    .await?;
    Ok(state.0 == "completed" && state.1 == "complete")
}

pub(crate) async fn is_complete_in_transaction(tx: &mut Transaction<'_, Sqlite>) -> Result<bool> {
    let state = sqlx::query_as::<_, (String, String)>(
        "SELECT status, phase FROM content_identity_upgrade_control WHERE id = 1",
    )
    .fetch_one(&mut **tx)
    .await?;
    Ok(state.0 == "completed" && state.1 == "complete")
}

pub(crate) async fn requeue_blocked_config(
    pool: &SqlitePool,
    writer: &SqliteWriteCoordinator,
    configuration_valid: bool,
) -> Result<i64> {
    if !configuration_valid {
        return Ok(0);
    }
    let mut requeued = 0_i64;
    loop {
        let (_permit, mut tx) = writer
            .begin_immediate_with_priority(
                pool,
                "content_identity_config_recovery",
                SqliteWritePriority::Background,
            )
            .await?;
        let upgrade_state: (String, String) = sqlx::query_as(
            "SELECT status, phase FROM content_identity_upgrade_control WHERE id = 1",
        )
        .fetch_one(&mut *tx)
        .await?;
        let recovery_phase_open = upgrade_state.0 == "running"
            && upgrade_state.1 == UpgradePhase::BlockedConfigRecovery.as_str();
        if upgrade_state.0 != "completed" && !recovery_phase_open {
            tx.rollback().await?;
            return Ok(requeued);
        }
        let identities = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT m.identity_id FROM content_work_identity_members m JOIN content_work_items w ON w.id = m.work_item_id WHERE w.status = 'blocked_config' AND NOT EXISTS (SELECT 1 FROM content_current_result_projections p WHERE p.identity_id = m.identity_id) ORDER BY m.identity_id LIMIT ?",
        )
        .bind(BATCH_SIZE)
        .fetch_all(&mut *tx)
        .await?;
        let now = Utc::now().to_rfc3339();
        for identity_id in &identities {
            if requeue_blocked_identity(&mut tx, identity_id, &now).await? {
                requeued = requeued.saturating_add(1);
            }
        }
        let count = identities.len();
        tx.commit().await?;
        if count < usize::try_from(BATCH_SIZE).unwrap_or(usize::MAX) {
            break;
        }
    }
    Ok(requeued)
}

pub fn spawn_worker(state: Arc<AppState>) -> tokio::task::AbortHandle {
    tokio::spawn(async move {
        loop {
            let configuration_valid = current_configuration_valid(state.as_ref()).await;
            match run_one_batch(
                &state.pool,
                &state.sqlite_writer,
                configuration_valid,
                BATCH_SIZE,
            )
            .await
            {
                Ok(true) => tokio::task::yield_now().await,
                Ok(false) => match is_complete(&state.pool).await {
                    Ok(true) => match refresh_search_index_if_pending(&state).await {
                        Ok(true) => break,
                        Ok(false) => tokio::time::sleep(POLL_INTERVAL).await,
                        Err(error) => {
                            tracing::warn!(?error, "content identity search index refresh failed");
                            tokio::time::sleep(POLL_INTERVAL).await;
                        }
                    },
                    Ok(false) => tokio::time::sleep(POLL_INTERVAL).await,
                    Err(error) => {
                        tracing::warn!(?error, "content identity upgrade status read failed");
                        tokio::time::sleep(POLL_INTERVAL).await;
                    }
                },
                Err(error) => {
                    tracing::warn!(?error, "content identity upgrade batch failed");
                    if let Err(mark_error) =
                        mark_failed(&state, "identity_upgrade_batch_failed").await
                    {
                        tracing::warn!(
                            ?mark_error,
                            "content identity upgrade failure state could not be saved"
                        );
                    }
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
            }
        }
    })
    .abort_handle()
}

async fn current_configuration_valid(state: &AppState) -> bool {
    let Some(config) = state.config.ai.as_ref() else {
        return false;
    };
    if config.api_key.trim().is_empty() || config.base_url.as_str().trim().is_empty() {
        return false;
    }
    let routing = state
        .llm_scheduler
        .routing_status(Some(config.model.as_str()))
        .await;
    !routing.llm_models.is_empty()
}

async fn mark_failed(state: &AppState, error_code: &str) -> Result<()> {
    let (_permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "content_identity_upgrade_failure",
            SqliteWritePriority::Background,
        )
        .await?;
    sqlx::query(
        "UPDATE content_identity_upgrade_control SET status = 'failed', last_error_code = ?, updated_at = CURRENT_TIMESTAMP WHERE id = 1 AND status IN ('pending', 'running')",
    )
    .bind(error_code)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn refresh_search_index_if_pending(state: &AppState) -> Result<bool> {
    let (control, _, _) = read_status(&state.pool).await?;
    if !control.cursor.search_index_refresh_pending {
        return Ok(true);
    }
    if !crate::search_index::refresh_content_projection_phase(state).await? {
        return Ok(false);
    }
    let (_permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "content_identity_search_refresh_complete",
            SqliteWritePriority::Background,
        )
        .await?;
    let mut control = load_control(&mut tx).await?;
    control.cursor.search_index_refresh_pending = false;
    sqlx::query(
        "UPDATE content_identity_upgrade_control SET cursor = ?, updated_at = CURRENT_TIMESTAMP WHERE id = 1 AND status = 'completed'",
    )
    .bind(encode_cursor(&control.cursor)?)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

async fn read_status(pool: &SqlitePool) -> Result<(UpgradeControl, String, i64)> {
    let row = sqlx::query(
        "SELECT generation, status, phase, cursor, last_error_code, started_at, completed_at, updated_at FROM content_identity_upgrade_control WHERE id = 1",
    )
    .fetch_one(pool)
    .await?;
    let control = UpgradeControl {
        generation: row.get("generation"),
        status: row.get("status"),
        phase: UpgradePhase::parse(row.get::<&str, _>("phase"))?,
        cursor: UpgradeCursor::decode(row.try_get::<Option<String>, _>("cursor")?.as_deref())?,
        last_error_code: row.get("last_error_code"),
        started_at: row.get("started_at"),
        completed_at: row.get("completed_at"),
    };
    let updated_at = row.get("updated_at");
    let waiting: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (SELECT canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version FROM content_work_items WHERE status = 'blocked_config' GROUP BY canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version) blocked WHERE NOT EXISTS (SELECT 1 FROM content_work_identities i JOIN content_current_result_projections p ON p.identity_id = i.id WHERE i.canonical_resource_type = blocked.canonical_resource_type AND i.canonical_resource_id = blocked.canonical_resource_id AND i.pipeline = blocked.pipeline AND i.variant = blocked.variant AND i.target_lang = blocked.target_lang AND i.source_hash = blocked.source_hash AND i.protocol_version = blocked.protocol_version)",
    )
    .fetch_one(pool)
    .await?;
    Ok((control, updated_at, waiting))
}

fn progress_response(progress: &PhaseProgress) -> PhaseProgressResponse {
    PhaseProgressResponse {
        processed: progress.processed,
        total: progress.total,
        completed_at: progress.completed_at.clone(),
        requeued: progress.requeued,
    }
}

async fn status_response(pool: &SqlitePool) -> Result<UpgradeStatusResponse> {
    let (control, updated_at, waiting) = read_status(pool).await?;
    Ok(UpgradeStatusResponse {
        generation: control.generation,
        status: control.status,
        phase: control.phase.as_str().to_owned(),
        last_error_code: control.last_error_code,
        started_at: control.started_at,
        completed_at: control.completed_at,
        search_index_refresh_pending: control.cursor.search_index_refresh_pending,
        work_identity_backfill: progress_response(&control.cursor.work_identity_backfill),
        projection_backfill: progress_response(&control.cursor.projection_backfill),
        blocked_config_recovery: progress_response(&control.cursor.blocked_config_recovery),
        waiting_blocked_config_identities: waiting,
        updated_at,
    })
}

pub async fn admin_get_status(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<UpgradeStatusResponse>, ApiError> {
    let _ = api::require_admin_user_id(state.as_ref(), &session).await?;
    status_response(&state.pool)
        .await
        .map(Json)
        .map_err(ApiError::internal)
}

pub async fn admin_control(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(request): Json<UpgradeControlRequest>,
) -> Result<Json<UpgradeStatusResponse>, ApiError> {
    let _ = api::require_admin_user_id(state.as_ref(), &session).await?;
    let (_permit, mut tx) = state
        .sqlite_writer
        .begin_immediate(&state.pool, "content_identity_upgrade_control")
        .await
        .map_err(ApiError::internal)?;
    let status: String =
        sqlx::query_scalar("SELECT status FROM content_identity_upgrade_control WHERE id = 1")
            .fetch_one(&mut *tx)
            .await
            .map_err(ApiError::internal)?;
    let now = Utc::now().to_rfc3339();
    match request.action {
        UpgradeAction::Pause if matches!(status.as_str(), "pending" | "running") => {
            sqlx::query(
                "UPDATE content_identity_upgrade_control SET status = 'paused', updated_at = ? WHERE id = 1 AND status IN ('pending', 'running')",
            )
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(ApiError::internal)?;
        }
        UpgradeAction::Resume if matches!(status.as_str(), "paused" | "failed") => {
            sqlx::query(
                "UPDATE content_identity_upgrade_control SET status = 'running', last_error_code = NULL, started_at = COALESCE(started_at, ?), updated_at = ? WHERE id = 1 AND status IN ('paused', 'failed')",
            )
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(ApiError::internal)?;
        }
        _ => {
            tx.rollback().await.map_err(ApiError::internal)?;
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "content_identity_upgrade_state_conflict",
                "identity upgrade cannot perform that action in its current state",
            ));
        }
    }
    tx.commit().await.map_err(ApiError::internal)?;
    status_response(&state.pool)
        .await
        .map(Json)
        .map_err(ApiError::internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect memory database");
        crate::database_migrations::run(&pool)
            .await
            .expect("run migrations");
        pool
    }

    async fn insert_work(pool: &SqlitePool, id: &str, model: &str, status: &str) {
        sqlx::query(
            "INSERT INTO content_work_items (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, source_hash, protocol_version, model_profile, source_snapshot_json, configuration_fingerprint, status, attempt_count, created_at, updated_at) VALUES (?, 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'hash-1', 'content-processing.v1', ?, '{}', 'legacy-config', ?, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .bind(id)
        .bind(model)
        .bind(status)
        .execute(pool)
        .await
        .expect("insert work item");
    }

    async fn run_to_completion(pool: &SqlitePool, valid: bool) {
        let writer = SqliteWriteCoordinator::new();
        for _ in 0..40 {
            run_one_batch(pool, &writer, valid, 2)
                .await
                .expect("run identity upgrade batch");
            if is_complete(pool).await.expect("read completion") {
                return;
            }
        }
        panic!("identity upgrade did not complete");
    }

    #[tokio::test]
    async fn upgrade_preserves_model_history_and_chooses_latest_valid_projection() {
        let pool = pool().await;
        insert_work(&pool, "model-a-work", "model-a", "blocked_config").await;
        insert_work(&pool, "model-b-work", "model-b", "ready").await;
        insert_work(&pool, "model-c-work", "model-c", "ready").await;
        sqlx::query(
            "INSERT INTO content_result_projections (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash, work_item_id, payload_json, published_at, updated_at) VALUES ('projection-b', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'content-processing.v1', 'model-b', 'hash-1', 'model-b-work', '{\"title_zh\":\"earlier\"}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'), ('projection-c', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'content-processing.v1', 'model-c', 'hash-1', 'model-c-work', '{\"title_zh\":\"latest\"}', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .expect("insert historical projections");
        sqlx::query(
            "INSERT INTO content_attempt_events (id, work_item_id, attempt_no, trigger, event_type, result_status, created_at) VALUES ('blocked-start', 'model-a-work', 1, 'initial', 'attempt_started', NULL, CURRENT_TIMESTAMP), ('blocked-end', 'model-a-work', 1, 'initial', 'attempt_completed', 'blocked_config', CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("insert blocked attempt history");

        run_to_completion(&pool, true).await;

        let current: (String, String, String) = sqlx::query_as(
            "SELECT p.work_item_id, p.source_projection_id, p.payload_json FROM content_current_result_projections p JOIN content_work_identities i ON i.id = p.identity_id WHERE i.source_hash = 'hash-1'",
        )
        .fetch_one(&pool)
        .await
        .expect("current result projection");
        assert_eq!(current.0, "model-c-work");
        assert_eq!(current.1, "projection-c");
        assert_eq!(
            serde_json::from_str::<Value>(&current.2).unwrap()["title_zh"],
            "latest"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_work_identity_members WHERE identity_id = (SELECT identity_id FROM content_work_identity_members WHERE work_item_id = 'model-a-work')",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            3
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_events WHERE work_item_id = 'model-a-work'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_work_items WHERE id IN ('model-a-work', 'model-b-work', 'model-c-work')",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            3
        );
    }

    #[tokio::test]
    async fn upgrade_recovers_one_blocked_item_per_identity_only_when_configuration_is_valid() {
        let pool = pool().await;
        insert_work(&pool, "blocked-a", "model-a", "blocked_config").await;
        insert_work(&pool, "blocked-b", "model-b", "blocked_config").await;

        run_to_completion(&pool, false).await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_work_items WHERE status = 'blocked_config'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_work_items WHERE status = 'queued'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );

        let writer = SqliteWriteCoordinator::new();
        assert_eq!(
            requeue_blocked_config(&pool, &writer, true)
                .await
                .expect("configuration reload recovery"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_work_items WHERE status = 'queued'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_attempt_events WHERE event_type = 'attempt_queued' AND trigger = 'automatic_recovery'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            requeue_blocked_config(&pool, &writer, true)
                .await
                .expect("idempotent configuration reload recovery"),
            0
        );
    }

    #[tokio::test]
    async fn upgrade_batches_resume_after_pause_without_rewriting_completed_rows() {
        let pool = pool().await;
        insert_work(&pool, "work-a", "model-a", "blocked_config").await;
        let writer = SqliteWriteCoordinator::new();
        run_one_batch(&pool, &writer, false, 1)
            .await
            .expect("first batch");
        sqlx::query("UPDATE content_identity_upgrade_control SET status = 'paused' WHERE id = 1")
            .execute(&pool)
            .await
            .expect("pause upgrade");
        assert!(
            !run_one_batch(&pool, &writer, false, 1)
                .await
                .expect("paused batch does not advance")
        );
        sqlx::query("UPDATE content_identity_upgrade_control SET status = 'running' WHERE id = 1")
            .execute(&pool)
            .await
            .expect("resume upgrade");
        run_to_completion(&pool, false).await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM content_work_identities")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn work_backfill_catches_rows_inserted_before_the_last_seen_id() {
        let pool = pool().await;
        insert_work(&pool, "work-a", "model-a", "blocked_config").await;
        let writer = SqliteWriteCoordinator::new();
        run_one_batch(&pool, &writer, false, 1)
            .await
            .expect("first work mapping batch");

        // Simulate a late writer whose ID sorts before the last observed key.
        insert_work(&pool, "work-0", "model-b", "blocked_config").await;
        run_to_completion(&pool, false).await;

        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_work_identity_members WHERE work_item_id IN ('work-a', 'work-0')",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(DISTINCT identity_id) FROM content_work_identity_members WHERE work_item_id IN ('work-a', 'work-0')",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn status_counts_blocked_identities_before_mapping_and_excludes_valid_results() {
        let pool = pool().await;
        insert_work(&pool, "blocked-a", "model-a", "blocked_config").await;
        insert_work(&pool, "blocked-b", "model-b", "blocked_config").await;
        assert_eq!(read_status(&pool).await.unwrap().2, 1);

        insert_work(&pool, "ready-c", "model-c", "ready").await;
        sqlx::query(
            "INSERT INTO content_result_projections (id, canonical_resource_type, canonical_resource_id, pipeline, variant, target_lang, protocol_version, model_profile, source_hash, work_item_id, payload_json, published_at, updated_at) VALUES ('projection-c', 'release', 'release-1', 'translation', 'summary', 'zh-CN', 'content-processing.v1', 'model-c', 'hash-1', 'ready-c', '{\"title_zh\":\"existing\"}', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let identity_id = ensure_identity_for_work(&mut tx, "ready-c", "2026-01-01T00:00:00Z")
            .await
            .unwrap();
        ensure_current_projection_for_key(&mut tx, &identity_id, "2026-01-01T00:00:00Z")
            .await
            .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(read_status(&pool).await.unwrap().2, 0);
    }
}
