//! PostgreSQL AI collection reads. Preserve the existing collection evidence and status algorithms.
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    future::Future,
    sync::{Arc, OnceLock},
    time::Duration,
};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{DateTime, NaiveDateTime, SecondsFormat, Timelike, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection as SqliteConnection, PgPool as SqlitePool, Postgres};
use tower_sessions::Session;

use crate::{
    error::ApiError,
    postgres_server::{PostgresServerState, require_admin},
    translations,
};
#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
}

const PAGE_SIZE_DEFAULT: i64 = 20;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum CollectionRecordKind {
    Release,
    Announcement,
    Notification,
    Brief,
}

impl CollectionRecordKind {
    fn parse(raw: &str) -> Result<Self, ApiError> {
        match raw {
            "release" => Ok(Self::Release),
            "announcement" => Ok(Self::Announcement),
            "notification" => Ok(Self::Notification),
            "brief" => Ok(Self::Brief),
            _ => Err(ApiError::bad_request("invalid collection record kind")),
        }
    }

    fn task_kinds(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Release => Some(("release_detail", "release_smart")),
            Self::Announcement => Some(("announcement_detail", "announcement_smart")),
            Self::Notification => Some(("notification", "notification_smart")),
            Self::Brief => None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AdminCollectionRecordListQuery {
    pub from: Option<String>,
    pub before: Option<String>,
    pub attempt_min: Option<i64>,
    pub attempt_max: Option<i64>,
    pub translation_status: Option<String>,
    pub polish_status: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AdminContentProcessingEvidence {
    pub status: String,
    pub status_origin: String,
    pub work_item_id: Option<String>,
    pub source_hash: Option<String>,
    pub updated_at: Option<String>,
    pub legacy_table: Option<String>,
    pub legacy_primary_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AdminCollectionTaskSummary {
    pub status: String,
    pub display_status: String,
    pub status_origin: String,
    pub attempt_count: i64,
    pub started_at: Option<String>,
    pub last_attempt_at: Option<String>,
    pub finished_at: Option<String>,
    pub global_work: Option<AdminContentProcessingEvidence>,
    pub result_projection: Option<AdminContentProcessingEvidence>,
    pub legacy_evidence: Option<AdminContentProcessingEvidence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdminCollectionRecordItem {
    pub id: String,
    pub kind: String,
    pub repository: Option<String>,
    pub title: String,
    pub occurred_at: Option<String>,
    pub detected_at: Option<String>,
    pub generated_at: Option<String>,
    pub translation: Option<AdminCollectionTaskSummary>,
    pub polish: AdminCollectionTaskSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdminCollectionRecordsResponse {
    pub items: Vec<AdminCollectionRecordItem>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdminCollectionActivityCell {
    pub id: String,
    pub title: String,
    pub repository: Option<String>,
    pub source_time: String,
    pub translation_status: Option<String>,
    pub polish_status: String,
    pub composite_status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdminCollectionActivityBucket {
    pub started_at: String,
    pub ended_at: String,
    pub cells: Vec<AdminCollectionActivityCell>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AdminCollectionActivitySummary {
    pub content_count: usize,
    pub completed_count: usize,
    pub processing_count: usize,
    pub exception_count: usize,
    pub neutral_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdminCollectionActivityResponse {
    pub kind: String,
    pub bucket_minutes: u8,
    pub bucket_count: u8,
    pub window_started_at: String,
    pub window_ended_at: String,
    pub summary: AdminCollectionActivitySummary,
    pub buckets: Vec<AdminCollectionActivityBucket>,
}

#[derive(Debug, sqlx::FromRow, Deserialize)]
struct AdminCollectionActivityRow {
    id: String,
    title: String,
    repository: Option<String>,
    source_time: String,
    translation_status: Option<String>,
    polish_status: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, Deserialize)]
pub struct AdminCollectionLlmLink {
    pub id: String,
    pub status: String,
    pub source: String,
    pub model: String,
    #[sqlx(default)]
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[sqlx(default)]
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation_role: Option<String>,
    #[sqlx(default)]
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_availability: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdminCollectionAttempt {
    pub id: String,
    pub pipeline: String,
    pub attempt_no: i64,
    pub trigger: String,
    pub status: String,
    pub started_at: Option<String>,
    pub last_attempt_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_code: Option<String>,
    pub error_summary: Option<String>,
    pub failure_class: Option<String>,
    pub processing_stage: Option<String>,
    pub provider_status: Option<String>,
    pub output_contract_status: Option<String>,
    pub retry_disposition: Option<String>,
    pub retry_eligible: bool,
    pub next_retry_at: Option<String>,
    pub llm_calls: Vec<AdminCollectionLlmLink>,
}

#[derive(Debug, Serialize)]
pub struct AdminCollectionRecordDetail {
    pub record: AdminCollectionRecordItem,
    pub attempts: Vec<AdminCollectionAttempt>,
}

#[derive(Debug, sqlx::FromRow, Deserialize)]
struct SourceRecordRow {
    id: String,
    repository: Option<String>,
    title: String,
    occurred_at: Option<String>,
    detected_at: Option<String>,
    generated_at: Option<String>,
    #[sqlx(default)]
    #[serde(default)]
    total_count: i64,
}

#[derive(Debug, sqlx::FromRow, Deserialize)]
struct TaskRow {
    id: String,
    kind: String,
    status: String,
    result_status: Option<String>,
    attempt_count: i64,
    started_at: Option<String>,
    finished_at: Option<String>,
    updated_at: String,
    last_attempt_at: Option<String>,
}

#[derive(Debug, sqlx::FromRow, Deserialize)]
struct TaskWithEntityRow {
    #[sqlx(flatten)]
    #[serde(flatten)]
    task: TaskRow,
    entity_id: String,
}

#[derive(Debug, Clone, sqlx::FromRow, Deserialize)]
struct ProcessingCoverageRow {
    record_id: String,
    pipeline: String,
    status_origin: String,
}

#[derive(Debug, sqlx::FromRow, Deserialize)]
struct AttemptEventRow {
    work_item_id: String,
    attempt_no: i64,
    trigger: String,
    event_type: String,
    result_status: Option<String>,
    error_code: Option<String>,
    error_summary: Option<String>,
    failure_class: Option<String>,
    processing_stage: Option<String>,
    provider_status: Option<String>,
    output_contract_status: Option<String>,
    retry_disposition: Option<String>,
    retry_eligible: i64,
    next_retry_at: Option<String>,
    llm_call_ids_json: String,
    created_at: String,
}

#[derive(Debug, sqlx::FromRow, Deserialize)]
struct AttemptCallLinkRow {
    work_item_id: String,
    attempt_no: i64,
    llm_call_id: String,
    stage: String,
    relation_role: String,
    evidence_availability: String,
}

#[derive(Default)]
struct AttemptBuild {
    pipeline: String,
    attempt_no: i64,
    trigger: String,
    status: String,
    started_at: Option<String>,
    last_attempt_at: String,
    finished_at: Option<String>,
    error_code: Option<String>,
    error_summary: Option<String>,
    failure_class: Option<String>,
    processing_stage: Option<String>,
    provider_status: Option<String>,
    output_contract_status: Option<String>,
    retry_disposition: Option<String>,
    retry_eligible: bool,
    next_retry_at: Option<String>,
    llm_calls: Vec<AdminCollectionLlmLink>,
}

fn apply_attempt_event(item: &mut AttemptBuild, event: &AttemptEventRow) {
    item.trigger = event.trigger.clone();
    item.last_attempt_at = event.created_at.clone();
    match event.event_type.as_str() {
        "attempt_queued" => {
            item.status = "queued".to_owned();
            item.started_at = None;
            item.finished_at = None;
            item.retry_eligible = false;
            item.next_retry_at = None;
        }
        "attempt_started" => {
            item.status = "running".to_owned();
            item.started_at = Some(event.created_at.clone());
        }
        "attempt_completed" => {
            item.status = event
                .result_status
                .clone()
                .unwrap_or_else(|| "completed".to_owned());
            item.finished_at = Some(event.created_at.clone());
        }
        "retry_scheduled" => {
            item.status = "retry_scheduled".to_owned();
            item.finished_at = Some(event.created_at.clone());
        }
        _ => {}
    }
    if let Some(value) = &event.error_code {
        item.error_code = Some(value.clone());
    }
    if let Some(value) = &event.error_summary {
        item.error_summary = Some(value.clone());
    }
    if let Some(value) = &event.failure_class {
        item.failure_class = Some(value.clone());
    }
    if let Some(value) = &event.processing_stage {
        item.processing_stage = Some(value.clone());
    }
    if let Some(value) = &event.provider_status {
        item.provider_status = Some(value.clone());
    }
    if let Some(value) = &event.output_contract_status {
        item.output_contract_status = Some(value.clone());
    }
    if let Some(value) = &event.retry_disposition {
        item.retry_disposition = Some(value.clone());
    }
    if event.event_type != "attempt_queued" {
        item.retry_eligible |= event.retry_eligible != 0;
        if let Some(value) = &event.next_retry_at {
            item.next_retry_at = Some(value.clone());
        }
    }
}

#[derive(Debug, sqlx::FromRow, Deserialize)]
struct BriefCallRow {
    id: String,
    status: String,
    source: String,
    attempt_count: i64,
    started_at: Option<String>,
    finished_at: Option<String>,
    updated_at: String,
    last_attempt_at: Option<String>,
    error_text: Option<String>,
}

#[derive(Debug, sqlx::FromRow, Deserialize)]
struct BriefAttemptEventRow {
    call_id: String,
    event_type: String,
    status: String,
    model: Option<String>,
    attempt: Option<i64>,
    terminal_attempt_count: Option<i64>,
    failure_class: Option<String>,
    created_at: String,
}

fn parse_timestamp(value: Option<String>, field: &str) -> Result<Option<String>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| ApiError::bad_request(format!("invalid {field} timestamp")))?;
    Ok(Some(parsed.with_timezone(&Utc).to_rfc3339()))
}

fn normalize_collection_window(
    from: Option<String>,
    before: Option<String>,
) -> Result<(String, String), ApiError> {
    let now = Utc::now();
    let parsed_from = from
        .as_deref()
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|parsed| parsed.with_timezone(&Utc))
                .map_err(|_| ApiError::bad_request("invalid from timestamp"))
        })
        .transpose()?;
    let parsed_before = before
        .as_deref()
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|parsed| parsed.with_timezone(&Utc))
                .map_err(|_| ApiError::bad_request("invalid before timestamp"))
        })
        .transpose()?;
    let before = parsed_before.unwrap_or(now);
    let from = parsed_from.unwrap_or(before - chrono::Duration::days(31));
    let span = before.signed_duration_since(from);
    if span < chrono::Duration::zero() {
        return Err(ApiError::bad_request("before must be after from"));
    }
    if span > chrono::Duration::days(31) {
        return Err(ApiError::bad_request(
            "collection record window cannot exceed 31 days",
        ));
    }
    Ok((from.to_rfc3339(), before.to_rfc3339()))
}

fn collection_list_window_key(
    requested_from: Option<String>,
    requested_before: Option<String>,
    normalized_from: String,
    normalized_before: String,
) -> (Option<String>, Option<String>) {
    match (requested_from, requested_before) {
        (None, None) => (None, None),
        (None, Some(_)) => (Some(normalized_from), Some(normalized_before)),
        (Some(from), None) => (Some(from), None),
        (Some(_), Some(_)) => (Some(normalized_from), Some(normalized_before)),
    }
}

const DISPLAY_STATUSES: [&str; 15] = [
    "not_started",
    "queued",
    "running",
    "succeeded",
    "failed",
    "missing",
    "disabled",
    "historical_unknown",
    "legacy_cached",
    "legacy_conflict",
    "deferred_provider",
    "blocked_config",
    "cancelled",
    "superseded",
    "not_applicable",
];

fn display_status_for(status: &str, status_origin: &str) -> String {
    match status {
        "queued" | "batched" | "retry_scheduled" => "queued".to_owned(),
        "running" => "running".to_owned(),
        "completed" | "succeeded" | "ready" => "succeeded".to_owned(),
        "failed" | "error" => "failed".to_owned(),
        "missing" => "missing".to_owned(),
        "disabled" => "disabled".to_owned(),
        "legacy_cached" => "legacy_cached".to_owned(),
        "legacy_conflict" => "legacy_conflict".to_owned(),
        "not_recorded" if status_origin == "never_started" => "not_started".to_owned(),
        "not_recorded" => "historical_unknown".to_owned(),
        _ if status.is_empty() && status_origin == "never_started" => "not_started".to_owned(),
        _ if status.is_empty() => "historical_unknown".to_owned(),
        other => other.to_owned(),
    }
}

fn parse_status_filter(raw: Option<String>, field: &str) -> Result<Option<Vec<String>>, ApiError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let mut values = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    if values.is_empty() {
        return Ok(None);
    }
    if let Some(value) = values
        .iter()
        .find(|value| !DISPLAY_STATUSES.contains(&value.as_str()))
    {
        return Err(ApiError::bad_request(format!(
            "{field} contains invalid status {value}"
        )));
    }
    Ok(Some(values))
}

fn collection_record_kind_label(kind: CollectionRecordKind) -> &'static str {
    match kind {
        CollectionRecordKind::Release => "release",
        CollectionRecordKind::Announcement => "announcement",
        CollectionRecordKind::Notification => "notification",
        CollectionRecordKind::Brief => "brief",
    }
}

async fn load_processing_coverage(
    pool: &SqlitePool,
    kind: CollectionRecordKind,
    record_ids: &[String],
) -> Result<HashMap<(String, String), String>, ApiError> {
    let mut connection = pool.acquire().await.map_err(ApiError::internal)?;
    load_processing_coverage_in_connection(&mut connection, kind, record_ids).await
}

async fn load_processing_coverage_in_connection(
    connection: &mut SqliteConnection,
    kind: CollectionRecordKind,
    record_ids: &[String],
) -> Result<HashMap<(String, String), String>, ApiError> {
    if record_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut query = PgReadBuilder::new(
        "SELECT record_id, pipeline, status_origin FROM admin_collection_processing_coverage WHERE record_kind = ",
    );
    query.push_bind(collection_record_kind_label(kind));
    query.push(" AND record_id IN (");
    let mut separated = query.separated(", ");
    for record_id in record_ids {
        separated.push_bind(record_id);
    }
    query.push(")");
    let rows = query
        .build_query_as::<ProcessingCoverageRow>()
        .fetch_all(&mut *connection)
        .await
        .map_err(ApiError::internal)?;
    Ok(rows
        .into_iter()
        .map(|row| ((row.record_id, row.pipeline), row.status_origin))
        .collect())
}

fn coverage_origin(
    coverage: &HashMap<(String, String), String>,
    record_id: &str,
    pipeline: &str,
) -> String {
    coverage
        .get(&(record_id.to_owned(), pipeline.to_owned()))
        .cloned()
        .unwrap_or_else(|| "historical_unknown".to_owned())
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct AttemptCountRange {
    min: i64,
    max: Option<i64>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct CollectionListKey {
    kind: CollectionRecordKind,
    from: Option<String>,
    before: Option<String>,
    attempts: AttemptCountRange,
    translation_filter: Vec<String>,
    polish_filter: Vec<String>,
    page: i64,
    page_size: i64,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct CollectionActivityKey {
    kind: CollectionRecordKind,
    window_started_at: String,
    window_ended_at: String,
}

fn parse_attempt_count_range(
    query: &AdminCollectionRecordListQuery,
) -> Result<AttemptCountRange, ApiError> {
    let min = query.attempt_min.unwrap_or(0);
    let max = query.attempt_max;
    if !(0..=10).contains(&min) {
        return Err(ApiError::bad_request(
            "attempt_min must be between 0 and 10",
        ));
    }
    if let Some(value) = max {
        if !(0..=10).contains(&value) {
            return Err(ApiError::bad_request(
                "attempt_max must be between 0 and 10",
            ));
        }
        if value < min {
            return Err(ApiError::bad_request(
                "attempt_max must be greater than or equal to attempt_min",
            ));
        }
    }
    Ok(AttemptCountRange { min, max })
}

fn pagination(query: &AdminCollectionRecordListQuery) -> Result<(i64, i64, i64), ApiError> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(PAGE_SIZE_DEFAULT).clamp(1, 100);
    let offset = (page - 1)
        .checked_mul(page_size)
        .ok_or_else(|| ApiError::bad_request("page is too large"))?;
    Ok((page, page_size, offset))
}

fn source_record_item(
    kind: CollectionRecordKind,
    row: SourceRecordRow,
    task_summaries: &HashMap<String, TaskSummaries>,
    brief_summaries: &HashMap<String, AdminCollectionTaskSummary>,
) -> AdminCollectionRecordItem {
    let tasks = task_summaries
        .get(&row.id)
        .cloned()
        .unwrap_or_else(|| TaskSummaries {
            translation: not_recorded_summary(),
            polish: not_recorded_summary(),
        });
    AdminCollectionRecordItem {
        id: row.id.clone(),
        kind: match kind {
            CollectionRecordKind::Release => "release",
            CollectionRecordKind::Announcement => "announcement",
            CollectionRecordKind::Notification => "notification",
            CollectionRecordKind::Brief => "brief",
        }
        .to_owned(),
        repository: row.repository,
        title: row.title,
        occurred_at: row.occurred_at,
        detected_at: row.detected_at,
        generated_at: row.generated_at,
        translation: (kind != CollectionRecordKind::Brief).then_some(tasks.translation),
        polish: if kind == CollectionRecordKind::Brief {
            brief_summaries
                .get(&row.id)
                .cloned()
                .unwrap_or_else(not_recorded_summary)
        } else {
            tasks.polish
        },
    }
}

#[derive(Clone)]
struct TaskSummaries {
    translation: AdminCollectionTaskSummary,
    polish: AdminCollectionTaskSummary,
}

fn not_recorded_summary() -> AdminCollectionTaskSummary {
    not_recorded_summary_for("historical_unknown")
}

fn not_recorded_summary_for(status_origin: &str) -> AdminCollectionTaskSummary {
    AdminCollectionTaskSummary {
        status: "not_recorded".to_owned(),
        display_status: display_status_for("not_recorded", status_origin),
        status_origin: status_origin.to_owned(),
        ..Default::default()
    }
}

fn legacy_evidence_summary(evidence: AdminContentProcessingEvidence) -> AdminCollectionTaskSummary {
    let status = evidence.status.clone();
    AdminCollectionTaskSummary {
        status: status.clone(),
        display_status: status.clone(),
        status_origin: "legacy_evidence".to_owned(),
        legacy_evidence: Some(evidence),
        ..Default::default()
    }
}

#[derive(Debug, sqlx::FromRow, Deserialize)]
struct GlobalTaskRow {
    pipeline: String,
    source_hash: String,
    status: String,
    attempt_count: i64,
    started_at: Option<String>,
    finished_at: Option<String>,
    updated_at: String,
    last_attempt_at: Option<String>,
    canonical_resource_id: String,
    projection_work_item_id: Option<String>,
    projection_source_hash: Option<String>,
    projection_updated_at: Option<String>,
}

#[derive(Debug, sqlx::FromRow, Deserialize)]
struct GlobalAttemptRow {
    event_id: String,
    work_item_id: String,
    pipeline: String,
    attempt_no: i64,
    trigger: String,
    event_type: String,
    result_status: Option<String>,
    error_code: Option<String>,
    error_summary: Option<String>,
    failure_class: Option<String>,
    retry_eligible: i64,
    next_retry_at: Option<String>,
    created_at: String,
}

fn missing_table(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database) if database.code().as_deref() == Some("42P01"))
}

async fn load_global_task_rows_in_connection(
    connection: &mut SqliteConnection,
    kind: CollectionRecordKind,
    entity_ids: &[String],
) -> Result<Vec<GlobalTaskRow>, ApiError> {
    if entity_ids.is_empty() {
        return Ok(Vec::new());
    }
    let identity_upgrade_state = match pg_query_as::<(String, String)>(
        "SELECT status, phase FROM content_identity_upgrade_control WHERE id = 1",
    )
    .fetch_optional(&mut *connection)
    .await
    {
        Ok(state) => state,
        Err(error) if missing_table(&error) => None,
        Err(error) => return Err(ApiError::internal(error)),
    };
    let identity_upgrade_complete = identity_upgrade_state
        .is_some_and(|(status, phase)| status == "completed" && phase == "complete");
    let source = if identity_upgrade_complete {
        "WITH ranked_members AS (SELECT m.identity_id, w.*, ROW_NUMBER() OVER (PARTITION BY m.identity_id ORDER BY CASE w.status WHEN 'queued' THEN 0 WHEN 'running' THEN 1 WHEN 'deferred_provider' THEN 2 WHEN 'blocked_config' THEN 3 WHEN 'failed' THEN 4 WHEN 'ready' THEN 5 WHEN 'superseded' THEN 9 ELSE 6 END, w.attempt_count DESC, julianday(w.updated_at) DESC, w.updated_at DESC, w.id DESC) AS member_rank FROM content_work_identity_members m JOIN content_work_items w ON w.id = m.work_item_id), canonical_identity_work AS (SELECT * FROM ranked_members WHERE member_rank = 1), ranked_global_work AS (SELECT *, ROW_NUMBER() OVER (PARTITION BY canonical_resource_id, CASE WHEN pipeline = 'polishing' THEN 'polish' ELSE 'translation' END ORDER BY julianday(created_at) DESC, created_at DESC, CASE status WHEN 'queued' THEN 0 WHEN 'running' THEN 1 WHEN 'deferred_provider' THEN 2 WHEN 'blocked_config' THEN 3 WHEN 'failed' THEN 4 WHEN 'ready' THEN 5 WHEN 'superseded' THEN 9 ELSE 6 END, CASE WHEN pipeline = 'translation' AND variant = 'detail' THEN 0 ELSE 1 END, julianday(updated_at) DESC, updated_at DESC, id DESC) AS global_rank FROM canonical_identity_work), canonical_work AS (SELECT * FROM ranked_global_work WHERE global_rank = 1) SELECT w.pipeline, i.source_hash, w.status, w.attempt_count, w.started_at, w.finished_at, w.updated_at, (SELECT MAX(e.created_at) FROM content_attempt_events e WHERE e.work_item_id = w.id) AS last_attempt_at, w.canonical_resource_id, p.work_item_id AS projection_work_item_id, i.source_hash AS projection_source_hash, p.updated_at AS projection_updated_at FROM canonical_work w JOIN content_work_identities i ON i.id = w.identity_id LEFT JOIN content_current_result_projections p ON p.identity_id = i.id WHERE w.canonical_resource_type = "
    } else {
        "SELECT w.pipeline, w.source_hash, w.status, w.attempt_count, w.started_at, w.finished_at, w.updated_at, (SELECT MAX(e.created_at) FROM content_attempt_events e WHERE e.work_item_id = w.id) AS last_attempt_at, w.canonical_resource_id, (SELECT p.work_item_id FROM content_result_projections p WHERE p.canonical_resource_type = w.canonical_resource_type AND p.canonical_resource_id = w.canonical_resource_id AND p.pipeline = w.pipeline AND p.variant = w.variant AND p.target_lang = w.target_lang AND p.protocol_version = w.protocol_version AND p.model_profile = w.model_profile AND p.source_hash = w.source_hash ORDER BY julianday(p.updated_at) DESC, p.updated_at DESC, p.id DESC LIMIT 1) AS projection_work_item_id, (SELECT p.source_hash FROM content_result_projections p WHERE p.canonical_resource_type = w.canonical_resource_type AND p.canonical_resource_id = w.canonical_resource_id AND p.pipeline = w.pipeline AND p.variant = w.variant AND p.target_lang = w.target_lang AND p.protocol_version = w.protocol_version AND p.model_profile = w.model_profile AND p.source_hash = w.source_hash ORDER BY julianday(p.updated_at) DESC, p.updated_at DESC, p.id DESC LIMIT 1) AS projection_source_hash, (SELECT p.updated_at FROM content_result_projections p WHERE p.canonical_resource_type = w.canonical_resource_type AND p.canonical_resource_id = w.canonical_resource_id AND p.pipeline = w.pipeline AND p.variant = w.variant AND p.target_lang = w.target_lang AND p.protocol_version = w.protocol_version AND p.model_profile = w.model_profile AND p.source_hash = w.source_hash ORDER BY julianday(p.updated_at) DESC, p.updated_at DESC, p.id DESC LIMIT 1) AS projection_updated_at FROM content_work_items w WHERE w.canonical_resource_type = "
    };
    let mut query = PgReadBuilder::new(source);
    query.push_bind(collection_record_kind_label(kind));
    query.push(" AND w.canonical_resource_id IN (");
    {
        let mut separated = query.separated(", ");
        for id in entity_ids {
            separated.push_bind(id);
        }
    }
    query.push(")");
    query.push(" AND ((w.pipeline = 'translation' AND w.variant IN ('detail', 'summary', 'shared')) OR (w.pipeline = 'polishing' AND w.variant = 'smart'))");
    query.push(" ORDER BY julianday(w.created_at) DESC, w.created_at DESC, CASE w.status WHEN 'queued' THEN 0 WHEN 'running' THEN 1 WHEN 'deferred_provider' THEN 2 WHEN 'blocked_config' THEN 3 WHEN 'failed' THEN 4 WHEN 'ready' THEN 5 WHEN 'superseded' THEN 9 ELSE 6 END, CASE WHEN w.pipeline = 'translation' AND w.variant = 'detail' THEN 0 ELSE 1 END, julianday(w.updated_at) DESC, w.updated_at DESC, w.id DESC");
    let rows = match query
        .build_query_as::<GlobalTaskRow>()
        .fetch_all(&mut *connection)
        .await
    {
        Ok(rows) => rows,
        Err(error) if missing_table(&error) => return Ok(Vec::new()),
        Err(error) => return Err(ApiError::internal(error)),
    };
    Ok(rows)
}

async fn identity_upgrade_complete_in_connection(
    connection: &mut SqliteConnection,
) -> Result<bool, ApiError> {
    match pg_query_as::<(String, String)>(
        "SELECT status, phase FROM content_identity_upgrade_control WHERE id = 1",
    )
    .fetch_optional(&mut *connection)
    .await
    {
        Ok(state) => {
            Ok(state.is_some_and(|(status, phase)| status == "completed" && phase == "complete"))
        }
        Err(error) if missing_table(&error) => Ok(false),
        Err(error) => Err(ApiError::internal(error)),
    }
}

fn global_summary(row: &GlobalTaskRow) -> AdminCollectionTaskSummary {
    AdminCollectionTaskSummary {
        status: row.status.clone(),
        display_status: display_status_for(&row.status, "global_work"),
        status_origin: "global_work".to_owned(),
        attempt_count: row.attempt_count.max(0),
        started_at: row.started_at.clone(),
        last_attempt_at: (row.attempt_count > 0)
            .then(|| row.last_attempt_at.clone())
            .flatten(),
        finished_at: row.finished_at.clone(),
        global_work: Some(AdminContentProcessingEvidence {
            status: row.status.clone(),
            status_origin: "global_work".to_owned(),
            source_hash: Some(row.source_hash.clone()),
            updated_at: Some(row.updated_at.clone()),
            ..Default::default()
        }),
        result_projection: row.projection_work_item_id.as_ref().map(|work_item_id| {
            AdminContentProcessingEvidence {
                status: "ready".to_owned(),
                status_origin: "result_projection".to_owned(),
                work_item_id: Some(work_item_id.clone()),
                source_hash: row.projection_source_hash.clone(),
                updated_at: row.projection_updated_at.clone(),
                ..Default::default()
            }
        }),
        ..Default::default()
    }
}

fn global_summary_from_rows(rows: &[&GlobalTaskRow]) -> AdminCollectionTaskSummary {
    let Some(latest) = rows.first() else {
        return not_recorded_summary();
    };
    let mut summary = global_summary(latest);
    summary.attempt_count = rows
        .iter()
        .map(|row| row.attempt_count.max(0))
        .max()
        .unwrap_or(0);
    summary.started_at = rows.iter().filter_map(|row| row.started_at.clone()).min();
    summary.last_attempt_at = rows
        .iter()
        .filter(|row| row.attempt_count > 0)
        .filter_map(|row| row.last_attempt_at.clone())
        .max();
    summary.finished_at = rows.iter().filter_map(|row| row.finished_at.clone()).max();
    summary
}

fn global_summary_with_legacy_evidence(
    mut summary: AdminCollectionTaskSummary,
    evidence: AdminContentProcessingEvidence,
) -> AdminCollectionTaskSummary {
    summary.status_origin = "legacy_evidence".to_owned();
    summary.attempt_count = 0;
    summary.started_at = None;
    summary.last_attempt_at = None;
    summary.finished_at = None;
    summary.legacy_evidence = Some(evidence);
    summary
}

fn compare_attempt_timestamps(left: &str, right: &str) -> Ordering {
    let parse = |value: &str| {
        DateTime::parse_from_rfc3339(value)
            .map(|parsed| parsed.with_timezone(&Utc))
            .ok()
            .or_else(|| {
                NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f")
                    .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S"))
                    .ok()
                    .map(|parsed| parsed.and_utc())
            })
    };
    match (parse(left), parse(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => left.cmp(right),
    }
}

async fn load_legacy_observations_in_connection(
    connection: &mut SqliteConnection,
    kind: CollectionRecordKind,
    entity_ids: &[String],
) -> Result<HashMap<(String, String), AdminContentProcessingEvidence>, ApiError> {
    if entity_ids.is_empty() || kind == CollectionRecordKind::Brief {
        return Ok(HashMap::new());
    }
    let mut query = PgReadBuilder::new(
        "SELECT canonical_resource_id, pipeline, classification, legacy_table, legacy_primary_key, observed_at, observation_basis_json FROM content_legacy_observations WHERE canonical_resource_type = ",
    );
    query.push_bind(collection_record_kind_label(kind));
    query.push(" AND canonical_resource_id IN (");
    {
        let mut separated = query.separated(", ");
        for id in entity_ids {
            separated.push_bind(id);
        }
    }
    query.push(")");
    #[derive(Debug, sqlx::FromRow, Deserialize)]
    struct LegacyObservationRow {
        canonical_resource_id: Option<String>,
        pipeline: Option<String>,
        classification: String,
        legacy_table: String,
        legacy_primary_key: String,
        observed_at: String,
        observation_basis_json: String,
    }
    let rows = match query
        .build_query_as::<LegacyObservationRow>()
        .fetch_all(&mut *connection)
        .await
    {
        Ok(rows) => rows,
        Err(error) if missing_table(&error) => return Ok(HashMap::new()),
        Err(error) => return Err(ApiError::internal(error)),
    };
    let mut paired_history = HashSet::new();
    let mut source_by_key = HashMap::<(String, String, String), HashSet<String>>::new();
    for row in &rows {
        let (Some(entity_id), Some(pipeline)) =
            (row.canonical_resource_id.clone(), row.pipeline.clone())
        else {
            continue;
        };
        let Some(source_hash) =
            serde_json::from_str::<serde_json::Value>(&row.observation_basis_json)
                .ok()
                .and_then(|basis| {
                    basis
                        .get("source_hash")
                        .and_then(|value| value.as_str())
                        .map(str::to_owned)
                })
        else {
            continue;
        };
        let key = (entity_id, pipeline, source_hash);
        source_by_key
            .entry(key)
            .or_default()
            .insert(row.legacy_table.clone());
    }
    for (key, sources) in source_by_key {
        if sources.contains("ai_translations") && sources.contains("translation_work_items") {
            paired_history.insert(key);
        }
    }

    let mut observations = HashMap::new();
    for row in rows {
        let (Some(entity_id), Some(pipeline)) = (row.canonical_resource_id, row.pipeline) else {
            continue;
        };
        let key = (entity_id.clone(), pipeline.clone());
        let source_hash = serde_json::from_str::<serde_json::Value>(&row.observation_basis_json)
            .ok()
            .and_then(|basis| {
                basis
                    .get("source_hash")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned)
            });
        if let Some(source_hash) = source_hash.as_deref()
            && paired_history.contains(&(
                entity_id.clone(),
                pipeline.clone(),
                source_hash.to_owned(),
            ))
        {
            continue;
        }
        let candidate = AdminContentProcessingEvidence {
            status: row.classification,
            status_origin: "legacy_evidence".to_owned(),
            source_hash,
            updated_at: Some(row.observed_at),
            legacy_table: Some(row.legacy_table),
            legacy_primary_key: Some(row.legacy_primary_key),
            ..Default::default()
        };
        observations
            .entry(key)
            .and_modify(|existing: &mut AdminContentProcessingEvidence| {
                if existing.status != "legacy_conflict" && candidate.status == "legacy_conflict" {
                    *existing = candidate.clone();
                }
            })
            .or_insert(candidate);
    }
    Ok(observations)
}

fn merge_summary(rows: &[TaskRow], status_origin: &str) -> AdminCollectionTaskSummary {
    let Some(latest) = rows.iter().max_by_key(|row| (&row.updated_at, &row.id)) else {
        return not_recorded_summary_for(status_origin);
    };
    let status = latest
        .result_status
        .clone()
        .unwrap_or_else(|| latest.status.clone());
    AdminCollectionTaskSummary {
        display_status: display_status_for(&status, "task"),
        status,
        status_origin: "task".to_owned(),
        attempt_count: rows
            .iter()
            .map(|row| row.attempt_count.max(0))
            .max()
            .unwrap_or(0),
        started_at: rows.iter().filter_map(|row| row.started_at.clone()).min(),
        last_attempt_at: rows
            .iter()
            .filter(|row| row.attempt_count > 0)
            .filter_map(|row| row.last_attempt_at.clone())
            .max(),
        finished_at: rows.iter().filter_map(|row| row.finished_at.clone()).max(),
        ..Default::default()
    }
}

fn legacy_summary_with_evidence(
    evidence: &AdminContentProcessingEvidence,
    rows: &[TaskRow],
) -> AdminCollectionTaskSummary {
    let mut summary = if rows.is_empty() {
        legacy_evidence_summary(evidence.clone())
    } else {
        let mut summary = merge_summary(rows, "task");
        summary.status = evidence.status.clone();
        summary.display_status = evidence.status.clone();
        summary.legacy_evidence = Some(evidence.clone());
        summary
    };
    if !rows.is_empty() {
        summary.status_origin = "legacy_evidence".to_owned();
    }
    summary
}

fn legacy_conflict_summary_with_tasks(rows: &[TaskRow]) -> AdminCollectionTaskSummary {
    let mut summary = merge_summary(rows, "task");
    summary.status = "legacy_conflict".to_owned();
    summary.display_status = "legacy_conflict".to_owned();
    summary.status_origin = "legacy_conflict".to_owned();
    summary.legacy_evidence = Some(AdminContentProcessingEvidence {
        status: "legacy_conflict".to_owned(),
        status_origin: "legacy_conflict".to_owned(),
        ..Default::default()
    });
    summary
}

async fn load_task_summaries(
    state: &AppState,
    kind: CollectionRecordKind,
    entity_ids: &[String],
    coverage: &HashMap<(String, String), String>,
    global_mode: bool,
) -> Result<HashMap<String, TaskSummaries>, ApiError> {
    let mut connection = state.pool.acquire().await.map_err(ApiError::internal)?;
    load_task_summaries_in_connection(&mut connection, kind, entity_ids, coverage, global_mode)
        .await
}

async fn load_task_summaries_in_connection(
    connection: &mut SqliteConnection,
    kind: CollectionRecordKind,
    entity_ids: &[String],
    coverage: &HashMap<(String, String), String>,
    global_mode: bool,
) -> Result<HashMap<String, TaskSummaries>, ApiError> {
    let Some((translation_kind, polish_kind)) = kind.task_kinds() else {
        return Ok(HashMap::new());
    };
    if entity_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut query = PgReadBuilder::new(
        "SELECT w.id, w.kind, w.status, w.result_status, w.attempt_count, w.started_at, w.finished_at, w.updated_at, (SELECT MAX(e.created_at) FROM translation_attempt_events e WHERE e.work_item_id = w.id) AS last_attempt_at, w.entity_id FROM translation_work_items w WHERE w.kind IN (",
    );
    {
        let mut separated = query.separated(", ");
        separated.push_bind(translation_kind);
        separated.push_bind(polish_kind);
    }
    query.push(") AND w.entity_id IN (");
    {
        let mut separated = query.separated(", ");
        for id in entity_ids {
            separated.push_bind(id);
        }
    }
    query.push(")");
    let rows = query
        .build_query_as::<TaskWithEntityRow>()
        .fetch_all(&mut *connection)
        .await
        .map_err(ApiError::internal)?;
    let global_rows = load_global_task_rows_in_connection(connection, kind, entity_ids).await?;
    let global_by_key = global_rows.iter().fold(HashMap::new(), |mut by_key, row| {
        let pipeline = if row.pipeline == "polishing" {
            "polish"
        } else {
            "translation"
        };
        by_key
            .entry((row.canonical_resource_id.clone(), pipeline.to_owned()))
            .or_insert_with(Vec::new)
            .push(row);
        by_key
    });
    let legacy_observations =
        load_legacy_observations_in_connection(connection, kind, entity_ids).await?;
    let mut grouped = HashMap::<String, (Vec<TaskRow>, Vec<TaskRow>)>::new();
    for row in rows {
        let entry = grouped.entry(row.entity_id).or_default();
        if row.task.kind == translation_kind {
            entry.0.push(row.task);
        } else {
            entry.1.push(row.task);
        }
    }
    Ok(entity_ids
        .iter()
        .map(|id| {
            let (translation, polish) = grouped.remove(id).unwrap_or_default();
            let translation_summary = global_by_key
                .get(&(id.clone(), "translation".to_owned()))
                .map(|rows| {
                    if let Some(evidence) =
                        legacy_observations.get(&(id.clone(), "translation".to_owned()))
                    {
                        global_summary_with_legacy_evidence(
                            global_summary_from_rows(rows),
                            evidence.clone(),
                        )
                    } else {
                        global_summary_from_rows(rows)
                    }
                })
                .or_else(|| {
                    legacy_observations
                        .get(&(id.clone(), "translation".to_owned()))
                        .map(|evidence| legacy_summary_with_evidence(evidence, &translation))
                })
                .unwrap_or_else(|| {
                    if global_mode && !translation.is_empty() {
                        legacy_conflict_summary_with_tasks(&translation)
                    } else {
                        merge_summary(&translation, &coverage_origin(coverage, id, "translation"))
                    }
                });
            let polish_summary = global_by_key
                .get(&(id.clone(), "polish".to_owned()))
                .map(|rows| {
                    if let Some(evidence) =
                        legacy_observations.get(&(id.clone(), "polish".to_owned()))
                    {
                        global_summary_with_legacy_evidence(
                            global_summary_from_rows(rows),
                            evidence.clone(),
                        )
                    } else {
                        global_summary_from_rows(rows)
                    }
                })
                .or_else(|| {
                    legacy_observations
                        .get(&(id.clone(), "polish".to_owned()))
                        .map(|evidence| legacy_summary_with_evidence(evidence, &polish))
                })
                .unwrap_or_else(|| {
                    if global_mode && !polish.is_empty() {
                        legacy_conflict_summary_with_tasks(&polish)
                    } else {
                        merge_summary(&polish, &coverage_origin(coverage, id, "polish"))
                    }
                });
            (
                id.clone(),
                TaskSummaries {
                    translation: translation_summary,
                    polish: polish_summary,
                },
            )
        })
        .collect())
}

async fn load_brief_summaries(
    state: &AppState,
    brief_ids: &[String],
    coverage: &HashMap<(String, String), String>,
) -> Result<HashMap<String, AdminCollectionTaskSummary>, ApiError> {
    let mut connection = state.pool.acquire().await.map_err(ApiError::internal)?;
    load_brief_summaries_in_connection(&mut connection, brief_ids, coverage).await
}

async fn load_brief_summaries_in_connection(
    connection: &mut SqliteConnection,
    brief_ids: &[String],
    coverage: &HashMap<(String, String), String>,
) -> Result<HashMap<String, AdminCollectionTaskSummary>, ApiError> {
    if brief_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut query = PgReadBuilder::new(
        "SELECT c.id, c.status, c.source, c.attempt_count, c.started_at, c.finished_at, c.updated_at,
            (SELECT MAX(e.created_at) FROM llm_call_events e WHERE e.call_id = c.id AND e.event_type IN ('llm.running', 'llm.attempt_failed', 'llm.succeeded', 'llm.failed')) AS last_attempt_at,
            c.error_text, c.parent_brief_id FROM llm_calls c WHERE c.parent_brief_id IN (",
    );
    {
        let mut separated = query.separated(", ");
        for id in brief_ids {
            separated.push_bind(id);
        }
    }
    query.push(")");
    #[derive(Debug, sqlx::FromRow, Deserialize)]
    struct BriefCallWithParentRow {
        #[sqlx(flatten)]
        #[serde(flatten)]
        call: BriefCallRow,
        parent_brief_id: String,
    }
    let rows = query
        .build_query_as::<BriefCallWithParentRow>()
        .fetch_all(&mut *connection)
        .await
        .map_err(ApiError::internal)?;
    let mut grouped = HashMap::<String, Vec<BriefCallRow>>::new();
    for row in rows {
        grouped
            .entry(row.parent_brief_id)
            .or_default()
            .push(row.call);
    }
    Ok(brief_ids
        .iter()
        .map(|id| {
            let calls = grouped.remove(id).unwrap_or_default();
            let latest = calls.iter().max_by_key(|call| (&call.updated_at, &call.id));
            let summary = latest.map_or_else(
                || not_recorded_summary_for(&coverage_origin(coverage, id, "polish")),
                |call| AdminCollectionTaskSummary {
                    status: call.status.clone(),
                    display_status: display_status_for(&call.status, "task"),
                    status_origin: "task".to_owned(),
                    attempt_count: calls.iter().fold(0_i64, |total, call| {
                        total.saturating_add(call.attempt_count.max(0))
                    }),
                    started_at: calls
                        .iter()
                        .filter_map(|call| call.started_at.clone())
                        .min(),
                    last_attempt_at: calls
                        .iter()
                        .filter(|call| call.attempt_count > 0)
                        .filter_map(|call| call.last_attempt_at.clone())
                        .max(),
                    finished_at: calls
                        .iter()
                        .filter_map(|call| call.finished_at.clone())
                        .max(),
                    ..Default::default()
                },
            );
            (id.clone(), summary)
        })
        .collect())
}

#[derive(Clone)]
enum CollectionQueryBind {
    Text(String),
    Integer(i64),
}

fn sql_display_status(raw_status: &str, origin: &str) -> String {
    format!(
        "CASE
            WHEN {raw_status} IN ('queued', 'batched', 'retry_scheduled') THEN 'queued'
            WHEN {raw_status} = 'running' THEN 'running'
            WHEN {raw_status} IN ('completed', 'succeeded', 'ready') THEN 'succeeded'
            WHEN {raw_status} IN ('failed', 'error') THEN 'failed'
            WHEN {raw_status} = 'missing' THEN 'missing'
            WHEN {raw_status} = 'disabled' THEN 'disabled'
            WHEN {raw_status} = 'legacy_cached' THEN 'legacy_cached'
            WHEN {raw_status} = 'legacy_conflict' THEN 'legacy_conflict'
            WHEN {raw_status} = 'not_recorded' AND {origin} = 'never_started' THEN 'not_started'
            WHEN {raw_status} = 'not_recorded' THEN 'historical_unknown'
            WHEN ({raw_status} IS NULL OR {raw_status} = '') AND {origin} = 'never_started' THEN 'not_started'
            WHEN {raw_status} IS NULL OR {raw_status} = '' THEN 'historical_unknown'
            ELSE {raw_status}
        END",
    )
}

fn sql_legacy_status(latest: &str, observation: &str, coverage: &str) -> String {
    let raw = format!("CASE WHEN {observation} IS NOT NULL THEN {observation} ELSE {latest} END");
    let origin =
        format!("CASE WHEN {observation} IS NOT NULL THEN 'legacy_evidence' ELSE {coverage} END");
    sql_display_status(&raw, &origin)
}

fn sql_global_status(
    latest: &str,
    observation: &str,
    legacy_latest: &str,
    coverage: &str,
) -> String {
    let raw = format!(
        "CASE
            WHEN {latest} IS NOT NULL THEN {latest}
            WHEN {observation} IS NOT NULL THEN {observation}
            WHEN {legacy_latest} IS NOT NULL THEN 'legacy_conflict'
            ELSE NULL
        END"
    );
    let origin = format!(
        "CASE
            WHEN {latest} IS NOT NULL THEN 'global_work'
            WHEN {observation} IS NOT NULL THEN 'legacy_evidence'
            WHEN {legacy_latest} IS NOT NULL THEN 'legacy_conflict'
            ELSE {coverage}
        END"
    );
    sql_display_status(&raw, &origin)
}

fn notification_kind_sql(kind: CollectionRecordKind) -> (&'static str, &'static str) {
    match kind {
        CollectionRecordKind::Release => ("release_detail", "release_smart"),
        CollectionRecordKind::Announcement => ("announcement_detail", "announcement_smart"),
        CollectionRecordKind::Notification => ("notification", "notification_smart"),
        CollectionRecordKind::Brief => ("", ""),
    }
}

fn source_window_sql(
    expression: &str,
    from: Option<&str>,
    before: Option<&str>,
    binds: &mut Vec<CollectionQueryBind>,
) -> String {
    let mut predicates = Vec::new();
    if let Some(value) = from {
        predicates.push(format!("julianday({expression}) >= julianday(?)"));
        binds.push(CollectionQueryBind::Text(value.to_owned()));
    }
    if let Some(value) = before {
        predicates.push(format!("julianday({expression}) < julianday(?)"));
        binds.push(CollectionQueryBind::Text(value.to_owned()));
    }
    if predicates.is_empty() {
        "1 = 1".to_owned()
    } else {
        predicates.join(" AND ")
    }
}

fn source_records_sql(
    kind: CollectionRecordKind,
    from: Option<&str>,
    before: Option<&str>,
    binds: &mut Vec<CollectionQueryBind>,
) -> String {
    match kind {
        CollectionRecordKind::Release => {
            let window = source_window_sql(
                "COALESCE(r.published_at, r.created_at, r.updated_at)",
                from,
                before,
                binds,
            );
            format!(
                "release_repositories AS (
                SELECT repo_id, MIN(repo_full_name) AS repository
                FROM repo_release_work_items
                GROUP BY repo_id
            ),
            raw_source_records AS (
                SELECT
                    CAST(r.release_id AS TEXT) AS id,
                    COALESCE(rr.repository, '仓库 #' || CAST(r.repo_id AS TEXT)) AS repository,
                    COALESCE(NULLIF(r.name, ''), r.tag_name) AS title,
                    COALESCE(r.published_at, r.created_at, r.updated_at) AS source_time,
                    COALESCE(r.published_at, r.created_at, r.updated_at) AS occurred_at,
                    r.detected_at,
                    NULL AS generated_at
                FROM repo_releases r
                LEFT JOIN release_repositories rr ON rr.repo_id = r.repo_id
                WHERE {window}
            )"
            )
        }
        CollectionRecordKind::Announcement => {
            let window = source_window_sql("e.occurred_at", from, before, binds);
            format!(
                "raw_source_records AS (
                SELECT
                    lower(e.repo_full_name) || '#' || CAST(e.discussion_number AS TEXT) AS id,
                    e.repo_full_name AS repository,
                    COALESCE(NULLIF(e.title, ''), '公告') AS title,
                    e.occurred_at AS source_time,
                    e.occurred_at AS occurred_at,
                    e.detected_at,
                    NULL AS generated_at
                FROM social_activity_events e
                WHERE e.kind = 'announcement'
                  AND e.repo_full_name IS NOT NULL
                  AND e.discussion_number IS NOT NULL
                  AND {window}
                  AND e.id = (
                    SELECT latest.id
                    FROM social_activity_events latest
                    WHERE latest.kind = 'announcement'
                      AND lower(latest.repo_full_name) = lower(e.repo_full_name)
                      AND latest.discussion_number = e.discussion_number
                    ORDER BY latest.occurred_at DESC, lower(COALESCE(latest.repo_full_name, '')) DESC, latest.discussion_number DESC, COALESCE(latest.title, '') DESC, COALESCE(latest.body, '') DESC, latest.id DESC
                    LIMIT 1
                  )
            )"
            )
        }
        CollectionRecordKind::Notification => {
            let window = source_window_sql("n.updated_at", from, before, binds);
            format!(
                "ranked_notifications AS (
                SELECT
                    n.*,
                    ROW_NUMBER() OVER (
                      PARTITION BY n.thread_id
                      ORDER BY n.updated_at DESC, COALESCE(n.repo_full_name, '') DESC,
                        COALESCE(n.subject_title, '') DESC, COALESCE(n.reason, '') DESC,
                        COALESCE(n.subject_type, '') DESC, n.id DESC
                    ) AS source_rank
                FROM notifications n
            ),
            raw_source_records AS (
                SELECT
                    n.thread_id AS id,
                    n.repo_full_name AS repository,
                    COALESCE(NULLIF(n.subject_title, ''), '通知') AS title,
                    n.updated_at AS source_time,
                    n.updated_at AS occurred_at,
                    NULL AS detected_at,
                    NULL AS generated_at
                FROM ranked_notifications n
                WHERE {window}
                  AND n.source_rank = 1
            )"
            )
        }
        CollectionRecordKind::Brief => {
            let window = source_window_sql("b.created_at", from, before, binds);
            format!(
                "raw_source_records AS (
                SELECT
                    b.id,
                    NULL AS repository,
                    b.date AS title,
                    b.created_at AS source_time,
                    NULL AS occurred_at,
                    NULL AS detected_at,
                    b.created_at AS generated_at
                FROM briefs b
                WHERE {window}
            )"
            )
        }
    }
}

fn activity_source_ctes(kind: CollectionRecordKind) -> String {
    match kind {
        CollectionRecordKind::Release => "bounded_source_records AS MATERIALIZED (
                SELECT
                    CAST(r.release_id AS TEXT) AS id,
                    COALESCE(
                        (SELECT MIN(wi.repo_full_name)
                         FROM repo_release_work_items wi
                         WHERE wi.repo_id = r.repo_id),
                        '仓库 #' || CAST(r.repo_id AS TEXT)
                    ) AS repository,
                    COALESCE(NULLIF(r.name, ''), r.tag_name) AS title,
                    COALESCE(r.published_at, r.created_at, r.updated_at) AS source_time
                FROM repo_releases r
                WHERE julianday(COALESCE(r.published_at, r.created_at, r.updated_at)) >= julianday(?)
                  AND julianday(COALESCE(r.published_at, r.created_at, r.updated_at)) < julianday(?)
            )"
        .to_owned(),
        CollectionRecordKind::Announcement => "bounded_announcement_rows AS MATERIALIZED (
                SELECT
                    e.id AS source_id,
                    lower(e.repo_full_name) AS repo_key,
                    e.discussion_number,
                    e.occurred_at AS source_time
                FROM social_activity_events e
                WHERE e.kind = 'announcement'
                  AND e.repo_full_name IS NOT NULL
                  AND e.discussion_number IS NOT NULL
                    AND julianday(e.occurred_at) >= julianday(?)
                    AND julianday(e.occurred_at) < julianday(?)
                  AND e.id = (
                    SELECT latest.id
                    FROM social_activity_events latest
                    WHERE latest.kind = 'announcement'
                      AND lower(latest.repo_full_name) = lower(e.repo_full_name)
                      AND latest.discussion_number = e.discussion_number
                    ORDER BY latest.occurred_at DESC, lower(COALESCE(latest.repo_full_name, '')) DESC, latest.discussion_number DESC, COALESCE(latest.title, '') DESC, COALESCE(latest.body, '') DESC, latest.id DESC
                    LIMIT 1
                )
            ),
            bounded_source_records AS MATERIALIZED (
                SELECT
                    c.repo_key || '#' || CAST(c.discussion_number AS TEXT) AS id,
                    e.repo_full_name AS repository,
                    COALESCE(NULLIF(e.title, ''), '公告') AS title,
                    c.source_time
                FROM bounded_announcement_rows c
                JOIN social_activity_events e
                  ON e.id = c.source_id
            )"
        .to_owned(),
        CollectionRecordKind::Notification => "bounded_source_records AS MATERIALIZED (
                WITH ranked_notifications AS (
                  SELECT
                    n.*,
                    ROW_NUMBER() OVER (
                      PARTITION BY n.thread_id
                      ORDER BY n.updated_at DESC, COALESCE(n.repo_full_name, '') DESC,
                        COALESCE(n.subject_title, '') DESC, COALESCE(n.reason, '') DESC,
                        COALESCE(n.subject_type, '') DESC, n.id DESC
                    ) AS source_rank
                  FROM notifications n
                )
                SELECT
                    n.thread_id AS id,
                    n.repo_full_name AS repository,
                    COALESCE(NULLIF(n.subject_title, ''), '通知') AS title,
                    n.updated_at AS source_time
                FROM ranked_notifications n
                WHERE julianday(n.updated_at) >= julianday(?)
                  AND julianday(n.updated_at) < julianday(?)
                  AND n.source_rank = 1
            )"
        .to_owned(),
        CollectionRecordKind::Brief => "bounded_source_records AS MATERIALIZED (
                SELECT
                    b.id,
                    NULL AS repository,
                    b.date AS title,
                    b.created_at AS source_time
                FROM briefs b
                WHERE julianday(b.created_at) >= julianday(?)
                  AND julianday(b.created_at) < julianday(?)
            )"
        .to_owned(),
    }
}

fn global_work_candidates_ctes(
    source_cte: &str,
    resource_type: &str,
    identity_upgrade_complete: bool,
) -> String {
    if identity_upgrade_complete {
        format!(
            "ranked_identity_members AS MATERIALIZED (
                SELECT m.identity_id, w.*,
                    ROW_NUMBER() OVER (
                        PARTITION BY m.identity_id
                        ORDER BY CASE w.status
                            WHEN 'queued' THEN 0 WHEN 'running' THEN 1
                            WHEN 'deferred_provider' THEN 2 WHEN 'blocked_config' THEN 3
                            WHEN 'failed' THEN 4 WHEN 'ready' THEN 5
                            WHEN 'superseded' THEN 9 ELSE 6 END,
                            w.attempt_count DESC, julianday(w.updated_at) DESC,
                            w.updated_at DESC, w.id DESC
                    ) AS member_rank
                FROM {source_cte} s
                CROSS JOIN content_work_identity_members m
                JOIN content_work_items w ON w.id = m.work_item_id
                WHERE w.canonical_resource_type = '{resource_type}'
                  AND w.canonical_resource_id = s.id
                  AND ((w.pipeline = 'translation' AND w.variant IN ('detail', 'summary', 'shared'))
                    OR (w.pipeline = 'polishing' AND w.variant = 'smart'))
            ),
            canonical_identity_work AS MATERIALIZED (
                SELECT * FROM ranked_identity_members WHERE member_rank = 1
            ),
            ranked_global_work AS MATERIALIZED (
                SELECT *,
                    ROW_NUMBER() OVER (
                        PARTITION BY canonical_resource_id,
                            CASE WHEN pipeline = 'polishing' THEN 'polish' ELSE 'translation' END
                        ORDER BY julianday(created_at) DESC, created_at DESC,
                            CASE status
                                WHEN 'queued' THEN 0
                                WHEN 'running' THEN 1
                                WHEN 'deferred_provider' THEN 2
                                WHEN 'blocked_config' THEN 3
                                WHEN 'failed' THEN 4
                                WHEN 'ready' THEN 5
                                WHEN 'superseded' THEN 9
                                ELSE 6
                            END,
                            CASE WHEN pipeline = 'translation' AND variant = 'detail' THEN 0 ELSE 1 END,
                            julianday(updated_at) DESC, updated_at DESC, id DESC
                    ) AS global_rank
                FROM canonical_identity_work
            ),
            global_work_candidates AS MATERIALIZED (
                SELECT * FROM ranked_global_work WHERE global_rank = 1
            )"
        )
    } else {
        format!(
            "global_work_candidates AS MATERIALIZED (
                SELECT w.* FROM {source_cte} s
                CROSS JOIN content_work_items w
                WHERE w.canonical_resource_type = '{resource_type}'
                  AND w.canonical_resource_id = s.id
                  AND ((w.pipeline = 'translation' AND w.variant IN ('detail', 'summary', 'shared'))
                    OR (w.pipeline = 'polishing' AND w.variant = 'smart'))
            )"
        )
    }
}

fn activity_query_sql(
    kind: CollectionRecordKind,
    global_mode: bool,
    identity_upgrade_complete: bool,
) -> String {
    let mut sql = String::from("WITH ");
    sql.push_str(&activity_source_ctes(kind));

    if kind == CollectionRecordKind::Brief {
        sql.push_str(
            ", brief_call_rows AS MATERIALIZED (
                SELECT
                    c.parent_brief_id AS entity_id,
                    c.status AS raw_status,
                    ROW_NUMBER() OVER (
                        PARTITION BY c.parent_brief_id
                        ORDER BY c.updated_at DESC, c.id DESC
                    ) AS row_rank
                FROM llm_calls c
                JOIN bounded_source_records s ON s.id = c.parent_brief_id
            ),
            coverage AS MATERIALIZED (
                SELECT c.record_id AS entity_id, c.status_origin
                FROM admin_collection_processing_coverage c
                JOIN bounded_source_records s ON s.id = c.record_id
                WHERE c.record_kind = 'brief' AND c.pipeline = 'polish'
            ),
                status_projection AS (
                    SELECT
                        s.*,
                        NULL AS translation_status,
                    CASE
                        WHEN b.raw_status IS NULL THEN ",
        );
        sql.push_str(&sql_display_status(
            "'not_recorded'",
            "COALESCE(c.status_origin, 'historical_unknown')",
        ));
        sql.push_str(" ELSE ");
        sql.push_str(&sql_display_status("b.raw_status", "'task'"));
        sql.push_str(
            " END AS polish_status
                FROM bounded_source_records s
                LEFT JOIN brief_call_rows b ON b.entity_id = s.id AND b.row_rank = 1
                LEFT JOIN coverage c ON c.entity_id = s.id
            )",
        );
    } else {
        let (translation_kind, polish_kind) = notification_kind_sql(kind);
        let resource_type = collection_record_kind_label(kind);
        sql.push_str(&format!(
            ", legacy_task_rows AS MATERIALIZED (
                SELECT
                    w.entity_id,
                    CASE WHEN w.kind = '{polish_kind}' THEN 'polish' ELSE 'translation' END AS pipeline,
                    COALESCE(w.result_status, w.status) AS raw_status,
                    ROW_NUMBER() OVER (
                        PARTITION BY w.entity_id,
                            CASE WHEN w.kind = '{polish_kind}' THEN 'polish' ELSE 'translation' END
                        ORDER BY w.updated_at DESC, w.id DESC
                    ) AS row_rank
                FROM translation_work_items w
                JOIN bounded_source_records s ON s.id = w.entity_id
                WHERE w.kind IN ('{translation_kind}', '{polish_kind}')
            ),
            legacy_latest AS (
                SELECT entity_id, pipeline, raw_status
                FROM legacy_task_rows
                WHERE row_rank = 1
            ),
            legacy_observation_rows AS MATERIALIZED (
                SELECT
                    o.canonical_resource_id AS entity_id,
                    CASE WHEN o.pipeline = 'polishing' THEN 'polish' ELSE 'translation' END AS pipeline,
                    o.legacy_table,
                    json_extract(o.observation_basis_json, '$.source_hash') AS source_hash,
                    o.classification
                FROM content_legacy_observations o
                JOIN bounded_source_records s ON s.id = o.canonical_resource_id
                WHERE o.canonical_resource_type = '{resource_type}'
                  AND json_extract(o.observation_basis_json, '$.source_hash') IS NOT NULL
            ),
            legacy_observations AS (
                SELECT
                    r.entity_id,
                    r.pipeline,
                    CASE WHEN MAX(CASE WHEN r.classification = 'legacy_conflict' THEN 1 ELSE 0 END) = 1
                        THEN 'legacy_conflict' ELSE 'legacy_cached' END AS raw_status
                FROM legacy_observation_rows r
                WHERE NOT EXISTS (
                    SELECT 1
                    FROM legacy_observation_rows paired
                    WHERE paired.entity_id = r.entity_id
                      AND paired.pipeline = r.pipeline
                      AND paired.source_hash = r.source_hash
                      AND paired.legacy_table <> r.legacy_table
                )
                GROUP BY r.entity_id, r.pipeline
            ),
            coverage AS MATERIALIZED (
                SELECT c.record_id AS entity_id, c.pipeline, c.status_origin
                FROM admin_collection_processing_coverage c
                JOIN bounded_source_records s ON s.id = c.record_id
                WHERE c.record_kind = '{resource_type}'
            )"
        ));

        if global_mode {
            let global_work_candidates = global_work_candidates_ctes(
                "bounded_source_records",
                resource_type,
                identity_upgrade_complete,
            );
            sql.push_str(&format!(
                ", {global_work_candidates}, global_task_rows AS MATERIALIZED (
                    SELECT
                        w.canonical_resource_id AS entity_id,
                        CASE WHEN w.pipeline = 'polishing' THEN 'polish' ELSE 'translation' END AS pipeline,
                        w.status AS raw_status,
                        ROW_NUMBER() OVER (
                            PARTITION BY w.canonical_resource_id,
                                CASE WHEN w.pipeline = 'polishing' THEN 'polish' ELSE 'translation' END
                            ORDER BY
                                julianday(w.created_at) DESC,
                                w.created_at DESC,
                                CASE w.status
                                    WHEN 'queued' THEN 0
                                    WHEN 'running' THEN 1
                                    WHEN 'deferred_provider' THEN 2
                                    WHEN 'blocked_config' THEN 3
                                    WHEN 'failed' THEN 4
                                    WHEN 'ready' THEN 5
                                    WHEN 'superseded' THEN 9
                                    ELSE 6
                                END,
                                CASE WHEN w.pipeline = 'translation' AND w.variant = 'detail' THEN 0 ELSE 1 END,
                                julianday(w.updated_at) DESC,
                                w.updated_at DESC,
                                w.id DESC
                        ) AS row_rank
                    FROM global_work_candidates w
                ),
                global_latest AS (
                    SELECT entity_id, pipeline, raw_status
                    FROM global_task_rows
                    WHERE row_rank = 1
                ),
                status_projection AS (
                    SELECT
                        s.*,
                        {translation_status} AS translation_status,
                        {polish_status} AS polish_status
                    FROM bounded_source_records s
                    LEFT JOIN global_latest gt ON gt.entity_id = s.id AND gt.pipeline = 'translation'
                    LEFT JOIN global_latest gp ON gp.entity_id = s.id AND gp.pipeline = 'polish'
                    LEFT JOIN legacy_latest lt ON lt.entity_id = s.id AND lt.pipeline = 'translation'
                    LEFT JOIN legacy_latest lp ON lp.entity_id = s.id AND lp.pipeline = 'polish'
                    LEFT JOIN legacy_observations ot ON ot.entity_id = s.id AND ot.pipeline = 'translation'
                    LEFT JOIN legacy_observations op ON op.entity_id = s.id AND op.pipeline = 'polish'
                    LEFT JOIN coverage ct ON ct.entity_id = s.id AND ct.pipeline = 'translation'
                    LEFT JOIN coverage cp ON cp.entity_id = s.id AND cp.pipeline = 'polish'
                )",
                global_work_candidates = global_work_candidates,
                translation_status = sql_global_status(
                    "gt.raw_status",
                    "ot.raw_status",
                    "lt.raw_status",
                    "ct.status_origin",
                ),
                polish_status = sql_global_status(
                    "gp.raw_status",
                    "op.raw_status",
                    "lp.raw_status",
                    "cp.status_origin",
                ),
            ));
        } else {
            sql.push_str(&format!(
                ", status_projection AS (
                    SELECT
                        s.*,
                        {translation_status} AS translation_status,
                        {polish_status} AS polish_status
                    FROM bounded_source_records s
                    LEFT JOIN legacy_latest lt ON lt.entity_id = s.id AND lt.pipeline = 'translation'
                    LEFT JOIN legacy_latest lp ON lp.entity_id = s.id AND lp.pipeline = 'polish'
                    LEFT JOIN legacy_observations ot ON ot.entity_id = s.id AND ot.pipeline = 'translation'
                    LEFT JOIN legacy_observations op ON op.entity_id = s.id AND op.pipeline = 'polish'
                    LEFT JOIN coverage ct ON ct.entity_id = s.id AND ct.pipeline = 'translation'
                    LEFT JOIN coverage cp ON cp.entity_id = s.id AND cp.pipeline = 'polish'
                )",
                translation_status = sql_legacy_status(
                    "lt.raw_status",
                    "ot.raw_status",
                    "ct.status_origin",
                ),
                polish_status = sql_legacy_status(
                    "lp.raw_status",
                    "op.raw_status",
                    "cp.status_origin",
                ),
            ));
        }
    }

    sql.push_str(
        " SELECT id, title, repository, source_time,
                 translation_status, polish_status
          FROM status_projection
          ORDER BY julianday(source_time) DESC, source_time DESC, id DESC",
    );
    sql
}

async fn load_activity_rows(
    pool: &SqlitePool,
    kind: CollectionRecordKind,
    global_mode: bool,
    window_started_at: &str,
    window_ended_at: &str,
) -> Result<Vec<AdminCollectionActivityRow>, ApiError> {
    let identity_upgrade_complete = if global_mode {
        let mut connection = pool.acquire().await.map_err(ApiError::internal)?;
        identity_upgrade_complete_in_connection(&mut connection).await?
    } else {
        false
    };
    pg_query_as::<AdminCollectionActivityRow>(&activity_query_sql(
        kind,
        global_mode,
        identity_upgrade_complete,
    ))
    .bind(window_started_at)
    .bind(window_ended_at)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)
}

fn composite_activity_status(translation: Option<&str>, polish: &str) -> &'static str {
    let lanes = [translation, Some(polish)];
    if lanes
        .into_iter()
        .flatten()
        .any(|status| matches!(status, "failed" | "deferred_provider" | "blocked_config"))
    {
        "exception"
    } else if lanes
        .into_iter()
        .flatten()
        .any(|status| matches!(status, "queued" | "running"))
    {
        "processing"
    } else if lanes
        .into_iter()
        .flatten()
        .all(|status| matches!(status, "succeeded" | "not_applicable"))
    {
        "completed"
    } else {
        "neutral"
    }
}

fn parse_activity_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|parsed| parsed.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f")
                .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S"))
                .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f"))
                .ok()
                .map(|parsed| parsed.and_utc())
        })
}

fn activity_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

fn activity_window(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>, DateTime<Utc>) {
    let current_hour = now
        .date_naive()
        .and_hms_opt(now.hour(), 0, 0)
        .expect("valid start of UTC hour")
        .and_utc();
    let window_ended_at = current_hour + chrono::Duration::hours(1);
    let window_started_at = window_ended_at - chrono::Duration::hours(12);
    (current_hour, window_started_at, window_ended_at)
}

fn build_activity_response(
    kind: CollectionRecordKind,
    now: DateTime<Utc>,
    rows: Vec<AdminCollectionActivityRow>,
) -> AdminCollectionActivityResponse {
    let (current_hour, window_started_at, window_ended_at) = activity_window(now);
    let mut buckets = (0..12)
        .map(|age| {
            let started_at = current_hour - chrono::Duration::hours(age);
            AdminCollectionActivityBucket {
                started_at: activity_timestamp(started_at),
                ended_at: activity_timestamp(started_at + chrono::Duration::hours(1)),
                cells: Vec::new(),
            }
        })
        .collect::<Vec<_>>();
    let mut summary = AdminCollectionActivitySummary::default();
    for row in rows {
        let Some(source_time) = parse_activity_timestamp(&row.source_time) else {
            continue;
        };
        if source_time < window_started_at || source_time >= window_ended_at {
            continue;
        }
        let source_hour = source_time.timestamp().div_euclid(3600) * 3600;
        let age = ((current_hour.timestamp() - source_hour) / 3600) as usize;
        if age >= buckets.len() {
            continue;
        }
        let composite_status =
            composite_activity_status(row.translation_status.as_deref(), &row.polish_status);
        summary.content_count += 1;
        match composite_status {
            "completed" => summary.completed_count += 1,
            "processing" => summary.processing_count += 1,
            "exception" => summary.exception_count += 1,
            _ => summary.neutral_count += 1,
        }
        buckets[age].cells.push(AdminCollectionActivityCell {
            id: row.id,
            title: row.title,
            repository: row.repository,
            source_time: activity_timestamp(source_time),
            translation_status: row.translation_status,
            polish_status: row.polish_status,
            composite_status: composite_status.to_owned(),
        });
    }
    AdminCollectionActivityResponse {
        kind: collection_record_kind_label(kind).to_owned(),
        bucket_minutes: 60,
        bucket_count: 12,
        window_started_at: activity_timestamp(window_started_at),
        window_ended_at: activity_timestamp(window_ended_at),
        summary,
        buckets,
    }
}

#[allow(clippy::too_many_arguments)]
fn collection_query_sql(
    kind: CollectionRecordKind,
    global_mode: bool,
    identity_upgrade_complete: bool,
    from: Option<&str>,
    before: Option<&str>,
    attempts: AttemptCountRange,
    translation_filter: Option<&[String]>,
    polish_filter: Option<&[String]>,
    page: Option<(i64, i64)>,
) -> (String, Vec<CollectionQueryBind>) {
    let mut binds = Vec::new();
    let mut sql = String::from("WITH ");
    let source_sql = source_records_sql(kind, from, before, &mut binds);
    sql.push_str(&source_sql);
    sql.push_str(", source_records AS MATERIALIZED (SELECT * FROM raw_source_records)");

    if kind == CollectionRecordKind::Brief {
        sql.push_str(
            ",
            brief_rows AS (
                SELECT
                    c.parent_brief_id AS entity_id,
                    c.status AS raw_status,
                    c.attempt_count,
                    ROW_NUMBER() OVER (
                        PARTITION BY c.parent_brief_id
                        ORDER BY c.updated_at DESC, c.id DESC
                    ) AS row_rank
                FROM llm_calls c
                JOIN source_records s ON s.id = c.parent_brief_id
            ),
            brief_attempts AS (
                SELECT parent_brief_id AS entity_id, SUM(MAX(attempt_count, 0)) AS attempt_count
                FROM llm_calls c
                JOIN source_records s ON s.id = c.parent_brief_id
                GROUP BY parent_brief_id
            ),
            status_projection AS (
                SELECT
                    s.*,
                    NULL AS translation_status,
                    CASE
                        WHEN b.raw_status IS NOT NULL THEN b.raw_status
                        WHEN bc.status_origin = 'never_started' THEN 'not_started'
                        ELSE 'historical_unknown'
                    END AS polish_status,
                    COALESCE(ba.attempt_count, 0) AS attempt_count,
                    CASE
                        WHEN ba.attempt_count IS NOT NULL THEN 1
                        WHEN bc.status_origin = 'never_started' THEN 1
                        ELSE 0
                    END AS attempt_count_known
                FROM source_records s
                LEFT JOIN brief_rows b
                  ON b.entity_id = s.id AND b.row_rank = 1
                LEFT JOIN brief_attempts ba ON ba.entity_id = s.id
                LEFT JOIN admin_collection_processing_coverage bc
                  ON bc.record_kind = 'brief' AND bc.record_id = s.id AND bc.pipeline = 'polish'
            )",
        );
    } else {
        let (translation_kind, polish_kind) = notification_kind_sql(kind);
        let resource_type = collection_record_kind_label(kind);
        let legacy_task_rows = format!(
            "legacy_task_rows AS (
                SELECT
                    w.entity_id,
                    CASE WHEN w.kind = '{polish_kind}' THEN 'polish' ELSE 'translation' END AS pipeline,
                    COALESCE(w.result_status, w.status) AS raw_status,
                    w.attempt_count,
                    w.updated_at,
                    w.id,
                    ROW_NUMBER() OVER (
                        PARTITION BY w.entity_id,
                            CASE WHEN w.kind = '{polish_kind}' THEN 'polish' ELSE 'translation' END
                        ORDER BY w.updated_at DESC, w.id DESC
                    ) AS row_rank
                FROM translation_work_items w
                JOIN source_records s ON s.id = w.entity_id
                WHERE w.kind IN ('{translation_kind}', '{polish_kind}')
            ),
            legacy_latest AS (
                SELECT entity_id, pipeline, raw_status, attempt_count
                FROM legacy_task_rows
                WHERE row_rank = 1
            ),
            legacy_attempts AS (
                SELECT entity_id, pipeline, MAX(attempt_count) AS attempt_count
                FROM legacy_task_rows
                GROUP BY entity_id, pipeline
            ),
            legacy_observation_rows AS (
                SELECT
                    canonical_resource_id AS entity_id,
                    CASE WHEN pipeline = 'polishing' THEN 'polish' ELSE 'translation' END AS pipeline,
                    legacy_table,
                    json_extract(observation_basis_json, '$.source_hash') AS source_hash,
                    classification
                FROM content_legacy_observations
                JOIN source_records s ON s.id = canonical_resource_id
                WHERE canonical_resource_type = '{resource_type}'
                  AND json_extract(observation_basis_json, '$.source_hash') IS NOT NULL
            ),
            legacy_observations AS (
                SELECT
                    entity_id,
                    pipeline,
                    CASE WHEN MAX(CASE WHEN classification = 'legacy_conflict' THEN 1 ELSE 0 END) = 1
                        THEN 'legacy_conflict' ELSE 'legacy_cached' END AS raw_status
                FROM legacy_observation_rows r
                WHERE NOT EXISTS (
                    SELECT 1
                    FROM legacy_observation_rows paired
                    WHERE paired.entity_id = r.entity_id
                      AND paired.pipeline = r.pipeline
                      AND paired.source_hash = r.source_hash
                      AND paired.legacy_table <> r.legacy_table
                )
                GROUP BY entity_id, pipeline
            ),
            coverage AS (
                SELECT record_id AS entity_id, pipeline, status_origin
                FROM admin_collection_processing_coverage
                JOIN source_records s ON s.id = record_id
                WHERE record_kind = '{resource_type}'
            )"
        );
        sql.push_str(", ");
        sql.push_str(&legacy_task_rows);
        if global_mode {
            let global_work_candidates = global_work_candidates_ctes(
                "source_records",
                resource_type,
                identity_upgrade_complete,
            );
            sql.push_str(", ");
            sql.push_str(&global_work_candidates);
            sql.push_str(
                ",
                global_task_rows AS (
                    SELECT
                        w.canonical_resource_id AS entity_id,
                        CASE WHEN w.pipeline = 'polishing' THEN 'polish' ELSE 'translation' END AS pipeline,
                        w.status AS raw_status,
                        w.attempt_count,
                        ROW_NUMBER() OVER (
                            PARTITION BY w.canonical_resource_id,
                                CASE WHEN w.pipeline = 'polishing' THEN 'polish' ELSE 'translation' END
                            ORDER BY
                                julianday(w.created_at) DESC,
                                w.created_at DESC,
                                CASE w.status
                                    WHEN 'queued' THEN 0
                                    WHEN 'running' THEN 1
                                    WHEN 'deferred_provider' THEN 2
                                    WHEN 'blocked_config' THEN 3
                                    WHEN 'failed' THEN 4
                                    WHEN 'ready' THEN 5
                                    WHEN 'superseded' THEN 9
                                    ELSE 6
                                END,
                                CASE WHEN w.pipeline = 'translation' AND w.variant = 'detail' THEN 0 ELSE 1 END,
                                julianday(w.updated_at) DESC,
                                w.updated_at DESC,
                                w.id DESC
                        ) AS row_rank
                FROM global_work_candidates w
                ),
                global_latest AS (
                    SELECT entity_id, pipeline, raw_status, attempt_count
                    FROM global_task_rows
                    WHERE row_rank = 1
                ),
                global_attempts AS (
                    SELECT
                        w.canonical_resource_id AS entity_id,
                        CASE WHEN w.pipeline = 'polishing' THEN 'polish' ELSE 'translation' END AS pipeline,
                        MAX(w.attempt_count) AS attempt_count
                    FROM global_work_candidates w
                    GROUP BY w.canonical_resource_id,
                        CASE WHEN w.pipeline = 'polishing' THEN 'polish' ELSE 'translation' END
                ),
                status_projection AS (
                    SELECT
                        s.*,
                        {translation_status} AS translation_status,
                        {polish_status} AS polish_status,
                        MAX(
                            CASE WHEN gt.entity_id IS NOT NULL
                                THEN COALESCE(gat.attempt_count, 0)
                                ELSE COALESCE(lat.attempt_count, 0)
                            END,
                            CASE WHEN gp.entity_id IS NOT NULL
                                THEN COALESCE(gap.attempt_count, 0)
                                ELSE COALESCE(lap.attempt_count, 0)
                            END
                        ) AS attempt_count,
                        CASE
                            WHEN (gt.entity_id IS NOT NULL OR ct.status_origin = 'never_started')
                             AND ot.entity_id IS NULL
                             AND (gp.entity_id IS NOT NULL OR cp.status_origin = 'never_started')
                             AND op.entity_id IS NULL
                            THEN 1 ELSE 0
                        END AS attempt_count_known
                    FROM source_records s
                    LEFT JOIN global_latest gt ON gt.entity_id = s.id AND gt.pipeline = 'translation'
                    LEFT JOIN global_latest gp ON gp.entity_id = s.id AND gp.pipeline = 'polish'
                    LEFT JOIN legacy_latest lt ON lt.entity_id = s.id AND lt.pipeline = 'translation'
                    LEFT JOIN legacy_latest lp ON lp.entity_id = s.id AND lp.pipeline = 'polish'
                    LEFT JOIN legacy_observations ot ON ot.entity_id = s.id AND ot.pipeline = 'translation'
                    LEFT JOIN legacy_observations op ON op.entity_id = s.id AND op.pipeline = 'polish'
                    LEFT JOIN coverage ct ON ct.entity_id = s.id AND ct.pipeline = 'translation'
                    LEFT JOIN coverage cp ON cp.entity_id = s.id AND cp.pipeline = 'polish'
                    LEFT JOIN global_attempts gat ON gat.entity_id = s.id AND gat.pipeline = 'translation'
                    LEFT JOIN global_attempts gap ON gap.entity_id = s.id AND gap.pipeline = 'polish'
                    LEFT JOIN legacy_attempts lat ON lat.entity_id = s.id AND lat.pipeline = 'translation'
                    LEFT JOIN legacy_attempts lap ON lap.entity_id = s.id AND lap.pipeline = 'polish'
                )",
            );
            let translation_status = sql_global_status(
                "gt.raw_status",
                "ot.raw_status",
                "lt.raw_status",
                "ct.status_origin",
            );
            let polish_status = sql_global_status(
                "gp.raw_status",
                "op.raw_status",
                "lp.raw_status",
                "cp.status_origin",
            );
            sql = sql.replace("{resource_type}", resource_type);
            sql = sql.replacen("{translation_status}", &translation_status, 1);
            sql = sql.replacen("{polish_status}", &polish_status, 1);
        } else {
            let translation_status =
                sql_legacy_status("lt.raw_status", "ot.raw_status", "ct.status_origin");
            let polish_status =
                sql_legacy_status("lp.raw_status", "op.raw_status", "cp.status_origin");
            sql.push_str(&format!(
                ",
                status_projection AS (
                    SELECT
                        s.*,
                        {translation_status} AS translation_status,
                        {polish_status} AS polish_status,
                        MAX(COALESCE(lat.attempt_count, 0), COALESCE(lap.attempt_count, 0)) AS attempt_count,
                        CASE
                            WHEN ((lt.entity_id IS NOT NULL AND ot.entity_id IS NULL) OR ct.status_origin = 'never_started')
                             AND ((lp.entity_id IS NOT NULL AND op.entity_id IS NULL) OR cp.status_origin = 'never_started')
                            THEN 1 ELSE 0
                        END AS attempt_count_known
                    FROM source_records s
                    LEFT JOIN legacy_latest lt ON lt.entity_id = s.id AND lt.pipeline = 'translation'
                    LEFT JOIN legacy_latest lp ON lp.entity_id = s.id AND lp.pipeline = 'polish'
                    LEFT JOIN legacy_observations ot ON ot.entity_id = s.id AND ot.pipeline = 'translation'
                    LEFT JOIN legacy_observations op ON op.entity_id = s.id AND op.pipeline = 'polish'
                    LEFT JOIN coverage ct ON ct.entity_id = s.id AND ct.pipeline = 'translation'
                    LEFT JOIN coverage cp ON cp.entity_id = s.id AND cp.pipeline = 'polish'
                    LEFT JOIN legacy_attempts lat ON lat.entity_id = s.id AND lat.pipeline = 'translation'
                    LEFT JOIN legacy_attempts lap ON lap.entity_id = s.id AND lap.pipeline = 'polish'
                )"
            ));
        }
    }

    sql.push_str(", filtered_records AS (SELECT * FROM status_projection WHERE 1 = 1");
    if attempts.min > 0 || attempts.max.is_some() {
        sql.push_str(" AND attempt_count_known = 1");
    }
    sql.push_str(" AND attempt_count >= ?");
    binds.push(CollectionQueryBind::Integer(attempts.min));
    if let Some(max) = attempts.max {
        sql.push_str(" AND attempt_count <= ?");
        binds.push(CollectionQueryBind::Integer(max));
    }
    if kind != CollectionRecordKind::Brief
        && let Some(values) = translation_filter
        && !values.is_empty()
    {
        sql.push_str(" AND translation_status IN (");
        for (index, _) in values.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            sql.push('?');
        }
        sql.push(')');
        binds.extend(values.iter().cloned().map(CollectionQueryBind::Text));
    }
    if let Some(values) = polish_filter
        && !values.is_empty()
    {
        sql.push_str(" AND polish_status IN (");
        for (index, _) in values.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            sql.push('?');
        }
        sql.push(')');
        binds.extend(values.iter().cloned().map(CollectionQueryBind::Text));
    }
    sql.push(')');
    if let Some((page_size, offset)) = page {
        let order = if kind == CollectionRecordKind::Release {
            "ORDER BY datetime(source_time) DESC, CAST(id AS INTEGER) DESC"
        } else {
            "ORDER BY datetime(source_time) DESC, id DESC"
        };
        sql.push_str(" SELECT id, repository, title, occurred_at, detected_at, generated_at, COUNT(*) OVER () AS total_count FROM filtered_records ");
        sql.push_str(order);
        sql.push_str(" LIMIT ? OFFSET ?");
        binds.push(CollectionQueryBind::Integer(page_size));
        binds.push(CollectionQueryBind::Integer(offset));
    }
    (sql, binds)
}

async fn execute_collection_query_in_connection<T>(
    connection: &mut SqliteConnection,
    sql: &str,
    binds: &[CollectionQueryBind],
) -> Result<Vec<T>, ApiError>
where
    T: serde::de::DeserializeOwned + Send + Unpin,
{
    let mut query = pg_query_as::<T>(sql);
    for bind in binds {
        query = match bind {
            CollectionQueryBind::Text(value) => query.bind(value.clone()),
            CollectionQueryBind::Integer(value) => query.bind(*value),
        };
    }
    query
        .fetch_all(&mut *connection)
        .await
        .map_err(ApiError::internal)
}

async fn execute_collection_scalar_in_connection(
    connection: &mut SqliteConnection,
    sql: &str,
    binds: &[CollectionQueryBind],
) -> Result<i64, ApiError> {
    let mut query = pg_query_scalar::<i64>(sql);
    for bind in binds {
        query = match bind {
            CollectionQueryBind::Text(value) => query.bind(value.clone()),
            CollectionQueryBind::Integer(value) => query.bind(*value),
        };
    }
    query
        .fetch_one(&mut *connection)
        .await
        .map_err(ApiError::internal)
}

#[allow(clippy::too_many_arguments)]
async fn list_collection_page_in_connection(
    connection: &mut SqliteConnection,
    kind: CollectionRecordKind,
    global_mode: bool,
    from: Option<&str>,
    before: Option<&str>,
    attempts: AttemptCountRange,
    translation_filter: Option<&[String]>,
    polish_filter: Option<&[String]>,
    page_size: i64,
    offset: i64,
) -> Result<(i64, Vec<SourceRecordRow>), ApiError> {
    let identity_upgrade_complete = if global_mode {
        identity_upgrade_complete_in_connection(connection).await?
    } else {
        false
    };
    let (page_sql, page_binds) = collection_query_sql(
        kind,
        global_mode,
        identity_upgrade_complete,
        from,
        before,
        attempts,
        translation_filter,
        polish_filter,
        Some((page_size, offset)),
    );
    let rows = execute_collection_query_in_connection::<SourceRecordRow>(
        connection,
        &page_sql,
        &page_binds,
    )
    .await?;
    let total = if let Some(row) = rows.first() {
        row.total_count
    } else {
        let (count_sql, count_binds) = collection_query_sql(
            kind,
            global_mode,
            identity_upgrade_complete,
            from,
            before,
            attempts,
            translation_filter,
            polish_filter,
            None,
        );
        let count_sql = format!("{count_sql} SELECT COUNT(*) FROM filtered_records");
        execute_collection_scalar_in_connection(connection, &count_sql, &count_binds).await?
    };
    Ok((total, rows))
}

struct SharedRead<T> {
    result: tokio::sync::watch::Receiver<Option<Result<Arc<T>, ApiError>>>,
}

type CollectionListFlights =
    tokio::sync::Mutex<HashMap<CollectionListKey, Arc<SharedRead<AdminCollectionRecordsResponse>>>>;
type CollectionActivityFlights = tokio::sync::Mutex<
    HashMap<CollectionActivityKey, Arc<SharedRead<AdminCollectionActivityResponse>>>,
>;

static COLLECTION_LIST_FLIGHTS: OnceLock<Arc<CollectionListFlights>> = OnceLock::new();
static COLLECTION_ACTIVITY_FLIGHTS: OnceLock<Arc<CollectionActivityFlights>> = OnceLock::new();

async fn bounded_collection_read<F, T>(read: F) -> Result<T, ApiError>
where
    F: Future<Output = Result<T, ApiError>> + Send,
    T: Send,
{
    bounded_collection_read_with_budget(read, Duration::from_secs(5)).await
}

async fn bounded_collection_read_with_budget<F, T>(read: F, budget: Duration) -> Result<T, ApiError>
where
    F: Future<Output = Result<T, ApiError>> + Send,
    T: Send,
{
    tokio::time::timeout(budget, read).await.map_err(|_| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "admin_collection_records_timeout",
            "admin collection records read timed out",
        )
        .with_retry_after(1)
    })?
}

async fn await_shared_read<T: Clone>(flight: Arc<SharedRead<T>>) -> Result<Arc<T>, ApiError> {
    let mut result_receiver = flight.result.clone();
    loop {
        let result = result_receiver.borrow_and_update().clone();
        if let Some(result) = result {
            return result;
        }
        if result_receiver.changed().await.is_err() {
            return Err(ApiError::internal(
                "shared collection read ended without a result",
            ));
        }
    }
}

async fn run_keyed_singleflight<K, T, F>(
    flights: Arc<tokio::sync::Mutex<HashMap<K, Arc<SharedRead<T>>>>>,
    key: K,
    read: F,
) -> Result<T, ApiError>
where
    K: Clone + Eq + std::hash::Hash + Send + 'static,
    T: Clone + Send + Sync + 'static,
    F: Future<Output = Result<T, ApiError>> + Send + 'static,
{
    let (flight, result_sender) = {
        let mut guard = flights.lock().await;
        if let Some(flight) = guard.get(&key) {
            (flight.clone(), None)
        } else {
            let (sender, receiver) = tokio::sync::watch::channel(None);
            let flight = Arc::new(SharedRead { result: receiver });
            guard.insert(key.clone(), flight.clone());
            (flight, Some(sender))
        }
    };
    if let Some(result_sender) = result_sender {
        let task_flights = flights.clone();
        tokio::spawn(async move {
            let result = match tokio::spawn(async move {
                bounded_collection_read(read).await.map(Arc::new)
            })
            .await
            {
                Ok(result) => result,
                Err(_) => Err(ApiError::internal("shared collection read task failed")),
            };
            task_flights.lock().await.remove(&key);
            result_sender.send_replace(Some(result));
        });
    }
    await_shared_read(flight)
        .await
        .map(|value| (*value).clone())
}

async fn run_collection_list_singleflight<F>(
    key: CollectionListKey,
    read: F,
) -> Result<AdminCollectionRecordsResponse, ApiError>
where
    F: Future<Output = Result<AdminCollectionRecordsResponse, ApiError>> + Send + 'static,
{
    let flights = COLLECTION_LIST_FLIGHTS
        .get_or_init(|| Arc::new(tokio::sync::Mutex::new(HashMap::new())))
        .clone();
    run_keyed_singleflight(flights, key, read).await
}

async fn run_collection_activity_singleflight<F>(
    key: CollectionActivityKey,
    read: F,
) -> Result<AdminCollectionActivityResponse, ApiError>
where
    F: Future<Output = Result<AdminCollectionActivityResponse, ApiError>> + Send + 'static,
{
    let flights = COLLECTION_ACTIVITY_FLIGHTS
        .get_or_init(|| Arc::new(tokio::sync::Mutex::new(HashMap::new())))
        .clone();
    run_keyed_singleflight(flights, key, read).await
}

pub async fn admin_list_collection_records(
    State(pg_state): State<PostgresServerState>,
    session: Session,
    Path(record_kind): Path<String>,
    Query(query): Query<AdminCollectionRecordListQuery>,
) -> Result<Json<AdminCollectionRecordsResponse>, ApiError> {
    require_admin(&pg_state, &session)
        .await
        .map_err(|status| ApiError::new(status, "admin_required", "admin access required"))?;
    let state = Arc::new(AppState {
        pool: pg_state.runtime.pool.clone(),
    });
    let kind = CollectionRecordKind::parse(record_kind.as_str())?;
    let (page, page_size, offset) = pagination(&query)?;
    let attempts = parse_attempt_count_range(&query)?;
    let translation_filter = parse_status_filter(query.translation_status, "translation_status")?;
    let polish_filter = parse_status_filter(query.polish_status, "polish_status")?;
    let from = parse_timestamp(query.from, "from")?;
    let before = parse_timestamp(query.before, "before")?;
    let requested_from = from.clone();
    let requested_before = before.clone();
    let (from, before) = normalize_collection_window(from, before)?;
    let (key_from, key_before) = collection_list_window_key(
        requested_from,
        requested_before,
        from.clone(),
        before.clone(),
    );
    let key = CollectionListKey {
        kind,
        from: key_from,
        before: key_before,
        attempts,
        translation_filter: translation_filter.clone().unwrap_or_default(),
        polish_filter: polish_filter.clone().unwrap_or_default(),
        page,
        page_size,
    };
    let read_state = state.clone();
    let read = async move {
        let mut transaction = read_state.pool.begin().await.map_err(ApiError::internal)?;
        let legacy_mode = legacy_mode_in_transaction(&mut transaction)
            .await
            .map_err(ApiError::internal)?;
        let global_mode = !legacy_mode;
        let (total, rows) = list_collection_page_in_connection(
            &mut transaction,
            kind,
            global_mode,
            Some(&from),
            Some(&before),
            attempts,
            translation_filter.as_deref(),
            polish_filter.as_deref(),
            page_size,
            offset,
        )
        .await?;
        let ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
        let coverage = load_processing_coverage_in_connection(&mut transaction, kind, &ids).await?;
        let task_summaries =
            load_task_summaries_in_connection(&mut transaction, kind, &ids, &coverage, global_mode)
                .await?;
        let brief_summaries = if kind == CollectionRecordKind::Brief {
            load_brief_summaries_in_connection(&mut transaction, &ids, &coverage).await?
        } else {
            HashMap::new()
        };
        let items = rows
            .into_iter()
            .map(|row| source_record_item(kind, row, &task_summaries, &brief_summaries))
            .collect::<Vec<_>>();
        let response = AdminCollectionRecordsResponse {
            items,
            page,
            page_size,
            total,
        };
        transaction.commit().await.map_err(ApiError::internal)?;
        Ok::<_, ApiError>(response)
    };
    Ok(Json(run_collection_list_singleflight(key, read).await?))
}

pub async fn admin_get_collection_activity(
    State(pg_state): State<PostgresServerState>,
    session: Session,
    Path(record_kind): Path<String>,
) -> Result<Json<AdminCollectionActivityResponse>, ApiError> {
    require_admin(&pg_state, &session)
        .await
        .map_err(|status| ApiError::new(status, "admin_required", "admin access required"))?;
    let state = Arc::new(AppState {
        pool: pg_state.runtime.pool.clone(),
    });
    let kind = CollectionRecordKind::parse(record_kind.as_str())?;
    let now = Utc::now();
    let (window_started_at, window_ended_at) = {
        let (_, started_at, ended_at) = activity_window(now);
        (activity_timestamp(started_at), activity_timestamp(ended_at))
    };
    let key = CollectionActivityKey {
        kind,
        window_started_at: window_started_at.clone(),
        window_ended_at: window_ended_at.clone(),
    };
    let read_state = state.clone();
    let read = async move {
        let global_mode = current_mode(&read_state.pool)
            .await
            .map_err(ApiError::internal)?;
        let rows = load_activity_rows(
            &read_state.pool,
            kind,
            global_mode,
            &window_started_at,
            &window_ended_at,
        )
        .await?;
        Ok::<_, ApiError>(build_activity_response(kind, now, rows))
    };
    Ok(Json(run_collection_activity_singleflight(key, read).await?))
}

async fn load_source_record(
    pool: &SqlitePool,
    kind: CollectionRecordKind,
    id: &str,
) -> Result<SourceRecordRow, ApiError> {
    let sql = match kind {
        CollectionRecordKind::Release => {
            "SELECT CAST(r.release_id AS TEXT) AS id, COALESCE((SELECT wi.repo_full_name FROM repo_release_work_items wi WHERE wi.repo_id = r.repo_id LIMIT 1), '仓库 #' || CAST(r.repo_id AS TEXT)) AS repository, COALESCE(NULLIF(r.name, ''), r.tag_name) AS title, COALESCE(r.published_at, r.created_at, r.updated_at) AS occurred_at, r.detected_at, NULL AS generated_at FROM repo_releases r WHERE CAST(r.release_id AS TEXT) = ? LIMIT 1"
        }
        CollectionRecordKind::Announcement => {
            "WITH ranked_announcements AS (SELECT e.*, ROW_NUMBER() OVER (PARTITION BY lower(e.repo_full_name), e.discussion_number ORDER BY e.occurred_at DESC, lower(COALESCE(e.repo_full_name, '')) DESC, e.discussion_number DESC, COALESCE(e.title, '') DESC, COALESCE(e.body, '') DESC, e.id DESC) AS source_rank FROM social_activity_events e WHERE e.kind = 'announcement' AND lower(e.repo_full_name) || '#' || CAST(e.discussion_number AS TEXT) = ?) SELECT lower(e.repo_full_name) || '#' || CAST(e.discussion_number AS TEXT) AS id, e.repo_full_name AS repository, COALESCE(NULLIF(e.title, ''), '公告') AS title, e.occurred_at, e.detected_at, NULL AS generated_at FROM ranked_announcements e WHERE e.source_rank = 1 LIMIT 1"
        }
        CollectionRecordKind::Notification => {
            "WITH ranked_notifications AS (SELECT n.*, ROW_NUMBER() OVER (PARTITION BY n.thread_id ORDER BY n.updated_at DESC, COALESCE(n.repo_full_name, '') DESC, COALESCE(n.subject_title, '') DESC, COALESCE(n.reason, '') DESC, COALESCE(n.subject_type, '') DESC, n.id DESC) AS source_rank FROM notifications n WHERE n.thread_id = ?) SELECT n.thread_id AS id, n.repo_full_name AS repository, COALESCE(NULLIF(n.subject_title, ''), '通知') AS title, n.updated_at AS occurred_at, NULL AS detected_at, NULL AS generated_at FROM ranked_notifications n WHERE n.source_rank = 1 LIMIT 1"
        }
        CollectionRecordKind::Brief => {
            "SELECT b.id, NULL AS repository, b.date AS title, NULL AS occurred_at, NULL AS detected_at, b.created_at AS generated_at FROM briefs b WHERE b.id = ? LIMIT 1"
        }
    };
    pg_query_as::<SourceRecordRow>(sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "collection record not found",
            )
        })
}

async fn load_record_tasks(
    state: &AppState,
    kind: CollectionRecordKind,
    entity_id: &str,
) -> Result<Vec<TaskRow>, ApiError> {
    let Some((translation_kind, polish_kind)) = kind.task_kinds() else {
        return Ok(Vec::new());
    };
    pg_query_as::< TaskRow>(
        "SELECT w.id, w.kind, w.status, w.result_status, w.attempt_count, w.started_at, w.finished_at, w.updated_at, (SELECT MAX(e.created_at) FROM translation_attempt_events e WHERE e.work_item_id = w.id) AS last_attempt_at FROM translation_work_items w WHERE w.entity_id = ? AND w.kind IN (?, ?)",
    )
    .bind(entity_id)
    .bind(translation_kind)
    .bind(polish_kind)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)
}

async fn load_task_attempts(
    state: &AppState,
    tasks: &[TaskRow],
) -> Result<Vec<AdminCollectionAttempt>, ApiError> {
    if tasks.is_empty() {
        return Ok(Vec::new());
    }
    let kinds = tasks
        .iter()
        .map(|task| (task.id.clone(), task.kind.clone()))
        .collect::<HashMap<_, _>>();
    let mut query = PgReadBuilder::new(
        "SELECT work_item_id, attempt_no, trigger, event_type, result_status, error_code, error_summary, failure_class, processing_stage, provider_status, output_contract_status, retry_disposition, retry_eligible, next_retry_at, llm_call_ids_json, created_at FROM translation_attempt_events WHERE work_item_id IN (",
    );
    {
        let mut separated = query.separated(", ");
        for task in tasks {
            separated.push_bind(task.id.as_str());
        }
    }
    query.push(") ORDER BY datetime(created_at) ASC, id ASC");
    let events = query
        .build_query_as::<AttemptEventRow>()
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    let mut call_ids = Vec::new();
    for event in &events {
        call_ids.extend(
            serde_json::from_str::<Vec<String>>(&event.llm_call_ids_json).unwrap_or_default(),
        );
    }
    call_ids.sort();
    call_ids.dedup();
    let calls_by_id = if call_ids.is_empty() {
        HashMap::new()
    } else {
        let mut query =
            PgReadBuilder::new("SELECT id, status, source, model FROM llm_calls WHERE id IN (");
        {
            let mut separated = query.separated(", ");
            for id in call_ids {
                separated.push_bind(id);
            }
        }
        query.push(")");
        query
            .build_query_as::<AdminCollectionLlmLink>()
            .fetch_all(&state.pool)
            .await
            .map_err(ApiError::internal)?
            .into_iter()
            .map(|call| (call.id.clone(), call))
            .collect::<HashMap<_, _>>()
    };
    let mut relation_query = PgReadBuilder::new(
        "SELECT work_item_id, attempt_no, llm_call_id, stage, relation_role, evidence_availability FROM translation_attempt_llm_calls WHERE work_item_id IN (",
    );
    {
        let mut separated = relation_query.separated(", ");
        for task in tasks {
            separated.push_bind(task.id.as_str());
        }
    }
    relation_query.push(")");
    let relation_rows = relation_query
        .build_query_as::<AttemptCallLinkRow>()
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    let relation_by_key = relation_rows
        .into_iter()
        .map(|row| {
            (
                (
                    row.work_item_id.clone(),
                    row.attempt_no,
                    row.llm_call_id.clone(),
                ),
                row,
            )
        })
        .collect::<HashMap<_, _>>();
    let mut grouped = HashMap::<(String, i64), AttemptBuild>::new();
    for event in events {
        let key = (event.work_item_id.clone(), event.attempt_no);
        let item = grouped.entry(key).or_insert_with(|| AttemptBuild {
            pipeline: if kinds
                .get(&event.work_item_id)
                .is_some_and(|kind| kind.ends_with("_smart"))
            {
                "polish".to_owned()
            } else {
                "translation".to_owned()
            },
            attempt_no: event.attempt_no,
            trigger: event.trigger.clone(),
            status: "queued".to_owned(),
            last_attempt_at: event.created_at.clone(),
            ..AttemptBuild::default()
        });
        apply_attempt_event(item, &event);
        for id in serde_json::from_str::<Vec<String>>(&event.llm_call_ids_json).unwrap_or_default()
        {
            if let Some(call) = calls_by_id.get(&id)
                && !item.llm_calls.iter().any(|existing| existing.id == call.id)
            {
                let mut linked_call = call.clone();
                if let Some(relation) =
                    relation_by_key.get(&(event.work_item_id.clone(), event.attempt_no, id.clone()))
                {
                    linked_call.stage = Some(relation.stage.clone());
                    linked_call.relation_role = Some(relation.relation_role.clone());
                    linked_call.evidence_availability =
                        Some(relation.evidence_availability.clone());
                } else {
                    linked_call.relation_role = Some("legacy_unverified".to_owned());
                    linked_call.evidence_availability = Some("not_captured".to_owned());
                }
                item.llm_calls.push(linked_call);
            }
        }
        for ((_work_item_id, _attempt_no, _), relation) in
            relation_by_key
                .iter()
                .filter(|((work_item_id, attempt_no, _), _)| {
                    work_item_id == &event.work_item_id && *attempt_no == event.attempt_no
                })
        {
            if item
                .llm_calls
                .iter()
                .any(|existing| existing.id == relation.llm_call_id)
            {
                continue;
            }
            item.llm_calls
                .push(calls_by_id.get(&relation.llm_call_id).cloned().unwrap_or(
                    AdminCollectionLlmLink {
                        id: relation.llm_call_id.clone(),
                        status: "expired".to_owned(),
                        source: "诊断载荷已过期".to_owned(),
                        model: "unknown".to_owned(),
                        stage: None,
                        relation_role: None,
                        evidence_availability: Some(relation.evidence_availability.clone()),
                    },
                ));
        }
    }
    let mut attempts = grouped
        .into_iter()
        .map(
            |((work_item_id, attempt_no), item)| AdminCollectionAttempt {
                id: format!("{work_item_id}:{attempt_no}"),
                pipeline: item.pipeline,
                attempt_no: item.attempt_no,
                trigger: item.trigger,
                status: item.status,
                started_at: item.started_at,
                last_attempt_at: Some(item.last_attempt_at),
                finished_at: item.finished_at,
                error_code: item.error_code,
                error_summary: item.error_summary,
                failure_class: item.failure_class,
                processing_stage: item.processing_stage,
                provider_status: item.provider_status,
                output_contract_status: item.output_contract_status,
                retry_disposition: item.retry_disposition,
                retry_eligible: item.retry_eligible,
                next_retry_at: item.next_retry_at,
                llm_calls: item.llm_calls,
            },
        )
        .collect::<Vec<_>>();
    attempts.sort_by(|left, right| {
        compare_attempt_timestamps(
            left.last_attempt_at.as_deref().unwrap_or_default(),
            right.last_attempt_at.as_deref().unwrap_or_default(),
        )
        .then_with(|| left.id.cmp(&right.id))
    });
    Ok(attempts)
}

async fn load_global_attempt_rows_in_connection(
    connection: &mut SqliteConnection,
    kind: CollectionRecordKind,
    entity_id: &str,
) -> Result<Vec<GlobalAttemptRow>, ApiError> {
    let identity_upgrade_complete = identity_upgrade_complete_in_connection(connection).await?;
    let sql = if identity_upgrade_complete {
        GLOBAL_CANONICAL_ATTEMPT_ROWS_SQL
    } else {
        GLOBAL_ATTEMPT_ROWS_SQL
    };
    match pg_query_as::<GlobalAttemptRow>(sql)
        .bind(collection_record_kind_label(kind))
        .bind(entity_id)
        .fetch_all(&mut *connection)
        .await
    {
        Ok(rows) => Ok(rows),
        Err(error) if missing_table(&error) => Ok(Vec::new()),
        Err(error) => Err(ApiError::internal(error)),
    }
}

async fn load_global_attempts(
    state: &AppState,
    kind: CollectionRecordKind,
    entity_id: &str,
) -> Result<Vec<AdminCollectionAttempt>, ApiError> {
    let mut connection = state.pool.acquire().await.map_err(ApiError::internal)?;
    let rows = load_global_attempt_rows_in_connection(&mut connection, kind, entity_id).await?;
    let mut call_query = PgReadBuilder::new(
        "SELECT id, attempt_event_id, status, model FROM content_attempt_llm_calls WHERE attempt_event_id IN (",
    );
    {
        let mut separated = call_query.separated(", ");
        for row in &rows {
            separated.push_bind(row.event_id.as_str());
        }
    }
    call_query.push(")");
    #[derive(Debug, sqlx::FromRow, Deserialize)]
    struct GlobalCallRow {
        id: String,
        status: String,
        model: String,
        attempt_event_id: String,
    }
    let calls = if rows.is_empty() {
        Vec::new()
    } else {
        match call_query
            .build_query_as::<GlobalCallRow>()
            .fetch_all(&mut *connection)
            .await
        {
            Ok(calls) => calls,
            Err(error) if missing_table(&error) => Vec::new(),
            Err(error) => return Err(ApiError::internal(error)),
        }
    };
    let calls_by_event = calls.into_iter().fold(
        HashMap::<String, Vec<AdminCollectionLlmLink>>::new(),
        |mut grouped, call| {
            grouped
                .entry(call.attempt_event_id)
                .or_default()
                .push(AdminCollectionLlmLink {
                    id: call.id,
                    status: call.status,
                    source: "global_content_processing".to_owned(),
                    model: call.model,
                    stage: Some("provider_call".to_owned()),
                    relation_role: Some("primary".to_owned()),
                    evidence_availability: Some("captured".to_owned()),
                });
            grouped
        },
    );
    let mut grouped = HashMap::<(String, i64), AdminCollectionAttempt>::new();
    for row in rows {
        let key = (row.work_item_id.clone(), row.attempt_no);
        let attempt = grouped
            .entry(key)
            .or_insert_with(|| AdminCollectionAttempt {
                id: format!("{}:{}", row.work_item_id, row.attempt_no),
                pipeline: if row.pipeline == "polishing" {
                    "polish".to_owned()
                } else {
                    "translation".to_owned()
                },
                attempt_no: row.attempt_no,
                trigger: row.trigger.clone(),
                status: "queued".to_owned(),
                started_at: None,
                last_attempt_at: Some(row.created_at.clone()),
                finished_at: None,
                error_code: None,
                error_summary: None,
                failure_class: None,
                processing_stage: None,
                provider_status: None,
                output_contract_status: None,
                retry_disposition: None,
                retry_eligible: false,
                next_retry_at: None,
                llm_calls: Vec::new(),
            });
        attempt.trigger = row.trigger;
        attempt.last_attempt_at = Some(row.created_at.clone());
        match row.event_type.as_str() {
            "attempt_started" => {
                attempt.status = "running".to_owned();
                attempt.started_at = Some(row.created_at.clone());
            }
            "attempt_completed" => {
                attempt.status = row.result_status.unwrap_or_else(|| "completed".to_owned());
                attempt.finished_at = Some(row.created_at.clone());
            }
            "retry_scheduled" => {
                attempt.status = "retry_scheduled".to_owned();
                attempt.finished_at = Some(row.created_at.clone());
            }
            "attempt_queued" => {
                attempt.status = "queued".to_owned();
                attempt.started_at = None;
                attempt.finished_at = None;
            }
            _ => {}
        }
        if row.error_code.is_some() {
            attempt.error_code = row.error_code;
        }
        if row.error_summary.is_some() {
            attempt.error_summary = row.error_summary;
        }
        if row.failure_class.is_some() {
            attempt.failure_class = row.failure_class;
        }
        attempt.retry_eligible |= row.retry_eligible != 0;
        if row.next_retry_at.is_some() {
            attempt.next_retry_at = row.next_retry_at;
        }
        if let Some(calls) = calls_by_event.get(&row.event_id) {
            for call in calls {
                if !attempt
                    .llm_calls
                    .iter()
                    .any(|existing| existing.id == call.id)
                {
                    attempt.llm_calls.push(call.clone());
                }
            }
        }
    }
    let mut attempts = grouped.into_values().collect::<Vec<_>>();
    attempts.sort_by(|left, right| {
        compare_attempt_timestamps(
            left.last_attempt_at.as_deref().unwrap_or_default(),
            right.last_attempt_at.as_deref().unwrap_or_default(),
        )
        .then_with(|| left.id.cmp(&right.id))
    });
    Ok(attempts)
}

const GLOBAL_ATTEMPT_ROWS_SQL: &str = "SELECT e.id AS event_id, e.work_item_id, w.pipeline, e.attempt_no, e.trigger, e.event_type, e.result_status, e.error_code, e.error_summary, e.failure_class, e.retry_eligible, e.next_retry_at, e.created_at FROM content_attempt_events e JOIN content_work_items w ON w.id = e.work_item_id WHERE w.canonical_resource_type = ? AND w.canonical_resource_id = ? AND ((w.pipeline = 'translation' AND w.variant IN ('detail', 'summary', 'shared')) OR (w.pipeline = 'polishing' AND w.variant = 'smart')) ORDER BY julianday(e.created_at) ASC, e.created_at ASC, e.id ASC";

const GLOBAL_CANONICAL_ATTEMPT_ROWS_SQL: &str = "WITH ranked_members AS (SELECT m.identity_id, w.*, ROW_NUMBER() OVER (PARTITION BY m.identity_id ORDER BY CASE w.status WHEN 'queued' THEN 0 WHEN 'running' THEN 1 WHEN 'deferred_provider' THEN 2 WHEN 'blocked_config' THEN 3 WHEN 'failed' THEN 4 WHEN 'ready' THEN 5 WHEN 'superseded' THEN 9 ELSE 6 END, w.attempt_count DESC, julianday(w.updated_at) DESC, w.updated_at DESC, w.id DESC) AS member_rank FROM content_work_identity_members m JOIN content_work_items w ON w.id = m.work_item_id WHERE w.canonical_resource_type = ? AND w.canonical_resource_id = ? AND ((w.pipeline = 'translation' AND w.variant IN ('detail', 'summary', 'shared')) OR (w.pipeline = 'polishing' AND w.variant = 'smart'))), canonical_identity_work AS (SELECT * FROM ranked_members WHERE member_rank = 1), ranked_global_work AS (SELECT *, ROW_NUMBER() OVER (PARTITION BY canonical_resource_id, CASE WHEN pipeline = 'polishing' THEN 'polish' ELSE 'translation' END ORDER BY julianday(created_at) DESC, created_at DESC, CASE status WHEN 'queued' THEN 0 WHEN 'running' THEN 1 WHEN 'deferred_provider' THEN 2 WHEN 'blocked_config' THEN 3 WHEN 'failed' THEN 4 WHEN 'ready' THEN 5 WHEN 'superseded' THEN 9 ELSE 6 END, CASE WHEN pipeline = 'translation' AND variant = 'detail' THEN 0 ELSE 1 END, julianday(updated_at) DESC, updated_at DESC, id DESC) AS global_rank FROM canonical_identity_work), canonical_work AS (SELECT id AS work_item_id FROM ranked_global_work WHERE global_rank = 1) SELECT e.id AS event_id, e.work_item_id, w.pipeline, e.attempt_no, e.trigger, e.event_type, e.result_status, e.error_code, e.error_summary, e.failure_class, e.retry_eligible, e.next_retry_at, e.created_at FROM content_attempt_events e JOIN canonical_work cw ON cw.work_item_id = e.work_item_id JOIN content_work_items w ON w.id = e.work_item_id ORDER BY julianday(e.created_at) ASC, e.created_at ASC, e.id ASC";

async fn load_brief_attempts(
    state: &AppState,
    brief_id: &str,
) -> Result<Vec<AdminCollectionAttempt>, ApiError> {
    let calls = pg_query_as::< BriefCallRow>(
        "SELECT c.id, c.status, c.source, c.attempt_count, c.started_at, c.finished_at, c.updated_at,
            (SELECT MAX(e.created_at) FROM llm_call_events e WHERE e.call_id = c.id AND e.event_type IN ('llm.running', 'llm.attempt_failed', 'llm.succeeded', 'llm.failed')) AS last_attempt_at,
            c.error_text FROM llm_calls c WHERE c.parent_brief_id = ? ORDER BY datetime(c.created_at) ASC, c.id ASC",
    )
    .bind(brief_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    if calls.is_empty() {
        return Ok(Vec::new());
    }
    let mut query = PgReadBuilder::new(
        "SELECT call_id, event_type, status, model, attempt,
            CASE WHEN event_type IN ('llm.succeeded', 'llm.failed')
                THEN CAST(json_extract(payload_json, '$.attempt_count') AS INTEGER)
            END AS terminal_attempt_count,
            failure_class, created_at
         FROM llm_call_events WHERE call_id IN (",
    );
    {
        let mut separated = query.separated(", ");
        for call in &calls {
            separated.push_bind(call.id.as_str());
        }
    }
    query.push(") ORDER BY datetime(created_at), id");
    let events = query
        .build_query_as::<BriefAttemptEventRow>()
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    Ok(build_brief_attempts(calls, events))
}

fn build_brief_attempts(
    calls: Vec<BriefCallRow>,
    events: Vec<BriefAttemptEventRow>,
) -> Vec<AdminCollectionAttempt> {
    let mut events_by_call = HashMap::<String, Vec<BriefAttemptEventRow>>::new();
    for event in events {
        events_by_call
            .entry(event.call_id.clone())
            .or_default()
            .push(event);
    }
    let mut attempts = Vec::new();
    for call in calls {
        let call_events = events_by_call.remove(&call.id).unwrap_or_default();
        let latest_attempt = call_events
            .iter()
            .filter_map(|event| event.attempt.or(event.terminal_attempt_count))
            .max()
            .unwrap_or(0)
            .max(call.attempt_count.max(0));
        for attempt_no in 1..=latest_attempt {
            let running = call_events.iter().find(|event| {
                event.event_type == "llm.running" && event.attempt == Some(attempt_no)
            });
            let failed = call_events.iter().find(|event| {
                event.event_type == "llm.attempt_failed" && event.attempt == Some(attempt_no)
            });
            let terminal = call_events.iter().find(|event| {
                matches!(event.event_type.as_str(), "llm.succeeded" | "llm.failed")
                    && event.terminal_attempt_count == Some(attempt_no)
            });
            let is_latest = attempt_no == latest_attempt;
            let status = terminal
                .map(|event| event.status.clone())
                .or_else(|| failed.map(|_| "failed".to_owned()))
                .or_else(|| running.map(|event| event.status.clone()))
                .unwrap_or_else(|| "not_recorded".to_owned());
            let failure_class = failed
                .and_then(|event| event.failure_class.clone())
                .or_else(|| terminal.and_then(|event| event.failure_class.clone()));
            let classified = (is_latest && status == "failed")
                .then(|| translations::classify_translation_error(call.error_text.as_deref()))
                .flatten();
            let retry_eligible = status == "failed"
                && attempt_no < latest_attempt
                && (failed.is_some() || terminal.is_some());
            let last_attempt_at = terminal
                .map(|event| event.created_at.clone())
                .or_else(|| failed.map(|event| event.created_at.clone()))
                .or_else(|| running.map(|event| event.created_at.clone()));
            let started_at = running.map(|event| event.created_at.clone());
            let finished_at = terminal
                .map(|event| event.created_at.clone())
                .or_else(|| failed.map(|event| event.created_at.clone()));
            let model = running
                .and_then(|event| event.model.clone())
                .or_else(|| failed.and_then(|event| event.model.clone()))
                .or_else(|| terminal.and_then(|event| event.model.clone()))
                .unwrap_or_else(|| "unknown".to_owned());
            let has_attempt_evidence = running.is_some() || failed.is_some() || terminal.is_some();
            attempts.push(AdminCollectionAttempt {
                id: format!("{}:{attempt_no}", call.id),
                pipeline: "polish".to_owned(),
                attempt_no,
                trigger: "daily_brief_generation".to_owned(),
                status: status.clone(),
                started_at,
                last_attempt_at,
                finished_at,
                error_code: classified.as_ref().map(|value| value.code.to_owned()),
                error_summary: classified.map(|value| value.summary.to_owned()),
                failure_class,
                processing_stage: Some("brief".to_owned()),
                provider_status: Some(status.clone()),
                output_contract_status: None,
                retry_disposition: if status == "not_recorded" {
                    None
                } else if retry_eligible {
                    Some("retry_scheduled".to_owned())
                } else {
                    Some("not_needed".to_owned())
                },
                retry_eligible,
                next_retry_at: None,
                llm_calls: if has_attempt_evidence {
                    vec![AdminCollectionLlmLink {
                        id: call.id.clone(),
                        status: status.clone(),
                        source: call.source.clone(),
                        model,
                        stage: Some("provider_call".to_owned()),
                        relation_role: Some("primary".to_owned()),
                        evidence_availability: Some("available".to_owned()),
                    }]
                } else {
                    Vec::new()
                },
            });
        }
    }
    attempts
}

fn select_attempt_history(
    global_mode: bool,
    summaries: &TaskSummaries,
    global_attempts: Vec<AdminCollectionAttempt>,
    legacy_attempts: Vec<AdminCollectionAttempt>,
) -> Vec<AdminCollectionAttempt> {
    if !global_mode {
        return legacy_attempts;
    }
    let has_global_work = |pipeline: &str| match pipeline {
        "translation" => summaries.translation.global_work.is_some(),
        "polish" => summaries.polish.global_work.is_some(),
        _ => false,
    };
    let mut attempts = global_attempts
        .into_iter()
        .filter(|attempt| has_global_work(&attempt.pipeline))
        .chain(
            legacy_attempts
                .into_iter()
                .filter(|attempt| !has_global_work(&attempt.pipeline)),
        )
        .collect::<Vec<_>>();
    attempts.sort_by(|left, right| {
        compare_attempt_timestamps(
            left.last_attempt_at.as_deref().unwrap_or_default(),
            right.last_attempt_at.as_deref().unwrap_or_default(),
        )
        .then_with(|| left.id.cmp(&right.id))
    });
    attempts
}

pub async fn admin_get_collection_record_detail(
    State(pg_state): State<PostgresServerState>,
    session: Session,
    Path((record_kind, record_id)): Path<(String, String)>,
) -> Result<Json<AdminCollectionRecordDetail>, ApiError> {
    require_admin(&pg_state, &session)
        .await
        .map_err(|status| ApiError::new(status, "admin_required", "admin access required"))?;
    let state = Arc::new(AppState {
        pool: pg_state.runtime.pool.clone(),
    });
    let kind = CollectionRecordKind::parse(record_kind.as_str())?;
    let source = load_source_record(&state.pool, kind, &record_id).await?;
    let ids = vec![source.id.clone()];
    let coverage = load_processing_coverage(&state.pool, kind, &ids).await?;
    let global_mode = current_mode(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    let task_summaries =
        load_task_summaries(state.as_ref(), kind, &ids, &coverage, global_mode).await?;
    let brief_summaries = if kind == CollectionRecordKind::Brief {
        load_brief_summaries(state.as_ref(), &ids, &coverage).await?
    } else {
        HashMap::new()
    };
    let record = source_record_item(kind, source, &task_summaries, &brief_summaries);
    let attempts = if kind == CollectionRecordKind::Brief {
        load_brief_attempts(state.as_ref(), &record.id).await?
    } else {
        let global_attempts = if global_mode {
            load_global_attempts(state.as_ref(), kind, &record.id).await?
        } else {
            Vec::new()
        };
        let legacy_tasks = load_record_tasks(state.as_ref(), kind, &record.id).await?;
        let legacy_attempts = load_task_attempts(state.as_ref(), &legacy_tasks).await?;
        let summaries = task_summaries
            .get(&record.id)
            .cloned()
            .unwrap_or_else(|| TaskSummaries {
                translation: not_recorded_summary(),
                polish: not_recorded_summary(),
            });
        select_attempt_history(global_mode, &summaries, global_attempts, legacy_attempts)
    };
    Ok(Json(AdminCollectionRecordDetail { record, attempts }))
}

fn pg_sql(input: &str) -> String {
    fn transform(input: &str) -> String {
        let bytes = input.as_bytes();
        let mut out = String::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\'' {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\'' {
                        i += 1;
                        if i < bytes.len() && bytes[i] == b'\'' {
                            i += 1;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                out.push_str(&input[start..i]);
                continue;
            }
            if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let word = &input[start..i];
                let mut open = i;
                while open < bytes.len() && bytes[open].is_ascii_whitespace() {
                    open += 1;
                }
                if open < bytes.len()
                    && bytes[open] == b'('
                    && matches!(
                        word.to_ascii_lowercase().as_str(),
                        "julianday" | "datetime" | "json_extract" | "max" | "min"
                    )
                {
                    let mut end = open + 1;
                    let mut depth = 1;
                    let mut quote = false;
                    let mut args = Vec::new();
                    let mut begin = end;
                    while end < bytes.len() {
                        match bytes[end] {
                            b'\'' => {
                                if quote && end + 1 < bytes.len() && bytes[end + 1] == b'\'' {
                                    end += 2;
                                    continue;
                                }
                                quote = !quote
                            }
                            b'(' if !quote => depth += 1,
                            b')' if !quote => {
                                depth -= 1;
                                if depth == 0 {
                                    args.push(&input[begin..end]);
                                    break;
                                }
                            }
                            b',' if !quote && depth == 1 => {
                                args.push(&input[begin..end]);
                                begin = end + 1
                            }
                            _ => {}
                        }
                        end += 1;
                    }
                    if depth == 0 {
                        let args = args.into_iter().map(transform).collect::<Vec<_>>();
                        let name = word.to_ascii_lowercase();
                        if matches!(name.as_str(), "julianday" | "datetime") {
                            out.push_str(&format!(
                                "EXTRACT(EPOCH FROM NULLIF(({})::text,'')::timestamptz)",
                                args[0]
                            ));
                        } else if name == "json_extract" {
                            let key = args[1].trim().trim_matches('\'').trim_start_matches("$.");
                            out.push_str(&format!("(({})::jsonb ->> '{}')", args[0], key));
                        } else {
                            let name = if args.len() > 1 {
                                if name == "max" { "GREATEST" } else { "LEAST" }
                            } else {
                                word
                            };
                            out.push_str(&format!("{}({})", name, args.join(",")));
                        }
                        i = end + 1;
                        continue;
                    }
                }
                out.push_str(word);
                continue;
            }
            let ch = input[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }
    let mut sql = transform(input).replace(" AS INTEGER)", " AS BIGINT)");
    for table in [
        "admin_collection_processing_coverage",
        "content_identity_upgrade_control",
        "content_work_identity_members",
        "content_work_identities",
        "content_current_result_projections",
        "content_work_items",
        "content_result_projections",
        "content_legacy_observations",
        "translation_work_items",
        "translation_attempt_events",
        "translation_attempt_llm_calls",
        "content_attempt_events",
        "content_attempt_llm_calls",
        "llm_calls",
        "llm_call_events",
        "repo_release_work_items",
        "repo_releases",
        "social_activity_events",
        "notifications",
        "briefs",
    ] {
        for prefix in ["FROM ", "JOIN "] {
            sql = sql.replace(
                &format!("{prefix}{table} "),
                &format!("{prefix}public.{table} "),
            );
        }
    }
    let mut output = String::new();
    let mut index = 0;
    let mut quoted = false;
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            output.push(c);
            if quoted && chars.peek() == Some(&'\'') {
                output.push(chars.next().unwrap());
                continue;
            }
            quoted = !quoted;
        } else if c == '?' && !quoted {
            index += 1;
            output.push_str(&format!("${index}"));
        } else {
            output.push(c)
        }
    }
    output
}

#[derive(Clone)]
enum PgBind {
    Text(Option<String>),
    Integer(Option<i64>),
}
trait IntoPgBind {
    fn into_pg_bind(self) -> PgBind;
}
impl IntoPgBind for String {
    fn into_pg_bind(self) -> PgBind {
        PgBind::Text(Some(self))
    }
}
impl IntoPgBind for &String {
    fn into_pg_bind(self) -> PgBind {
        PgBind::Text(Some(self.clone()))
    }
}
impl IntoPgBind for &str {
    fn into_pg_bind(self) -> PgBind {
        PgBind::Text(Some(self.to_owned()))
    }
}
impl IntoPgBind for Option<&str> {
    fn into_pg_bind(self) -> PgBind {
        PgBind::Text(self.map(str::to_owned))
    }
}
impl IntoPgBind for i64 {
    fn into_pg_bind(self) -> PgBind {
        PgBind::Integer(Some(self))
    }
}
impl IntoPgBind for Option<i64> {
    fn into_pg_bind(self) -> PgBind {
        PgBind::Integer(self)
    }
}

struct PgReadQuery<T> {
    sql: String,
    binds: Vec<PgBind>,
    scalar: bool,
    marker: std::marker::PhantomData<T>,
}
fn pg_query_as<T>(sql: &str) -> PgReadQuery<T> {
    PgReadQuery {
        sql: sql.to_owned(),
        binds: Vec::new(),
        scalar: false,
        marker: std::marker::PhantomData,
    }
}
fn pg_query_scalar<T>(sql: &str) -> PgReadQuery<T> {
    PgReadQuery {
        sql: sql.to_owned(),
        binds: Vec::new(),
        scalar: true,
        marker: std::marker::PhantomData,
    }
}
impl<T: serde::de::DeserializeOwned + Send + Unpin> PgReadQuery<T> {
    fn bind(mut self, value: impl IntoPgBind) -> Self {
        self.binds.push(value.into_pg_bind());
        self
    }
    async fn fetch_all<'e, E>(self, executor: E) -> Result<Vec<T>, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = Postgres>,
    {
        let sql = pg_sql(&self.sql);
        let wrapped = format!("SELECT to_jsonb(pg_collection_row) FROM ({sql}) pg_collection_row");
        let mut query = sqlx::query_scalar::<_, serde_json::Value>(&wrapped);
        for value in self.binds {
            query = match value {
                PgBind::Text(v) => query.bind(v),
                PgBind::Integer(v) => query.bind(v),
            }
        }
        let rows = query.fetch_all(executor).await?;
        rows.into_iter()
            .map(|mut row| {
                normalize_pg_json(&mut row);
                let row = if self.scalar {
                    row.as_object()
                        .and_then(|o| o.values().next())
                        .cloned()
                        .unwrap_or(serde_json::Value::Null)
                } else if std::any::type_name::<T>().starts_with('(') {
                    serde_json::json!([row["status"], row["phase"]])
                } else {
                    row
                };
                serde_json::from_value(row).map_err(|e| sqlx::Error::Decode(Box::new(e)))
            })
            .collect()
    }
    async fn fetch_optional<'e, E>(self, executor: E) -> Result<Option<T>, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = Postgres>,
    {
        Ok(self.fetch_all(executor).await?.into_iter().next())
    }
    async fn fetch_one<'e, E>(self, executor: E) -> Result<T, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = Postgres>,
    {
        self.fetch_optional(executor)
            .await?
            .ok_or(sqlx::Error::RowNotFound)
    }
}
fn normalize_pg_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Bool(v) => *value = serde_json::json!(i64::from(*v)),
        serde_json::Value::Object(values) => {
            for v in values.values_mut() {
                normalize_pg_json(v)
            }
        }
        serde_json::Value::Array(values) => {
            for v in values {
                normalize_pg_json(v)
            }
        }
        _ => {}
    }
}
struct PgReadBuilder {
    sql: String,
    binds: Vec<PgBind>,
}
impl PgReadBuilder {
    fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            binds: Vec::new(),
        }
    }
    fn push(&mut self, sql: impl std::fmt::Display) -> &mut Self {
        self.sql.push_str(&sql.to_string());
        self
    }
    fn push_bind(&mut self, value: impl IntoPgBind) -> &mut Self {
        self.sql.push('?');
        self.binds.push(value.into_pg_bind());
        self
    }
    fn separated(&mut self, separator: &str) -> PgSeparated<'_> {
        PgSeparated {
            builder: self,
            separator: separator.to_owned(),
            first: true,
        }
    }
    fn build_query_as<T>(&mut self) -> PgReadQuery<T> {
        PgReadQuery {
            sql: self.sql.clone(),
            binds: self.binds.clone(),
            scalar: false,
            marker: std::marker::PhantomData,
        }
    }
}
struct PgSeparated<'a> {
    builder: &'a mut PgReadBuilder,
    separator: String,
    first: bool,
}
impl PgSeparated<'_> {
    fn push_bind(&mut self, value: impl IntoPgBind) -> &mut Self {
        if !self.first {
            self.builder.push(&self.separator);
        }
        self.first = false;
        self.builder.push_bind(value);
        self
    }
}
async fn current_mode(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT mode FROM public.content_processing_control WHERE id=1",
    )
    .fetch_one(pool)
    .await?
        != "legacy")
}
async fn legacy_mode_in_transaction(
    tx: &mut sqlx::Transaction<'_, Postgres>,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT mode FROM public.content_processing_control WHERE id=1",
    )
    .fetch_one(&mut **tx)
    .await?
        == "legacy")
}
