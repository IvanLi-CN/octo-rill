use std::{
    ffi::CString,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use sqlx::{QueryBuilder, Row, Sqlite, Transaction};
use tokio::task::AbortHandle;
use tracing::{info, warn};

use crate::{sqlite_write::SqliteWritePriority, state::AppState};

const INDEX_BATCH_SIZE: i64 = 100;
const DEFAULT_MIN_FREE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const RETRY_DELAY: Duration = Duration::from_secs(5);

const PHASES: &[&str] = &[
    "releases",
    "announcements",
    "notifications",
    "briefs",
    "repo_associations",
    "starred_repos",
    "content_projections",
    "translations",
];

#[cfg(test)]
pub(crate) const PHASE_COUNT: usize = PHASES.len();

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BatchOutcome {
    Progress,
    Idle,
    Paused,
    Ready,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct BackfillState {
    phase: String,
    cursor: i64,
    status: String,
}

pub fn spawn_worker(state: Arc<AppState>) -> AbortHandle {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        loop {
            match run_batch(state.as_ref()).await {
                Ok(BatchOutcome::Ready) => break,
                Ok(BatchOutcome::Progress) => tokio::task::yield_now().await,
                Ok(BatchOutcome::Idle | BatchOutcome::Paused) => {
                    tokio::time::sleep(RETRY_DELAY).await;
                }
                Err(error) => {
                    warn!(?error, "search projection backfill batch failed");
                    let _ = mark_failed(state.as_ref(), &error.to_string()).await;
                    tokio::time::sleep(RETRY_DELAY).await;
                }
            }
        }
        info!("search projection backfill completed");
    })
    .abort_handle()
}

async fn run_batch(state: &AppState) -> Result<BatchOutcome> {
    let directory = database_directory(&state.config.database_url);
    let minimum_free_bytes = min_free_bytes();
    let free_bytes = available_bytes(&directory)
        .with_context(|| format!("check free space for {}", directory.display()))?;
    run_batch_with_free_bytes(state, free_bytes, minimum_free_bytes).await
}

#[cfg(test)]
pub(crate) async fn run_batch_for_test(state: &AppState, free_bytes: u64) -> Result<()> {
    run_batch_with_free_bytes(state, free_bytes, 1)
        .await
        .map(|_| ())
}

async fn run_batch_with_free_bytes(
    state: &AppState,
    free_bytes: u64,
    minimum_free_bytes: u64,
) -> Result<BatchOutcome> {
    let current_status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM search_projection_backfill_state WHERE id = 1",
    )
    .fetch_optional(&state.pool)
    .await
    .context("read search projection backfill status")?;
    if current_status.as_deref() == Some("ready") {
        return Ok(BatchOutcome::Ready);
    }
    if free_bytes < minimum_free_bytes {
        mark_status(state, "paused_low_disk", None).await?;
        return Ok(BatchOutcome::Paused);
    }

    let (permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "search_projection_backfill",
            SqliteWritePriority::Background,
        )
        .await?;
    let state_row = load_state(&mut tx).await?;
    if state_row.status == "ready" {
        tx.rollback().await.ok();
        drop(permit);
        return Ok(BatchOutcome::Ready);
    }
    let phase = phase_index(&state_row.phase)?;
    let rowids = load_rowids(&mut tx, state_row.phase.as_str(), state_row.cursor).await?;
    if rowids.is_empty() {
        if phase + 1 >= PHASES.len() {
            update_state(&mut tx, "translations", 0, "ready", None).await?;
            tx.commit().await?;
            drop(permit);
            return Ok(BatchOutcome::Ready);
        }
        update_state(&mut tx, PHASES[phase + 1], 0, "building", None).await?;
        tx.commit().await?;
        drop(permit);
        return Ok(BatchOutcome::Idle);
    }

    update_state(
        &mut tx,
        state_row.phase.as_str(),
        state_row.cursor,
        "building",
        None,
    )
    .await?;
    process_phase(&mut tx, state_row.phase.as_str(), &rowids).await?;
    let next_cursor = *rowids.last().expect("non-empty batch");
    update_state(
        &mut tx,
        state_row.phase.as_str(),
        next_cursor,
        "building",
        None,
    )
    .await?;
    tx.commit().await?;
    drop(permit);
    Ok(BatchOutcome::Progress)
}

async fn mark_status(state: &AppState, status: &str, error: Option<&str>) -> Result<()> {
    let (permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "search_projection_backfill_status",
            SqliteWritePriority::Background,
        )
        .await?;
    let current = load_state(&mut tx).await?;
    update_state(&mut tx, &current.phase, current.cursor, status, error).await?;
    tx.commit().await?;
    drop(permit);
    Ok(())
}

async fn mark_failed(state: &AppState, error: &str) -> Result<()> {
    let (permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "search_projection_backfill_failure",
            SqliteWritePriority::Background,
        )
        .await?;
    let current = load_state(&mut tx).await?;
    update_state(
        &mut tx,
        &current.phase,
        current.cursor,
        "failed",
        Some(error),
    )
    .await?;
    tx.commit().await?;
    drop(permit);
    Ok(())
}

pub(crate) async fn refresh_content_projection_phase(state: &AppState) -> Result<bool> {
    let (_permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "search_content_projection_refresh",
            SqliteWritePriority::Background,
        )
        .await?;
    let current = load_state(&mut tx).await?;
    let current_phase = phase_index(&current.phase)?;
    let content_phase = phase_index("content_projections")?;
    if current_phase < content_phase {
        tx.rollback().await?;
        return Ok(true);
    }
    if current.status == "paused_low_disk" {
        tx.rollback().await?;
        return Ok(false);
    }
    update_state(&mut tx, "content_projections", 0, "building", None).await?;
    tx.commit().await?;
    Ok(true)
}

async fn load_state(tx: &mut Transaction<'_, Sqlite>) -> Result<BackfillState> {
    sqlx::query_as::<_, BackfillState>(
        "SELECT phase, cursor, status FROM search_projection_backfill_state WHERE id = 1",
    )
    .fetch_one(&mut **tx)
    .await
    .context("load search projection backfill state")
}

async fn update_state(
    tx: &mut Transaction<'_, Sqlite>,
    phase: &str,
    cursor: i64,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE search_projection_backfill_state SET phase = ?, cursor = ?, status = ?, last_error = ?, updated_at = CURRENT_TIMESTAMP WHERE id = 1",
    )
    .bind(phase)
    .bind(cursor)
    .bind(status)
    .bind(error)
    .execute(&mut **tx)
    .await
    .context("update search projection backfill state")?;
    Ok(())
}

fn phase_index(phase: &str) -> Result<usize> {
    PHASES
        .iter()
        .position(|candidate| *candidate == phase)
        .with_context(|| format!("unknown search projection phase {phase}"))
}

async fn load_rowids(
    tx: &mut Transaction<'_, Sqlite>,
    phase: &str,
    cursor: i64,
) -> Result<Vec<i64>> {
    let query = match phase {
        "releases" => "SELECT rowid FROM repo_releases WHERE rowid > ? ORDER BY rowid LIMIT ?",
        "announcements" => {
            "SELECT rowid FROM social_activity_events WHERE kind = 'announcement' AND rowid > ? ORDER BY rowid LIMIT ?"
        }
        "notifications" => "SELECT rowid FROM notifications WHERE rowid > ? ORDER BY rowid LIMIT ?",
        "briefs" => "SELECT rowid FROM briefs WHERE rowid > ? ORDER BY rowid LIMIT ?",
        "repo_associations" => {
            "SELECT rowid FROM user_repo_associations WHERE rowid > ? ORDER BY rowid LIMIT ?"
        }
        "starred_repos" => "SELECT rowid FROM starred_repos WHERE rowid > ? ORDER BY rowid LIMIT ?",
        "content_projections" => {
            "SELECT rowid FROM content_result_projections WHERE pipeline IN ('translation', 'polishing') AND target_lang = 'zh-CN' AND rowid > ? ORDER BY rowid LIMIT ?"
        }
        "translations" => {
            "SELECT rowid FROM ai_translations WHERE lang = 'zh-CN' AND status IN ('ready', 'disabled', 'missing') AND (title IS NOT NULL OR summary IS NOT NULL) AND rowid > ? ORDER BY rowid LIMIT ?"
        }
        _ => bail!("unknown search projection phase {phase}"),
    };
    let rows = sqlx::query(query)
        .bind(cursor)
        .bind(INDEX_BATCH_SIZE)
        .fetch_all(&mut **tx)
        .await
        .with_context(|| format!("load rowids for search phase {phase}"))?;
    Ok(rows.into_iter().map(|row| row.get("rowid")).collect())
}

async fn process_phase(
    tx: &mut Transaction<'_, Sqlite>,
    phase: &str,
    rowids: &[i64],
) -> Result<()> {
    match phase {
        "releases" => {
            let mut query = QueryBuilder::<Sqlite>::new(
                "INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_id,repo_full_name,owner_login,title,body,source_time,target_path,target_url,created_at,updated_at) SELECT 'release:'||r.release_id,NULL,'release',CAST(r.release_id AS TEXT),r.repo_id,COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id=r.repo_id LIMIT 1),(SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id=r.repo_id LIMIT 1),(SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id=r.repo_id LIMIT 1),(SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id=r.repo_id LIMIT 1)),COALESCE((SELECT CASE WHEN instr(rwi.repo_full_name,'/')>0 THEN substr(rwi.repo_full_name,1,instr(rwi.repo_full_name,'/')-1) ELSE rwi.repo_full_name END FROM repo_release_work_items rwi WHERE rwi.repo_id=r.repo_id LIMIT 1),(SELECT vr.owner_login FROM user_release_visible_repos vr WHERE vr.repo_id=r.repo_id LIMIT 1),(SELECT ura.owner_login FROM user_repo_associations ura WHERE ura.repo_id=r.repo_id LIMIT 1),(SELECT sr.owner_login FROM starred_repos sr WHERE sr.repo_id=r.repo_id LIMIT 1)),COALESCE(r.name,r.tag_name),r.body,COALESCE(r.published_at,r.created_at,r.updated_at),CASE WHEN COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id=r.repo_id LIMIT 1),(SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id=r.repo_id LIMIT 1),(SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id=r.repo_id LIMIT 1),(SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id=r.repo_id LIMIT 1)) IS NOT NULL THEN '/'||COALESCE((SELECT rwi.repo_full_name FROM repo_release_work_items rwi WHERE rwi.repo_id=r.repo_id LIMIT 1),(SELECT vr.full_name FROM user_release_visible_repos vr WHERE vr.repo_id=r.repo_id LIMIT 1),(SELECT ura.repo_full_name FROM user_repo_associations ura WHERE ura.repo_id=r.repo_id LIMIT 1),(SELECT sr.full_name FROM starred_repos sr WHERE sr.repo_id=r.repo_id LIMIT 1))||'/releases/tag/'||r.tag_name END,r.html_url,r.updated_at,r.updated_at FROM repo_releases r WHERE r.rowid IN (",
            );
            push_rowids(&mut query, rowids);
            query.push(") ON CONFLICT(id) DO UPDATE SET repo_id=excluded.repo_id,repo_full_name=excluded.repo_full_name,owner_login=excluded.owner_login,title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_path=excluded.target_path,target_url=excluded.target_url,updated_at=excluded.updated_at");
            query.build().execute(&mut **tx).await?;
        }
        "announcements" => {
            let mut query = QueryBuilder::<Sqlite>::new(
                "INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_id,repo_full_name,owner_login,title,body,source_time,unread,target_path,target_url,created_at,updated_at) SELECT 'announcement:'||e.id,e.user_id,'announcement',CASE WHEN e.repo_full_name IS NOT NULL AND e.discussion_number IS NOT NULL THEN lower(e.repo_full_name)||'#'||e.discussion_number ELSE e.id END,e.repo_id,e.repo_full_name,CASE WHEN instr(COALESCE(e.repo_full_name,''),'/')>0 THEN substr(e.repo_full_name,1,instr(e.repo_full_name,'/')-1) ELSE e.repo_full_name END,e.title,e.body,e.occurred_at,NULL,CASE WHEN e.repo_full_name IS NOT NULL AND e.discussion_number IS NOT NULL THEN '/'||e.repo_full_name||'/discussions/'||e.discussion_number END,e.html_url,e.created_at,e.updated_at FROM social_activity_events e WHERE e.kind='announcement' AND e.rowid IN (",
            );
            push_rowids(&mut query, rowids);
            query.push(") ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET repo_id=excluded.repo_id,repo_full_name=excluded.repo_full_name,owner_login=excluded.owner_login,title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_url=excluded.target_url,updated_at=excluded.updated_at");
            query.build().execute(&mut **tx).await?;
        }
        "notifications" => {
            let mut query = QueryBuilder::<Sqlite>::new(
                "INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_full_name,owner_login,title,body,source_time,unread,target_url,created_at,updated_at) SELECT 'notification:'||n.user_id||':'||n.thread_id,n.user_id,'notification',n.thread_id,n.repo_full_name,CASE WHEN instr(COALESCE(n.repo_full_name,''),'/')>0 THEN substr(n.repo_full_name,1,instr(n.repo_full_name,'/')-1) ELSE n.repo_full_name END,n.subject_title,COALESCE(n.subject_type,'')||' '||COALESCE(n.reason,''),n.updated_at,n.unread,n.html_url,COALESCE(n.updated_at,CURRENT_TIMESTAMP),COALESCE(n.updated_at,CURRENT_TIMESTAMP) FROM notifications n WHERE n.rowid IN (",
            );
            push_rowids(&mut query, rowids);
            query.push(") ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET repo_full_name=excluded.repo_full_name,owner_login=excluded.owner_login,title=excluded.title,body=excluded.body,source_time=excluded.source_time,unread=excluded.unread,target_url=excluded.target_url,updated_at=excluded.updated_at");
            query.build().execute(&mut **tx).await?;
        }
        "briefs" => {
            let mut query = QueryBuilder::<Sqlite>::new(
                "INSERT INTO search_documents (id,user_id,resource_type,resource_id,title,body,source_time,target_path,created_at,updated_at) SELECT 'brief:'||b.id,b.user_id,'brief',b.id,'日报 '||b.date,b.content_markdown,COALESCE(b.window_end_utc,b.created_at),'/briefs?brief='||b.id,b.created_at,b.updated_at FROM briefs b WHERE b.rowid IN (",
            );
            push_rowids(&mut query, rowids);
            query.push(") ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_path=excluded.target_path,updated_at=excluded.updated_at");
            query.build().execute(&mut **tx).await?;
        }
        "repo_associations" => {
            let mut query = QueryBuilder::<Sqlite>::new(
                "INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_id,repo_full_name,owner_login,title,body,source_time,target_path,target_url,created_at,updated_at) SELECT 'repository:'||u.user_id||':'||u.repo_full_name_lower,u.user_id,'repository',u.repo_full_name_lower,u.repo_id,u.repo_full_name,u.owner_login,u.repo_name,u.description,u.updated_at,'/focus/repo/'||u.owner_login||'/'||u.repo_name,u.html_url,u.created_at,u.updated_at FROM user_repo_associations u WHERE u.rowid IN (",
            );
            push_rowids(&mut query, rowids);
            query.push(") ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET repo_id=excluded.repo_id,repo_full_name=excluded.repo_full_name,owner_login=excluded.owner_login,title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_path=excluded.target_path,target_url=excluded.target_url,updated_at=excluded.updated_at");
            query.build().execute(&mut **tx).await?;
        }
        "starred_repos" => {
            let mut query = QueryBuilder::<Sqlite>::new(
                "INSERT INTO search_documents (id,user_id,resource_type,resource_id,repo_id,repo_full_name,owner_login,title,body,source_time,target_path,target_url,created_at,updated_at) SELECT 'repository:'||s.user_id||':'||lower(s.full_name),s.user_id,'repository',lower(s.full_name),s.repo_id,s.full_name,s.owner_login,s.name,s.description,s.updated_at,'/focus/repo/'||s.owner_login||'/'||s.name,s.html_url,s.updated_at,s.updated_at FROM starred_repos s WHERE s.rowid IN (",
            );
            push_rowids(&mut query, rowids);
            query.push(") AND NOT EXISTS (SELECT 1 FROM user_repo_associations u WHERE u.user_id=s.user_id AND u.repo_full_name_lower=lower(s.full_name)) ON CONFLICT(user_id,resource_type,resource_id) DO UPDATE SET repo_id=excluded.repo_id,repo_full_name=excluded.repo_full_name,owner_login=excluded.owner_login,title=excluded.title,body=excluded.body,source_time=excluded.source_time,target_path=excluded.target_path,target_url=excluded.target_url,updated_at=excluded.updated_at");
            query.build().execute(&mut **tx).await?;
        }
        "content_projections" => apply_content_projection_batch(tx, rowids).await?,
        "translations" => apply_translation_batch(tx, rowids).await?,
        _ => bail!("unknown search projection phase {phase}"),
    }

    refresh_phase_fts(tx, phase, rowids).await?;
    Ok(())
}

fn push_rowids(query: &mut QueryBuilder<'_, Sqlite>, rowids: &[i64]) {
    for (index, rowid) in rowids.iter().enumerate() {
        if index > 0 {
            query.push(",");
        }
        query.push_bind(*rowid);
    }
}

async fn apply_content_projection_batch(
    tx: &mut Transaction<'_, Sqlite>,
    rowids: &[i64],
) -> Result<()> {
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT canonical_resource_type, canonical_resource_id, pipeline FROM content_result_projections WHERE rowid IN (",
    );
    push_rowids(&mut query, rowids);
    query.push(")");
    let rows = query.build().fetch_all(&mut **tx).await?;
    let identity_upgrade_complete =
        crate::content_identity_upgrade::is_complete_in_transaction(tx).await?;
    for row in rows {
        let resource_type: String = row.get("canonical_resource_type");
        let resource_id: String = row.get("canonical_resource_id");
        let pipeline: String = row.get("pipeline");
        let projection_text_query = if identity_upgrade_complete {
            "SELECT trim(COALESCE(json_extract(p.payload_json, '$.title'), json_extract(p.payload_json, '$.title_zh'), '') || ' ' || COALESCE(json_extract(p.payload_json, '$.summary'), json_extract(p.payload_json, '$.body_md'), '')) FROM content_current_result_projections p JOIN content_work_identities i ON i.id = p.identity_id LEFT JOIN content_work_items active ON active.id = COALESCE(p.active_work_item_id, p.work_item_id) WHERE i.canonical_resource_type = ? AND i.canonical_resource_id = ? AND i.pipeline = ? AND i.target_lang = 'zh-CN' ORDER BY CASE WHEN active.status = 'ready' THEN 0 ELSE 1 END, julianday(p.published_at) DESC, p.published_at DESC, i.source_hash DESC LIMIT 1"
        } else {
            "SELECT trim(COALESCE(json_extract(p.payload_json, '$.title'), json_extract(p.payload_json, '$.title_zh'), '') || ' ' || COALESCE(json_extract(p.payload_json, '$.summary'), json_extract(p.payload_json, '$.body_md'), '')) FROM content_result_projections p WHERE p.canonical_resource_type = ? AND p.canonical_resource_id = ? AND p.pipeline = ? AND p.target_lang = 'zh-CN' ORDER BY CASE WHEN EXISTS (SELECT 1 FROM content_work_items w WHERE w.id = p.active_work_item_id AND w.status = 'ready') THEN 0 ELSE 1 END, p.updated_at DESC, p.id DESC LIMIT 1"
        };
        let text = sqlx::query_scalar::<_, Option<String>>(projection_text_query)
            .bind(&resource_type)
            .bind(&resource_id)
            .bind(&pipeline)
            .fetch_optional(&mut **tx)
            .await?;
        let column = if pipeline == "polishing" {
            "smart_text"
        } else {
            "translated_text"
        };
        let sql = format!(
            "UPDATE search_documents SET {column} = ?, updated_at = MAX(updated_at, CURRENT_TIMESTAMP) WHERE resource_type = ? AND resource_id = ?"
        );
        sqlx::query(&sql)
            .bind(text.flatten())
            .bind(resource_type)
            .bind(resource_id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

async fn apply_translation_batch(tx: &mut Transaction<'_, Sqlite>, rowids: &[i64]) -> Result<()> {
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT rowid, user_id, entity_type, entity_id, title, summary, updated_at FROM ai_translations WHERE rowid IN (",
    );
    push_rowids(&mut query, rowids);
    query.push(")");
    let rows = query.build().fetch_all(&mut **tx).await?;
    for row in rows {
        let user_id: String = row.get("user_id");
        let entity_type: String = row.get("entity_type");
        let entity_id: String = row.get("entity_id");
        let title: Option<String> = row.get("title");
        let summary: Option<String> = row.get("summary");
        let updated_at: String = row.get("updated_at");
        let is_smart = entity_type.to_ascii_lowercase().contains("smart");
        let resource_type = if entity_type.to_ascii_lowercase().starts_with("release") {
            "release"
        } else if entity_type.to_ascii_lowercase().starts_with("announcement") {
            "announcement"
        } else if entity_type.to_ascii_lowercase().starts_with("notification") {
            "notification"
        } else {
            continue;
        };
        let resource_id = if resource_type == "announcement" {
            entity_id.to_ascii_lowercase()
        } else {
            entity_id.clone()
        };
        let document_id = sqlx::query_scalar::<_, Option<String>>(
            "SELECT d.id FROM search_documents d WHERE d.resource_type = ? AND (d.resource_id = ? OR (d.resource_type = 'announcement' AND (lower(d.resource_id) = lower(?) OR d.id = 'announcement:' || ?))) AND (d.user_id = ? OR d.user_id IS NULL) ORDER BY CASE WHEN d.user_id = ? THEN 0 ELSE 1 END, d.id LIMIT 1",
        )
        .bind(resource_type)
        .bind(&resource_id)
        .bind(&entity_id)
        .bind(&entity_id)
        .bind(&user_id)
        .bind(&user_id)
        .fetch_one(&mut **tx)
        .await?;
        let Some(document_id) = document_id else {
            continue;
        };
        let text = title
            .into_iter()
            .chain(summary)
            .collect::<Vec<_>>()
            .join(" ");
        let (translated, smart) = if is_smart {
            (None, Some(text))
        } else {
            (Some(text), None)
        };
        sqlx::query(
            "INSERT INTO search_document_user_lanes (document_id,user_id,translated_text,smart_text,updated_at) VALUES (?,?,?,?,?) ON CONFLICT(document_id,user_id) DO UPDATE SET translated_text=COALESCE(excluded.translated_text,search_document_user_lanes.translated_text), smart_text=COALESCE(excluded.smart_text,search_document_user_lanes.smart_text), updated_at=MAX(search_document_user_lanes.updated_at,excluded.updated_at)",
        )
        .bind(document_id)
        .bind(user_id)
        .bind(translated)
        .bind(smart)
        .bind(updated_at)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn refresh_phase_fts(
    tx: &mut Transaction<'_, Sqlite>,
    phase: &str,
    rowids: &[i64],
) -> Result<()> {
    let (query, lane) = match phase {
        "releases" => (
            "SELECT 'release:'||release_id AS id FROM repo_releases WHERE rowid IN (",
            false,
        ),
        "announcements" => (
            "SELECT 'announcement:'||id AS id FROM social_activity_events WHERE kind='announcement' AND rowid IN (",
            false,
        ),
        "notifications" => (
            "SELECT 'notification:'||user_id||':'||thread_id AS id FROM notifications WHERE rowid IN (",
            false,
        ),
        "briefs" => (
            "SELECT 'brief:'||id AS id FROM briefs WHERE rowid IN (",
            false,
        ),
        "repo_associations" => (
            "SELECT 'repository:'||user_id||':'||repo_full_name_lower AS id FROM user_repo_associations WHERE rowid IN (",
            false,
        ),
        "starred_repos" => (
            "SELECT 'repository:'||user_id||':'||lower(full_name) AS id FROM starred_repos WHERE rowid IN (",
            false,
        ),
        "content_projections" => (
            "SELECT DISTINCT d.id FROM content_result_projections p JOIN search_documents d ON d.resource_type=p.canonical_resource_type AND d.resource_id=p.canonical_resource_id WHERE p.rowid IN (",
            false,
        ),
        "translations" => (
            "SELECT DISTINCT d.id FROM ai_translations t JOIN search_documents d ON ((d.resource_type='release' AND lower(t.entity_type) LIKE 'release%' AND d.resource_id=t.entity_id) OR (d.resource_type='announcement' AND lower(t.entity_type) LIKE 'announcement%' AND d.resource_id=lower(t.entity_id)) OR (d.resource_type='notification' AND lower(t.entity_type) LIKE 'notification%' AND d.resource_id=t.entity_id)) WHERE t.rowid IN (",
            true,
        ),
        _ => bail!("unknown search projection phase {phase}"),
    };
    let mut ids_query = QueryBuilder::<Sqlite>::new(query);
    push_rowids(&mut ids_query, rowids);
    ids_query.push(")");
    let ids = ids_query.build().fetch_all(&mut **tx).await?;
    let ids = ids
        .into_iter()
        .map(|row| row.get::<String, _>("id"))
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    let mut delete = QueryBuilder::<Sqlite>::new(if lane {
        "DELETE FROM search_document_user_lanes_fts WHERE doc_id IN ("
    } else {
        "DELETE FROM search_documents_fts WHERE doc_id IN ("
    });
    push_ids(&mut delete, &ids);
    delete.push(")");
    delete.build().execute(&mut **tx).await?;
    let mut insert = QueryBuilder::<Sqlite>::new(if lane {
        "INSERT INTO search_document_user_lanes_fts (doc_id,user_id,translated_text,smart_text) SELECT document_id,user_id,COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_document_user_lanes WHERE document_id IN ("
    } else {
        "INSERT INTO search_documents_fts (doc_id,title,body,repo_full_name,translated_text,smart_text) SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE id IN ("
    });
    push_ids(&mut insert, &ids);
    insert.push(")");
    insert.build().execute(&mut **tx).await?;
    Ok(())
}

fn push_ids(query: &mut QueryBuilder<'_, Sqlite>, ids: &[String]) {
    for (index, id) in ids.iter().enumerate() {
        if index > 0 {
            query.push(",");
        }
        query.push_bind(id.clone());
    }
}

fn database_directory(database_url: &str) -> PathBuf {
    let path = database_url
        .strip_prefix("sqlite:")
        .unwrap_or(database_url)
        .trim_start_matches("//")
        .split('?')
        .next()
        .unwrap_or("");
    Path::new(path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_owned()
}

fn min_free_bytes() -> u64 {
    std::env::var("OCTORILL_SEARCH_INDEX_MIN_FREE_BYTES")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .filter(|value: &u64| *value > 0)
        .unwrap_or(DEFAULT_MIN_FREE_BYTES)
}

fn available_bytes(path: &Path) -> io::Result<u64> {
    #[cfg(unix)]
    {
        let path = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid database path"))?;
        let mut stats = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
        let result = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        let stats = unsafe { stats.assume_init() };
        u128::from(stats.f_bavail)
            .checked_mul(u128::from(stats.f_frsize))
            .and_then(|bytes| u64::try_from(bytes).ok())
            .ok_or_else(|| io::Error::other("free-space value overflow"))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_directory_handles_sqlite_urls() {
        assert_eq!(
            database_directory("sqlite:./.data/octo-rill.db"),
            PathBuf::from("./.data")
        );
        assert_eq!(database_directory("sqlite::memory:"), PathBuf::from("."));
    }

    #[test]
    fn free_space_is_nonzero_for_workspace() {
        assert!(available_bytes(Path::new(".")).expect("statvfs") > 0);
    }
}
