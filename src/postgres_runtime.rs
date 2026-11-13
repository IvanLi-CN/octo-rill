use crate::session_store;
use anyhow::{Context, Result, bail};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower_sessions_sqlx_store::PostgresStore;

const MIN_POSTGRES_VERSION_NUM: i64 = 180_000;

/// Runtime-owned PostgreSQL resources. The migration utility uses the same
/// baseline, but this type is intended for the long-lived application process.
#[derive(Clone)]
pub struct PostgresRuntime {
    pub pool: PgPool,
    pub sessions: PostgresStore,
}

impl std::fmt::Debug for PostgresRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresRuntime")
            .field("pool", &"<postgres pool>")
            .field("sessions", &self.sessions)
            .finish()
    }
}

pub async fn connect(database_url: &str, max_connections: u32) -> Result<PostgresRuntime> {
    let pool = PgPoolOptions::new()
        .max_connections(max_connections.max(1))
        .connect(database_url)
        .await
        .context("connect PostgreSQL runtime")?;

    let version_num: i64 = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(&pool)
        .await
        .context("read PostgreSQL server_version_num")?;
    if version_num < MIN_POSTGRES_VERSION_NUM {
        bail!("PostgreSQL 18 is required; server_version_num={version_num}");
    }

    let mut migration_tx = pool
        .begin()
        .await
        .context("begin PostgreSQL runtime baseline")?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('octo-rill:runtime-baseline', 0))")
        .execute(&mut *migration_tx)
        .await
        .context("lock PostgreSQL runtime baseline")?;
    sqlx::raw_sql(include_str!("../migrations_pg/0001_runtime.sql"))
        .execute(&mut *migration_tx)
        .await
        .context("apply PostgreSQL runtime baseline")?;
    migration_tx
        .commit()
        .await
        .context("commit PostgreSQL runtime baseline")?;

    let sessions = session_store::postgres_session_store(pool.clone())?;
    sessions
        .migrate()
        .await
        .context("migrate PostgreSQL session store")?;

    Ok(PostgresRuntime { pool, sessions })
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
}
