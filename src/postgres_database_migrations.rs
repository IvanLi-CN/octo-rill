use anyhow::{Context, Result};
use sqlx::{PgPool, migrate::Migrator};

/// PostgreSQL keeps an independent SQLx migration history. The SQLite
/// `_sqlx_migrations` table is deliberately never read or reused here.
static MIGRATOR: Migrator = sqlx::migrate!("./migrations_pg");

pub async fn run(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await.context("begin PostgreSQL migrations")?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('octo-rill:runtime-baseline', 0))")
        .execute(&mut *tx)
        .await
        .context("lock PostgreSQL migration history")?;
    MIGRATOR
        .run(&mut *tx)
        .await
        .context("failed to apply PostgreSQL migrations")?;
    tx.commit().await.context("commit PostgreSQL migrations")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::MIGRATOR;

    #[test]
    fn postgres_migrator_has_an_independent_baseline() {
        assert_eq!(MIGRATOR.iter().count(), 1);
        assert_eq!(MIGRATOR.iter().next().expect("baseline").version, 1);
    }
}
