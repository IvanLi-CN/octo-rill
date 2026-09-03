use std::{collections::HashMap, sync::Arc};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite};
use tower_sessions::Session;

use crate::{api, error::ApiError, state::AppState};

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
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AdminCollectionTaskSummary {
    pub status: String,
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
    retry_eligible: i64,
    next_retry_at: Option<String>,
    llm_call_ids_json: String,
    created_at: String,
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
    let tasks = task_summaries.get(&row.id).cloned().unwrap_or_default();
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

#[derive(Clone, Default)]
struct TaskSummaries {
    translation: AdminCollectionTaskSummary,
    polish: AdminCollectionTaskSummary,
}

fn not_recorded_summary() -> AdminCollectionTaskSummary {
    AdminCollectionTaskSummary {
        status: "not_recorded".to_owned(),
        ..AdminCollectionTaskSummary::default()
    }
}

fn merge_summary(rows: &[TaskRow]) -> AdminCollectionTaskSummary {
    let Some(latest) = rows.iter().max_by_key(|row| (&row.updated_at, &row.id)) else {
        return not_recorded_summary();
    };
    AdminCollectionTaskSummary {
        status: latest
            .result_status
            .clone()
            .unwrap_or_else(|| latest.status.clone()),
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
    Ok(grouped
        .into_iter()
        .map(|(id, (translation, polish))| {
            (
                id,
                TaskSummaries {
                    translation: merge_summary(&translation),
                    polish: merge_summary(&polish),
                },
            )
        })
        .collect())
}

async fn load_brief_summaries(
    state: &AppState,
    brief_ids: &[String],
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
    Ok(grouped
        .into_iter()
        .map(|(id, calls)| {
            let latest = calls.iter().max_by_key(|call| (&call.updated_at, &call.id));
            let summary =
                latest.map_or_else(not_recorded_summary, |call| AdminCollectionTaskSummary {
                    status: call.status.clone(),
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
                });
            (id, summary)
        })
        .collect())
}

async fn list_source_rows(
    state: &AppState,
    kind: CollectionRecordKind,
    from: Option<&str>,
    before: Option<&str>,
    page_size: i64,
    offset: i64,
) -> Result<(Vec<SourceRecordRow>, i64), ApiError> {
    let (base, order_by) = match kind {
        CollectionRecordKind::Release => (
            "SELECT CAST(r.release_id AS TEXT) AS id, COALESCE((SELECT wi.repo_full_name FROM repo_release_work_items wi WHERE wi.repo_id = r.repo_id LIMIT 1), '仓库 #' || CAST(r.repo_id AS TEXT)) AS repository, COALESCE(NULLIF(r.name, ''), r.tag_name) AS title, COALESCE(r.published_at, r.created_at, r.updated_at) AS occurred_at, r.detected_at, NULL AS generated_at FROM repo_releases r WHERE (? IS NULL OR datetime(r.detected_at) >= datetime(?)) AND (? IS NULL OR datetime(r.detected_at) < datetime(?))",
            " ORDER BY datetime(detected_at) DESC, CAST(id AS INTEGER) DESC",
        ),
        CollectionRecordKind::Announcement => (
            "SELECT lower(e.repo_full_name) || '#' || CAST(e.discussion_number AS TEXT) AS id, MAX(e.repo_full_name) AS repository, COALESCE(MAX(NULLIF(e.title, '')), '公告') AS title, MAX(e.occurred_at) AS occurred_at, MIN(e.detected_at) AS detected_at, NULL AS generated_at FROM social_activity_events e WHERE e.kind = 'announcement' AND e.repo_full_name IS NOT NULL AND e.discussion_number IS NOT NULL AND (? IS NULL OR datetime(e.detected_at) >= datetime(?)) AND (? IS NULL OR datetime(e.detected_at) < datetime(?)) GROUP BY lower(e.repo_full_name), e.discussion_number",
            " ORDER BY datetime(detected_at) DESC, id DESC",
        ),
        CollectionRecordKind::Brief => (
            "SELECT b.id, NULL AS repository, b.date AS title, NULL AS occurred_at, NULL AS detected_at, b.created_at AS generated_at FROM briefs b WHERE (? IS NULL OR datetime(b.created_at) >= datetime(?)) AND (? IS NULL OR datetime(b.created_at) < datetime(?))",
            " ORDER BY datetime(generated_at) DESC, id DESC",
        ),
    };
    let total_sql = format!("SELECT COUNT(*) FROM ({base}) source_records");
    let total = sqlx::query_scalar::<_, i64>(&total_sql)
        .bind(from)
        .bind(from)
        .bind(before)
        .bind(before)
        .fetch_one(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    let sql = format!("{base}{order_by} LIMIT ? OFFSET ?");
    let rows = sqlx::query_as::<_, SourceRecordRow>(&sql)
        .bind(from)
        .bind(from)
        .bind(before)
        .bind(before)
        .bind(page_size)
        .bind(offset)
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::internal)?;
    Ok((rows, total))
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
    let from = parse_timestamp(query.from, "from")?;
    let before = parse_timestamp(query.before, "before")?;
    let (rows, total) = list_source_rows(
        state.as_ref(),
        kind,
        from.as_deref(),
        before.as_deref(),
        page_size,
        offset,
    )
    .await?;
    let ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    let task_summaries = load_task_summaries(state.as_ref(), kind, &ids).await?;
    let brief_summaries = if kind == CollectionRecordKind::Brief {
        load_brief_summaries(state.as_ref(), &ids).await?
    } else {
        HashMap::new()
    };
    Ok(Json(AdminCollectionRecordsResponse {
        items: rows
            .into_iter()
            .map(|row| source_record_item(kind, row, &task_summaries, &brief_summaries))
            .collect(),
        page,
        page_size,
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
        "SELECT work_item_id, attempt_no, trigger, event_type, result_status, error_code, error_summary, failure_class, retry_eligible, next_retry_at, llm_call_ids_json, created_at FROM translation_attempt_events WHERE work_item_id IN (",
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
        retry_eligible: bool,
        next_retry_at: Option<String>,
        llm_calls: Vec<AdminCollectionLlmLink>,
    }
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
        item.last_attempt_at = event.created_at.clone();
        if event.event_type == "attempt_started" {
            item.status = "running".to_owned();
            item.started_at = Some(event.created_at.clone());
        } else if event.event_type == "attempt_completed" {
            item.status = event
                .result_status
                .clone()
                .unwrap_or_else(|| "completed".to_owned());
            item.finished_at = Some(event.created_at.clone());
        } else if event.event_type == "retry_scheduled" {
            item.status = "retry_scheduled".to_owned();
            item.finished_at = Some(event.created_at.clone());
        }
        item.error_code = event.error_code.or(item.error_code.take());
        item.error_summary = event.error_summary.or(item.error_summary.take());
        item.failure_class = event.failure_class.or(item.failure_class.take());
        item.retry_eligible |= event.retry_eligible != 0;
        item.next_retry_at = event.next_retry_at.or(item.next_retry_at.take());
        for id in serde_json::from_str::<Vec<String>>(&event.llm_call_ids_json).unwrap_or_default()
        {
            if let Some(call) = calls_by_id.get(&id)
                && !item.llm_calls.iter().any(|existing| existing.id == call.id)
            {
                item.llm_calls.push(call.clone());
            }
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
        .map(|(index, call)| AdminCollectionAttempt {
            id: call.id.clone(),
            pipeline: "polish".to_owned(),
            attempt_no: i64::try_from(index + 1).unwrap_or(i64::MAX),
            trigger: "daily_brief_generation".to_owned(),
            status: call.status.clone(),
            started_at: call.started_at,
            last_attempt_at: call.updated_at,
            finished_at: call.finished_at,
            error_code: None,
            error_summary: call.error_text,
            failure_class: call.failure_class,
            retry_eligible: false,
            next_retry_at: None,
            llm_calls: vec![AdminCollectionLlmLink {
                id: call.id,
                status: call.status,
                source: call.source,
                model: call.model,
            }],
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
    let task_summaries = load_task_summaries(state.as_ref(), kind, &ids).await?;
    let brief_summaries = if kind == CollectionRecordKind::Brief {
        load_brief_summaries(state.as_ref(), &ids).await?
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
