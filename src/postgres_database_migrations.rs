use anyhow::{Context, Result};
use sqlx::{PgPool, migrate::Migrator};

/// PostgreSQL keeps an independent SQLx migration history. The SQLite
/// `_sqlx_migrations` table is deliberately never read or reused here.
static MIGRATOR: Migrator = sqlx::migrate!("./migrations_pg/");

pub async fn run(pool: &PgPool) -> Result<()> {
    let mut tx = crate::postgres_write::begin(pool)
        .await
        .context("begin PostgreSQL migrations")?;
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
        assert_eq!(MIGRATOR.iter().count(), 17);
        assert_eq!(MIGRATOR.iter().next().expect("baseline").version, 1);
        let content = MIGRATOR
            .iter()
            .find(|migration| migration.version == 6)
            .expect("content processing migration");
        assert!(content.sql.contains("content_work_items"));
        assert!(content.sql.contains("timestamptz"));
        assert!(content.sql.contains("content_result_projections"));
        let translation = MIGRATOR
            .iter()
            .find(|migration| migration.version == 7)
            .expect("translation migration");
        assert!(translation.sql.contains("translation_work_items"));
        assert!(translation.sql.contains("translation_batches"));
        let audit = MIGRATOR
            .iter()
            .find(|migration| migration.version == 8)
            .expect("translation audit migration");
        assert!(audit.sql.contains("translation_attempt_events"));
        assert!(audit.sql.contains("llm_recovery_flags"));
        let brief = MIGRATOR
            .iter()
            .find(|migration| migration.version == 9)
            .expect("brief migration");
        assert!(brief.sql.contains("briefs"));
        let application = MIGRATOR
            .iter()
            .find(|migration| migration.version == 15)
            .expect("application schema migration");
        assert!(application.sql.contains("public.repo_releases"));
        assert!(application.sql.contains("public.user_repo_associations"));
        assert!(
            application
                .sql
                .contains("CREATE OR REPLACE VIEW public.user_release_visible_repos")
        );
        assert!(!application.sql.contains("OWNER TO"));
        let baseline = MIGRATOR.iter().next().expect("runtime baseline");
        for table in [
            "public.users",
            "public.github_connections",
            "public.linuxdo_connections",
            "public.user_passkeys",
            "public.user_api_keys",
        ] {
            assert!(baseline.sql.contains(table), "baseline missing {table}");
        }
    }
}
