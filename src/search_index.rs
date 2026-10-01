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

const INDEX_BATCH_SIZE: i64 = 25;
const DEFAULT_MIN_FREE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const BACKGROUND_BATCH_DELAY: Duration = Duration::from_millis(10);
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
    "metadata",
    "fts_documents",
    "fts_user_lanes",
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
                Ok(BatchOutcome::Ready) => tokio::time::sleep(RETRY_DELAY).await,
                Ok(BatchOutcome::Progress) => tokio::time::sleep(BACKGROUND_BATCH_DELAY).await,
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
    let ready_queue_pending = if current_status.as_deref() == Some("ready") {
        sqlx::query_scalar::<_, i64>("SELECT EXISTS(SELECT 1 FROM search_metadata_backfill_queue)")
            .fetch_one(&state.pool)
            .await
            .context("check pending search metadata work")?
            != 0
    } else {
        false
    };
    if current_status.as_deref() == Some("ready") && !ready_queue_pending {
        return Ok(BatchOutcome::Ready);
    }
    if free_bytes < minimum_free_bytes {
        if current_status.as_deref() != Some("paused_low_disk") {
            mark_status(state, "paused_low_disk", None).await?;
            warn!(
                event = "search.index.paused_low_disk",
                status = "paused_low_disk",
                free_bytes,
                minimum_free_bytes,
                retry_after_seconds = RETRY_DELAY.as_secs(),
                "search projection recovery paused below the free-space watermark"
            );
        }
        return Ok(BatchOutcome::Paused);
    }
    let was_paused = current_status.as_deref() == Some("paused_low_disk");

    let (permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "search_projection_backfill",
            SqliteWritePriority::Background,
        )
        .await?;
    let state_row = load_state(tx.as_transaction_mut()).await?;
    if state_row.status == "ready" {
        let rowids = load_rowids(tx.as_transaction_mut(), "metadata", 0).await?;
        if rowids.is_empty() {
            tx.rollback().await.ok();
            drop(permit);
            return Ok(BatchOutcome::Ready);
        }
        process_phase(tx.as_transaction_mut(), "metadata", &rowids).await?;
        tx.commit().await?;
        drop(permit);
        return Ok(BatchOutcome::Progress);
    }
    let phase = phase_index(&state_row.phase)?;
    let rowids = load_rowids(
        tx.as_transaction_mut(),
        state_row.phase.as_str(),
        state_row.cursor,
    )
    .await?;
    if rowids.is_empty() {
        let next_phase = if phase + 1 >= PHASES.len() {
            if !load_rowids(tx.as_transaction_mut(), "metadata", 0)
                .await?
                .is_empty()
            {
                Some("metadata")
            } else {
                None
            }
        } else {
            Some(PHASES[phase + 1])
        };
        if let Some(next_phase) = next_phase {
            update_state(tx.as_transaction_mut(), next_phase, 0, "building", None).await?;
        } else {
            update_state(tx.as_transaction_mut(), "translations", 0, "ready", None).await?;
        }
        tx.commit().await?;
        drop(permit);
        if was_paused {
            info!(
                event = "search.index.resumed",
                phase = next_phase.unwrap_or("translations"),
                free_bytes,
                minimum_free_bytes,
                "search projection recovery resumed above the free-space watermark"
            );
        }
        return Ok(BatchOutcome::Idle);
    }

    update_state(
        tx.as_transaction_mut(),
        state_row.phase.as_str(),
        state_row.cursor,
        "building",
        None,
    )
    .await?;
    process_phase(tx.as_transaction_mut(), state_row.phase.as_str(), &rowids).await?;
    let next_cursor = if state_row.phase == "metadata" {
        0
    } else {
        *rowids.last().expect("non-empty batch")
    };
    update_state(
        tx.as_transaction_mut(),
        state_row.phase.as_str(),
        next_cursor,
        "building",
        None,
    )
    .await?;
    tx.commit().await?;
    drop(permit);
    if was_paused {
        info!(
            event = "search.index.resumed",
            phase = state_row.phase,
            cursor = next_cursor,
            free_bytes,
            minimum_free_bytes,
            "search projection recovery resumed above the free-space watermark"
        );
    }
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
    let current = load_state(tx.as_transaction_mut()).await?;
    update_state(
        tx.as_transaction_mut(),
        &current.phase,
        current.cursor,
        status,
        error,
    )
    .await?;
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
    let current = load_state(tx.as_transaction_mut()).await?;
    update_state(
        tx.as_transaction_mut(),
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
    let current = load_state(tx.as_transaction_mut()).await?;
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
    update_state(
        tx.as_transaction_mut(),
        "content_projections",
        0,
        "building",
        None,
    )
    .await?;
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
        "metadata" => {
            "SELECT repo_id AS rowid FROM search_metadata_backfill_queue ORDER BY updated_at, repo_id LIMIT 1"
        }
        "fts_documents" => {
            "SELECT rowid FROM search_documents WHERE rowid > ? ORDER BY rowid LIMIT ?"
        }
        "fts_user_lanes" => {
            "SELECT rowid FROM search_document_user_lanes WHERE rowid > ? ORDER BY rowid LIMIT ?"
        }
        _ => bail!("unknown search projection phase {phase}"),
    };
    let rows = if phase == "metadata" {
        sqlx::query(query).fetch_all(&mut **tx).await
    } else {
        sqlx::query(query)
            .bind(cursor)
            .bind(INDEX_BATCH_SIZE)
            .fetch_all(&mut **tx)
            .await
    }
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
        "metadata" => apply_metadata_batch(tx, rowids).await?,
        "fts_documents" | "fts_user_lanes" => {}
        _ => bail!("unknown search projection phase {phase}"),
    }

    if matches!(phase, "fts_documents" | "fts_user_lanes") {
        refresh_fts_recovery_batch(tx, phase, rowids).await?;
    }
    Ok(())
}

async fn apply_metadata_batch(tx: &mut Transaction<'_, Sqlite>, repo_ids: &[i64]) -> Result<()> {
    for repo_id in repo_ids {
        let cursor = sqlx::query_scalar::<_, i64>(
            "SELECT release_cursor FROM search_metadata_backfill_queue WHERE repo_id = ?",
        )
        .bind(repo_id)
        .fetch_optional(&mut **tx)
        .await?
        .unwrap_or_default();
        let rowids = sqlx::query(
            "SELECT rowid FROM search_documents WHERE resource_type = 'release' AND repo_id = ? AND rowid > ? ORDER BY rowid LIMIT ?",
        )
        .bind(repo_id)
        .bind(cursor)
        .bind(INDEX_BATCH_SIZE)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(|row| row.get::<i64, _>("rowid"))
        .collect::<Vec<_>>();
        if rowids.is_empty() {
            sqlx::query("DELETE FROM search_metadata_backfill_queue WHERE repo_id = ?")
                .bind(repo_id)
                .execute(&mut **tx)
                .await?;
            continue;
        }
        let mut update = QueryBuilder::<Sqlite>::new(
            "UPDATE search_documents SET repo_full_name = (SELECT repo_full_name FROM search_release_metadata WHERE id = search_documents.id), owner_login = (SELECT owner_login FROM search_release_metadata WHERE id = search_documents.id), target_path = (SELECT target_path FROM search_release_metadata WHERE id = search_documents.id) WHERE rowid IN (",
        );
        push_rowids(&mut update, &rowids);
        update.push(")");
        update.build().execute(&mut **tx).await?;
        let next_cursor = *rowids.last().expect("non-empty metadata batch");
        if rowids.len() < INDEX_BATCH_SIZE as usize {
            sqlx::query("DELETE FROM search_metadata_backfill_queue WHERE repo_id = ?")
                .bind(repo_id)
                .execute(&mut **tx)
                .await?;
        } else {
            sqlx::query(
                "UPDATE search_metadata_backfill_queue SET release_cursor = ? WHERE repo_id = ?",
            )
            .bind(next_cursor)
            .bind(repo_id)
            .execute(&mut **tx)
            .await?;
        }
    }
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

async fn refresh_fts_recovery_batch(
    tx: &mut Transaction<'_, Sqlite>,
    phase: &str,
    rowids: &[i64],
) -> Result<()> {
    let mut query = match phase {
        "fts_documents" => QueryBuilder::<Sqlite>::new(
            "INSERT INTO search_documents_fts (doc_id,title,body,repo_full_name,translated_text,smart_text) SELECT id,COALESCE(title,''),COALESCE(body,''),COALESCE(repo_full_name,''),COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_documents WHERE rowid IN (",
        ),
        "fts_user_lanes" => QueryBuilder::<Sqlite>::new(
            "INSERT INTO search_document_user_lanes_fts (doc_id,user_id,translated_text,smart_text) SELECT document_id,user_id,COALESCE(translated_text,''),COALESCE(smart_text,'') FROM search_document_user_lanes WHERE rowid IN (",
        ),
        _ => bail!("unknown FTS recovery phase {phase}"),
    };
    push_rowids(&mut query, rowids);
    query.push(")");
    query.build().execute(&mut **tx).await?;
    Ok(())
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
