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
    let owns_lease = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM online_migration_leases WHERE lease_name = ? AND owner_id = ? AND datetime(lease_expires_at) > datetime('now')",
    )
    .bind(LEASE_NAME)
    .bind(&state.runtime_owner_id)
    .fetch_one(&mut *tx)
    .await?
        > 0;
    if !owns_lease {
        tx.rollback().await.ok();
        return Ok(());
    }
    sqlx::query("UPDATE online_migration_runs SET status = 'failed', last_error = ?, updated_at = ? WHERE migration_id = ? AND status != 'completed' AND EXISTS (SELECT 1 FROM online_migration_leases WHERE lease_name = ? AND owner_id = ? AND datetime(lease_expires_at) > datetime('now'))")
        .bind(&safe_error)
        .bind(&now)
        .bind(MIGRATION_ID)
        .bind(LEASE_NAME)
        .bind(&state.runtime_owner_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE online_migration_operations SET status = 'failed', last_error = ?, updated_at = ? WHERE migration_id = ? AND operation_id = (SELECT operation_id FROM online_migration_operations WHERE migration_id = ? AND status != 'completed' ORDER BY operation_order LIMIT 1) AND EXISTS (SELECT 1 FROM online_migration_leases WHERE lease_name = ? AND owner_id = ? AND datetime(lease_expires_at) > datetime('now'))",
    )
    .bind(&safe_error)
    .bind(&now)
    .bind(MIGRATION_ID)
    .bind(MIGRATION_ID)
    .bind(LEASE_NAME)
    .bind(&state.runtime_owner_id)
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
        "INSERT INTO online_migration_leases (lease_name, owner_id, lease_expires_at, updated_at) VALUES (?, ?, ?, ?) ON CONFLICT(lease_name) DO UPDATE SET owner_id = excluded.owner_id, lease_expires_at = excluded.lease_expires_at, updated_at = excluded.updated_at WHERE online_migration_leases.owner_id = excluded.owner_id OR (datetime(online_migration_leases.lease_expires_at) <= datetime('now') AND NOT EXISTS (SELECT 1 FROM runtime_owners WHERE runtime_owner_id = online_migration_leases.owner_id AND datetime(lease_heartbeat_at) > datetime('now', '-90 seconds')))",
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
    if redacted.len() > 500 {
        let mut end = 500;
        while !redacted.is_char_boundary(end) {
            end -= 1;
        }
        redacted.truncate(end);
    }
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
    let (phase, last_rowid) = cursor
        .split_once('|')
        .unwrap_or(("translation_work_items", ""));
    // Legacy primary keys are nanoid/text values and do not provide a stable
    // ordering. The cursor records the last observed SQLite rowid for
    // operators, but observation absence is the completion/re-entry predicate:
    // SQLite may reuse a deleted rowid and such a row must still be observed.
    let last_rowid = last_rowid.parse::<i64>().unwrap_or(0);
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
        let rows = sqlx::query("SELECT legacy.rowid AS migration_rowid, legacy.id, legacy.kind, legacy.entity_id, legacy.source_hash, legacy.status FROM translation_work_items AS legacy WHERE NOT EXISTS (SELECT 1 FROM content_legacy_observations observation WHERE observation.legacy_table = 'translation_work_items' AND observation.legacy_primary_key = legacy.id) ORDER BY legacy.rowid LIMIT ?")
            .bind(OP_BATCH_SIZE)
            .fetch_all(&mut **tx)
            .await?;
        if rows.is_empty() {
            return Ok(("ai_translations|".to_owned(), 0, false));
        }
        let mut next = last_rowid;
        for row in &rows {
            let rowid: i64 = row.get("migration_rowid");
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
                false,
            )
            .await?;
            next = rowid;
        }
        return Ok((
            format!("translation_work_items|{next}"),
            rows.len() as i64,
            false,
        ));
    }

    let rows = sqlx::query("SELECT legacy.rowid AS migration_rowid, legacy.id, legacy.entity_type, legacy.entity_id, legacy.source_hash, legacy.status, legacy.title, legacy.summary FROM ai_translations AS legacy WHERE NOT EXISTS (SELECT 1 FROM content_legacy_observations observation WHERE observation.legacy_table = 'ai_translations' AND observation.legacy_primary_key = legacy.id) ORDER BY legacy.rowid LIMIT ?")
        .bind(OP_BATCH_SIZE)
        .fetch_all(&mut **tx)
        .await?;
    if rows.is_empty() {
        return Ok((cursor.to_owned(), 0, true));
    }
    let mut next = last_rowid;
    for row in &rows {
        let rowid: i64 = row.get("migration_rowid");
        let id: String = row.get("id");
        let kind: String = row.get("entity_type");
        let entity_id: String = row.get("entity_id");
        let source_hash: String = row.get("source_hash");
        let status: String = row.get("status");
        let title: Option<String> = row.get("title");
        let summary: Option<String> = row.get("summary");
        let displayable = status == "ready"
            && [title.as_deref(), summary.as_deref()]
                .into_iter()
                .flatten()
                .any(|value| !value.trim().is_empty());
        insert_observation(
            tx,
            "ai_translations",
            &id,
            &kind,
            &entity_id,
            &source_hash,
            &status,
            displayable,
        )
        .await?;
        next = rowid;
    }
    Ok((format!("ai_translations|{next}"), rows.len() as i64, false))
}

#[allow(clippy::too_many_arguments)]
async fn insert_observation(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    id: &str,
    kind: &str,
    entity_id: &str,
    source_hash: &str,
    status: &str,
    displayable_cache: bool,
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
    let classification = if table == "ai_translations" && displayable_cache {
        "legacy_cached"
    } else {
        "legacy_conflict"
    };
    let basis = json!({"kind": kind, "source_hash": source_hash, "status": status, "classification": classification}).to_string();
    sqlx::query("INSERT OR IGNORE INTO content_legacy_observations (id, legacy_table, legacy_primary_key, canonical_resource_type, canonical_resource_id, pipeline, classification, observation_basis_json, observed_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)")
        .bind(local_id::generate_local_id().to_string())
        .bind(table)
        .bind(id)
        .bind(resource_type)
        .bind(entity_id)
        .bind(pipeline)
        .bind(classification)
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
                "last_error": row.get::<Option<String>, _>("last_error").map(|error| redact_error_summary(&error)),
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
                "last_error": row.get::<Option<String>, _>("last_error").map(|error| redact_error_summary(&error)),
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
    ensure_bootstrap(state.as_ref())
        .await
        .map_err(crate::error::ApiError::internal)?;
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
    ensure_bootstrap(state.as_ref())
        .await
        .map_err(crate::error::ApiError::internal)?;
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
    use std::net::SocketAddr;
    use std::sync::Arc;

    use super::*;
    use crate::ai::LlmScheduler;
    use crate::config::AppConfig;
    use crate::crypto::EncryptionKey;
    use crate::observability::LoggingThresholds;
    use crate::state::{build_oauth_client, build_webauthn};
    use crate::translations::{TranslationRuntimeConfig, TranslationSchedulerController};
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode, header},
        routing::{get, post},
    };
    use sqlx::sqlite::SqlitePoolOptions;
    use tower::ServiceExt;
    use tower_sessions::{MemoryStore, Session, SessionManagerLayer};
    use url::Url;

    async fn pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        sqlx::raw_sql(
            "CREATE TABLE translation_work_items (id TEXT PRIMARY KEY, kind TEXT NOT NULL, entity_id TEXT NOT NULL, source_hash TEXT NOT NULL, status TEXT NOT NULL); CREATE TABLE ai_translations (id TEXT PRIMARY KEY, entity_type TEXT NOT NULL, entity_id TEXT NOT NULL, source_hash TEXT NOT NULL, status TEXT NOT NULL, title TEXT, summary TEXT); CREATE TABLE content_legacy_observations (id TEXT PRIMARY KEY, legacy_table TEXT NOT NULL, legacy_primary_key TEXT NOT NULL, canonical_resource_type TEXT, canonical_resource_id TEXT, pipeline TEXT, classification TEXT NOT NULL, observation_basis_json TEXT NOT NULL, observed_at TEXT NOT NULL, UNIQUE(legacy_table, legacy_primary_key));",
        )
        .execute(&pool)
        .await
        .expect("create migration fixture");
        pool
    }

    fn test_state(pool: sqlx::SqlitePool, runtime_owner_id: &str) -> Arc<AppState> {
        let encryption_key =
            EncryptionKey::from_base64("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
                .expect("build encryption key");
        let config = AppConfig {
            bind_addr: "127.0.0.1:58090"
                .parse::<SocketAddr>()
                .expect("parse bind addr"),
            public_base_url: Url::parse("http://127.0.0.1:58090").expect("parse public url"),
            database_url: "sqlite::memory:".to_owned(),
            sqlite_pool_max_connections: 1,
            static_dir: None,
            task_log_dir: std::env::temp_dir().join("octo-rill-online-migration-tests"),
            job_worker_concurrency: 1,
            encryption_key: encryption_key.clone(),
            github: crate::config::GitHubOAuthConfig {
                client_id: "test-client-id".to_owned(),
                client_secret: "test-client-secret".to_owned(),
                redirect_url: Url::parse("http://127.0.0.1:58090/auth/callback")
                    .expect("parse redirect url"),
            },
            linuxdo: None,
            ai: None,
            ai_max_concurrency: 1,
            ai_daily_at_local: None,
            app_default_time_zone: "UTC".to_owned(),
            logging: LoggingThresholds::default(),
        };
        Arc::new(AppState {
            config: config.clone(),
            pool,
            sqlite_writer: crate::sqlite_write::SqliteWriteCoordinator::new(),
            api_key_last_used_touches: crate::api_keys::ApiKeyLastUsedTouchQueue::new(),
            http: reqwest::Client::new(),
            github_rest_http: reqwest::Client::new(),
            github_rest_api_base: Url::parse("https://api.github.com/").expect("parse api url"),
            github_graphql_url: Url::parse("https://api.github.com/graphql")
                .expect("parse graphql url"),
            github_oauth: build_oauth_client(&config).expect("build github oauth"),
            linuxdo_oauth: None,
            webauthn: build_webauthn(&config).expect("build webauthn"),
            encryption_key,
            llm_scheduler: Arc::new(LlmScheduler::new(1)),
            translation_scheduler: Arc::new(TranslationSchedulerController::new(
                TranslationRuntimeConfig::default(),
            )),
            admin_collection_read_gate: Arc::new(tokio::sync::Semaphore::new(1)),
            runtime_owner_id: runtime_owner_id.to_owned(),
        })
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
        assert_eq!(cursor, "translation_work_items|100");

        let mut tx = pool.begin().await.expect("begin second batch");
        let (next_cursor, processed, complete) = backfill_batch(&mut tx, &cursor)
            .await
            .expect("second batch");
        tx.commit().await.expect("commit second batch");
        assert_eq!(processed, 5);
        assert!(!complete);
        assert_eq!(next_cursor, "translation_work_items|105");
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
        for (index, expected_processed) in [1, 0].into_iter().enumerate() {
            let mut tx = pool.begin().await.expect("begin batch");
            let (_, processed, _) = backfill_batch(&mut tx, "").await.expect("backfill row");
            tx.commit().await.expect("commit batch");
            assert_eq!(processed, expected_processed, "re-entry pass {index}");
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM content_legacy_observations")
                .fetch_one(&pool)
                .await
                .expect("count observations"),
            1
        );
    }

    #[tokio::test]
    async fn backfill_cursor_uses_rowid_for_random_legacy_ids() {
        let pool = pool().await;
        for id in ["z-last", "a-first"] {
            sqlx::query(
                "INSERT INTO translation_work_items (id, kind, entity_id, source_hash, status) VALUES (?, 'release_summary', ?, 'hash', 'completed')",
            )
            .bind(id)
            .bind(format!("release-{id}"))
            .execute(&pool)
            .await
            .expect("insert fixture row");
        }

        let mut tx = pool.begin().await.expect("begin first batch");
        let (cursor, processed, complete) = backfill_batch(&mut tx, "").await.expect("first batch");
        tx.commit().await.expect("commit first batch");
        assert_eq!(processed, 2);
        assert!(!complete);
        assert_eq!(cursor, "translation_work_items|2");

        sqlx::query(
            "INSERT INTO translation_work_items (id, kind, entity_id, source_hash, status) VALUES ('0-new', 'release_summary', 'release-new', 'hash', 'completed')",
        )
        .execute(&pool)
        .await
        .expect("insert row after cursor");

        let mut tx = pool.begin().await.expect("begin second batch");
        let (next_cursor, processed, complete) = backfill_batch(&mut tx, &cursor)
            .await
            .expect("second batch");
        tx.commit().await.expect("commit second batch");
        assert_eq!(processed, 1);
        assert!(!complete);
        assert_eq!(next_cursor, "translation_work_items|3");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM content_legacy_observations")
                .fetch_one(&pool)
                .await
                .expect("count observations"),
            3
        );
    }

    #[tokio::test]
    async fn backfill_reobserves_rowid_reused_after_delete() {
        let pool = pool().await;
        for id in ["first", "second"] {
            sqlx::query(
                "INSERT INTO translation_work_items (id, kind, entity_id, source_hash, status) VALUES (?, 'release_summary', ?, 'hash', 'completed')",
            )
            .bind(id)
            .bind(format!("release-{id}"))
            .execute(&pool)
            .await
            .expect("insert fixture row");
        }

        let mut tx = pool.begin().await.expect("begin first batch");
        let (cursor, processed, _) = backfill_batch(&mut tx, "")
            .await
            .expect("backfill initial rows");
        tx.commit().await.expect("commit initial batch");
        assert_eq!(processed, 2);
        assert_eq!(cursor, "translation_work_items|2");

        sqlx::query("DELETE FROM translation_work_items WHERE id = 'second'")
            .execute(&pool)
            .await
            .expect("delete highest rowid");
        sqlx::query(
            "INSERT INTO translation_work_items (id, kind, entity_id, source_hash, status) VALUES ('reused', 'release_summary', 'release-reused', 'hash', 'completed')",
        )
        .execute(&pool)
        .await
        .expect("insert row with reused rowid");

        let mut tx = pool.begin().await.expect("begin resumed batch");
        let (next_cursor, processed, complete) = backfill_batch(&mut tx, &cursor)
            .await
            .expect("backfill reused rowid");
        tx.commit().await.expect("commit resumed batch");
        assert_eq!(processed, 1);
        assert!(!complete);
        assert_eq!(next_cursor, "translation_work_items|2");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM content_legacy_observations WHERE legacy_table = 'translation_work_items'",
            )
            .fetch_one(&pool)
            .await
            .expect("count work observations"),
            3
        );
    }

    #[tokio::test]
    async fn backfill_classifies_legacy_evidence_without_promoting_it() {
        let pool = pool().await;
        sqlx::query(
            "INSERT INTO translation_work_items (id, kind, entity_id, source_hash, status) VALUES ('failed-work', 'release_summary', 'release-failed', 'work-hash', 'failed')",
        )
        .execute(&pool)
        .await
        .expect("insert failed legacy work");
        sqlx::query(
            "INSERT INTO ai_translations (id, entity_type, entity_id, source_hash, status, title, summary) VALUES ('ready-cache', 'release', 'release-ready', 'cache-hash', 'ready', 'Cached title', NULL), ('incomplete-cache', 'release', 'release-incomplete', 'incomplete-hash', 'queued', NULL, NULL)",
        )
        .execute(&pool)
        .await
        .expect("insert legacy cache evidence");

        let mut tx = pool.begin().await.expect("begin work observation");
        let (_, processed, _) = backfill_batch(&mut tx, "")
            .await
            .expect("observe legacy work");
        tx.commit().await.expect("commit work observation");
        assert_eq!(processed, 1);

        let mut tx = pool.begin().await.expect("begin cache observation");
        let (_, processed, _) = backfill_batch(&mut tx, "ai_translations|")
            .await
            .expect("observe legacy caches");
        tx.commit().await.expect("commit cache observation");
        assert_eq!(processed, 2);

        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT legacy_table, legacy_primary_key, classification FROM content_legacy_observations ORDER BY legacy_table, legacy_primary_key",
        )
        .fetch_all(&pool)
        .await
        .expect("load classifications");
        assert_eq!(
            rows,
            vec![
                (
                    "ai_translations".to_owned(),
                    "incomplete-cache".to_owned(),
                    "legacy_conflict".to_owned(),
                ),
                (
                    "ai_translations".to_owned(),
                    "ready-cache".to_owned(),
                    "legacy_cached".to_owned(),
                ),
                (
                    "translation_work_items".to_owned(),
                    "failed-work".to_owned(),
                    "legacy_conflict".to_owned(),
                ),
            ]
        );
    }

    #[tokio::test]
    async fn bootstrap_does_not_reclaim_expired_lease_with_live_owner() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        sqlx::query(
            "CREATE TABLE runtime_owners (runtime_owner_id TEXT PRIMARY KEY, lease_heartbeat_at TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .expect("create runtime owners");
        sqlx::query(
            "INSERT INTO runtime_owners (runtime_owner_id, lease_heartbeat_at, created_at, updated_at) VALUES ('owner-b', datetime('now'), CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("insert live owner");
        for statement in BOOTSTRAP_DDL {
            sqlx::query(statement)
                .execute(&pool)
                .await
                .expect("create migration control table");
        }
        sqlx::query(
            "INSERT INTO online_migration_leases (lease_name, owner_id, lease_expires_at, updated_at) VALUES (?, 'owner-b', datetime('now', '-1 second'), CURRENT_TIMESTAMP)",
        )
        .bind(LEASE_NAME)
        .execute(&pool)
        .await
        .expect("insert expired lease");

        let state = test_state(pool.clone(), "owner-a");
        assert!(!ensure_bootstrap(&state).await.expect("bootstrap lease"));
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT owner_id FROM online_migration_leases WHERE lease_name = ?",
            )
            .bind(LEASE_NAME)
            .fetch_one(&pool)
            .await
            .expect("read protected lease"),
            "owner-b"
        );

        sqlx::query(
            "UPDATE runtime_owners SET lease_heartbeat_at = datetime('now', '-91 seconds') WHERE runtime_owner_id = 'owner-b'",
        )
        .execute(&pool)
        .await
        .expect("stale the old owner");
        assert!(
            ensure_bootstrap(&state)
                .await
                .expect("take over stale lease")
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT owner_id FROM online_migration_leases WHERE lease_name = ?",
            )
            .bind(LEASE_NAME)
            .fetch_one(&pool)
            .await
            .expect("read taken lease"),
            "owner-a"
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

    #[test]
    fn migration_failure_summary_truncates_at_utf8_boundary() {
        let summary = redact_error_summary(&"界".repeat(200));
        assert!(summary.len() <= 500);
        assert!(summary.is_char_boundary(summary.len()));
    }

    #[tokio::test]
    async fn stale_owner_cannot_mark_active_migration_failed() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        for statement in BOOTSTRAP_DDL {
            sqlx::query(statement)
                .execute(&pool)
                .await
                .expect("create migration control table");
        }
        sqlx::query("INSERT INTO online_migration_runs (migration_id, definition_checksum, status, created_at, updated_at) VALUES (?, ?, 'running', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)")
            .bind(MIGRATION_ID)
            .bind(MIGRATION_CHECKSUM)
            .execute(&pool)
            .await
            .expect("insert migration run");
        sqlx::query("INSERT INTO online_migration_operations (migration_id, operation_id, definition_checksum, operation_kind, operation_order, status, updated_at) VALUES (?, 'ddl-001', ?, 'ddl', 1, 'running', CURRENT_TIMESTAMP)")
            .bind(MIGRATION_ID)
            .bind(OPERATION_DEFINITIONS[0].3)
            .execute(&pool)
            .await
            .expect("insert migration operation");
        sqlx::query("INSERT INTO online_migration_leases (lease_name, owner_id, lease_expires_at, updated_at) VALUES (?, 'owner-b', datetime('now', '+30 seconds'), CURRENT_TIMESTAMP)")
            .bind(LEASE_NAME)
            .execute(&pool)
            .await
            .expect("insert active lease");

        let state = test_state(pool.clone(), "owner-a");
        mark_failed(&state, "stale owner error")
            .await
            .expect("mark failed");

        let run_status = sqlx::query_scalar::<_, String>(
            "SELECT status FROM online_migration_runs WHERE migration_id = ?",
        )
        .bind(MIGRATION_ID)
        .fetch_one(&pool)
        .await
        .expect("read run status");
        assert_eq!(run_status, "running");
        let operation_status = sqlx::query_scalar::<_, String>(
            "SELECT status FROM online_migration_operations WHERE migration_id = ? AND operation_id = 'ddl-001'",
        )
        .bind(MIGRATION_ID)
        .fetch_one(&pool)
        .await
        .expect("read operation status");
        assert_eq!(operation_status, "running");
    }

    async fn issue_admin_session(session: Session) -> StatusCode {
        session
            .insert(
                "user_id",
                crate::local_id::test_local_id("online-migration-admin"),
            )
            .await
            .expect("insert admin session");
        StatusCode::NO_CONTENT
    }

    #[tokio::test]
    async fn admin_http_read_redacts_persisted_error_summary() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        sqlx::query("CREATE TABLE users (id TEXT PRIMARY KEY, is_admin INTEGER NOT NULL, is_disabled INTEGER NOT NULL, paused_at TEXT, last_active_at TEXT)")
            .execute(&pool)
            .await
            .expect("create users table");
        sqlx::query("INSERT INTO users (id, is_admin, is_disabled) VALUES (?, 1, 0)")
            .bind(crate::local_id::test_local_id("online-migration-admin"))
            .execute(&pool)
            .await
            .expect("insert admin user");
        for statement in BOOTSTRAP_DDL {
            sqlx::query(statement)
                .execute(&pool)
                .await
                .expect("create migration control table");
        }
        sqlx::query("INSERT INTO online_migration_runs (migration_id, definition_checksum, status, last_error, created_at, updated_at) VALUES (?, ?, 'failed', 'authorization=Bearer live-secret', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)")
            .bind(MIGRATION_ID)
            .bind(MIGRATION_CHECKSUM)
            .execute(&pool)
            .await
            .expect("insert failed migration run");
        let state = test_state(pool, "online-admin-runtime");
        let app = Router::new()
            .route("/login", get(issue_admin_session))
            .route("/admin/jobs/migrations", get(admin_list))
            .with_state(state)
            .layer(SessionManagerLayer::new(MemoryStore::default()).with_name("sid"));

        let login = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/login")
                    .body(Body::empty())
                    .expect("build login request"),
            )
            .await
            .expect("login response");
        let cookie = login
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .expect("session cookie")
            .to_owned();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/jobs/migrations")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .expect("build admin request"),
            )
            .await
            .expect("admin response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read admin response body");
        let payload: Value = serde_json::from_slice(&body).expect("parse admin response");
        let last_error = payload["run"]["last_error"].as_str().expect("last error");
        assert_eq!(last_error, "authorization=<redacted>");
        assert!(!last_error.contains("live-secret"));
    }

    #[tokio::test]
    async fn admin_pause_bootstraps_control_tables_before_updating_request() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        sqlx::query("CREATE TABLE users (id TEXT PRIMARY KEY, is_admin INTEGER NOT NULL, is_disabled INTEGER NOT NULL, paused_at TEXT, last_active_at TEXT)")
            .execute(&pool)
            .await
            .expect("create users table");
        sqlx::query("INSERT INTO users (id, is_admin, is_disabled) VALUES (?, 1, 0)")
            .bind(crate::local_id::test_local_id("online-migration-admin"))
            .execute(&pool)
            .await
            .expect("insert admin user");
        sqlx::query("CREATE TABLE runtime_owners (runtime_owner_id TEXT PRIMARY KEY, lease_heartbeat_at TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)")
            .execute(&pool)
            .await
            .expect("create runtime owners table");
        sqlx::query("INSERT INTO runtime_owners (runtime_owner_id, lease_heartbeat_at, created_at, updated_at) VALUES (?, datetime('now'), CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)")
            .bind("online-admin-runtime")
            .execute(&pool)
            .await
            .expect("insert runtime owner");
        let state = test_state(pool.clone(), "online-admin-runtime");
        let app = Router::new()
            .route("/login", get(issue_admin_session))
            .route(
                "/admin/jobs/migrations/{migration_id}/pause",
                post(admin_pause),
            )
            .with_state(state)
            .layer(SessionManagerLayer::new(MemoryStore::default()).with_name("sid"));

        let login = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/login")
                    .body(Body::empty())
                    .expect("build login request"),
            )
            .await
            .expect("login response");
        let cookie = login
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .expect("session cookie")
            .to_owned();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/admin/jobs/migrations/{MIGRATION_ID}/pause"))
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .expect("build pause request"),
            )
            .await
            .expect("pause response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT pause_requested FROM online_migration_runs WHERE migration_id = ?",
            )
            .bind(MIGRATION_ID)
            .fetch_one(&pool)
            .await
            .expect("read pause request"),
            1
        );
    }
}
