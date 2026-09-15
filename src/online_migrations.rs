use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction};
use tokio::{task::AbortHandle, time::sleep};
use tower_sessions::Session;
use tracing::warn;

use crate::{api, local_id, sqlite_write::SqliteWritePriority, state::AppState};

const MIGRATION_ID: &str = "content-processing-online-v1";
const MIGRATION_CHECKSUM: &str = "d7bf80e8389baa89db826966758ef410a960853bbae98605c0137504469a6846";
const OP_BATCH_SIZE: i64 = 100;
const LEASE_NAME: &str = "online-migration-operator";

const OPERATION_DEFINITIONS: &[(&str, &str, i64, &str)] = &[
    (
        "ddl-001",
        "ddl",
        1,
        "65d459e9ec29e136329dc8bb8a2bbd7bf87280999ce7d6ae2ecef3118cad0d36",
    ),
    (
        "dml-001",
        "dml",
        2,
        "bd49f0b3c9584c116821cb8a70473424b5035cc2a912c393b349869bf76d6b13",
    ),
    (
        "backfill-001",
        "backfill",
        3,
        "abfd0ba152798ac3ba49736a257e0c34f4ca5a576691abbf4e3e199f4d49a4e3",
    ),
];

const BOOTSTRAP_DDL: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS online_migration_leases (lease_name TEXT PRIMARY KEY, owner_id TEXT NOT NULL, lease_expires_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
    "CREATE TABLE IF NOT EXISTS online_migration_runs (migration_id TEXT PRIMARY KEY, definition_checksum TEXT NOT NULL, status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'paused', 'failed', 'completed')), pause_requested INTEGER NOT NULL DEFAULT 0 CHECK (pause_requested IN (0, 1)), owner_id TEXT, lease_heartbeat_at TEXT, last_error TEXT, started_at TEXT, completed_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
    "CREATE TABLE IF NOT EXISTS online_migration_operations (migration_id TEXT NOT NULL, operation_id TEXT NOT NULL, definition_checksum TEXT NOT NULL, operation_kind TEXT NOT NULL CHECK (operation_kind IN ('ddl', 'dml', 'backfill')), operation_order INTEGER NOT NULL, status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'paused', 'failed', 'completed')), cursor TEXT NOT NULL DEFAULT '', rows_processed INTEGER NOT NULL DEFAULT 0 CHECK (rows_processed >= 0), last_error TEXT, owner_id TEXT, lease_heartbeat_at TEXT, started_at TEXT, completed_at TEXT, updated_at TEXT NOT NULL, PRIMARY KEY (migration_id, operation_id), FOREIGN KEY (migration_id) REFERENCES online_migration_runs(migration_id))",
    "CREATE INDEX IF NOT EXISTS idx_online_migration_operations_status ON online_migration_operations(migration_id, status, operation_order)",
];

#[derive(Debug, Clone)]
struct Operation {
    operation_id: String,
    operation_kind: String,
    status: String,
    cursor: String,
}

pub fn spawn_operator(state: Arc<AppState>) -> AbortHandle {
    tokio::spawn(async move {
        loop {
            if let Err(error) = run_once(state.as_ref()).await {
                warn!(
                    ?error,
                    migration_id = MIGRATION_ID,
                    "online migration operation failed"
                );
                if let Err(mark_error) = mark_failed(state.as_ref(), &error.to_string()).await {
                    warn!(
                        ?mark_error,
                        migration_id = MIGRATION_ID,
                        "failed to persist migration error"
                    );
                }
            }
            sleep(Duration::from_secs(1)).await;
        }
    })
    .abort_handle()
}

async fn mark_failed(state: &AppState, error: &str) -> Result<()> {
    let (_permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "online_migration_failure",
            SqliteWritePriority::Migration,
        )
        .await?;
    let safe_error = redact_error_summary(error);
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE online_migration_runs SET status = 'failed', last_error = ?, updated_at = ? WHERE migration_id = ? AND status != 'completed'")
        .bind(&safe_error)
        .bind(&now)
        .bind(MIGRATION_ID)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE online_migration_operations SET status = 'failed', last_error = ?, updated_at = ? WHERE migration_id = ? AND operation_id = (SELECT operation_id FROM online_migration_operations WHERE migration_id = ? AND status != 'completed' ORDER BY operation_order LIMIT 1)",
    )
    .bind(&safe_error)
    .bind(&now)
    .bind(MIGRATION_ID)
    .bind(MIGRATION_ID)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn run_once(state: &AppState) -> Result<()> {
    if !ensure_bootstrap(state).await? {
        return Ok(());
    }

    let Some(operation) = load_next_operation(state).await? else {
        return Ok(());
    };
    let (_permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "online_migration_operation",
            SqliteWritePriority::Migration,
        )
        .await?;
    if !renew_lease(&mut tx, state).await? {
        tx.rollback().await.ok();
        return Ok(());
    }

    let pause_requested = sqlx::query_scalar::<_, i64>(
        "SELECT pause_requested FROM online_migration_runs WHERE migration_id = ?",
    )
    .bind(MIGRATION_ID)
    .fetch_one(&mut *tx)
    .await?
        != 0;
    if pause_requested {
        update_operation(
            &mut tx,
            &state.runtime_owner_id,
            &operation.operation_id,
            "paused",
            &operation.cursor,
            0,
            None,
        )
        .await?;
        update_run(&mut tx, &state.runtime_owner_id, "paused", None).await?;
        tx.commit().await?;
        return Ok(());
    }

    update_run(&mut tx, &state.runtime_owner_id, "running", None).await?;
    let cursor = if operation.status == "paused" {
        operation.cursor.clone()
    } else {
        operation.cursor
    };
    match operation.operation_kind.as_str() {
        "ddl" => {
            update_operation(
                &mut tx,
                &state.runtime_owner_id,
                &operation.operation_id,
                "completed",
                &cursor,
                0,
                None,
            )
            .await?;
        }
        "dml" => {
            normalize_content_mode(&mut tx).await?;
            update_operation(
                &mut tx,
                &state.runtime_owner_id,
                &operation.operation_id,
                "completed",
                &cursor,
                1,
                None,
            )
            .await?;
        }
        "backfill" => {
            let (next_cursor, processed, complete) = backfill_batch(&mut tx, &cursor).await?;
            update_operation(
                &mut tx,
                &state.runtime_owner_id,
                &operation.operation_id,
                if complete { "completed" } else { "running" },
                &next_cursor,
                processed,
                None,
            )
            .await?;
        }
        kind => anyhow::bail!("unknown online migration operation kind: {kind}"),
    }

    let pending = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM online_migration_operations WHERE migration_id = ? AND status != 'completed'",
    )
    .bind(MIGRATION_ID)
    .fetch_one(&mut *tx)
    .await?;
    if pending == 0 {
        update_run(&mut tx, &state.runtime_owner_id, "completed", None).await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn ensure_bootstrap(state: &AppState) -> Result<bool> {
    let (_permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "online_migration_bootstrap",
            SqliteWritePriority::Migration,
        )
        .await?;
    for statement in BOOTSTRAP_DDL {
        sqlx::query(statement).execute(&mut *tx).await?;
    }
    let now = chrono::Utc::now().to_rfc3339();
    let expires = (chrono::Utc::now() + chrono::Duration::seconds(30)).to_rfc3339();
    sqlx::query(
        "INSERT INTO online_migration_leases (lease_name, owner_id, lease_expires_at, updated_at) VALUES (?, ?, ?, ?) ON CONFLICT(lease_name) DO UPDATE SET owner_id = excluded.owner_id, lease_expires_at = excluded.lease_expires_at, updated_at = excluded.updated_at WHERE online_migration_leases.owner_id = excluded.owner_id OR datetime(online_migration_leases.lease_expires_at) <= datetime('now') OR NOT EXISTS (SELECT 1 FROM runtime_owners WHERE runtime_owner_id = online_migration_leases.owner_id AND datetime(lease_heartbeat_at) > datetime('now', '-90 seconds'))",
    )
    .bind(LEASE_NAME)
    .bind(&state.runtime_owner_id)
    .bind(&expires)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let owner = sqlx::query_scalar::<_, String>(
        "SELECT owner_id FROM online_migration_leases WHERE lease_name = ?",
    )
    .bind(LEASE_NAME)
    .fetch_one(&mut *tx)
    .await?;
    if owner != state.runtime_owner_id {
        tx.rollback().await.ok();
        return Ok(false);
    }

    let existing_checksum = sqlx::query_scalar::<_, String>(
        "SELECT definition_checksum FROM online_migration_runs WHERE migration_id = ?",
    )
    .bind(MIGRATION_ID)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(existing_checksum) = existing_checksum
        && existing_checksum != MIGRATION_CHECKSUM
    {
        anyhow::bail!("online migration definition checksum mismatch");
    }
    sqlx::query("INSERT OR IGNORE INTO online_migration_runs (migration_id, definition_checksum, status, created_at, updated_at) VALUES (?, ?, 'pending', ?, ?)")
        .bind(MIGRATION_ID)
        .bind(MIGRATION_CHECKSUM)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    for (operation_id, kind, order, checksum) in OPERATION_DEFINITIONS {
        let existing_checksum = sqlx::query_scalar::<_, String>(
            "SELECT definition_checksum FROM online_migration_operations WHERE migration_id = ? AND operation_id = ?",
        )
        .bind(MIGRATION_ID)
        .bind(operation_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(existing_checksum) = existing_checksum
            && existing_checksum != *checksum
        {
            anyhow::bail!(
                "online migration operation definition checksum mismatch: {operation_id}"
            );
        }
        sqlx::query("INSERT OR IGNORE INTO online_migration_operations (migration_id, operation_id, definition_checksum, operation_kind, operation_order, status, updated_at) VALUES (?, ?, ?, ?, ?, 'pending', ?)")
            .bind(MIGRATION_ID)
            .bind(operation_id)
            .bind(checksum)
            .bind(kind)
            .bind(order)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(true)
}

fn redact_error_summary(error: &str) -> String {
    const SENSITIVE_MARKERS: &[&str] = &[
        "api_key",
        "api-key",
        "apikey",
        "authorization",
        "access_token",
        "refresh_token",
        "password",
        "secret",
        "private_key",
        "cookie",
    ];
    let mut redacted = String::with_capacity(error.len().min(500));
    for (index, line) in error.lines().enumerate() {
        if index > 0 {
            redacted.push('\n');
        }
        let lower = line.to_ascii_lowercase();
        if let Some(marker_start) = SENSITIVE_MARKERS
            .iter()
            .filter_map(|marker| lower.find(marker))
            .min()
        {
            if let Some(separator) = line[marker_start..]
                .find(':')
                .or_else(|| line[marker_start..].find('='))
            {
                redacted.push_str(&line[..marker_start + separator + 1]);
                redacted.push_str("<redacted>");
            } else {
                redacted.push_str("<redacted sensitive content>");
            }
        } else {
            redacted.push_str(line);
        }
        if redacted.len() >= 500 {
            break;
        }
    }
    redacted.truncate(500);
    redacted
}

async fn load_next_operation(state: &AppState) -> Result<Option<Operation>> {
    sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT operation_id, operation_kind, status, cursor FROM online_migration_operations WHERE migration_id = ? AND status != 'completed' ORDER BY operation_order LIMIT 1",
    )
    .bind(MIGRATION_ID)
    .fetch_optional(&state.pool)
    .await
    .map(|row| {
        row.map(|(operation_id, operation_kind, status, cursor)| Operation {
            operation_id,
            operation_kind,
            status,
            cursor,
        })
    })
    .context("load online migration operation")
}

async fn renew_lease(tx: &mut Transaction<'_, Sqlite>, state: &AppState) -> Result<bool> {
    let now = chrono::Utc::now().to_rfc3339();
    let expires = (chrono::Utc::now() + chrono::Duration::seconds(30)).to_rfc3339();
    Ok(sqlx::query("UPDATE online_migration_leases SET lease_expires_at = ?, updated_at = ? WHERE lease_name = ? AND owner_id = ?")
        .bind(&expires)
        .bind(&now)
        .bind(LEASE_NAME)
        .bind(&state.runtime_owner_id)
        .execute(&mut **tx)
        .await?
        .rows_affected()
        == 1)
}

async fn update_run(
    tx: &mut Transaction<'_, Sqlite>,
    owner_id: &str,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE online_migration_runs SET status = ?, owner_id = ?, lease_heartbeat_at = ?, last_error = ?, started_at = COALESCE(started_at, ?), completed_at = CASE WHEN ? = 'completed' THEN ? ELSE completed_at END, updated_at = ? WHERE migration_id = ?")
        .bind(status)
        .bind(owner_id)
        .bind(&now)
        .bind(error)
        .bind(&now)
        .bind(status)
        .bind(&now)
        .bind(&now)
        .bind(MIGRATION_ID)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn update_operation(
    tx: &mut Transaction<'_, Sqlite>,
    owner_id: &str,
    operation_id: &str,
    status: &str,
    cursor: &str,
    processed: i64,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE online_migration_operations SET status = ?, cursor = ?, rows_processed = rows_processed + ?, last_error = ?, owner_id = ?, lease_heartbeat_at = ?, started_at = COALESCE(started_at, ?), completed_at = CASE WHEN ? = 'completed' THEN ? ELSE completed_at END, updated_at = ? WHERE migration_id = ? AND operation_id = ?")
        .bind(status)
        .bind(cursor)
        .bind(processed)
        .bind(error)
        .bind(owner_id)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(status)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(MIGRATION_ID)
        .bind(operation_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn normalize_content_mode(tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'content_processing_control'",
    )
    .fetch_one(&mut **tx)
    .await?;
    if exists == 0 {
        return Ok(());
    }
    sqlx::query("UPDATE content_processing_control SET mode = 'global', updated_at = CURRENT_TIMESTAMP WHERE id = 1 AND mode IN ('legacy', 'rollback_freeze')")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn backfill_batch(
    tx: &mut Transaction<'_, Sqlite>,
    cursor: &str,
) -> Result<(String, i64, bool)> {
    let (phase, last_id) = cursor
        .split_once('|')
        .unwrap_or(("translation_work_items", ""));
    let table_exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
    )
    .bind(phase)
    .fetch_one(&mut **tx)
    .await?
        != 0;
    if !table_exists {
        return Ok(match phase {
            "translation_work_items" => ("ai_translations|".to_owned(), 0, false),
            _ => (cursor.to_owned(), 0, true),
        });
    }

    if phase == "translation_work_items" {
        let rows = sqlx::query("SELECT id, kind, entity_id, source_hash, status FROM translation_work_items WHERE id > ? ORDER BY id LIMIT ?")
            .bind(last_id)
            .bind(OP_BATCH_SIZE)
            .fetch_all(&mut **tx)
            .await?;
        if rows.is_empty() {
            return Ok(("ai_translations|".to_owned(), 0, false));
        }
        let mut next = last_id.to_owned();
        for row in &rows {
            let id: String = row.get("id");
            let kind: String = row.get("kind");
            let entity_id: String = row.get("entity_id");
            let source_hash: String = row.get("source_hash");
            let status: String = row.get("status");
            insert_observation(
                tx,
                "translation_work_items",
                &id,
                &kind,
                &entity_id,
                &source_hash,
                &status,
            )
            .await?;
            next = id;
        }
        return Ok((
            format!("translation_work_items|{next}"),
            rows.len() as i64,
            false,
        ));
    }

    let rows = sqlx::query("SELECT id, entity_type, entity_id, source_hash FROM ai_translations WHERE id > ? ORDER BY id LIMIT ?")
        .bind(last_id)
        .bind(OP_BATCH_SIZE)
        .fetch_all(&mut **tx)
        .await?;
    if rows.is_empty() {
        return Ok((cursor.to_owned(), 0, true));
    }
    let mut next = last_id.to_owned();
    for row in &rows {
        let id: String = row.get("id");
        let kind: String = row.get("entity_type");
        let entity_id: String = row.get("entity_id");
        let source_hash: String = row.get("source_hash");
        insert_observation(
            tx,
            "ai_translations",
            &id,
            &kind,
            &entity_id,
            &source_hash,
            "cached",
        )
        .await?;
        next = id;
    }
    Ok((format!("ai_translations|{next}"), rows.len() as i64, false))
}

async fn insert_observation(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    id: &str,
    kind: &str,
    entity_id: &str,
    source_hash: &str,
    status: &str,
) -> Result<()> {
    let resource_type = ["release", "announcement", "notification"]
        .iter()
        .find(|value| kind.contains(**value))
        .copied();
    let pipeline = if kind.contains("smart") {
        "polishing"
    } else {
        "translation"
    };
    let basis = json!({"kind": kind, "source_hash": source_hash, "status": status}).to_string();
    sqlx::query("INSERT OR IGNORE INTO content_legacy_observations (id, legacy_table, legacy_primary_key, canonical_resource_type, canonical_resource_id, pipeline, classification, observation_basis_json, observed_at) VALUES (?, ?, ?, ?, ?, ?, 'legacy_cached', ?, CURRENT_TIMESTAMP)")
        .bind(local_id::generate_local_id().to_string())
        .bind(table)
        .bind(id)
        .bind(resource_type)
        .bind(entity_id)
        .bind(pipeline)
        .bind(basis)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn admin_list(
    State(state): State<Arc<AppState>>,
    session: Session,
) -> Result<Json<Value>, crate::error::ApiError> {
    let _ = api::require_admin_user_id(state.as_ref(), &session).await?;
    let control_tables_ready = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('online_migration_runs', 'online_migration_operations')",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(crate::error::ApiError::internal)?
        == 2;
    if !control_tables_ready {
        return Ok(Json(json!({"run": Value::Null, "operations": []})));
    }
    let run = sqlx::query("SELECT migration_id, definition_checksum, status, pause_requested, owner_id, lease_heartbeat_at, last_error, started_at, completed_at, created_at, updated_at FROM online_migration_runs WHERE migration_id = ?")
        .bind(MIGRATION_ID)
        .fetch_optional(&state.pool)
        .await
        .map_err(crate::error::ApiError::internal)?;
    let operations = sqlx::query("SELECT operation_id, definition_checksum, operation_kind, operation_order, status, cursor, rows_processed, last_error, owner_id, lease_heartbeat_at, started_at, completed_at, updated_at FROM online_migration_operations WHERE migration_id = ? ORDER BY operation_order")
        .bind(MIGRATION_ID)
        .fetch_all(&state.pool)
        .await
        .map_err(crate::error::ApiError::internal)?;
    let run = run
        .map(|row| {
            json!({
                "migration_id": row.get::<String, _>("migration_id"),
                "definition_checksum": row.get::<String, _>("definition_checksum"),
                "status": row.get::<String, _>("status"),
                "pause_requested": row.get::<i64, _>("pause_requested") != 0,
                "owner_id": row.get::<Option<String>, _>("owner_id"),
                "lease_heartbeat_at": row.get::<Option<String>, _>("lease_heartbeat_at"),
                "last_error": row.get::<Option<String>, _>("last_error"),
                "started_at": row.get::<Option<String>, _>("started_at"),
                "completed_at": row.get::<Option<String>, _>("completed_at"),
                "created_at": row.get::<String, _>("created_at"),
                "updated_at": row.get::<String, _>("updated_at"),
            })
        })
        .unwrap_or(Value::Null);
    let operations = operations
        .into_iter()
        .map(|row| {
            json!({
                "operation_id": row.get::<String, _>("operation_id"),
                "definition_checksum": row.get::<String, _>("definition_checksum"),
                "operation_kind": row.get::<String, _>("operation_kind"),
                "operation_order": row.get::<i64, _>("operation_order"),
                "status": row.get::<String, _>("status"),
                "cursor": row.get::<String, _>("cursor"),
                "rows_processed": row.get::<i64, _>("rows_processed"),
                "last_error": row.get::<Option<String>, _>("last_error"),
                "owner_id": row.get::<Option<String>, _>("owner_id"),
                "lease_heartbeat_at": row.get::<Option<String>, _>("lease_heartbeat_at"),
                "started_at": row.get::<Option<String>, _>("started_at"),
                "completed_at": row.get::<Option<String>, _>("completed_at"),
                "updated_at": row.get::<String, _>("updated_at"),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({"run": run, "operations": operations})))
}

pub async fn admin_detail(
    state: State<Arc<AppState>>,
    session: Session,
    path: Path<String>,
) -> Result<Json<Value>, crate::error::ApiError> {
    if path.0 != MIGRATION_ID {
        return Err(crate::error::ApiError::new(
            axum::http::StatusCode::NOT_FOUND,
            "not_found",
            "migration not found",
        ));
    }
    admin_list(state, session).await
}

pub async fn admin_pause(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(migration_id): Path<String>,
) -> Result<Json<Value>, crate::error::ApiError> {
    let _ = api::require_admin_user_id(state.as_ref(), &session).await?;
    if migration_id != MIGRATION_ID {
        return Err(crate::error::ApiError::new(
            axum::http::StatusCode::NOT_FOUND,
            "not_found",
            "migration not found",
        ));
    }
    let (_permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "online_migration_pause",
            SqliteWritePriority::Foreground,
        )
        .await
        .map_err(crate::error::ApiError::internal)?;
    sqlx::query("UPDATE online_migration_runs SET pause_requested = 1, updated_at = CURRENT_TIMESTAMP WHERE migration_id = ?")
        .bind(MIGRATION_ID)
        .execute(&mut *tx)
        .await
        .map_err(crate::error::ApiError::internal)?;
    tx.commit()
        .await
        .map_err(crate::error::ApiError::internal)?;
    admin_list(State(state), session).await
}

pub async fn admin_resume(
    State(state): State<Arc<AppState>>,
    session: Session,
    Path(migration_id): Path<String>,
) -> Result<Json<Value>, crate::error::ApiError> {
    let _ = api::require_admin_user_id(state.as_ref(), &session).await?;
    if migration_id != MIGRATION_ID {
        return Err(crate::error::ApiError::new(
            axum::http::StatusCode::NOT_FOUND,
            "not_found",
            "migration not found",
        ));
    }
    let (_permit, mut tx) = state
        .sqlite_writer
        .begin_immediate_with_priority(
            &state.pool,
            "online_migration_resume",
            SqliteWritePriority::Foreground,
        )
        .await
        .map_err(crate::error::ApiError::internal)?;
    sqlx::query("UPDATE online_migration_runs SET pause_requested = 0, status = CASE WHEN status IN ('paused', 'failed') THEN 'running' ELSE status END, last_error = CASE WHEN status = 'failed' THEN NULL ELSE last_error END, updated_at = CURRENT_TIMESTAMP WHERE migration_id = ?")
        .bind(MIGRATION_ID)
        .execute(&mut *tx)
        .await
        .map_err(crate::error::ApiError::internal)?;
    tx.commit()
        .await
        .map_err(crate::error::ApiError::internal)?;
    admin_list(State(state), session).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        sqlx::raw_sql(
            "CREATE TABLE translation_work_items (id TEXT PRIMARY KEY, kind TEXT NOT NULL, entity_id TEXT NOT NULL, source_hash TEXT NOT NULL, status TEXT NOT NULL); CREATE TABLE content_legacy_observations (id TEXT PRIMARY KEY, legacy_table TEXT NOT NULL, legacy_primary_key TEXT NOT NULL, canonical_resource_type TEXT, canonical_resource_id TEXT, pipeline TEXT, classification TEXT NOT NULL, observation_basis_json TEXT NOT NULL, observed_at TEXT NOT NULL, UNIQUE(legacy_table, legacy_primary_key));",
        )
        .execute(&pool)
        .await
        .expect("create migration fixture");
        pool
    }

    #[tokio::test]
    async fn backfill_is_bounded_and_resumes_from_cursor() {
        let pool = pool().await;
        for index in 0..105 {
            sqlx::query(
                "INSERT INTO translation_work_items (id, kind, entity_id, source_hash, status) VALUES (?, 'release_summary', ?, 'hash', 'completed')",
            )
            .bind(format!("work-{index:03}"))
            .bind(format!("release-{index}"))
            .execute(&pool)
            .await
            .expect("insert fixture row");
        }

        let mut tx = pool.begin().await.expect("begin first batch");
        let (cursor, processed, complete) = backfill_batch(&mut tx, "").await.expect("first batch");
        tx.commit().await.expect("commit first batch");
        assert_eq!(processed, 100);
        assert!(!complete);
        assert!(cursor.starts_with("translation_work_items|work-099"));

        let mut tx = pool.begin().await.expect("begin second batch");
        let (next_cursor, processed, complete) = backfill_batch(&mut tx, &cursor)
            .await
            .expect("second batch");
        tx.commit().await.expect("commit second batch");
        assert_eq!(processed, 5);
        assert!(!complete);
        assert!(next_cursor.starts_with("translation_work_items|work-104"));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM content_legacy_observations")
                .fetch_one(&pool)
                .await
                .expect("count observations"),
            105
        );
    }

    #[tokio::test]
    async fn backfill_reentry_does_not_duplicate_legacy_observations() {
        let pool = pool().await;
        sqlx::query(
            "INSERT INTO translation_work_items (id, kind, entity_id, source_hash, status) VALUES ('work-1', 'release_summary', 'release-1', 'hash', 'completed')",
        )
        .execute(&pool)
        .await
        .expect("insert fixture row");
        for _ in 0..2 {
            let mut tx = pool.begin().await.expect("begin batch");
            let (_, processed, _) = backfill_batch(&mut tx, "").await.expect("backfill row");
            tx.commit().await.expect("commit batch");
            assert_eq!(processed, 1);
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM content_legacy_observations")
                .fetch_one(&pool)
                .await
                .expect("count observations"),
            1
        );
    }

    #[test]
    fn migration_failure_summary_is_redacted_and_bounded() {
        let summary = redact_error_summary(
            "request failed: authorization=Bearer live-secret\nsource: safe\nsecret=private-value",
        );
        assert!(!summary.contains("live-secret"));
        assert!(!summary.contains("private-value"));
        assert!(summary.contains("authorization=<redacted>"));
        assert!(summary.len() <= 500);
    }
}
