use std::{collections::HashMap, sync::Arc};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use tower_sessions::Session;

use crate::{api, error::ApiError, state::AppState, translations};

const PAGE_SIZE_DEFAULT: i64 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CollectionRecordKind {
    Release,
    Announcement,
    Brief,
}

impl CollectionRecordKind {
    fn parse(raw: &str) -> Result<Self, ApiError> {
        match raw {
            "release" => Ok(Self::Release),
            "announcement" => Ok(Self::Announcement),
            "brief" => Ok(Self::Brief),
            _ => Err(ApiError::bad_request("invalid collection record kind")),
        }
    }

    fn task_kinds(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Release => Some(("release_detail", "release_smart")),
            Self::Announcement => Some(("announcement_detail", "announcement_smart")),
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
pub struct AdminCollectionTaskSummary {
    pub status: String,
    pub display_status: String,
    pub status_origin: String,
    pub retry_count: i64,
    pub started_at: Option<String>,
    pub last_attempt_at: Option<String>,
    pub finished_at: Option<String>,
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

#[derive(Debug, Serialize)]
pub struct AdminCollectionRecordsResponse {
    pub items: Vec<AdminCollectionRecordItem>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AdminCollectionLlmLink {
    pub id: String,
    pub status: String,
    pub source: String,
    pub model: String,
    #[sqlx(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[sqlx(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation_role: Option<String>,
    #[sqlx(default)]
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
    pub last_attempt_at: String,
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

#[derive(Debug, sqlx::FromRow)]
struct SourceRecordRow {
    id: String,
    repository: Option<String>,
    title: String,
    occurred_at: Option<String>,
    detected_at: Option<String>,
    generated_at: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
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

#[derive(Debug, Clone, sqlx::FromRow)]
struct ProcessingCoverageRow {
    record_id: String,
    pipeline: String,
    status_origin: String,
}

#[derive(Debug, sqlx::FromRow)]
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

#[derive(Debug, sqlx::FromRow)]
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

#[derive(Debug, sqlx::FromRow)]
struct BriefCallRow {
    id: String,
    status: String,
    source: String,
    model: String,
    attempt_count: i64,
    started_at: Option<String>,
    finished_at: Option<String>,
    updated_at: String,
    error_text: Option<String>,
    failure_class: Option<String>,
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

const DISPLAY_STATUSES: [&str; 8] = [
    "not_started",
    "queued",
    "running",
    "succeeded",
    "failed",
    "missing",
    "disabled",
    "historical_unknown",
];

fn display_status_for(status: &str, status_origin: &str) -> String {
    match status {
        "queued" | "batched" | "retry_scheduled" => "queued".to_owned(),
        "running" => "running".to_owned(),
        "completed" | "succeeded" | "ready" => "succeeded".to_owned(),
        "failed" | "error" => "failed".to_owned(),
        "missing" => "missing".to_owned(),
        "disabled" => "disabled".to_owned(),
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

fn status_matches(summary: &AdminCollectionTaskSummary, filter: Option<&[String]>) -> bool {
    filter.is_none_or(|values| values.iter().any(|value| value == &summary.display_status))
}

fn collection_record_kind_label(kind: CollectionRecordKind) -> &'static str {
    match kind {
        CollectionRecordKind::Release => "release",
        CollectionRecordKind::Announcement => "announcement",
        CollectionRecordKind::Brief => "brief",
    }
}

fn collection_pipelines(kind: CollectionRecordKind) -> &'static [&'static str] {
    match kind {
        CollectionRecordKind::Release | CollectionRecordKind::Announcement => {
            &["translation", "polish"]
        }
        CollectionRecordKind::Brief => &["polish"],
    }
}

async fn ensure_processing_coverage(
    pool: &SqlitePool,
    kind: CollectionRecordKind,
    record_ids: &[String],
) -> Result<(), ApiError> {
    if record_ids.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::<Sqlite>::new(
        "INSERT OR IGNORE INTO admin_collection_processing_coverage (record_kind, record_id, pipeline, status_origin) ",
    );
    query.push("VALUES ");
    let mut first = true;
    for record_id in record_ids {
        for pipeline in collection_pipelines(kind) {
            if !first {
                query.push(", ");
            }
            first = false;
            query
                .push("(")
                .push_bind(collection_record_kind_label(kind))
                .push(", ")
                .push_bind(record_id)
                .push(", ")
                .push_bind(*pipeline)
                .push(", 'never_started')");
        }
    }
    query
        .build()
        .execute(pool)
        .await
        .map_err(ApiError::internal)?;
    Ok(())
}

async fn load_processing_coverage(
    pool: &SqlitePool,
    kind: CollectionRecordKind,
    record_ids: &[String],
) -> Result<HashMap<(String, String), String>, ApiError> {
    if record_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut query = QueryBuilder::<Sqlite>::new(
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
        .fetch_all(pool)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AttemptCountRange {
    min: i64,
    max: Option<i64>,
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
        retry_count: rows
            .iter()
            .map(|row| row.attempt_count.saturating_sub(1))
            .max()
            .unwrap_or(0),
        started_at: rows.iter().filter_map(|row| row.started_at.clone()).min(),
        last_attempt_at: rows
            .iter()
            .filter_map(|row| row.last_attempt_at.clone())
            .max(),
        finished_at: rows.iter().filter_map(|row| row.finished_at.clone()).max(),
    }
}

async fn load_task_summaries(
    state: &AppState,
    kind: CollectionRecordKind,
    entity_ids: &[String],
    coverage: &HashMap<(String, String), String>,
) -> Result<HashMap<String, TaskSummaries>, ApiError> {
    let Some((translation_kind, polish_kind)) = kind.task_kinds() else {
        return Ok(HashMap::new());
    };
    if entity_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut query = QueryBuilder::<Sqlite>::new(
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
    #[derive(Debug, sqlx::FromRow)]
    struct TaskWithEntityRow {
        #[sqlx(flatten)]
        task: TaskRow,
        entity_id: String,
    }
    let rows = query
        .build_query_as::<TaskWithEntityRow>()
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::internal)?;
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
            (
                id.clone(),
                TaskSummaries {
                    translation: merge_summary(
                        &translation,
                        &coverage_origin(coverage, id, "translation"),
                    ),
                    polish: merge_summary(&polish, &coverage_origin(coverage, id, "polish")),
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
    if brief_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT id, status, source, model, attempt_count, started_at, finished_at, updated_at, error_text, failure_class, parent_brief_id FROM llm_calls WHERE parent_brief_id IN (",
    );
    {
        let mut separated = query.separated(", ");
        for id in brief_ids {
            separated.push_bind(id);
        }
    }
    query.push(")");
    #[derive(Debug, sqlx::FromRow)]
    struct BriefCallWithParentRow {
        #[sqlx(flatten)]
        call: BriefCallRow,
        parent_brief_id: String,
    }
    let rows = query
        .build_query_as::<BriefCallWithParentRow>()
        .fetch_all(&state.pool)
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
                    retry_count: calls
                        .iter()
                        .map(|call| call.attempt_count.saturating_sub(1))
                        .max()
                        .unwrap_or(0),
                    started_at: calls
                        .iter()
                        .filter_map(|call| call.started_at.clone())
                        .min(),
                    last_attempt_at: calls.iter().map(|call| call.updated_at.clone()).max(),
                    finished_at: calls
                        .iter()
                        .filter_map(|call| call.finished_at.clone())
                        .max(),
                },
            );
            (id.clone(), summary)
        })
        .collect())
}

async fn list_source_rows(
    pool: &SqlitePool,
    kind: CollectionRecordKind,
    from: Option<&str>,
    before: Option<&str>,
    attempts: AttemptCountRange,
) -> Result<Vec<SourceRecordRow>, ApiError> {
    let source_sql = match kind {
        CollectionRecordKind::Release => {
            "WITH source_records AS (
                SELECT
                  CAST(r.release_id AS TEXT) AS id,
                  COALESCE((SELECT wi.repo_full_name FROM repo_release_work_items wi WHERE wi.repo_id = r.repo_id LIMIT 1), '仓库 #' || CAST(r.repo_id AS TEXT)) AS repository,
                  COALESCE(NULLIF(r.name, ''), r.tag_name) AS title,
                  COALESCE(r.published_at, r.created_at, r.updated_at) AS source_time,
                  COALESCE(r.published_at, r.created_at, r.updated_at) AS occurred_at,
                  r.detected_at,
                  NULL AS generated_at,
                  COALESCE((SELECT MAX(w.attempt_count) FROM translation_work_items w WHERE w.entity_id = CAST(r.release_id AS TEXT) AND w.kind IN ('release_detail', 'release_smart')), 0) AS attempt_count
                FROM repo_releases r
            )
            SELECT id, repository, title, occurred_at, detected_at, generated_at
            FROM source_records
            WHERE (? IS NULL OR datetime(source_time) >= datetime(?))
              AND (? IS NULL OR datetime(source_time) < datetime(?))
              AND attempt_count >= ?
              AND attempt_count <= COALESCE(?, attempt_count)
            ORDER BY datetime(source_time) DESC, CAST(id AS INTEGER) DESC"
        }
        CollectionRecordKind::Announcement => {
            "WITH source_records AS (
                SELECT
                  lower(e.repo_full_name) || '#' || CAST(e.discussion_number AS TEXT) AS id,
                  MAX(e.repo_full_name) AS repository,
                  COALESCE(MAX(NULLIF(e.title, '')), '公告') AS title,
                  MAX(e.occurred_at) AS source_time,
                  MAX(e.occurred_at) AS occurred_at,
                  MIN(e.detected_at) AS detected_at,
                  NULL AS generated_at,
                  COALESCE((SELECT MAX(w.attempt_count) FROM translation_work_items w WHERE w.entity_id = lower(e.repo_full_name) || '#' || CAST(e.discussion_number AS TEXT) AND w.kind IN ('announcement_detail', 'announcement_smart')), 0) AS attempt_count
                FROM social_activity_events e
                WHERE e.kind = 'announcement'
                  AND e.repo_full_name IS NOT NULL
                  AND e.discussion_number IS NOT NULL
                GROUP BY lower(e.repo_full_name), e.discussion_number
            )
            SELECT id, repository, title, occurred_at, detected_at, generated_at
            FROM source_records
            WHERE (? IS NULL OR datetime(source_time) >= datetime(?))
              AND (? IS NULL OR datetime(source_time) < datetime(?))
              AND attempt_count >= ?
              AND attempt_count <= COALESCE(?, attempt_count)
            ORDER BY datetime(source_time) DESC, id DESC"
        }
        CollectionRecordKind::Brief => {
            "WITH source_records AS (
                SELECT
                  b.id,
                  NULL AS repository,
                  b.date AS title,
                  b.created_at AS source_time,
                  NULL AS occurred_at,
                  NULL AS detected_at,
                  b.created_at AS generated_at,
                  COALESCE((SELECT MAX(c.attempt_count) FROM llm_calls c WHERE c.parent_brief_id = b.id), 0) AS attempt_count
                FROM briefs b
            )
            SELECT id, repository, title, occurred_at, detected_at, generated_at
            FROM source_records
            WHERE (? IS NULL OR datetime(source_time) >= datetime(?))
              AND (? IS NULL OR datetime(source_time) < datetime(?))
              AND attempt_count >= ?
              AND attempt_count <= COALESCE(?, attempt_count)
            ORDER BY datetime(source_time) DESC, id DESC"
        }
    };
    sqlx::query_as::<_, SourceRecordRow>(source_sql)
        .bind(from)
        .bind(from)
        .bind(before)
        .bind(before)
        .bind(attempts.min)
        .bind(attempts.max)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)
}

pub async fn admin_list_collection_records(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(record_kind): Path<String>,
    Query(query): Query<AdminCollectionRecordListQuery>,
) -> Result<Json<AdminCollectionRecordsResponse>, ApiError> {
    let _acting_user_id = api::require_admin_user_id(state.as_ref(), &session).await?;
    let kind = CollectionRecordKind::parse(record_kind.as_str())?;
    let (page, page_size, offset) = pagination(&query)?;
    let attempts = parse_attempt_count_range(&query)?;
    let translation_filter = parse_status_filter(query.translation_status, "translation_status")?;
    let polish_filter = parse_status_filter(query.polish_status, "polish_status")?;
    let from = parse_timestamp(query.from, "from")?;
    let before = parse_timestamp(query.before, "before")?;
    let rows = list_source_rows(
        &state.pool,
        kind,
        from.as_deref(),
        before.as_deref(),
        attempts,
    )
    .await?;
    let ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    ensure_processing_coverage(&state.pool, kind, &ids).await?;
    let coverage = load_processing_coverage(&state.pool, kind, &ids).await?;
    let task_summaries = load_task_summaries(state.as_ref(), kind, &ids, &coverage).await?;
    let brief_summaries = if kind == CollectionRecordKind::Brief {
        load_brief_summaries(state.as_ref(), &ids, &coverage).await?
    } else {
        HashMap::new()
    };
    let mut items = rows
        .into_iter()
        .map(|row| source_record_item(kind, row, &task_summaries, &brief_summaries))
        .filter(|item| {
            let translation_matches = if kind == CollectionRecordKind::Brief {
                true
            } else {
                item.translation
                    .as_ref()
                    .is_some_and(|summary| status_matches(summary, translation_filter.as_deref()))
            };
            translation_matches && status_matches(&item.polish, polish_filter.as_deref())
        })
        .collect::<Vec<_>>();
    let total = i64::try_from(items.len()).unwrap_or(i64::MAX);
    let offset = usize::try_from(offset).unwrap_or(usize::MAX);
    let page_size = usize::try_from(page_size).unwrap_or(usize::MAX);
    let items = if offset >= items.len() {
        Vec::new()
    } else {
        let end = items.len().min(offset.saturating_add(page_size));
        items.drain(offset..end).collect()
    };
    Ok(Json(AdminCollectionRecordsResponse {
        items,
        page,
        page_size: i64::try_from(page_size).unwrap_or(i64::MAX),
        total,
    }))
}

async fn load_source_record(
    state: &AppState,
    kind: CollectionRecordKind,
    id: &str,
) -> Result<SourceRecordRow, ApiError> {
    let sql = match kind {
        CollectionRecordKind::Release => {
            "SELECT CAST(r.release_id AS TEXT) AS id, COALESCE((SELECT wi.repo_full_name FROM repo_release_work_items wi WHERE wi.repo_id = r.repo_id LIMIT 1), '仓库 #' || CAST(r.repo_id AS TEXT)) AS repository, COALESCE(NULLIF(r.name, ''), r.tag_name) AS title, COALESCE(r.published_at, r.created_at, r.updated_at) AS occurred_at, r.detected_at, NULL AS generated_at FROM repo_releases r WHERE r.release_id = ? LIMIT 1"
        }
        CollectionRecordKind::Announcement => {
            "SELECT lower(e.repo_full_name) || '#' || CAST(e.discussion_number AS TEXT) AS id, MAX(e.repo_full_name) AS repository, COALESCE(MAX(NULLIF(e.title, '')), '公告') AS title, MAX(e.occurred_at) AS occurred_at, MIN(e.detected_at) AS detected_at, NULL AS generated_at FROM social_activity_events e WHERE e.kind = 'announcement' AND lower(e.repo_full_name) || '#' || CAST(e.discussion_number AS TEXT) = ? GROUP BY lower(e.repo_full_name), e.discussion_number LIMIT 1"
        }
        CollectionRecordKind::Brief => {
            "SELECT b.id, NULL AS repository, b.date AS title, NULL AS occurred_at, NULL AS detected_at, b.created_at AS generated_at FROM briefs b WHERE b.id = ? LIMIT 1"
        }
    };
    sqlx::query_as::<_, SourceRecordRow>(sql)
        .bind(id)
        .fetch_optional(&state.pool)
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
    sqlx::query_as::<_, TaskRow>(
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
    let mut query = QueryBuilder::<Sqlite>::new(
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
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT id, status, source, model FROM llm_calls WHERE id IN (",
        );
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
    let mut relation_query = QueryBuilder::<Sqlite>::new(
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
                last_attempt_at: item.last_attempt_at,
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
    attempts.sort_by_key(|attempt| attempt.last_attempt_at.clone());
    Ok(attempts)
}

async fn load_brief_attempts(
    state: &AppState,
    brief_id: &str,
) -> Result<Vec<AdminCollectionAttempt>, ApiError> {
    let calls = sqlx::query_as::<_, BriefCallRow>(
        "SELECT id, status, source, model, attempt_count, started_at, finished_at, updated_at, error_text, failure_class FROM llm_calls WHERE parent_brief_id = ? ORDER BY datetime(created_at) ASC, id ASC",
    )
    .bind(brief_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| {
            let classified = translations::classify_translation_error(call.error_text.as_deref());
            AdminCollectionAttempt {
                id: call.id.clone(),
                pipeline: "polish".to_owned(),
                attempt_no: i64::try_from(index + 1).unwrap_or(i64::MAX),
                trigger: "daily_brief_generation".to_owned(),
                status: call.status.clone(),
                started_at: call.started_at,
                last_attempt_at: call.updated_at,
                finished_at: call.finished_at,
                error_code: classified.as_ref().map(|value| value.code.to_owned()),
                error_summary: classified.map(|value| value.summary.to_owned()),
                failure_class: call.failure_class,
                processing_stage: Some("brief".to_owned()),
                provider_status: Some(
                    if call.status == "succeeded" {
                        "succeeded"
                    } else {
                        "failed"
                    }
                    .to_owned(),
                ),
                output_contract_status: None,
                retry_disposition: Some("not_needed".to_owned()),
                retry_eligible: false,
                next_retry_at: None,
                llm_calls: vec![AdminCollectionLlmLink {
                    id: call.id,
                    status: call.status,
                    source: call.source,
                    model: call.model,
                    stage: None,
                    relation_role: None,
                    evidence_availability: None,
                }],
            }
        })
        .collect())
}

pub async fn admin_get_collection_record_detail(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path((record_kind, record_id)): Path<(String, String)>,
) -> Result<Json<AdminCollectionRecordDetail>, ApiError> {
    let _acting_user_id = api::require_admin_user_id(state.as_ref(), &session).await?;
    let kind = CollectionRecordKind::parse(record_kind.as_str())?;
    let source = load_source_record(state.as_ref(), kind, &record_id).await?;
    let ids = vec![source.id.clone()];
    ensure_processing_coverage(&state.pool, kind, &ids).await?;
    let coverage = load_processing_coverage(&state.pool, kind, &ids).await?;
    let task_summaries = load_task_summaries(state.as_ref(), kind, &ids, &coverage).await?;
    let brief_summaries = if kind == CollectionRecordKind::Brief {
        load_brief_summaries(state.as_ref(), &ids, &coverage).await?
    } else {
        HashMap::new()
    };
    let record = source_record_item(kind, source, &task_summaries, &brief_summaries);
    let attempts = if kind == CollectionRecordKind::Brief {
        load_brief_attempts(state.as_ref(), &record.id).await?
    } else {
        load_task_attempts(
            state.as_ref(),
            &load_record_tasks(state.as_ref(), kind, &record.id).await?,
        )
        .await?
    };
    Ok(Json(AdminCollectionRecordDetail { record, attempts }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_pool() -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite test pool")
    }

    fn list_query(
        attempt_min: Option<i64>,
        attempt_max: Option<i64>,
    ) -> AdminCollectionRecordListQuery {
        AdminCollectionRecordListQuery {
            from: None,
            before: None,
            attempt_min,
            attempt_max,
            translation_status: None,
            polish_status: None,
            page: None,
            page_size: None,
        }
    }

    #[test]
    fn attempt_count_range_defaults_to_zero_and_unbounded() {
        assert_eq!(
            parse_attempt_count_range(&list_query(None, None)).expect("default range"),
            AttemptCountRange { min: 0, max: None }
        );
    }

    #[test]
    fn attempt_count_range_accepts_inclusive_bounds() {
        assert_eq!(
            parse_attempt_count_range(&list_query(Some(2), Some(5))).expect("bounded range"),
            AttemptCountRange {
                min: 2,
                max: Some(5)
            }
        );
        assert_eq!(
            parse_attempt_count_range(&list_query(Some(10), Some(10))).expect("single value"),
            AttemptCountRange {
                min: 10,
                max: Some(10)
            }
        );
    }

    #[test]
    fn attempt_count_range_rejects_invalid_bounds() {
        for query in [
            list_query(Some(-1), None),
            list_query(Some(11), None),
            list_query(None, Some(-1)),
            list_query(None, Some(11)),
            list_query(Some(6), Some(5)),
        ] {
            assert!(parse_attempt_count_range(&query).is_err());
        }
    }

    #[test]
    fn display_status_preserves_not_started_and_historical_unknown_origins() {
        assert_eq!(
            display_status_for("not_recorded", "never_started"),
            "not_started"
        );
        assert_eq!(
            display_status_for("not_recorded", "historical_unknown"),
            "historical_unknown"
        );
        assert_eq!(display_status_for("failed", "task"), "failed");
        assert_eq!(display_status_for("retry_scheduled", "task"), "queued");
    }

    #[test]
    fn status_filter_is_sorted_deduplicated_and_validated() {
        assert_eq!(
            parse_status_filter(Some("failed, running,failed".to_owned()), "polish_status")
                .expect("valid filter"),
            Some(vec!["failed".to_owned(), "running".to_owned()])
        );
        assert!(parse_status_filter(Some("unknown".to_owned()), "translation_status").is_err());
        assert_eq!(
            parse_status_filter(Some(",,".to_owned()), "polish_status").unwrap(),
            None
        );
    }

    #[test]
    fn status_filter_matches_within_pipeline_or_semantics() {
        let summary = not_recorded_summary_for("never_started");
        assert!(status_matches(&summary, None));
        assert!(status_matches(
            &summary,
            Some(&["failed".to_owned(), "not_started".to_owned()])
        ));
        assert!(!status_matches(
            &summary,
            Some(&["historical_unknown".to_owned()])
        ));
    }

    #[tokio::test]
    async fn processing_coverage_marks_new_records_without_overwriting_legacy_origin() {
        let pool = test_pool().await;
        sqlx::query(
            "CREATE TABLE admin_collection_processing_coverage (record_kind TEXT NOT NULL, record_id TEXT NOT NULL, pipeline TEXT NOT NULL, status_origin TEXT NOT NULL, PRIMARY KEY (record_kind, record_id, pipeline))",
        )
        .execute(&pool)
        .await
        .expect("create coverage table");
        sqlx::query(
            "INSERT INTO admin_collection_processing_coverage (record_kind, record_id, pipeline, status_origin) VALUES ('release', 'legacy', 'translation', 'historical_unknown')",
        )
        .execute(&pool)
        .await
        .expect("seed legacy coverage");

        ensure_processing_coverage(
            &pool,
            CollectionRecordKind::Release,
            &["legacy".to_owned(), "new".to_owned()],
        )
        .await
        .expect("ensure coverage");
        let coverage = load_processing_coverage(
            &pool,
            CollectionRecordKind::Release,
            &["legacy".to_owned(), "new".to_owned()],
        )
        .await
        .expect("load coverage");
        assert_eq!(
            coverage_origin(&coverage, "legacy", "translation"),
            "historical_unknown"
        );
        assert_eq!(
            coverage_origin(&coverage, "new", "translation"),
            "never_started"
        );
        assert_eq!(coverage_origin(&coverage, "new", "polish"), "never_started");
    }

    #[tokio::test]
    async fn release_source_time_and_attempt_range_apply_before_pagination() {
        let pool = test_pool().await;
        sqlx::query(
            "CREATE TABLE repo_releases (release_id INTEGER, repo_id INTEGER, name TEXT, tag_name TEXT, published_at TEXT, created_at TEXT, updated_at TEXT, detected_at TEXT)",
        )
        .execute(&pool)
        .await
        .expect("create releases");
        sqlx::query("CREATE TABLE repo_release_work_items (repo_id INTEGER, repo_full_name TEXT)")
            .execute(&pool)
            .await
            .expect("create release work items");
        sqlx::query(
            "CREATE TABLE translation_work_items (entity_id TEXT, kind TEXT, attempt_count INTEGER)",
        )
        .execute(&pool)
        .await
        .expect("create translation work items");
        sqlx::query(
            "INSERT INTO repo_releases (release_id, repo_id, name, tag_name, published_at, created_at, updated_at, detected_at) VALUES (100, 1, 'legacy', 'v1', '2026-07-08T09:00:00Z', '2026-07-08T09:00:00Z', '2026-07-08T09:00:00Z', NULL), (101, 1, 'processed', 'v2', '2026-07-08T08:00:00Z', '2026-07-08T08:00:00Z', '2026-07-08T08:00:00Z', NULL)",
        )
        .execute(&pool)
        .await
        .expect("seed releases");
        sqlx::query(
            "INSERT INTO translation_work_items (entity_id, kind, attempt_count) VALUES ('101', 'release_detail', 2), ('101', 'release_smart', 1)",
        )
        .execute(&pool)
        .await
        .expect("seed attempts");

        let rows = list_source_rows(
            &pool,
            CollectionRecordKind::Release,
            Some("2026-07-08T07:00:00Z"),
            Some("2026-07-08T10:00:00Z"),
            AttemptCountRange { min: 0, max: None },
        )
        .await
        .expect("list releases");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "100");
        assert_eq!(rows[0].detected_at, None);

        let rows = list_source_rows(
            &pool,
            CollectionRecordKind::Release,
            Some("2026-07-08T07:00:00Z"),
            Some("2026-07-08T10:00:00Z"),
            AttemptCountRange { min: 2, max: None },
        )
        .await
        .expect("filter releases by attempts");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "101");
    }

    #[tokio::test]
    async fn announcement_and_brief_use_source_time_for_window() {
        let pool = test_pool().await;
        sqlx::query(
            "CREATE TABLE social_activity_events (repo_full_name TEXT, discussion_number INTEGER, title TEXT, occurred_at TEXT, detected_at TEXT, kind TEXT)",
        )
        .execute(&pool)
            .await
            .expect("create social events");
        sqlx::query(
            "CREATE TABLE translation_work_items (entity_id TEXT, kind TEXT, attempt_count INTEGER)",
        )
        .execute(&pool)
        .await
        .expect("create announcement work items");
        sqlx::query(
            "INSERT INTO social_activity_events (repo_full_name, discussion_number, title, occurred_at, detected_at, kind) VALUES ('octo/demo', 42, '公告', '2026-07-08T09:00:00Z', '2026-07-08T01:00:00Z', 'announcement')",
        )
        .execute(&pool)
        .await
        .expect("seed announcement");
        let announcement_rows = list_source_rows(
            &pool,
            CollectionRecordKind::Announcement,
            Some("2026-07-08T08:00:00Z"),
            Some("2026-07-08T10:00:00Z"),
            AttemptCountRange { min: 0, max: None },
        )
        .await
        .expect("list announcements");
        assert_eq!(announcement_rows.len(), 1);

        sqlx::query("CREATE TABLE briefs (id TEXT, date TEXT, created_at TEXT)")
            .execute(&pool)
            .await
            .expect("create briefs");
        sqlx::query("CREATE TABLE llm_calls (parent_brief_id TEXT, attempt_count INTEGER)")
            .execute(&pool)
            .await
            .expect("create brief calls");
        sqlx::query(
            "INSERT INTO briefs (id, date, created_at) VALUES ('brief-1', '2026-07-08', '2026-07-08T09:30:00Z')",
        )
        .execute(&pool)
        .await
        .expect("seed brief");
        let brief_rows = list_source_rows(
            &pool,
            CollectionRecordKind::Brief,
            Some("2026-07-08T09:00:00Z"),
            Some("2026-07-08T10:00:00Z"),
            AttemptCountRange { min: 0, max: None },
        )
        .await
        .expect("list briefs");
        assert_eq!(brief_rows.len(), 1);
    }

    fn event(trigger: &str, event_type: &str, created_at: &str) -> AttemptEventRow {
        AttemptEventRow {
            work_item_id: "work-item-1".to_owned(),
            attempt_no: 2,
            trigger: trigger.to_owned(),
            event_type: event_type.to_owned(),
            result_status: None,
            error_code: None,
            error_summary: None,
            failure_class: None,
            processing_stage: None,
            provider_status: None,
            output_contract_status: None,
            retry_disposition: None,
            retry_eligible: 0,
            next_retry_at: None,
            llm_call_ids_json: "[]".to_owned(),
            created_at: created_at.to_owned(),
        }
    }

    #[test]
    fn queued_event_replaces_stale_scheduled_retry_state() {
        let mut attempt = AttemptBuild {
            pipeline: "translation".to_owned(),
            attempt_no: 2,
            ..AttemptBuild::default()
        };
        let mut scheduled = event(
            "automatic_recovery",
            "retry_scheduled",
            "2026-09-04T01:00:00Z",
        );
        scheduled.error_summary = Some("temporary upstream failure".to_owned());
        scheduled.retry_eligible = 1;
        scheduled.next_retry_at = Some("2026-09-04T01:05:00Z".to_owned());
        apply_attempt_event(&mut attempt, &scheduled);

        let queued = event(
            "automatic_recovery",
            "attempt_queued",
            "2026-09-04T01:05:00Z",
        );
        apply_attempt_event(&mut attempt, &queued);

        assert_eq!(attempt.status, "queued");
        assert_eq!(attempt.trigger, "automatic_recovery");
        assert_eq!(
            attempt.error_summary.as_deref(),
            Some("temporary upstream failure")
        );
        assert!(!attempt.retry_eligible);
        assert_eq!(attempt.next_retry_at, None);
        assert_eq!(attempt.started_at, None);
        assert_eq!(attempt.finished_at, None);
    }

    #[test]
    fn latest_queued_event_preserves_system_requeue_trigger() {
        let mut attempt = AttemptBuild {
            pipeline: "translation".to_owned(),
            attempt_no: 1,
            ..AttemptBuild::default()
        };
        apply_attempt_event(
            &mut attempt,
            &event("initial", "attempt_queued", "2026-09-04T01:00:00Z"),
        );
        apply_attempt_event(
            &mut attempt,
            &event("system_requeue", "attempt_queued", "2026-09-04T01:01:00Z"),
        );

        assert_eq!(attempt.status, "queued");
        assert_eq!(attempt.trigger, "system_requeue");
    }
}
