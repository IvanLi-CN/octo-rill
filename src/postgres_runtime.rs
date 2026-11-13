use crate::session_store;
use anyhow::{Context, Result, bail};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use tower_sessions_sqlx_store::PostgresStore;

use crate::config::AppConfig;
use crate::postgres_jobs::ClaimedTask;
use chrono::{DateTime, Utc};

const MIN_POSTGRES_VERSION_NUM: i64 = 180_000;

/// Runtime-owned PostgreSQL resources. The migration utility uses the same
/// baseline, but this type is intended for the long-lived application process.
#[derive(Clone)]
pub struct PostgresRuntime {
    pub pool: PgPool,
    pub sessions: PostgresStore,
}

/// Validate the persisted session representation before the service accepts
/// traffic. The SQLx session store owns writes, while this check makes the
/// migration boundary auditable: every sampled row must still be a valid
/// MessagePack `tower_sessions::Record` and have a non-null expiry.
pub async fn verify_session_store(pool: &PgPool, sample_limit: i64) -> Result<usize> {
    let sample_limit = sample_limit.max(1);
    let rows = sqlx::query(
        "SELECT id, data, expiry_date FROM octo_rill.tower_sessions ORDER BY id LIMIT $1",
    )
    .bind(sample_limit)
    .fetch_all(pool)
    .await
    .context("read PostgreSQL session verification sample")?;

    for row in &rows {
        let id: String = row.try_get("id").context("decode PostgreSQL session id")?;
        let data: Vec<u8> = row
            .try_get("data")
            .with_context(|| format!("decode PostgreSQL session data for {id}"))?;
        let expiry: Option<chrono::DateTime<chrono::Utc>> = row
            .try_get("expiry_date")
            .with_context(|| format!("decode PostgreSQL session expiry for {id}"))?;
        if expiry.is_none() {
            bail!("PostgreSQL session {id} has no expiry_date");
        }
        rmp_serde::from_slice::<tower_sessions::session::Record>(&data)
            .with_context(|| format!("decode MessagePack PostgreSQL session {id}"))?;
    }

    Ok(rows.len())
}

/// Refuse to expose the PG service until the migration importer has created
/// the authoritative application tables. Runtime-only bootstrap tables are
/// intentionally excluded so `OCTORILL_PG_BOOTSTRAP_ONLY=1` remains useful.
pub async fn verify_application_schema(pool: &PgPool) -> Result<()> {
    let required = [
        "users",
        "github_connections",
        "job_tasks",
        "job_task_events",
        "search_documents",
    ];
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public' AND table_name = ANY($1)",
    )
    .bind(&required)
    .fetch_all(pool)
    .await
    .context("inspect PostgreSQL application schema")?;
    let missing = required
        .iter()
        .filter(|name| !rows.iter().any(|found| found == *name))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!(
            "PostgreSQL application schema is incomplete; migration import is required before service startup: missing {}",
            missing.join(", ")
        );
    }
    Ok(())
}

impl std::fmt::Debug for PostgresRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresRuntime")
            .field("pool", &"<postgres pool>")
            .field("sessions", &self.sessions)
            .finish()
    }
}

impl PostgresRuntime {
    /// Claim a queued task using the PostgreSQL worker transaction contract.
    pub async fn claim_next_queued_task(
        &self,
        runtime_owner_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<ClaimedTask>> {
        crate::postgres_jobs::claim_next_queued_task(&self.pool, runtime_owner_id, now).await
    }

    /// Finalize a task through the same owner-checked transaction boundary.
    pub async fn finalize_task(
        &self,
        task_id: &str,
        runtime_owner_id: &str,
        status: &str,
        result_json: Option<&str>,
        error_message: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        crate::postgres_jobs::finalize_task(
            &self.pool,
            task_id,
            runtime_owner_id,
            status,
            result_json,
            error_message,
            now,
        )
        .await
    }
}

pub async fn connect(database_url: &str, max_connections: u32) -> Result<PostgresRuntime> {
    let pool = PgPoolOptions::new()
        .max_connections(max_connections.max(1))
        .connect(database_url)
        .await
        .context("connect PostgreSQL runtime")?;

    let version_num: i64 = sqlx::query_scalar::<_, String>("SHOW server_version_num")
        .fetch_one(&pool)
        .await
        .context("read PostgreSQL server_version_num")?
        .parse()
        .context("parse PostgreSQL server_version_num")?;
    if version_num < MIN_POSTGRES_VERSION_NUM {
        bail!("PostgreSQL 18 is required; server_version_num={version_num}");
    }

    crate::postgres_database_migrations::run(&pool).await?;

    let sessions = session_store::postgres_session_store(pool.clone())?;
    sessions
        .migrate()
        .await
        .context("migrate PostgreSQL session store")?;

    verify_session_store(&pool, 256).await?;

    Ok(PostgresRuntime { pool, sessions })
}

pub async fn connect_from_config(config: &AppConfig) -> Result<PostgresRuntime> {
    connect(
        &config.database_url,
        config.database_pool_max_connections as u32,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_version_gate_matches_major_18_boundary() {
        assert!(MIN_POSTGRES_VERSION_NUM >= 180_000);
        assert!(18_0000_i64 >= MIN_POSTGRES_VERSION_NUM);
        assert!(17_9999_i64 < MIN_POSTGRES_VERSION_NUM);
    }

    #[test]
    fn application_schema_gate_requires_authoritative_tables() {
        let source = include_str!("postgres_runtime.rs");
        for table in [
            "users",
            "github_connections",
            "job_tasks",
            "job_task_events",
            "search_documents",
        ] {
            assert!(source.contains(table));
        }
        assert!(source.contains("migration import is required before service startup"));
    }
}
